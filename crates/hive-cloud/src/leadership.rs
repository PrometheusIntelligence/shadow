//! Leader-only BACKGROUND jobs: the one question every single-writer job asks.
//!
//! Each job used to carry its own leadership test — `is_control_plane_leader()`
//! here, `control_plane_leader() == node_name` there, a 2-tick window for the
//! billing meter, a raw `control_plane_owner(..)` with the DNS pin for DNS and
//! ACME — so they had different strengths, and the weakest one decided the
//! damage. On 2026-09-24 fc-virginia's per-observer view flapped to "I am the
//! owner" while fc-sanjose kept serving: its self-heal redeployed a live node's
//! projects 1512 times in 10 h, it took DNS leadership beside the real writer,
//! and it metered billing from a stale fork. [`may_act`] is now the ONLY gate
//! those jobs consult, and it is deliberately stricter than the request-path
//! test (`CloudState::is_control_plane_leader`, which only decides where a
//! mutation is served):
//!
//! * **owner** — this node is the job's designated owner. With
//!   `HIVE_CP_OWNER_CHAIN` set that is the STRICT chain owner
//!   (`Cluster::strict_chain_owner`: first chain entry present in the gossip-
//!   fresh set with an identity and a public address; the observer's own
//!   `healthy` flag is never consulted). No qualifying entry = no owner = HOLD
//!   everywhere; the identity election and the `HIVE_CP_LEADER` /
//!   `HIVE_DNS_LEADER_NODE` / `HIVE_BILLING_COORDINATOR_NODE` pins are ignored
//!   while a chain is set. Without a chain, a node with a mesh roster
//!   (`HIVE_EXPECTED_NODE_IDS`, else `HIVE_TRUSTED_NODE_IDS`) HOLDS with an
//!   ERROR — a curated fleet member missing its chain must never elect itself —
//!   and only a roster-less (dev) mesh keeps the pre-chain designations;
//! * **fresh view** — this node has completed at least one gossip round (a
//!   node with no roster has none to wait for). A booting backup's registry is
//!   empty, so before its first round it would name ITSELF the strict owner;
//! * **not isolated** — `mesh_health().isolated` is false;
//! * **voter majority, for a BACKUP owner only** — when this node owns without
//!   being the chain head (`chain[0]`), it must have fresh DIRECT contact
//!   ([`CloudState::direct_contact_ids`]: an outbound dial/trunk success or an
//!   inbound self-report, never relayed gossip) with a strict majority of
//!   `HIVE_CP_VOTERS` (identical fleet-wide, public servers, odd count),
//!   counting itself when it is one. The head acts on presence (design CS-1
//!   1b): a quorum on the head turned "present through relays but short of
//!   direct reach" into a writer nobody replaces — every other node still sees
//!   it present, so none of them owns. `HIVE_CP_VOTERS` unset while a chain is
//!   set logs an ERROR once and drops the quorum term entirely (voters derived
//!   per observer from differing chains are not a quorum);
//! * **tenure** — every precondition above has held CONTINUOUSLY for
//!   [`Job::min_tenure`] (sampled every 3 s by the cluster loop and on every
//!   call; any lapse — ownership, freshness, isolation, quorum — resets it). A
//!   returning or re-quorate owner waits its tenure again, which is the window
//!   in which a backup that acted in its absence sees it present and stops.
//!
//! Each continuous run of OWNERSHIP has a term id ([`Verdict::term`]). A lapse
//! of the other preconditions shorter than [`TERM_LAPSE`] keeps the term — no
//! other node can have taken over in that time — and a longer one starts a new
//! term when they hold again (a backup may have written meanwhile). Callers
//! that do take-over work on "I just became the writer" key it on the term,
//! never on an edge of [`may_act`]: a quorum or isolation blip is not a new
//! tenure.
//!
//! What this gate cannot do: move the writer away from an owner that is present
//! but cannot act (a backup short of a voter majority, an isolated head).
//! Every node still sees it as owner, so the jobs HOLD until it recovers or
//! disappears. Only the signed control-plane lease (CS-3) expires such an
//! owner. A future lease swaps in underneath [`may_act`] without any caller
//! changing. Two leader-coordinated surfaces deliberately do NOT use this gate
//! yet: the cron loop (fires every node's local jobs) and raw-port allocation
//! (request-path, leader-local `raw_ports.json`). Both need a replicated store
//! before a stricter gate can move their writer safely.

use crate::cluster::Cluster;
use crate::state::CloudState;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Every leader-only background job. One variant per job, never per call
/// site; a job's designation follows the store it writes (ACME's DNS-01
/// renewals and its custom-domain HTTP-01 pass write different stores, whose
/// writers differ on a chain-less mesh, so they are two jobs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Job {
    /// `production_deployments::spawn_node_death_reconcile` — redeploys a dead
    /// host's production projects elsewhere.
    NodeDeathRelocate,
    /// `git::spawn_git_poll_reconcile` — polls tracked branches and deploys.
    GitPoll,
    /// `main.rs::spawn_billing_meter_loop` (metering + ledger prune), the
    /// relational mirror's billing section, and the token-mint enterprise
    /// lock (`admin::mint_token`) — one writer for the billing store.
    BillingMeter,
    /// `vercel_dns::spawn_reconciler` — the Vercel DNS writer.
    DnsWrite,
    /// `acme::spawn_acme` — DNS-01 platform bundle renewals (its challenge
    /// TXT records must sit with the DNS writer's orphan sweeper).
    Acme,
    /// `acme::custom_cert_pass` — custom-domain HTTP-01 issuance. Its tokens
    /// live in the `acme_http01` store followers pull from the control-plane
    /// owner, so it sits exactly where that store's writer does.
    AcmeHttp01,
    /// `admin::spawn_domain_verify_loop` — custom-domain verification and
    /// tenant-zone apex pinning.
    DomainVerifyPin,
    /// `push::spawn_push_dispatcher` — web push + SMS delivery, and the fleet
    /// VAPID keypair (`push::ensure_vapid_on_leader`).
    PushDispatch,
    /// `inference::spawn_reconcile`'s leader slice — `HIVE_INFERENCE_URL`
    /// injection into project env.
    InferenceEnv,
    /// `main.rs::spawn_guardian_reap_loop` — deletes departed nodes' snapshot
    /// keys from the replicated store.
    GuardianReap,
    /// `main.rs::spawn_relational_mirror_loop`'s leader section — project and
    /// team backfills into the SQL projection.
    RelationalLeaderWrites,
    /// `browser_admission::snapshot_bytes` / `browser_presence::snapshot_bytes`
    /// expiry (closes live browser endpoints).
    BrowserExpiry,
    /// `listener_audit::spawn` — raises foreign-listener incidents.
    ListenerAudit,
    /// `incidents::spawn_reconciler` — resolves automated incidents whose
    /// observation lapsed, collapses duplicates and bounds the history: the
    /// incident store's one writer.
    IncidentReconcile,
}

impl Job {
    pub const ALL: [Job; 14] = [
        Job::NodeDeathRelocate,
        Job::GitPoll,
        Job::BillingMeter,
        Job::DnsWrite,
        Job::Acme,
        Job::AcmeHttp01,
        Job::DomainVerifyPin,
        Job::PushDispatch,
        Job::InferenceEnv,
        Job::GuardianReap,
        Job::RelationalLeaderWrites,
        Job::BrowserExpiry,
        Job::ListenerAudit,
        Job::IncidentReconcile,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Job::NodeDeathRelocate => "node-death-relocate",
            Job::GitPoll => "git-poll",
            Job::BillingMeter => "billing-meter",
            Job::DnsWrite => "dns-write",
            Job::Acme => "acme",
            Job::AcmeHttp01 => "acme-http01",
            Job::DomainVerifyPin => "domain-verify-pin",
            Job::PushDispatch => "push-dispatch",
            Job::InferenceEnv => "inference-env",
            Job::GuardianReap => "guardian-reap",
            Job::RelationalLeaderWrites => "relational-leader-writes",
            Job::BrowserExpiry => "browser-expiry",
            Job::ListenerAudit => "listener-audit",
            Job::IncidentReconcile => "incident-reconcile",
        }
    }

    /// How long every precondition must have held, continuously, before the
    /// job may act. Sized to the damage one wrong tenure does: a relocation
    /// rebuilds a live tenant elsewhere, a billing tick charges a delta, an
    /// ACME order spends Let's Encrypt's duplicate-certificate budget.
    pub fn min_tenure(self) -> Duration {
        Duration::from_secs(match self {
            Job::NodeDeathRelocate => 600,
            Job::Acme | Job::AcmeHttp01 => 300,
            Job::BillingMeter => 120,
            Job::DnsWrite | Job::GitPoll => 60,
            _ => 30,
        })
    }
}

/// One evaluation of the gate.
#[derive(Clone, Debug)]
pub struct Verdict {
    pub allowed: bool,
    /// The job's designated owner in this node's view (`None` = HOLD).
    pub owner: Option<String>,
    /// Id of this node's current continuous OWNERSHIP run for the job (`None`
    /// when it is not the owner). Stable across blips of the other
    /// preconditions; a new value means a new tenure.
    pub term: Option<u64>,
    pub reason: String,
}

#[derive(Default)]
struct JobState {
    /// This node's current ownership run, while it is the owner.
    term: Option<u64>,
    /// Since when every precondition except tenure has held continuously.
    clear_since: Option<Instant>,
    /// Since when the preconditions have NOT held while this node owned.
    lapse_since: Option<Instant>,
    last_allowed: Option<bool>,
    last_log: Option<Instant>,
    /// Transitions swallowed by the log rate limit since the last line.
    suppressed: u32,
}

/// Minimum spacing between two transition lines for one job; transitions in
/// between are counted and reported on the next line.
const TRANSITION_LOG_EVERY: Duration = Duration::from_secs(60);

/// A lapse this long (or longer) in an owner's ability to act ends its term:
/// the registry's gossip freshness window, i.e. the earliest any other node can
/// have seen this one gone and become the owner in its own view. Shorter
/// lapses cannot have handed the writer to anyone else. Also the promotion
/// reconcile's re-run bound (`main.rs::spawn_promotion_reconcile_loop`).
pub const TERM_LAPSE: Duration = Duration::from_secs(30);

static NEXT_TERM: AtomicU64 = AtomicU64::new(1);

fn states() -> &'static Mutex<HashMap<Job, JobState>> {
    static STATES: OnceLock<Mutex<HashMap<Job, JobState>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_list(key: &str) -> Vec<String> {
    env_nonempty(key)
        .map(|v| {
            v.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Everything the gate reads, gathered once per evaluation (and once per
/// cluster-loop sample for all jobs).
struct View {
    chain: Vec<String>,
    nodes: Vec<hive_edge::NodeInfo>,
    /// At least one gossip round completed (or no roster to gossip with).
    fresh: bool,
    isolated: bool,
    voters: Vec<String>,
    /// Voters this node has fresh direct contact with (itself included).
    reached: usize,
}

impl View {
    fn gather(cloud: &CloudState) -> View {
        let chain = Cluster::owner_chain_from_env();
        let nodes = cloud.registry.nodes();
        let voters = env_list("HIVE_CP_VOTERS");
        let reached = voter_reach(cloud, &voters, &nodes);
        View {
            fresh: cloud.expected_peer_ids.is_empty() || cloud.last_gossip_ms() > 0,
            isolated: cloud.mesh_health().isolated,
            chain,
            nodes,
            voters,
            reached,
        }
    }

    /// Whether `me` owning needs a voter majority: only a BACKUP owner (not the
    /// chain head) and only when voters are configured.
    fn quorum_required(&self, me: &str) -> bool {
        !self.voters.is_empty() && self.chain.first().map(String::as_str) != Some(me)
    }

    fn has_majority(&self) -> bool {
        self.reached * 2 > self.voters.len()
    }
}

/// The node `job` is designated to run on, from this node's view. With a chain
/// configured every job sits on the strict chain owner (or on nobody — HOLD).
/// Without one, a node with a mesh roster HOLDS (ERROR once); a roster-less
/// node keeps each job's pre-chain designation: the identity election with the
/// `HIVE_CP_LEADER` pin (this node when nothing is electable), the
/// `HIVE_DNS_LEADER_NODE` pin first for DNS/DNS-01, and the hard
/// `HIVE_BILLING_COORDINATOR_NODE` pin for billing. Also the routing answer for
/// reads that must reach the job's writer (`admin::billing_authority_node`).
pub fn job_owner(cloud: &CloudState, job: Job) -> Option<String> {
    job_owner_in(
        cloud,
        job,
        &Cluster::owner_chain_from_env(),
        &cloud.registry.nodes(),
    )
}

fn job_owner_in(
    cloud: &CloudState,
    job: Job,
    chain: &[String],
    nodes: &[hive_edge::NodeInfo],
) -> Option<String> {
    if !chain.is_empty() {
        return Cluster::strict_chain_owner(chain, nodes);
    }
    if !cloud.expected_peer_ids.is_empty() {
        static WARNED: AtomicBool = AtomicBool::new(false);
        if !WARNED.swap(true, Ordering::Relaxed) {
            tracing::error!(
                roster = cloud.expected_peer_ids.len(),
                "HIVE_CP_OWNER_CHAIN is unset on a node with a mesh roster -- every leader-only \
                 job HOLDS on this node (a fleet member missing its chain must never elect \
                 itself). Set the fleet's HIVE_CP_OWNER_CHAIN."
            );
        }
        return None;
    }
    let cp_pref = env_nonempty("HIVE_CP_LEADER");
    match job {
        Job::BillingMeter => env_nonempty("HIVE_BILLING_COORDINATOR_NODE").or_else(|| {
            Cluster::billing_leader_with_pref(cp_pref.as_deref(), nodes)
                .or_else(|| Some(cloud.node_name.clone()))
        }),
        Job::DnsWrite | Job::Acme => {
            let pref = env_nonempty("HIVE_DNS_LEADER_NODE").or(cp_pref);
            Cluster::billing_leader_with_pref(pref.as_deref(), nodes)
        }
        _ => Cluster::billing_leader_with_pref(cp_pref.as_deref(), nodes)
            .or_else(|| Some(cloud.node_name.clone())),
    }
}

/// Voters this node has fresh DIRECT contact with, itself included when it is
/// one.
fn voter_reach(cloud: &CloudState, voters: &[String], nodes: &[hive_edge::NodeInfo]) -> usize {
    if voters.is_empty() {
        return 0;
    }
    let contact = cloud.direct_contact_ids();
    voters
        .iter()
        .filter(|voter| {
            if **voter == cloud.node_name {
                return true;
            }
            nodes
                .iter()
                .find(|n| &n.name == *voter)
                .and_then(|n| n.peer_id.clone())
                .or_else(|| {
                    cloud
                        .registry
                        .peer_identity(voter)
                        .and_then(|identity| identity.endpoint_id)
                })
                .and_then(|id| id.parse::<iroh::EndpointId>().ok())
                .is_some_and(|id| contact.contains(&id))
        })
        .count()
}

/// Record `job`'s continuity: the ownership term (while owner; renewed after a
/// lapse of at least [`TERM_LAPSE`]) and how long every other precondition has
/// held with it.
fn track(job: Job, is_owner: bool, clear: bool) -> (Option<u64>, Option<Duration>) {
    let now = Instant::now();
    let mut map = states().lock();
    let st = map.entry(job).or_default();
    if !is_owner {
        st.term = None;
        st.clear_since = None;
        st.lapse_since = None;
        return (None, None);
    }
    let new_term = || NEXT_TERM.fetch_add(1, Ordering::Relaxed);
    let mut term = *st.term.get_or_insert_with(new_term);
    if !clear {
        st.clear_since = None;
        st.lapse_since.get_or_insert(now);
        return (Some(term), None);
    }
    if st
        .lapse_since
        .take()
        .is_some_and(|since| now.saturating_duration_since(since) >= TERM_LAPSE)
    {
        term = new_term();
        st.term = Some(term);
    }
    let since = *st.clear_since.get_or_insert(now);
    (Some(term), Some(now.saturating_duration_since(since)))
}

fn evaluate_in(cloud: &CloudState, view: &View, job: Job) -> Verdict {
    let me = cloud.node_name.as_str();
    let owner = job_owner_in(cloud, job, &view.chain, &view.nodes);
    let is_owner = owner.as_deref() == Some(me);
    let blocker = if !is_owner {
        Some(match &owner {
            None => "no owner resolvable in this node's view -- HOLD".to_string(),
            Some(other) => format!("owner is {other}"),
        })
    } else if !view.fresh {
        Some("no gossip round has completed since boot -- this node's view is not fresh".into())
    } else if view.isolated {
        Some("this node is mesh-isolated (no direct peer reachable)".into())
    } else if view.quorum_required(me) && !view.has_majority() {
        Some(format!(
            "backup owner (chain head is {}) without a voter majority: direct contact with \
             {}/{} voters {:?}",
            view.chain.first().map(String::as_str).unwrap_or("-"),
            view.reached,
            view.voters.len(),
            view.voters
        ))
    } else {
        None
    };
    let (term, held) = track(job, is_owner, blocker.is_none());
    let verdict = |allowed: bool, reason: String| Verdict {
        allowed,
        owner: owner.clone(),
        term,
        reason,
    };
    if let Some(reason) = blocker {
        return verdict(false, reason);
    }
    let held = held.unwrap_or_default();
    let need = job.min_tenure();
    if held < need {
        return verdict(
            false,
            format!(
                "tenure {}s < required {}s (owner, fresh, not isolated{} continuously)",
                held.as_secs(),
                need.as_secs(),
                if view.quorum_required(me) { ", voter majority" } else { "" }
            ),
        );
    }
    verdict(
        true,
        format!(
            "owner for {}s, direct contact with {}/{} voters",
            held.as_secs(),
            view.reached,
            view.voters.len()
        ),
    )
}

/// Evaluate the gate (sampling the job's continuity) without logging a
/// transition. [`check`] / [`may_act`] are the job path.
pub fn evaluate(cloud: &CloudState, job: Job) -> Verdict {
    evaluate_in(cloud, &View::gather(cloud), job)
}

/// THE gate for a leader-only background job, with the full verdict (for
/// callers that key take-over work on [`Verdict::term`]). Logs each change of
/// the answer per job at INFO (rate-limited to one line a minute, swallowed
/// flips counted on the next line).
pub fn check(cloud: &CloudState, job: Job) -> Verdict {
    let verdict = evaluate(cloud, job);
    note(job, &verdict);
    verdict
}

/// THE gate for a leader-only background job — see the module doc.
pub fn may_act(cloud: &CloudState, job: Job) -> bool {
    check(cloud, job).allowed
}

/// Whether this node's view is fresh enough to count ABSENCE as evidence:
/// a gossip round completed, not isolated, and — when voters are configured —
/// direct contact with a voter majority. A node that cannot hear the fleet
/// sees everyone as gone; nothing may start an absence clock from that view.
pub fn view_is_fresh(cloud: &CloudState) -> bool {
    let view = View::gather(cloud);
    view.fresh && !view.isolated && (view.voters.is_empty() || view.has_majority())
}

fn note(job: Job, verdict: &Verdict) {
    let now = Instant::now();
    let mut map = states().lock();
    let st = map.entry(job).or_default();
    let changed = st.last_allowed != Some(verdict.allowed);
    if changed {
        st.last_allowed = Some(verdict.allowed);
        st.suppressed = st.suppressed.saturating_add(1);
    }
    let due = st
        .last_log
        .is_none_or(|at| now.saturating_duration_since(at) >= TRANSITION_LOG_EVERY);
    if st.suppressed == 0 || !due {
        return;
    }
    tracing::info!(
        job = job.name(),
        acting = verdict.allowed,
        owner = verdict.owner.as_deref().unwrap_or("none"),
        term = verdict.term.unwrap_or(0),
        reason = %verdict.reason,
        transitions = st.suppressed,
        "leader-only job {}",
        if verdict.allowed { "acquired" } else { "not acting" }
    );
    st.last_log = Some(now);
    st.suppressed = 0;
}

/// Sample every job's continuity. Called by the cluster loop every 3 s so a
/// job that asks only every few minutes still sees an unbroken tenure, never a
/// stale one. Also logs, once, the effective voter set and its source, and
/// warns when pins are set that a configured chain makes inert.
pub fn observe(cloud: &CloudState) {
    let view = View::gather(cloud);
    static ANNOUNCED: AtomicBool = AtomicBool::new(false);
    if !ANNOUNCED.swap(true, Ordering::Relaxed) {
        announce(&view);
    }
    for job in Job::ALL {
        evaluate_in(cloud, &view, job);
    }
}

fn announce(view: &View) {
    if view.chain.is_empty() {
        tracing::info!(
            voters = ?view.voters,
            "leader-only job gate: no HIVE_CP_OWNER_CHAIN -- chain-less designations (roster-less \
             meshes only)"
        );
        return;
    }
    let ignored: Vec<&str> = [
        "HIVE_CP_LEADER",
        "HIVE_DNS_LEADER_NODE",
        "HIVE_BILLING_COORDINATOR_NODE",
    ]
    .into_iter()
    .filter(|key| env_nonempty(key).is_some())
    .collect();
    if !ignored.is_empty() {
        tracing::warn!(
            ?ignored,
            chain = ?view.chain,
            "leader pins are IGNORED while HIVE_CP_OWNER_CHAIN is set -- every leader-only \
             job sits on the strict chain owner"
        );
    }
    if view.voters.is_empty() {
        tracing::error!(
            chain = ?view.chain,
            "HIVE_CP_VOTERS is unset while HIVE_CP_OWNER_CHAIN is set -- a BACKUP owner takes \
             over leader-only jobs on gossip presence alone, without proving a voter majority. \
             Set HIVE_CP_VOTERS (public servers, odd count) identically on every node."
        );
    } else {
        tracing::info!(
            voters = ?view.voters,
            source = "HIVE_CP_VOTERS",
            chain = ?view.chain,
            head = view.chain.first().map(String::as_str).unwrap_or("-"),
            "leader-only job gate: voter set (a backup owner needs direct contact with a \
             majority; the chain head acts on presence)"
        );
    }
}
