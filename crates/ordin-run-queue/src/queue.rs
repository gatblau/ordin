//! `DatastoreRunQueue`: validation, metrics and log events around a `RunStore` (RunQueue.md
//! *Internal Logic*).

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ordin_journal::Datastore;
use ordin_types::domain::agent_profile::{
    RunEndKind, RunRecord, RunRecordState, RunRequest, RunStarter,
};
use ordin_types::paging::{Cursor, Page, PAGE_SIZE};
use ordin_types::time::Timestamp;
use ordin_types::ScopeId;

use crate::error::RunQueueError;
use crate::store::{
    EndWrite, InsertFiring, InsertRun, PgRunStore, ProgressWrite, RunStore, Selection,
    SqliteRunStore,
};
use crate::types::{
    CancelOutcome, DeletedRuns, InProgressFilter, LeaseRenewal, MetricsSink, NewRun, RunEnd,
    RunOutcome, RunPause, RunView, StoredRunEvent, TakenRun, MAX_DELETE_LIMIT,
    MAX_PROGRESS_LINE_CHARS,
};
use crate::validate;

#[async_trait]
pub trait RunQueue: Send + Sync {
    async fn enqueue(&self, new: NewRun) -> Result<RunRecord, RunQueueError>;
    async fn enqueue_firing(
        &self,
        entry_key: &str,
        due: Timestamp,
        scope_id: ScopeId,
        command: &str,
        arguments: &str,
    ) -> Result<Option<RunRecord>, RunQueueError>;
    async fn thread_owner(&self, thread_id: &str) -> Result<Option<RunStarter>, RunQueueError>;
    async fn take(&self, instance: &str) -> Result<Option<TakenRun>, RunQueueError>;
    async fn renew(&self, run_id: &str, instance: &str) -> Result<LeaseRenewal, RunQueueError>;
    async fn end(&self, run_id: &str, instance: &str, end: RunEnd) -> Result<(), RunQueueError>;
    async fn pause(
        &self,
        run_id: &str,
        instance: &str,
        pause: RunPause,
    ) -> Result<(), RunQueueError>;
    async fn resume(
        &self,
        run_id: &str,
        decision: serde_json::Value,
    ) -> Result<RunRecord, RunQueueError>;
    async fn cancel(&self, run_id: &str) -> Result<CancelOutcome, RunQueueError>;
    async fn record_cancelled(
        &self,
        run_id: &str,
        instance: &str,
        outcome: RunOutcome,
    ) -> Result<(), RunQueueError>;
    async fn decision(&self, run_id: &str) -> Result<Option<serde_json::Value>, RunQueueError>;
    async fn get(&self, run_id: &str) -> Result<Option<RunView>, RunQueueError>;
    async fn list(
        &self,
        states: Option<&[RunRecordState]>,
        cursor: Option<&str>,
    ) -> Result<Page<RunView>, RunQueueError>;
    async fn in_progress(&self, filter: &InProgressFilter) -> Result<u64, RunQueueError>;
    /// Whether a paused run other than `except_run_id` has one of `commands` (the Rules Engine's
    /// duty-overlap fact, through the program's `RulesHost`).
    async fn paused_with_command(
        &self,
        commands: &[String],
        except_run_id: &str,
    ) -> Result<bool, RunQueueError>;
    async fn mark_interrupted(&self) -> Result<Vec<String>, RunQueueError>;
    async fn append_progress(
        &self,
        run_id: &str,
        instance: &str,
        line: &str,
    ) -> Result<bool, RunQueueError>;
    async fn events_after(
        &self,
        run_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<StoredRunEvent>, RunQueueError>;
    // retention and erasure
    async fn count_ended_before(
        &self,
        cutoff: Timestamp,
        limit: u32,
    ) -> Result<DeletedRuns, RunQueueError>;
    async fn delete_ended_before(
        &self,
        cutoff: Timestamp,
        limit: u32,
    ) -> Result<DeletedRuns, RunQueueError>;
    async fn count_started_by_asker(
        &self,
        connection: &str,
        asker: &str,
    ) -> Result<DeletedRuns, RunQueueError>;
    async fn delete_started_by_asker(
        &self,
        connection: &str,
        asker: &str,
    ) -> Result<DeletedRuns, RunQueueError>;
    async fn count_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError>;
    async fn delete_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError>;
}

/// The most run ids kept to log `run_events_full` once per run; older ones are forgotten.
const FULL_LOGGED_CAP: usize = 1_024;

/// The one implementation; it holds a storage trait object chosen by the datastore.
pub struct DatastoreRunQueue {
    store: Box<dyn RunStore>,
    metrics: Arc<dyn MetricsSink>,
    /// Runs whose first refused progress line has been logged.
    events_full_logged: Mutex<HashSet<String>>,
}

impl DatastoreRunQueue {
    pub fn new(datastore: &Datastore, metrics: Arc<dyn MetricsSink>) -> Self {
        let store: Box<dyn RunStore> = match datastore {
            Datastore::Sqlite(pool) => Box::new(SqliteRunStore::new(pool.clone())),
            Datastore::Postgres(pool) => Box::new(PgRunStore::new(pool.clone())),
        };
        DatastoreRunQueue {
            store,
            metrics,
            events_full_logged: Mutex::new(HashSet::new()),
        }
    }
}

fn kind_label(starter: &RunStarter) -> &'static str {
    match starter {
        RunStarter::Caller { .. } => "caller",
        RunStarter::ScheduleEntry { .. } => "schedule",
        RunStarter::Intake { .. } => "intake",
        RunStarter::ChatConnection { .. } => "chat",
    }
}

fn command_of(request: &RunRequest) -> &str {
    match request {
        RunRequest::Command { command, .. } => command,
        RunRequest::Message { .. } => "none",
    }
}

impl DatastoreRunQueue {
    fn count_queued(&self, record: &RunRecord) {
        let kind = kind_label(&record.started_by);
        self.metrics
            .add("wrightrun_runs_queued_total", &[("started_by", kind)], 1);
        tracing::info!(
            target: "ordin_run_queue",
            run_id = record.run_id.as_str(),
            thread_id = record.thread_id.as_str(),
            started_by = kind,
            command = command_of(&record.request),
            "run_queued"
        );
    }

    fn count_cancelled(&self, run_id: &str, from: &str) {
        self.metrics
            .add("wrightrun_runs_ended_total", &[("state", "cancelled")], 1);
        tracing::info!(target: "ordin_run_queue", run_id, from, "run_cancelled");
    }

    fn log_deleted(&self, reason: &'static str, deleted: &DeletedRuns) {
        tracing::info!(
            target: "ordin_run_queue",
            reason,
            threads = deleted.thread_ids.len() as u64,
            runs = deleted.runs,
            "runs_deleted"
        );
    }
}

#[async_trait]
impl RunQueue for DatastoreRunQueue {
    // spec: ordin-desktop/components/RunQueue#SA-ID-01
    async fn enqueue(&self, new: NewRun) -> Result<RunRecord, RunQueueError> {
        validate::new_run(&new)?;
        let run_id = match &new.run_id {
            Some(given) => given.clone(),
            None => uuid::Uuid::now_v7().hyphenated().to_string(),
        };
        let (thread_id, new_thread) = match &new.thread_id {
            None => (uuid::Uuid::now_v7().hyphenated().to_string(), true),
            Some(given) => match validate::uuid_text(given) {
                Some(id) => (id, false),
                None => return Err(RunQueueError::ThreadNotFound),
            },
        };
        let record = self
            .store
            .insert_run(&InsertRun {
                run_id,
                thread_id,
                new_thread,
                started_by: new.started_by,
                scope_id: new.scope_id,
                request: new.request,
                gathered_context: new.gathered_context,
            })
            .await?;
        self.count_queued(&record);
        Ok(record)
    }

    async fn enqueue_firing(
        &self,
        entry_key: &str,
        due: Timestamp,
        scope_id: ScopeId,
        command: &str,
        arguments: &str,
    ) -> Result<Option<RunRecord>, RunQueueError> {
        validate::firing(entry_key, command, arguments)?;
        let record = self
            .store
            .insert_firing(&InsertFiring {
                run_id: uuid::Uuid::now_v7().hyphenated().to_string(),
                thread_id: uuid::Uuid::now_v7().hyphenated().to_string(),
                entry_key: entry_key.to_owned(),
                due,
                scope_id,
                command: command.to_owned(),
                arguments: arguments.to_owned(),
            })
            .await?;
        if let Some(record) = &record {
            self.count_queued(record);
        }
        Ok(record)
    }

    async fn thread_owner(&self, thread_id: &str) -> Result<Option<RunStarter>, RunQueueError> {
        match validate::uuid_text(thread_id) {
            Some(id) => self.store.thread_owner(&id).await,
            None => Ok(None),
        }
    }

    async fn take(&self, instance: &str) -> Result<Option<TakenRun>, RunQueueError> {
        let taken = self.store.take(instance).await?;
        if let Some(taken) = &taken {
            let record = &taken.view.record;
            self.metrics.add(
                "wrightrun_runs_started_total",
                &[("started_by", kind_label(&record.started_by))],
                1,
            );
            tracing::info!(
                target: "ordin_run_queue",
                run_id = record.run_id.as_str(),
                thread_id = record.thread_id.as_str(),
                instance,
                "run_taken"
            );
        }
        Ok(taken)
    }

    async fn renew(&self, run_id: &str, instance: &str) -> Result<LeaseRenewal, RunQueueError> {
        match validate::uuid_text(run_id) {
            Some(id) => self.store.renew(&id, instance).await,
            None => Ok(LeaseRenewal::Lost),
        }
    }

    async fn end(&self, run_id: &str, instance: &str, end: RunEnd) -> Result<(), RunQueueError> {
        let end_kind = match (end.end_kind, end.outcome.ended_by_control) {
            (RunEndKind::Control, false) => {
                return Err(RunQueueError::BadInput { field: "end_kind" })
            }
            (RunEndKind::Completed, true) => RunEndKind::Control,
            (kind, _) => kind,
        };
        let wire_state = match end_kind {
            RunEndKind::Completed => "finished",
            RunEndKind::Control => "ended-by-control",
            RunEndKind::Failed => "failed",
            RunEndKind::RunLimit => "ended-at-limit",
        };
        let id = validate::uuid_text(run_id).ok_or(RunQueueError::LeaseLost)?;
        self.store
            .end(
                &id,
                instance,
                &EndWrite {
                    end_kind,
                    wire_state,
                    outcome: end.outcome,
                    stretch_summary: end.stretch_summary,
                },
            )
            .await?;
        self.metrics
            .add("wrightrun_runs_ended_total", &[("state", wire_state)], 1);
        tracing::info!(target: "ordin_run_queue", run_id = id.as_str(), state = wire_state, "run_ended");
        Ok(())
    }

    async fn pause(
        &self,
        run_id: &str,
        instance: &str,
        pause: RunPause,
    ) -> Result<(), RunQueueError> {
        validate::pause(&pause)?;
        let id = validate::uuid_text(run_id).ok_or(RunQueueError::LeaseLost)?;
        self.store.pause(&id, instance, &pause).await?;
        for item in pause
            .outcome
            .pending
            .iter()
            .filter(|item| item.reason == "approval-list")
        {
            self.metrics.add(
                "wrightrun_tool_calls_paused_for_approval_total",
                &[("tool", item.tool.as_deref().unwrap_or(""))],
                1,
            );
        }
        tracing::info!(
            target: "ordin_run_queue",
            run_id = id.as_str(),
            pending = pause.outcome.pending.len() as u64,
            "run_paused"
        );
        Ok(())
    }

    async fn resume(
        &self,
        run_id: &str,
        decision: serde_json::Value,
    ) -> Result<RunRecord, RunQueueError> {
        let id = validate::uuid_text(run_id).ok_or(RunQueueError::NotPaused)?;
        let record = self.store.resume(&id, &decision).await?;
        // The decision is never logged.
        tracing::info!(target: "ordin_run_queue", run_id = id.as_str(), "run_resumed");
        Ok(record)
    }

    async fn cancel(&self, run_id: &str) -> Result<CancelOutcome, RunQueueError> {
        let id = validate::uuid_text(run_id).ok_or(RunQueueError::NotCancellable)?;
        let (outcome, from) = self.store.cancel(&id).await?;
        match &outcome {
            CancelOutcome::Cancelled(_) => self.count_cancelled(&id, from),
            CancelOutcome::Requested(_) => {
                tracing::info!(target: "ordin_run_queue", run_id = id.as_str(), "run_cancel_requested");
            }
        }
        Ok(outcome)
    }

    async fn record_cancelled(
        &self,
        run_id: &str,
        instance: &str,
        outcome: RunOutcome,
    ) -> Result<(), RunQueueError> {
        let id = validate::uuid_text(run_id).ok_or(RunQueueError::LeaseLost)?;
        self.store.record_cancelled(&id, instance, &outcome).await?;
        self.count_cancelled(&id, "executing");
        Ok(())
    }

    async fn decision(&self, run_id: &str) -> Result<Option<serde_json::Value>, RunQueueError> {
        match validate::uuid_text(run_id) {
            Some(id) => self.store.decision(&id).await,
            None => Ok(None),
        }
    }

    async fn get(&self, run_id: &str) -> Result<Option<RunView>, RunQueueError> {
        match validate::uuid_text(run_id) {
            Some(id) => self.store.get(&id).await,
            None => Ok(None),
        }
    }

    async fn list(
        &self,
        states: Option<&[RunRecordState]>,
        cursor: Option<&str>,
    ) -> Result<Page<RunView>, RunQueueError> {
        if matches!(states, Some([])) {
            return Ok(Page {
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let before = match cursor {
            None => None,
            Some(raw) => {
                let cursor: Cursor =
                    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
                        .map_err(|_| RunQueueError::BadCursor)?;
                Some(
                    cursor
                        .decode::<i64>()
                        .map_err(|_| RunQueueError::BadCursor)?,
                )
            }
        };
        let mut rows = self
            .store
            .list(states, before, PAGE_SIZE as i64 + 1)
            .await?;
        let next_cursor = if rows.len() > PAGE_SIZE {
            rows.truncate(PAGE_SIZE);
            rows.last().map(|(seq, _)| Cursor::encode(seq))
        } else {
            None
        };
        Ok(Page {
            items: rows.into_iter().map(|(_, view)| view).collect(),
            next_cursor,
        })
    }

    async fn in_progress(&self, filter: &InProgressFilter) -> Result<u64, RunQueueError> {
        self.store.in_progress(filter).await
    }

    async fn paused_with_command(
        &self,
        commands: &[String],
        except_run_id: &str,
    ) -> Result<bool, RunQueueError> {
        if commands.is_empty() {
            return Ok(false);
        }
        if commands.len() > validate::MAX_COMMANDS {
            return Err(RunQueueError::BadInput { field: "commands" });
        }
        let except = validate::uuid_text(except_run_id);
        self.store
            .paused_with_command(commands, except.as_deref())
            .await
    }

    async fn mark_interrupted(&self) -> Result<Vec<String>, RunQueueError> {
        let closed = self.store.mark_interrupted().await?;
        for run in closed.iter().filter(|run| run.state == "interrupted") {
            self.metrics.add("wrightrun_runs_interrupted_total", &[], 1);
            tracing::warn!(
                target: "ordin_run_queue",
                run_id = run.run_id.as_str(),
                thread_id = run.thread_id.as_str(),
                instance = run.instance.as_str(),
                "run_interrupted"
            );
        }
        Ok(closed.into_iter().map(|run| run.run_id).collect())
    }

    async fn append_progress(
        &self,
        run_id: &str,
        instance: &str,
        line: &str,
    ) -> Result<bool, RunQueueError> {
        let line: String = line.trim().chars().take(MAX_PROGRESS_LINE_CHARS).collect();
        if line.is_empty() {
            return Ok(false);
        }
        let id = validate::uuid_text(run_id).ok_or(RunQueueError::LeaseLost)?;
        match self.store.append_progress(&id, instance, &line).await? {
            ProgressWrite::Stored => Ok(true),
            ProgressWrite::Full => {
                let first = match self.events_full_logged.lock() {
                    Ok(mut logged) => {
                        if logged.len() >= FULL_LOGGED_CAP {
                            logged.clear();
                        }
                        logged.insert(id.clone())
                    }
                    Err(_) => false,
                };
                if first {
                    tracing::warn!(target: "ordin_run_queue", run_id = id.as_str(), "run_events_full");
                }
                Ok(false)
            }
        }
    }

    async fn events_after(
        &self,
        run_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<StoredRunEvent>, RunQueueError> {
        match validate::uuid_text(run_id) {
            Some(id) => {
                self.store
                    .events_after(&id, after_seq, limit.clamp(1, 500))
                    .await
            }
            None => Ok(Vec::new()),
        }
    }

    async fn count_ended_before(
        &self,
        cutoff: Timestamp,
        limit: u32,
    ) -> Result<DeletedRuns, RunQueueError> {
        self.store
            .purge(
                &Selection::EndedBefore {
                    cutoff,
                    limit: limit.clamp(1, MAX_DELETE_LIMIT),
                },
                false,
            )
            .await
    }

    async fn delete_ended_before(
        &self,
        cutoff: Timestamp,
        limit: u32,
    ) -> Result<DeletedRuns, RunQueueError> {
        let deleted = self
            .store
            .purge(
                &Selection::EndedBefore {
                    cutoff,
                    limit: limit.clamp(1, MAX_DELETE_LIMIT),
                },
                true,
            )
            .await?;
        self.log_deleted("retention", &deleted);
        Ok(deleted)
    }

    async fn count_started_by_asker(
        &self,
        connection: &str,
        asker: &str,
    ) -> Result<DeletedRuns, RunQueueError> {
        self.store
            .purge(
                &Selection::Asker {
                    connection: connection.to_owned(),
                    asker: asker.to_owned(),
                },
                false,
            )
            .await
    }

    async fn delete_started_by_asker(
        &self,
        connection: &str,
        asker: &str,
    ) -> Result<DeletedRuns, RunQueueError> {
        let deleted = self
            .store
            .purge(
                &Selection::Asker {
                    connection: connection.to_owned(),
                    asker: asker.to_owned(),
                },
                true,
            )
            .await?;
        // The asker is never logged.
        self.log_deleted("erase-person", &deleted);
        Ok(deleted)
    }

    async fn count_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError> {
        self.store.purge(&Selection::Scope(scope_id), false).await
    }

    async fn delete_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError> {
        let deleted = self.store.purge(&Selection::Scope(scope_id), true).await?;
        self.log_deleted("erase-scope", &deleted);
        Ok(deleted)
    }
}
