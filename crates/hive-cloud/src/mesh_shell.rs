//! Cross-node interactive-shell forwarding for sandboxes over the existing
//! `hive_p2p::STREAM_RAW_TARGET` mesh surface — no new wire protocol.
//!
//! WHY THIS EXISTS: `sandboxes_api::open_shell` used to answer a client that
//! landed on a non-owner node with a `wrong_node` message naming the owner
//! and closing — the frontend has no way to reconnect anywhere useful (no
//! cert-covered per-node hostname exists on this fleet; `acme.rs` issues
//! certs only for `*.{apps_domain}` and a fixed short list under
//! `{platform_domain}`, never arbitrary node subdomains). A real sandbox
//! (`sbx_253aa161efc04c5b`, genuinely running via real Firecracker on
//! fc-bangkok) was reachable only by luck of round-robin/geo DNS, and every
//! other landing showed a permanently "disconnected" terminal.
//!
//! THE FIX: the non-owner node opens a `RawTarget` mesh stream to the owner
//! (the exact same `STREAM_RAW_TARGET` handshake/failover machinery
//! `raw_proxy.rs`/`udp_relay.rs` already use for generic cross-node
//! forwarding — `mesh_raw::resolve` learns to recognize a SANDBOX-shaped
//! target and bridges it to a real local pty instead of a deployment's
//! container port) and pumps FRAMED bytes both ways. The browser's
//! websocket to `api.<domain>` never closes or redirects; only the
//! server-side plumbing changes, so the frontend (`terminal-panel.tsx`)
//! needs zero changes.
//!
//! ADMISSION (this module's protocol, carried inside `RawTarget.deployment`):
//! a versioned handshake naming the protocol id, its version, the project the
//! opener authorized, the tenant it authorized as, and the initial terminal
//! size. The owner admits only an exact match of protocol id + version and
//! then binds the target to the record EXACTLY: the record's `project_id` must
//! equal the claimed project and the record must name THIS node as its owner
//! (an id-only lookup — the shape this module shipped with — answers a shell
//! for any sandbox id a peer happens to name, whatever project it belongs to).
//! Every refusal is explicit: a `TAG_REFUSED` frame naming why, not a silent
//! `RAW_TARGET_NOT_FOUND`, so a client can tell "wrong project" / "unsupported
//! protocol version" / "owner could not open the shell" apart from "that peer
//! has no idea what this target is".
//!
//! WIRE FORMAT over the mesh TCP splice (this module's own, not
//! `hive_p2p`'s): `[1B tag][4B BE len][payload]`, chosen to preserve the
//! EXACT distinction `pump_shell`'s websocket wire contract already makes
//! between raw pty bytes (`Message::Binary`) and JSON control messages
//! (`Message::Text`) — collapsing them onto one untyped byte stream would
//! make a literal `{` a client types indistinguishable from a resize
//! control message, the same hazard `pump_shell`'s own doc comment already
//! calls out for the websocket leg.
//!   TAG_DATA (0)    — raw pty bytes, either direction. Payloads larger than
//!                     [`MAX_FRAME`] are split into several frames (the pty is
//!                     a byte stream, and a single oversized frame would be
//!                     rejected by the reader's bound and kill the session).
//!   TAG_RESIZE (1)  — owner-bound only: `[u16 BE cols][u16 BE rows]`. Applied
//!                     in stream order, after the initial size the handshake
//!                     established at `open_shell`.
//!   TAG_EXITED (2)  — client-bound only: `[i32 BE exit_code]` (`i32::MIN`
//!                     sentinel means "no exit code", mirroring `AgentEvent::
//!                     PtyExited`'s `Option<i32>`).
//!   TAG_REFUSED (3) — client-bound only: a UTF-8 operator-facing reason
//!                     (bounded, and deliberately free of the values that
//!                     decided it — those go to this node's log, never to a
//!                     browser). Ends the session.

use std::sync::Arc;
use std::time::Duration;

use hive_p2p::{RawProto, RawTarget, RawTargetConn};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::sandboxes::SandboxProvider;
use crate::state::CloudState;

/// Sentinel `RawTarget.function` prefix marking a sandbox-shell target — kept
/// inside the existing `RawTarget{project,function,port,proto}` shape rather
/// than adding a new stream mode, since the wire/failover/admission machinery
/// this needs already exists verbatim for exactly this purpose.
const SHELL_MARKER: &str = "__sandbox_shell__";

/// Protocol id inside the handshake payload. A peer that carries the marker
/// but a DIFFERENT id is not speaking this protocol at all (it gets a plain
/// not-found, i.e. the opener's normal failover), while a peer that carries
/// this id at an unsupported VERSION gets an explicit refusal — the
/// mixed-version case, which must never degrade into serving a shell whose
/// framing the two ends disagree about.
const SHELL_PROTO_ID: &str = "hive-sandbox-shell";
/// Bump on any change to this module's frame tags, handshake fields, or the
/// admission rules below; the owner refuses any other value out loud.
const SHELL_PROTO_VERSION: u32 = 1;

const TAG_DATA: u8 = 0;
const TAG_RESIZE: u8 = 1;
const TAG_EXITED: u8 = 2;
const TAG_REFUSED: u8 = 3;

/// Largest payload of one frame — the same bound `hive_p2p` puts on a raw
/// datagram (`RAW_MAX_DATAGRAM`). This stream is TCP (not
/// datagram-boundary-preserving), but bounding it defends against a
/// corrupted/hostile length prefix the same way every other framed mesh read
/// in this codebase does, and gives `write_data` its chunk size.
const MAX_FRAME: usize = hive_p2p::RAW_MAX_DATAGRAM;
/// How long the owner holds a freshly opened pty waiting for the mesh opener's
/// local connect. The transport connects to the resolved address immediately,
/// so this only ever fires when the opener died between admission and connect
/// — and without it that pty (and its listener) would be held until the
/// sandbox timed out.
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(10);
/// Bounded outbound queue on the client side: pty output decoded from the mesh
/// waits here before it reaches the websocket. Bounded (rather than unbounded)
/// so a browser that stops reading applies backpressure to the mesh read
/// instead of growing this queue without limit.
const CLIENT_QUEUE: usize = 128;
/// Cap on a refusal reason's bytes (`TAG_REFUSED` payload).
const MAX_REASON: usize = 512;

/// The admission handshake this module encodes into `RawTarget.deployment`
/// (a field no other target kind reads — `mesh_raw::resolve` returns for shell
/// targets before it ever looks at it). Carrying it as JSON keeps the fields
/// unambiguous: project ids are tenant-controlled strings.
#[derive(Serialize, Deserialize)]
struct ShellHandshake {
    /// [`SHELL_PROTO_ID`] — a different value means this payload is not ours.
    p: String,
    /// [`SHELL_PROTO_VERSION`].
    v: u32,
    /// The project the OPENING node authorized the caller for. The owner binds
    /// the record to exactly this value.
    project: String,
    /// The tenant the opening node authorized — carried for this node's log
    /// and audit trail, never as an extra refusal rule: a project maps to one
    /// team (`CloudState` project rows), so the project binding above already
    /// is the authorization boundary, and a tenant string recorded at sandbox
    /// creation time can legitimately lag a later team change.
    tenant: String,
    cols: u16,
    rows: u16,
}

/// Build the mesh target for a shell session on `sandbox_id`, authorized by
/// `tenant` for `project`.
pub(crate) fn shell_target(
    project: &str,
    tenant: &str,
    sandbox_id: &str,
    cols: u16,
    rows: u16,
) -> RawTarget {
    // A degenerate size (a client that sends `?cols=0&rows=0`) would open a
    // pty the guest shell cannot lay out; fall back to the same defaults the
    // HTTP query uses.
    let hs = ShellHandshake {
        p: SHELL_PROTO_ID.into(),
        v: SHELL_PROTO_VERSION,
        project: project.to_string(),
        tenant: tenant.to_string(),
        cols: if cols == 0 { 80 } else { cols },
        rows: if rows == 0 { 24 } else { rows },
    };
    RawTarget {
        project: SHELL_MARKER.into(),
        function: sandbox_id.to_string(),
        deployment: serde_json::to_string(&hs).unwrap_or_default(),
        // `port` is unused by this target kind and left 0: the terminal size
        // travels in the handshake, not in a field that means "container port"
        // to every other target kind.
        port: 0,
        proto: RawProto::Tcp,
    }
}

/// Owner-side resolution: does `t` name a sandbox this node actually owns,
/// bound to the project the opener authorized, under a protocol version this
/// node speaks?
///
/// `None` keeps the transport's normal `RAW_TARGET_NOT_FOUND` failover
/// semantics (this peer is simply not the place — another node may be); a
/// `Some` refusal connection instead says "this IS the place and the answer is
/// no", which is the distinction a client cannot make from a status byte.
///
/// `mesh_raw::resolve` calls this FIRST, before its own deployment-lease
/// resolution, so a real sandbox target never falls through to (and is never
/// confused with) a deployment raw-port target.
pub(crate) async fn resolve_sandbox_shell(
    cloud: &Arc<CloudState>,
    t: &RawTarget,
) -> Option<RawTargetConn> {
    if t.project != SHELL_MARKER {
        return None;
    }
    let hs: ShellHandshake = match serde_json::from_str(&t.deployment) {
        Ok(h) => h,
        Err(_) => {
            // The marker says "shell target" but the payload is not a readable
            // handshake — that is an admission failure, not a routing miss, so
            // refuse out loud instead of letting the opener fail over looking
            // for another owner.
            tracing::warn!(
                sandbox = %t.function,
                "sandbox shell target refused: unreadable handshake payload"
            );
            return refusal_conn("this node could not read the sandbox-shell handshake").await;
        }
    };
    if hs.p != SHELL_PROTO_ID {
        return None;
    }
    if hs.v != SHELL_PROTO_VERSION {
        tracing::warn!(
            sandbox = %t.function,
            version = hs.v,
            supported = SHELL_PROTO_VERSION,
            "sandbox shell target refused: unsupported protocol version"
        );
        return refusal_conn(&format!(
            "the owning node speaks sandbox-shell protocol version {SHELL_PROTO_VERSION}, not {}",
            hs.v
        ))
        .await;
    }
    let sandbox_id = t.function.clone();
    let (cols, rows) = (hs.cols, hs.rows);

    let Some(rec) = cloud.sandboxes.get_sandbox_by_id(&sandbox_id) else {
        tracing::info!(
            sandbox = %sandbox_id,
            project = %hs.project,
            "sandbox shell target: no such sandbox on this node"
        );
        return None;
    };

    // EXACT binding: the record the id resolves to must belong to the project
    // the opener authorized. Looking the id up alone (no project) means any
    // peer able to open a raw-target stream gets a shell on any sandbox it can
    // name, whatever tenant owns it.
    if rec.project_id != hs.project {
        tracing::warn!(
            sandbox = %sandbox_id,
            claimed_project = %hs.project,
            record_project = %rec.project_id,
            tenant = %hs.tenant,
            "sandbox shell target refused: sandbox belongs to a different project than the forwarded request claims"
        );
        return refusal_conn("this sandbox does not belong to the project this request was authorized for").await;
    }
    if !hs.tenant.is_empty() && rec.tenant_id != hs.tenant {
        // Recorded, not refused: see `ShellHandshake::tenant`. The project
        // binding above is what actually authorizes the session.
        tracing::warn!(
            sandbox = %sandbox_id,
            project = %hs.project,
            claimed_tenant = %hs.tenant,
            record_tenant = %rec.tenant_id,
            "sandbox shell target: forwarded tenant differs from the sandbox record's tenant"
        );
    }

    // Real ownership check — a sandbox record with a DIFFERENT owner_node must
    // not be served here even if the id happens to match something local
    // (e.g. a stale adopted-metadata copy on a node that isn't the real
    // owner). An EMPTY owner_node is a record persisted before the field
    // existed, which the leader-placement rule owned — the same reading
    // `sandboxes_api::open_shell` applies on the forwarding side, so the two
    // ends can never disagree about who serves a legacy record.
    let owned_here = if rec.owner_node.is_empty() {
        cloud.is_control_plane_leader()
    } else {
        rec.owner_node == cloud.node_name
    };
    if !owned_here {
        tracing::info!(
            sandbox = %sandbox_id,
            project = %hs.project,
            owner_node = %rec.owner_node,
            "sandbox shell target: this node is not the sandbox's owner"
        );
        return None;
    }

    let (rx, pty) = match cloud
        .sandboxes
        .open_shell(&rec.project_id, &sandbox_id, cols, rows)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            // The detail stays on this node (a backend error string can name
            // host paths); the client gets the typed refusal.
            tracing::warn!(
                sandbox = %sandbox_id,
                project = %hs.project,
                error = ?e,
                "sandbox shell target refused: could not open a shell on the owned sandbox"
            );
            return refusal_conn("the owning node could not open a shell on this sandbox").await;
        }
    };

    // A real local TCP listener bridging this ONE pty session — `RawTargetConn`
    // only ever carries an ADDRESS (the contract every other target kind
    // already uses), so the bridge has to be a real socket, not a direct
    // handoff of `rx`/`pty`. Bound to loopback, ephemeral port, torn down
    // when the single accepted connection ends (one-shot: `mesh_raw`'s
    // opener connects to it immediately after this returns, so there is no
    // window for a second client to race the accept).
    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(sandbox = %sandbox_id, error = %e, "sandbox shell: owner-side bridge listener failed to bind");
            return None;
        }
    };
    let addr = match listener.local_addr() {
        Ok(a) => a.to_string(),
        Err(_) => return None,
    };
    tokio::spawn(async move {
        // The pty is a reservation: if the opener never connects (its dial died
        // between the admission and this connect), returning here drops `rx`
        // and `pty` instead of holding a live shell until the sandbox times
        // out. Bounded, never unbounded, and never released on an error branch
        // alone — this is the `ColdStartGuard` rule on a second surface.
        match tokio::time::timeout(ACCEPT_TIMEOUT, listener.accept()).await {
            Ok(Ok((stream, _))) => bridge_owner_side(stream, rx, pty).await,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "sandbox shell: owner-side bridge accept failed")
            }
            Err(_) => tracing::warn!(
                sandbox = %sandbox_id,
                "sandbox shell: owner-side bridge accept timed out; released the pty"
            ),
        }
    });
    Some(RawTargetConn { addr, guard: None })
}

/// A refusal the OWNER can actually send: `serve_raw_target` only ever reports
/// a status byte (no reason channel), so a refusal rides the same one-shot
/// local listener the real bridge uses — the transport connects, splices, and
/// the client reads one `TAG_REFUSED` frame before EOF.
async fn refusal_conn(reason: &str) -> Option<RawTargetConn> {
    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(_) => return None,
    };
    let addr = match listener.local_addr() {
        Ok(a) => a.to_string(),
        Err(_) => return None,
    };
    let mut reason = reason.to_string();
    reason.truncate(MAX_REASON);
    tokio::spawn(async move {
        match tokio::time::timeout(ACCEPT_TIMEOUT, listener.accept()).await {
            Ok(Ok((stream, _))) => {
                let (_, mut w) = stream.into_split();
                let _ = write_frame(&mut w, TAG_REFUSED, reason.as_bytes()).await;
                let _ = w.flush().await;
            }
            Ok(Err(_)) | Err(_) => {}
        }
    });
    Some(RawTargetConn { addr, guard: None })
}

/// Aborts a spawned pump task when the future holding it is dropped — axum
/// drops a request future the moment a client disconnects, and a dropped
/// `JoinHandle` does NOT stop the task (it would go on reading the mesh half
/// forever, holding the far end's pty open).
struct AbortOnDrop<T>(tokio::task::JoinHandle<T>);
impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Owner side of the bridge: pump `AgentEvent::PtyOutput`/`PtyExited` and
/// `PtyIo` (the exact same real pty this node's OWN `pump_shell` already
/// drives for a local client) onto the framed TCP connection instead of a
/// websocket. Mirrors `sandboxes_api::pump_shell`'s behavior exactly, so a
/// client sees identical output whether it's local or reached over the
/// mesh.
///
/// `read_frame` is NOT cancel-safe mid-frame (a `select!` branch that loses
/// the race drops a partially-read length prefix or payload, corrupting
/// framing for the rest of the connection) — so, mirroring `udp_relay.rs`'s
/// `pump_mesh`, the inbound (client→pty) direction runs in its OWN spawned
/// task, never as a `select!` arm. The outbound (pty→client) loop owns the
/// write half exclusively; dropping it on exit closes the connection, which
/// is this bridge's end-of-session signal for the inbound task.
///
/// The reverse direction matters just as much: a client that hangs up (or a
/// trunk that drops) ends the INBOUND task, and that must end the session too
/// — otherwise this node keeps a real shell running for a browser that is
/// already gone. `done_rx` carries that, and `AbortOnDrop` releases the pty
/// even when this future itself is dropped.
async fn bridge_owner_side(
    stream: tokio::net::TcpStream,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<hive_core::AgentEvent>,
    pty: hive_backend::PtyIo,
) {
    let (mut r, mut w) = stream.into_split();
    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel::<()>();
    let inbound = tokio::spawn(async move {
        // Dropped (=> `done_rx` completes) whenever this task ends, for any
        // reason: clean EOF, transport error, or abort.
        let _done = done_tx;
        loop {
            match read_frame(&mut r).await {
                Ok(Some((TAG_DATA, payload))) => pty.input(payload),
                Ok(Some((TAG_RESIZE, payload))) if payload.len() == 4 => {
                    let cols = u16::from_be_bytes([payload[0], payload[1]]);
                    let rows = u16::from_be_bytes([payload[2], payload[3]]);
                    pty.resize(cols, rows);
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
    });
    let inbound = AbortOnDrop(inbound);
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Some(hive_core::AgentEvent::PtyOutput { bytes, .. }) => {
                    if write_data(&mut w, &bytes).await.is_err() {
                        break;
                    }
                }
                Some(hive_core::AgentEvent::PtyExited { exit_code, .. }) => {
                    let code = exit_code.unwrap_or(i32::MIN);
                    let _ = write_frame(&mut w, TAG_EXITED, &code.to_be_bytes()).await;
                    break;
                }
                Some(_) => {}
                None => break,
            },
            // Client is gone (websocket closed, trunk dropped). End the
            // session here so dropping the pty kills the guest shell — the
            // same thing `pump_shell` does when its own socket ends.
            _ = &mut done_rx => break,
        }
    }
    inbound.0.abort();
}

/// Non-owner side: given an already-admitted raw mesh stream (opened via
/// `PeerPool::open_raw_to_port`), pump it against a real axum `WebSocket` —
/// translating this module's frame tags back into the exact
/// `Message::Binary`/`Message::Text` shapes `pump_shell` already produces
/// for a local client, so `terminal-panel.tsx` sees byte-identical behavior
/// regardless of which node it landed on.
///
/// Same cancel-safety split as `bridge_owner_side`: `read_frame` on the mesh
/// leg runs in its own task (writing decoded frames to the websocket
/// directly — `WebSocket::send` needs no external synchronization against
/// the socket's own recv side, axum's `WebSocket` splits cleanly), while
/// this function's own loop owns `socket.recv()` (cancel-safe: backed by an
/// internal channel poll, unlike a raw partial-frame read) and writes
/// outbound frames onto the mesh stream.
pub(crate) async fn bridge_client_side<S>(
    mut socket: axum::extract::ws::WebSocket,
    raw: S,
    owner: String,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    use axum::extract::ws::Message;
    let (mut r, mut w) = tokio::io::split(raw);
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<Message>(CLIENT_QUEUE);
    let inbound = tokio::spawn(async move {
        loop {
            match read_frame(&mut r).await {
                Ok(Some((TAG_DATA, payload))) => {
                    if out_tx.send(Message::Binary(payload)).await.is_err() {
                        break;
                    }
                }
                Ok(Some((TAG_EXITED, payload))) if payload.len() == 4 => {
                    let code = i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                    let exit_code = if code == i32::MIN { None } else { Some(code) };
                    let _ = out_tx
                        .send(Message::Text(
                            serde_json::json!({ "type": "exited", "exit_code": exit_code })
                                .to_string(),
                        ))
                        .await;
                    break;
                }
                Ok(Some((TAG_REFUSED, payload))) => {
                    // The owner admitted the stream and then said no — a typed
                    // refusal, not an unreachable peer. Reuse `wrong_node`'s
                    // shape (the one control frame the UI already renders) with
                    // the owner's own reason as the message.
                    let reason = String::from_utf8_lossy(&payload);
                    tracing::warn!(owner = %owner, reason = %reason, "sandbox shell mesh forward refused by owner");
                    let _ = out_tx
                        .send(Message::Text(
                            serde_json::json!({
                                "type": "wrong_node",
                                "owner": owner,
                                "message": reason,
                            })
                            .to_string(),
                        ))
                        .await;
                    break;
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            }
        }
    });
    let inbound = AbortOnDrop(inbound);
    loop {
        tokio::select! {
            m = out_rx.recv() => match m {
                Some(msg) => {
                    if socket.send(msg).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
            client = socket.recv() => match client {
                Some(Ok(Message::Binary(bytes))) => {
                    if write_data(&mut w, &bytes).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Text(t))) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                        if v.get("type").and_then(|x| x.as_str()) == Some("resize") {
                            let cols = v.get("cols").and_then(|x| x.as_u64()).unwrap_or(80) as u16;
                            let rows = v.get("rows").and_then(|x| x.as_u64()).unwrap_or(24) as u16;
                            let mut payload = Vec::with_capacity(4);
                            payload.extend_from_slice(&cols.to_be_bytes());
                            payload.extend_from_slice(&rows.to_be_bytes());
                            if write_frame(&mut w, TAG_RESIZE, &payload).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(_)) => break,
                _ => {}
            },
        }
    }
    inbound.0.abort();
}

async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    tag: u8,
    payload: &[u8],
) -> std::io::Result<()> {
    debug_assert!(payload.len() <= MAX_FRAME);
    w.write_all(&[tag]).await?;
    w.write_all(&(payload.len() as u32).to_be_bytes()).await?;
    w.write_all(payload).await?;
    w.flush().await
}

/// Write pty bytes as one or more `TAG_DATA` frames. Anything over
/// [`MAX_FRAME`] is split: the pty is a byte stream (a large paste, or a
/// `cat` of a big file, is one logical burst with no frame meaning of its
/// own), and a single oversized frame would be rejected by `read_frame`'s
/// bound — which would kill the session mid-output instead of delivering it.
async fn write_data<W: AsyncWrite + Unpin>(w: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    for chunk in bytes.chunks(MAX_FRAME) {
        write_frame(w, TAG_DATA, chunk).await?;
    }
    Ok(())
}

/// `Ok(None)` on a clean EOF at a frame boundary (owner closed / stream
/// ended normally); `Err` on any other read failure (mesh transport died
/// mid-frame — surfaced to the caller as a real error so the browser
/// websocket closes with an error rather than hanging silently, per this
/// module's own framing-cancellation discipline).
async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    // `Ok(0)` / `UnexpectedEof` = clean EOF AT a frame boundary (owner closed
    // the session, the mesh stream ended) — `Ok(None)` so the caller ends the
    // session quietly. Any other failure (or a short read mid-frame) is
    // transport death, and is returned as a real error.
    let mut tag = [0u8; 1];
    match r.read_exact(&mut tag).await {
        Ok(0) => return Ok(None),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "sandbox shell frame too large",
        ));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload).await?;
    Ok(Some((tag[0], payload)))
}
