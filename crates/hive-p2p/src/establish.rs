//! Connection-ESTABLISHMENT liveness for the hive-p2p endpoint: the inbound
//! budgets, the accept deadline's bookkeeping, and the counters that say
//! whether this endpoint can still form a NEW connection in either direction.
//!
//! The failure this exists for (fc-virginia 2026-09-12 and 09-18, fc-sanjose
//! 2026-09-24): iroh's layer above noq stops completing connection setup —
//! the last outbound trunk opens, `net_report` stops, and every inbound
//! connection that finishes its handshake then waits forever in iroh's
//! `register_connection`. Warm trunks keep serving, so every peer-count
//! signal stays up. The accept task used to hold its budget permit across
//! that unbounded await, so the budget filled with permits of connections
//! that no longer existed and the endpoint answered CONNECTION_REFUSED to
//! the whole fleet for hours (335,055 refusals on fc-sanjose in one day).
//! [`crate::serve_tunnels_full`] now bounds the accept, and everything it and
//! [`crate::PeerPool`] observe about establishment is recorded here, so the
//! wedge is a number ([`establish_stats`]) instead of a journal archaeology
//! session, and `hive-cloud`'s meshwatch can act on it.
//!
//! Counters are process-global like `VERIFY_STATS`/`PQ_KEX_STATS`: one
//! endpoint per process serves the fleet ALPN, and the stats must be
//! readable without threading a handle through every caller.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::BrowserCounts;

/// One establishment event. The discriminant indexes the counter arrays.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    /// A real outbound connect attempt (cached hint or fresh discovery) —
    /// never a reused trunk, never a dial skipped by a backoff window.
    OutboundAttempt = 0,
    /// An outbound connect completed its handshake (a FRESH connection).
    OutboundEstablished = 1,
    /// A peer refused our dial (CONNECTION_REFUSED before the handshake, or
    /// the post-handshake overload close).
    OutboundRefused = 2,
    /// The accept loop was handed an `Incoming` (any Initial, any sender).
    InboundInitial = 3,
    /// An accepted connection completed handshake AND iroh registration.
    InboundEstablished = 4,
    /// An accept exceeded its deadline and was closed. Any sender can cause
    /// one (a handshake kept alive past the deadline), so on its own it is
    /// never restart evidence.
    AcceptStuck = 5,
    /// This endpoint refused an inbound connection (a full budget or the
    /// per-endpoint cap).
    Refused = 6,
}

const EVENTS: usize = 7;
/// Ring granularity and horizon: 10 s buckets, one hour back — long enough
/// for any window meshwatch is configured with.
const BUCKET_SECS: u64 = 10;
const HORIZON_BUCKETS: u64 = 360;

struct Log {
    /// `(bucket epoch, per-event counts)`, oldest first.
    buckets: VecDeque<(u64, [u64; EVENTS])>,
    totals: [u64; EVENTS],
    last_ms: [u64; EVENTS],
    /// Accepts dropped at the deadline since the last successful accept.
    stuck_since_inbound_ok: u64,
}

static LOG: Mutex<Log> = Mutex::new(Log {
    buckets: VecDeque::new(),
    totals: [0; EVENTS],
    last_ms: [0; EVENTS],
    stuck_since_inbound_ok: 0,
});

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Record one event. Pure in-memory arithmetic under a short lock; called from
/// the accept loop and the dial path, never across an await.
pub(crate) fn note(event: Event) {
    let now = now_ms();
    let epoch = now / 1000 / BUCKET_SECS;
    let i = event as usize;
    let mut log = lock(&LOG);
    // A clock that stepped backwards folds into the newest bucket rather than
    // breaking the ring's ordering.
    if log.buckets.back().is_none_or(|(e, _)| *e < epoch) {
        log.buckets.push_back((epoch, [0; EVENTS]));
    }
    while log
        .buckets
        .front()
        .is_some_and(|(e, _)| e + HORIZON_BUCKETS <= epoch)
    {
        log.buckets.pop_front();
    }
    if let Some((_, counts)) = log.buckets.back_mut() {
        counts[i] += 1;
    }
    log.totals[i] += 1;
    log.last_ms[i] = now;
    match event {
        Event::AcceptStuck => log.stuck_since_inbound_ok += 1,
        Event::InboundEstablished => log.stuck_since_inbound_ok = 0,
        _ => {}
    }
}

// ---- in-flight accepts ---------------------------------------------------

static INFLIGHT: Mutex<BTreeMap<u64, Instant>> = Mutex::new(BTreeMap::new());
static INFLIGHT_SEQ: AtomicU64 = AtomicU64::new(0);

/// One accept in progress. The entry leaves the table on EVERY exit path
/// (success, error, the deadline, runtime cancellation) because it is removed
/// in `Drop` — the rule that decides whether `accept_inflight_oldest_ms` can
/// be believed.
pub(crate) struct InflightAccept(u64);

impl InflightAccept {
    pub(crate) fn start() -> Self {
        let id = INFLIGHT_SEQ.fetch_add(1, Ordering::Relaxed);
        lock(&INFLIGHT).insert(id, Instant::now());
        Self(id)
    }
}

impl Drop for InflightAccept {
    fn drop(&mut self) {
        lock(&INFLIGHT).remove(&self.0);
    }
}

// ---- inbound budgets -----------------------------------------------------

/// Which inbound budget a connection is charged to. Before the handshake it
/// is what the ClientHello's ALPN list says when the first Initial carries a
/// parseable one; a ClientHello that does not fit one packet (every hybrid
/// X25519MLKEM768 dial: the 1216-byte key share cannot) is `Pending` until
/// the negotiated ALPN decides after the handshake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnClass {
    Fleet,
    Browser,
    Pending,
}

impl ConnClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Fleet => "fleet",
            Self::Browser => "browser",
            Self::Pending => "pending",
        }
    }
}

/// One semaphore-backed budget; `limit == 0` means uncapped (no semaphore).
#[derive(Clone)]
pub(crate) struct Budget {
    sem: Option<Arc<tokio::sync::Semaphore>>,
    limit: usize,
}

impl Budget {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            sem: (limit > 0).then(|| Arc::new(tokio::sync::Semaphore::new(limit))),
            limit,
        }
    }

    /// Never waits: an exhausted budget is a refusal, not a queue (awaiting a
    /// semaphore in the single accept loop head-of-line blocks every later
    /// connection, fleet trunks included).
    pub(crate) fn try_acquire(
        &self,
    ) -> Result<Option<tokio::sync::OwnedSemaphorePermit>, tokio::sync::TryAcquireError> {
        match &self.sem {
            None => Ok(None),
            Some(sem) => sem.clone().try_acquire_owned().map(Some),
        }
    }

    fn in_use(&self) -> usize {
        self.sem
            .as_ref()
            .map(|sem| self.limit.saturating_sub(sem.available_permits()))
            .unwrap_or(0)
    }

    fn limit(&self) -> Option<usize> {
        self.sem.as_ref().map(|_| self.limit)
    }
}

/// Every inbound budget of the fleet endpoint, plus the per-endpoint
/// connection counts the admission caps are checked against.
#[derive(Clone)]
pub(crate) struct InboundBudgets {
    pub(crate) fleet: Budget,
    pub(crate) browser: Budget,
    pub(crate) pending: Budget,
    pub(crate) fleet_peers: BrowserCounts,
    pub(crate) browser_peers: BrowserCounts,
}

impl InboundBudgets {
    pub(crate) fn for_class(&self, class: ConnClass) -> &Budget {
        match class {
            ConnClass::Fleet => &self.fleet,
            ConnClass::Browser => &self.browser,
            ConnClass::Pending => &self.pending,
        }
    }
}

static INBOUND: Mutex<Option<InboundBudgets>> = Mutex::new(None);

/// Publish the serving loop's budgets for [`establish_stats`].
pub(crate) fn register_inbound(budgets: &InboundBudgets) {
    *lock(&INBOUND) = Some(budgets.clone());
}

// ---- rate-limited warnings -------------------------------------------------

/// Admits one log line per interval and counts what it suppressed, so a
/// refusal storm (9/s on fc-sanjose) is a line every few seconds carrying a
/// count instead of a journal flood.
pub(crate) struct WarnGate {
    last_ms: AtomicU64,
    suppressed: AtomicU64,
}

impl WarnGate {
    pub(crate) const fn new() -> Self {
        Self {
            last_ms: AtomicU64::new(0),
            suppressed: AtomicU64::new(0),
        }
    }

    /// `Some(suppressed_since_last)` when a line is due, else `None`.
    pub(crate) fn admit(&self, every: Duration) -> Option<u64> {
        let now = now_ms();
        let last = self.last_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= every.as_millis() as u64
            && self
                .last_ms
                .compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            return Some(self.suppressed.swap(0, Ordering::Relaxed));
        }
        self.suppressed.fetch_add(1, Ordering::Relaxed);
        None
    }
}

// ---- connect-error classification ---------------------------------------

/// What a failed connect says about the PEER, as far as the dial path cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectFailure {
    /// An endpoint answered `CONNECTION_REFUSED`: "not now". It precedes the
    /// handshake and names no identity, so through a direct address it is
    /// only the dialed peer's answer once an identity-routed path confirms it
    /// (`PeerPool::acquire`). Re-dialing a confirmed refusal at once is
    /// exactly the storm that hammered the wedged leader.
    Refused,
    /// The address answered with a DIFFERENT identity than the one dialed
    /// (`invalid peer certificate: UnknownIssuer` — iroh's verifier compares
    /// the presented key to the dialed `EndpointId`): the hint's IP addresses
    /// belong to someone else, and every dial through them fails the same way.
    IdentityMismatch,
    Other,
}

/// Classify by the error's rendered chain. iroh's error types are
/// `transparent` stacks whose `source()` skips levels, so the text is the one
/// stable contract (the same strings the journal lines carry).
pub(crate) fn classify_connect_error(error: &(dyn std::error::Error + 'static)) -> ConnectFailure {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(s) = source {
        text.push_str(" | ");
        text.push_str(&s.to_string());
        source = s.source();
    }
    if text.contains("refused to accept a new connection") {
        ConnectFailure::Refused
    } else if text.contains("UnknownIssuer") {
        ConnectFailure::IdentityMismatch
    } else {
        ConnectFailure::Other
    }
}

// ---- the exported view -----------------------------------------------------

/// Per-class inbound numbers.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct ClassCounts {
    pub fleet: usize,
    pub browser: usize,
    pub pending: usize,
}

/// Per-class budget ceilings; `None` = uncapped.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct ClassLimits {
    pub fleet: Option<usize>,
    pub browser: Option<usize>,
    pub pending: Option<usize>,
}

/// One remote endpoint's share of an inbound budget. `peer` is the short
/// (10-hex) form iroh's own log lines use.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PeerBudget {
    pub peer: String,
    pub class: &'static str,
    pub conns: usize,
}

/// Connection-establishment liveness of this process's fleet endpoint.
/// Timestamps are epoch ms (`None` = never since boot); `*_window` counts
/// cover the last `window_secs` (10 s bucket granularity, 1 h max).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct EstablishStats {
    pub window_secs: u64,
    pub accept_deadline_ms: u64,
    /// Accepts dropped at the deadline since boot, and when the last one was.
    pub accept_stuck: u64,
    pub accept_stuck_last_ms: Option<u64>,
    /// Accepts dropped at the deadline since the last SUCCESSFUL accept.
    pub accept_stuck_since_inbound_ok: u64,
    pub accept_inflight: usize,
    pub accept_inflight_oldest_ms: Option<u64>,
    pub last_inbound_established_ms: Option<u64>,
    pub last_outbound_established_ms: Option<u64>,
    pub last_inbound_initial_ms: Option<u64>,
    pub last_outbound_attempt_ms: Option<u64>,
    pub outbound_attempts_window: u64,
    pub outbound_established_window: u64,
    pub outbound_refused_window: u64,
    pub inbound_initials_window: u64,
    pub inbound_established_window: u64,
    pub accept_stuck_window: u64,
    pub refused_window: u64,
    /// Inbound connections THIS endpoint refused since boot.
    pub refused_total: u64,
    /// Our dials a peer refused since boot.
    pub outbound_refused_total: u64,
    pub budget_in_use: ClassCounts,
    pub budget_limit: ClassLimits,
    /// Top 10 remote endpoints by concurrently admitted connections.
    pub budget_by_peer: Vec<PeerBudget>,
}

/// Snapshot the establishment counters over the last `window_secs`.
/// Live connections this process holds to `peer_id` (matched against the
/// [`PeerBudget::peer`] short form / full endpoint id keys).
///
/// Placement uses this to tell "advertises an address" apart from "actually
/// reachable". A NAT'd peer may advertise only an RFC1918 address and no relay,
/// which no remote coordinator can dial — but a peer that dialled OUT to us
/// (which NAT permits) leaves a live trunk we can use in both directions, so the
/// path is real even though the advertised address is not.
pub fn peer_conns(peer_id: &str) -> usize {
    if peer_id.is_empty() {
        return 0;
    }
    if let Some(budgets) = lock(&INBOUND).clone() {
        for counts in [&budgets.fleet_peers, &budgets.browser_peers] {
            for (peer, conns) in lock(counts).clone() {
                if peer_id.starts_with(&peer) || peer.starts_with(peer_id) {
                    return conns;
                }
            }
        }
    }
    0
}

pub fn establish_stats(window_secs: u64) -> EstablishStats {
    let now = now_ms();
    let window_secs = window_secs.clamp(BUCKET_SECS, BUCKET_SECS * HORIZON_BUCKETS);
    let first_epoch = now.saturating_sub(window_secs * 1000) / 1000 / BUCKET_SECS;
    let (window, totals, last_ms, stuck_since_ok) = {
        let log = lock(&LOG);
        let mut window = [0u64; EVENTS];
        for (_, counts) in log.buckets.iter().filter(|(e, _)| *e >= first_epoch) {
            for (w, c) in window.iter_mut().zip(counts.iter()) {
                *w += c;
            }
        }
        (window, log.totals, log.last_ms, log.stuck_since_inbound_ok)
    };
    let at = |event: Event| Some(last_ms[event as usize]).filter(|ms| *ms > 0);
    let (inflight, oldest) = {
        let table = lock(&INFLIGHT);
        let oldest = table
            .values()
            .next()
            .map(|started| started.elapsed().as_millis() as u64);
        (table.len(), oldest)
    };
    let mut stats = EstablishStats {
        window_secs,
        accept_deadline_ms: crate::accept_deadline().as_millis() as u64,
        accept_stuck: totals[Event::AcceptStuck as usize],
        accept_stuck_last_ms: at(Event::AcceptStuck),
        accept_stuck_since_inbound_ok: stuck_since_ok,
        accept_inflight: inflight,
        accept_inflight_oldest_ms: oldest,
        last_inbound_established_ms: at(Event::InboundEstablished),
        last_outbound_established_ms: at(Event::OutboundEstablished),
        last_inbound_initial_ms: at(Event::InboundInitial),
        last_outbound_attempt_ms: at(Event::OutboundAttempt),
        outbound_attempts_window: window[Event::OutboundAttempt as usize],
        outbound_established_window: window[Event::OutboundEstablished as usize],
        outbound_refused_window: window[Event::OutboundRefused as usize],
        inbound_initials_window: window[Event::InboundInitial as usize],
        inbound_established_window: window[Event::InboundEstablished as usize],
        accept_stuck_window: window[Event::AcceptStuck as usize],
        refused_window: window[Event::Refused as usize],
        refused_total: totals[Event::Refused as usize],
        outbound_refused_total: totals[Event::OutboundRefused as usize],
        ..EstablishStats::default()
    };
    if let Some(budgets) = lock(&INBOUND).clone() {
        stats.budget_in_use = ClassCounts {
            fleet: budgets.fleet.in_use(),
            browser: budgets.browser.in_use(),
            pending: budgets.pending.in_use(),
        };
        stats.budget_limit = ClassLimits {
            fleet: budgets.fleet.limit(),
            browser: budgets.browser.limit(),
            pending: budgets.pending.limit(),
        };
        let mut by_peer: Vec<PeerBudget> = Vec::new();
        for (class, counts) in [
            ("fleet", &budgets.fleet_peers),
            ("browser", &budgets.browser_peers),
        ] {
            let counts: HashMap<String, usize> = lock(counts).clone();
            by_peer.extend(counts.into_iter().map(|(peer, conns)| PeerBudget {
                peer: peer.chars().take(10).collect(),
                class,
                conns,
            }));
        }
        by_peer.sort_by(|a, b| b.conns.cmp(&a.conns).then_with(|| a.peer.cmp(&b.peer)));
        by_peer.truncate(10);
        stats.budget_by_peer = by_peer;
    }
    stats
}
