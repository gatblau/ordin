//! The background tasks of *Internal Logic* §1 step 6: the memory component's three workers, the
//! lease reaper, the reconciler (§6), the retention sweep (§7) and the name backfill, each under the
//! shutdown token. §13: on shutdown the write pipeline finishes or abandons its job within 5 s.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use chrono::Duration as ChronoDuration;
use memory::queue::LeaseReaper;
use ordin_types::time::Timestamp;
use ordin_types::MemoryFactId;
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::{REAPER_SECS, SWEEP_SECS, TENANT};
use crate::db::q;
use crate::error::MemoryError;
use crate::model::FactState;
use crate::service::Inner;
use crate::store::{self, RefRow};

/// §6 step 1.
const RECONCILE_BATCH: i64 = 200;
/// §6 step 2: a write not stored this long after it was written was quarantined, rejected as noise
/// or failed.
const NOT_KEPT_AFTER_SECS: i64 = 600;
/// §7 step 1.
const SWEEP_BATCH: i64 = 500;
/// §7 (addition).
const RECALL_LOG_BATCH: i64 = 5_000;
/// Model idle check.
const UNLOAD_CHECK: Duration = Duration::from_secs(60);

fn task(
    tasks: &mut Vec<JoinHandle<()>>,
    name: &'static str,
    work: impl std::future::Future<Output = ()> + Send + 'static,
) {
    tasks.push(tokio::spawn(async move {
        work.await;
        tracing::info!(target: "ordin::memory", task = name, "memory.task_stopped");
    }));
}

/// The write pipeline's task, started once per service: at start when every model file is present,
/// or later by `recheck_models` (MEM-MD-5). `None` when it already runs.
pub(crate) fn start_write_pipeline(inner: &Arc<Inner>) -> Option<JoinHandle<()>> {
    if inner.pipeline_running.swap(true, Ordering::SeqCst) {
        return None;
    }
    let pipeline = inner.pipeline.clone();
    let token = inner.shutdown.clone();
    let mut tasks = Vec::new();
    task(&mut tasks, "write_pipeline", async move {
        pipeline.run(token).await
    });
    tasks.pop()
}

/// Runs `step` at each `every` until the token is cancelled.
async fn every<F, Fut>(every: Duration, cancel: CancellationToken, mut step: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut ticker = tokio::time::interval(every);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = ticker.tick() => step().await,
        }
    }
}

pub(crate) fn spawn(inner: &Arc<Inner>) {
    let cancel = inner.shutdown.clone();
    let mut tasks = Vec::new();

    // §1 step 3: with a model missing or failing its hash, nothing embeds until a start or
    // `recheck_models` finds every file (MEM-MD-2).
    if !inner.models_missing.load(Ordering::Relaxed) {
        if let Some(handle) = start_write_pipeline(inner) {
            tasks.push(handle);
        }
    }

    // The maintenance cycle closes dead letters at the end of `run_cycle`, so this task is what
    // makes them expire.
    let maintenance = inner.maintenance.clone();
    let token = cancel.clone();
    task(&mut tasks, "maintenance_scheduler", async move {
        maintenance.run_scheduler(token).await
    });

    let maintenance = inner.maintenance.clone();
    let token = cancel.clone();
    task(&mut tasks, "maintenance_consumer", async move {
        maintenance.run_consumer(token).await
    });

    let reaper = LeaseReaper::new(
        inner.storage.clone(),
        Duration::from_secs(u64::from(REAPER_SECS)),
    );
    let token = cancel.clone();
    task(&mut tasks, "lease_reaper", async move {
        reaper.run(token).await
    });

    let this = inner.clone();
    let token = cancel.clone();
    let period = inner.reconcile_every;
    task(&mut tasks, "reconciler", async move {
        every(period, token, || {
            let this = this.clone();
            async move {
                if let Err(error) = this.reconcile_once().await {
                    tracing::warn!(target: "ordin::memory", code = error.code(), "memory.sweep_error");
                }
            }
        })
        .await
    });

    let this = inner.clone();
    let token = cancel.clone();
    task(&mut tasks, "retention_sweep", async move {
        every(Duration::from_secs(SWEEP_SECS), token, || {
            let this = this.clone();
            async move { this.sweep_once().await }
        })
        .await
    });

    let this = inner.clone();
    let token = cancel.clone();
    task(&mut tasks, "name_backfill", async move {
        this.backfill_names(token).await
    });

    if let Some(models) = inner.models.clone() {
        let token = cancel.clone();
        task(&mut tasks, "model_unload", async move {
            every(UNLOAD_CHECK, token, || {
                let models = models.clone();
                async move { models.unload(false) }
            })
            .await
        });
    }

    let models = inner.models.clone();
    tokio::spawn(async move {
        cancel.cancelled().await;
        let all = async {
            for handle in tasks {
                let _ = handle.await;
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(5), all).await;
        if let Some(models) = models {
            models.unload(true);
        }
        tracing::info!(target: "ordin::memory", "memory.stopped");
    });
}

impl Inner {
    /// §6, one pass.
    pub(crate) async fn reconcile_once(&self) -> Result<(), MemoryError> {
        let rows = store::unsettled(&self.db, RECONCILE_BATCH).await?;
        let context = self.maintenance_context();
        let now = Timestamp::now();
        for row in rows {
            let stored = self
                .storage
                .get_fact(&context, &row.memory_fact_id)
                .await
                .map_err(crate::service::store_error_of)?;
            if row.state == FactState::Replaced {
                self.settle_replaced_early(&row, stored.is_some(), now)
                    .await?;
                continue;
            }
            match stored {
                Some(fact) => {
                    if store::settle(&self.db, row.id, FactState::Pending, FactState::Kept, now)
                        .await?
                    {
                        self.after_kept(&row, &fact, now).await?;
                    }
                }
                None if now.0 - row.written_at.0
                    >= ChronoDuration::seconds(NOT_KEPT_AFTER_SECS) =>
                {
                    let from = FactState::Pending;
                    if store::settle(&self.db, row.id, from, FactState::NotKept, now).await? {
                        tracing::info!(target: "ordin::memory", fact_ref = row.id.to_string().as_str(), "memory.fact_not_kept");
                    }
                }
                None => {}
            }
        }
        Ok(())
    }

    /// A row retired while its write was still queued: once the fact lands it is retired too.
    async fn settle_replaced_early(
        &self,
        row: &RefRow,
        stored: bool,
        now: Timestamp,
    ) -> Result<(), MemoryError> {
        let expired = now.0 - row.written_at.0 >= ChronoDuration::seconds(NOT_KEPT_AFTER_SECS);
        if stored {
            let place = row.place()?;
            self.retire_fact(
                place,
                &row.memory_fact_id,
                &format!("late-retire:{}", row.id),
            )
            .await?;
        }
        if stored || expired {
            let from = FactState::Replaced;
            store::settle(&self.db, row.id, from, FactState::Replaced, now).await?;
        }
        Ok(())
    }

    /// §6 step 3 and the ADR-111 addition.
    async fn after_kept(
        &self,
        row: &RefRow,
        fact: &memory::types::domain::Fact,
        now: Timestamp,
    ) -> Result<(), MemoryError> {
        if let Some(old) = row.replaces_id {
            if let Some(old_row) = store::ref_by_id(&self.db, old).await? {
                self.supersede_fact(&old_row, &row.memory_fact_id).await?;
                store::set_state(&self.db, old, FactState::Replaced, old_row.settled_at).await?;
            }
        }
        if fact.derived_from.len() >= 2 {
            if let Some(workspace_id) = row.workspace_id {
                for thing in &fact.entities {
                    self.db
                        .exec(
                            q(
                                "INSERT INTO memory_consolidations (workspace_id, thing_id, \
                               fact_ref, written_at) VALUES ($1, $2, $3, $4) \
                               ON CONFLICT DO NOTHING",
                            )
                            .uuid(workspace_id.0)
                            .text(thing.clone())
                            .uuid(row.id.0)
                            .ts(now),
                        )
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// §7, one pass.
    pub(crate) async fn sweep_once(&self) {
        if let Err(error) = self.sweep_facts().await {
            tracing::warn!(target: "ordin::memory", code = error.code(), "memory.sweep_error");
        }
        if let Err(error) = self.sweep_recall_log().await {
            tracing::warn!(target: "ordin::memory", code = error.code(), "memory.sweep_error");
        }
        if let Err(error) = self.storage.idempotency_prune(chrono::Utc::now()).await {
            let code = crate::service::store_error_of(error).code();
            tracing::warn!(target: "ordin::memory", code, "memory.sweep_error");
        }
    }

    async fn sweep_facts(&self) -> Result<(), MemoryError> {
        let now = Timestamp::now();
        for row in store::rows_to_expire(&self.db, now, SWEEP_BATCH).await? {
            let key = format!("retention:{}", row.id);
            match self.erase_row(&row, &key, now).await {
                Ok(erased) if erased.stored => {
                    let fact_ref = row.id.to_string();
                    self.journal(
                        "fact_expired",
                        row.run_id,
                        None,
                        row.workspace_id,
                        "A fact passed its retention limit and was deleted.",
                        json!({ "fact_ref": fact_ref, "proof_digest": erased.digest }),
                    )
                    .await;
                    tracing::info!(target: "ordin::memory", fact_ref = row.id.to_string().as_str(), "memory.fact_expired");
                }
                Ok(_) => {}
                Err(error) => {
                    let code = error.code();
                    tracing::warn!(target: "ordin::memory", code, "memory.sweep_error");
                }
            }
        }
        Ok(())
    }

    async fn sweep_recall_log(&self) -> Result<(), MemoryError> {
        let days = i64::from(self.recall_log_days.load(Ordering::Relaxed));
        let before = Timestamp(Timestamp::now().0 - ChronoDuration::days(days));
        let rows = self
            .db
            .all(
                q("SELECT id FROM memory_recalls WHERE recalled_at < $1 LIMIT $2")
                    .ts(before)
                    .int(RECALL_LOG_BATCH),
            )
            .await?;
        for row in rows {
            self.db
                .exec(q("DELETE FROM memory_recalls WHERE id = $1").uuid(row.uuid("id")?))
                .await?;
        }
        Ok(())
    }

    /// *Existing installations* (ADR-111 item 9): until the backfill ends recall runs without the
    /// graph list (`SqlStorage` knows), and this logs that once.
    async fn backfill_names(&self, cancel: CancellationToken) {
        let mut entities = 0u64;
        let mut announced = false;
        loop {
            if cancel.is_cancelled() {
                return;
            }
            match self.storage.backfill_entity_names(TENANT, 500).await {
                Ok(0) => break,
                Ok(written) => {
                    if !announced {
                        tracing::info!(target: "ordin::memory", "memory.graph_backfill_pending");
                        announced = true;
                    }
                    entities += written;
                }
                Err(error) => {
                    let code = crate::service::store_error_of(error).code();
                    tracing::warn!(target: "ordin::memory", code, "memory.sweep_error");
                    return;
                }
            }
        }
        if announced {
            tracing::info!(target: "ordin::memory", entities, "memory.graph_backfill_done");
        }
    }

    #[cfg(test)]
    pub(crate) async fn run_backfill_for_test(&self, cancel: CancellationToken) {
        self.backfill_names(cancel).await;
    }

    /// A forget's rows: the fact's, and those of the summaries that went with it (§9 step 3).
    pub(crate) async fn forget_with_derived(
        &self,
        row: &RefRow,
        derived_removed: &[String],
        now: Timestamp,
    ) -> Result<Vec<MemoryFactId>, MemoryError> {
        let mut ids = vec![row.id];
        ids.extend(
            store::refs_by_facts(&self.db, derived_removed)
                .await?
                .into_iter()
                .map(|r| r.id),
        );
        store::forget_rows(&self.db, &ids, now).await?;
        Ok(ids)
    }
}
