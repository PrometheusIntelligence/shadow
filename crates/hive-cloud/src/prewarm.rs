//! Speculative prewarm: start the function a request is about to invoke while
//! the request's TLS handshake is still finishing.
//!
//! A cold start (litebox ~3 s, container seconds) is the whole latency a first
//! request pays. The ClientHello already tells us the host, so instead of
//! discovering the cold pool after the handshake we start it during the
//! handshake: `acme::SniResolver::resolve` hands the SNI to a bounded channel,
//! and this worker walks the hints off the request path (on the bulkhead
//! runtime, never on a serving tokio worker).
//!
//! Two properties are load-bearing:
//!
//! * **Never block, never unbounded.** Hints are dropped when the queue is
//!   full and deduped per host, and warming goes through `Fluid::warm`, which
//!   refuses without side effects when the pool is already warm, in backoff,
//!   behind an open crash-loop circuit, saturated or shutting down. A wrong
//!   guess costs one idle instance that scale-to-zero then drains — never a
//!   permanent one, and never a request.
//! * **Warm where the deployment IS, not where the handshake landed.** Public
//!   hosts are round-robin DNS, so the entry node is frequently not the owner.
//!   A hint for a host this node does not serve is forwarded to a healthy peer
//!   that does, which hides the mesh hop too — that hop is the larger half of
//!   the latency when the entry node is on another continent.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::state::CloudState;

/// Hints buffered before new ones are dropped. Bounded: a burst of handshakes
/// must never grow memory or queue cold starts faster than they complete.
const QUEUE_MAX: usize = 256;

/// Concurrent warm attempts. Small: a cold start is seconds long, and warming
/// faster than instances can start only wastes cells.
const MAX_INFLIGHT: usize = 4;

/// Ignore a repeat hint for the same host inside this window — a browser opens
/// several connections for one page load and they all carry the same SNI.
const DEDUPE_MS: u64 = 30_000;

/// Cap on the dedupe map (hosts, not bytes). Old entries are dropped on write.
const DEDUPE_MAX: usize = 4096;

static TX: OnceLock<tokio::sync::mpsc::Sender<String>> = OnceLock::new();

static HINTS: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static DUPES: AtomicU64 = AtomicU64::new(0);
static FORWARDED: AtomicU64 = AtomicU64::new(0);

/// Last hint time per host, for dedupe.
static LAST: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();

fn last_map() -> &'static Mutex<HashMap<String, u64>> {
    LAST.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record a hint and say whether it is new enough to act on.
fn should_warm(host: &str) -> bool {
    let now = hive_core::now_ms();
    let mut m = match last_map().lock() {
        Ok(m) => m,
        Err(e) => e.into_inner(),
    };
    if m.len() >= DEDUPE_MAX {
        m.retain(|_, t| now.saturating_sub(*t) < DEDUPE_MS);
        if m.len() >= DEDUPE_MAX {
            m.clear();
        }
    }
    match m.get(host) {
        Some(t) if now.saturating_sub(*t) < DEDUPE_MS => false,
        _ => {
            m.insert(host.to_string(), now);
            true
        }
    }
}

/// Hand a hostname observed in a ClientHello to the prewarm worker. Called
/// synchronously from the TLS SNI resolver, so it never blocks and never
/// waits: a full queue drops the hint.
pub fn hint(host: &str) {
    // Reject the shapes that cannot name a served deployment before they
    // reach the queue: bare names, IPs, over-long labels.
    if host.is_empty() || host.len() > 253 || !host.contains('.') {
        return;
    }
    let Some(tx) = TX.get() else { return };
    match tx.try_send(host.to_ascii_lowercase()) {
        Ok(()) => {
            let n = HINTS.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 200 == 0 {
                tracing::info!(
                    hints = n,
                    dropped = DROPPED.load(Ordering::Relaxed),
                    dupes = DUPES.load(Ordering::Relaxed),
                    forwarded = FORWARDED.load(Ordering::Relaxed),
                    "prewarm: hint census"
                );
            }
        }
        Err(_) => {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Install the SNI hook and start the worker. Called once at boot, after the
/// gateway exists.
pub fn spawn(cloud: Arc<CloudState>) {
    let (tx, rx) = tokio::sync::mpsc::channel(QUEUE_MAX);
    if TX.set(tx).is_err() {
        return;
    }
    crate::acme::install_warm_hook(Arc::new(|host: &str| hint(host)));
    crate::bulkhead::spawn(worker(cloud, rx));
}

async fn worker(cloud: Arc<CloudState>, mut rx: tokio::sync::mpsc::Receiver<String>) {
    let sem = Arc::new(tokio::sync::Semaphore::new(MAX_INFLIGHT));
    while let Some(host) = rx.recv().await {
        if !should_warm(&host) {
            DUPES.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let permit = match Arc::clone(&sem).acquire_owned().await {
            Ok(p) => p,
            Err(_) => return,
        };
        let c = cloud.clone();
        crate::bulkhead::spawn(async move {
            let _permit = permit;
            handle(&c, &host).await;
        });
    }
}

async fn handle(cloud: &Arc<CloudState>, host: &str) {
    if cloud.gw.serves_host(host) {
        match cloud.gw.warm_host(host).await {
            fluid_gateway::WarmHost::Started => {
                tracing::info!(host = %host, "prewarm: started an instance ahead of the request");
            }
            other => {
                tracing::debug!(host = %host, outcome = ?other, "prewarm: no start needed");
            }
        }
        return;
    }
    // Not ours: forward to a healthy peer that does serve it. Same owner
    // selection the request path uses — full-host key first (custom domains
    // are keyed by their whole hostname), then the first label.
    let healthy_ids: std::collections::HashSet<String> = cloud
        .registry
        .nodes()
        .into_iter()
        .filter(|n| n.healthy)
        .map(|n| n.id)
        .collect();
    let host_key = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    let sub = host_key.split('.').next().unwrap_or(&host_key).to_string();
    let owner = {
        let routes = cloud.peer_routes.read();
        routes
            .get(&host_key)
            .into_iter()
            .chain(routes.get(&sub))
            .flatten()
            .find(|r| r.healthy && healthy_ids.contains(&r.node_id))
            .map(|r| r.node_id.clone())
    };
    let Some(node_id) = owner else { return };
    let addr = cloud
        .registry
        .nodes()
        .into_iter()
        .find(|n| n.id == node_id && !n.is_self)
        .and_then(|n| n.iroh_addr.clone());
    let Some(addr) = addr else { return };
    let body = serde_json::json!({ "host": host }).to_string();
    FORWARDED.fetch_add(1, Ordering::Relaxed);
    // Fire and forget: a hint is an optimization, never a dependency, and the
    // owner applies its own gates on arrival.
    let _ = crate::gossip::request_to(
        cloud,
        &node_id,
        &addr,
        hive_p2p::GOSSIP_POST,
        "/v1/warm-hint",
        body.as_bytes(),
        3,
    )
    .await;
}

/// Handle a hint that arrived over the mesh (see the `/v1/warm-hint` gossip
/// arm). This node was selected as a host that serves the deployment, so warm
/// locally and never re-forward.
pub fn handle_remote(cloud: Arc<CloudState>, host: String) {
    if !should_warm(&host) {
        DUPES.fetch_add(1, Ordering::Relaxed);
        return;
    }
    crate::bulkhead::spawn(async move {
        match cloud.gw.warm_host(&host).await {
            fluid_gateway::WarmHost::Started => {
                tracing::info!(host = %host, "prewarm: started an instance from a mesh hint");
            }
            other => {
                tracing::debug!(host = %host, outcome = ?other, "prewarm: mesh hint needed no start");
            }
        }
    });
}
