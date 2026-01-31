//! C2 — Durable Work Queue, backend-neutral part: the lease reaper and the round-robin claim helper.
//!
//! The queue itself is a seam of the storage port (`WorkQueue`, `QueueMaintenance` on
//! `crate::types::ports::StoragePort`); the SurrealDB implementation is
//! `crate::store::surreal::StoreWorkQueue`. Nothing here names a storage engine: the reaper drives
//! `QueueMaintenance::reap_expired_leases` on whichever storage it is given.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::Value as Json;
use tokio_util::sync::CancellationToken;

use crate::types::job::{JobKind, WorkJob};
use crate::types::ports::{QueueError, StoragePort, WorkQueue};

/// The payload key the write pipeline sets on a job it has redacted. Only the pipeline writes it: the
/// Service builds a job's payload field by field, so a request cannot set it.
pub(crate) const REDACTED_KEY: &str = "redacted";

/// What a dead-lettered job may keep (SA-QUEUE-03). An `ExtractFact` payload not marked redacted loses
/// its text, whoever failed it and however, so no path parks raw content; what is left still lets an
/// erasure find the job. The other kinds carry ids only.
// spec: phase-1-analysis#SA-QUEUE-03
pub(crate) fn parked_payload(kind: JobKind, payload: &Json) -> Json {
    if kind != JobKind::ExtractFact || payload.get(REDACTED_KEY) == Some(&Json::Bool(true)) {
        return payload.clone();
    }
    let mut out = payload.as_object().cloned().unwrap_or_default();
    for field in ["content", "entities", "links", "source"] {
        out.remove(field);
    }
    out.insert("withheld".into(), Json::Bool(true));
    Json::Object(out)
}

/// Lease-reaper: a long-running task that periodically reverts expired leases to `Pending` across all
/// tenant namespaces so a crashed worker's job is reclaimed. Runs until the cancellation token fires.
pub struct LeaseReaper {
    storage: Arc<dyn StoragePort>,
    /// Reaper sweep cadence (`MEMORY_QUEUE_REAPER_SECS`, default 30 s).
    interval: Duration,
}

impl LeaseReaper {
    /// Build a reaper over the storage whose queue it sweeps.
    pub fn new(storage: Arc<dyn StoragePort>, interval: Duration) -> Self {
        Self { storage, interval }
    }

    /// One sweep: every expired lease returns to `pending`. Returns the total count reclaimed. A
    /// per-namespace failure is the storage's to log and skip; only a whole-sweep failure surfaces.
    pub async fn reap_once(&self) -> Result<u64, QueueError> {
        self.storage.reap_expired_leases(Utc::now()).await
    }

    /// Sweep loop: every `interval`, run `reap_once` (swallowing per-namespace failures, already
    /// logged); stop when the cancellation token fires.
    pub async fn run(self, cancel: CancellationToken) {
        let mut ticker = tokio::time::interval(self.interval);
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(e) = self.reap_once().await {
                        tracing::warn!(target: "memory", error = %e, "queue.reap.sweep_failed");
                    }
                }
                _ = cancel.cancelled() => return,
            }
        }
    }
}

/// Claim one job across many tenants, round-robin.
///
/// `claim` takes exactly one tenant (RISK-016), so a consumer serving every tenant on the instance has
/// to choose an order. Round-robin from a rotating `cursor` rather than always starting at index 0:
/// with a fixed start, a busy first tenant would be claimed from on every tick and a tenant later in
/// the list could starve indefinitely. The cursor advances past the tenant that yielded, so the next
/// tick begins after it.
///
/// A per-tenant claim error is logged and skipped rather than aborting the round — one unreachable
/// namespace must not stall work for every other tenant, which is the same stance `reap_once` takes.
///
/// Cost is one round-trip per empty tenant per tick, so this is shaped for the tens of namespaces
/// ADR-011 implies rather than for tens of thousands. RFC 03's Stage 2 changes the topology and should
/// revisit this together with the maintenance sweep — they are the same question.
pub async fn claim_across_tenants(
    queue: &dyn WorkQueue,
    tenants: &[String],
    kinds: &[JobKind],
    lease: Duration,
    cursor: &mut usize,
) -> Option<WorkJob> {
    if tenants.is_empty() {
        return None;
    }
    for offset in 0..tenants.len() {
        let idx = (*cursor + offset) % tenants.len();
        let tenant = &tenants[idx];
        match queue.claim(tenant, kinds, lease).await {
            Ok(Some(job)) => {
                *cursor = (idx + 1) % tenants.len();
                return Some(job);
            }
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(
                    target: "memory",
                    tenant = %tenant,
                    error = %e,
                    "queue.claim.tenant_failed"
                );
                continue;
            }
        }
    }
    // Every tenant was empty this round; start the next one where this one began so the rotation does
    // not drift on idle ticks.
    None
}
