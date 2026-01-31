//! The storage trait `RunStore` and its two implementations, `SqliteRunStore` and `PgRunStore`
//! (SA-DB-01). Each method is the statement (or short transaction) of the matching *Internal Logic*
//! step: the test and the change are one statement, so the datastore picks the single winner.
//! Validation, metrics and log events belong to `DatastoreRunQueue`; a store only reads and writes.

use async_trait::async_trait;
use ordin_journal::Datastore;
use ordin_types::domain::agent_profile::{
    RunEndKind, RunRecord, RunRecordState, RunRequest, RunStarter,
};
use ordin_types::domain::TaskArgument;
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{RunRecordId, ScopeId};
use serde::de::DeserializeOwned;

use crate::db::{self, Arg, DbRow, Tx, Unique};
use crate::error::RunQueueError;
use crate::types::{
    CancelOutcome, DeletedRuns, InProgressFilter, LeaseRenewal, RunEventKind, RunOutcome, RunPause,
    RunView, StoredRunEvent, TakenRun, MAX_DELETE_LIMIT, MAX_EVENTS_PER_RUN,
};

/// What `insert_run` writes. `new_thread` says whether the thread row is written with it.
#[derive(Debug, Clone)]
pub struct InsertRun {
    pub run_id: String,
    pub thread_id: String,
    pub new_thread: bool,
    pub started_by: RunStarter,
    pub scope_id: ScopeId,
    pub request: RunRequest,
    pub gathered_context: serde_json::Value,
}

/// What `insert_firing` writes (one run on one new thread).
#[derive(Debug, Clone)]
pub struct InsertFiring {
    pub run_id: String,
    pub thread_id: String,
    pub entry_key: String,
    pub due: Timestamp,
    pub scope_id: ScopeId,
    pub command: String,
    pub arguments: String,
}

/// What `end` writes; `wire_state` is the state the stream shows (`finished`, `ended-by-control`,
/// `failed`, `ended-at-limit`).
#[derive(Debug, Clone)]
pub struct EndWrite {
    pub end_kind: RunEndKind,
    pub wire_state: &'static str,
    pub outcome: RunOutcome,
    pub stretch_summary: Option<String>,
}

/// A run `mark_interrupted` closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedRun {
    pub run_id: String,
    pub thread_id: String,
    pub instance: String,
    /// `interrupted`, or `cancelled` for a run the caller had asked to cancel.
    pub state: &'static str,
}

/// What `append_progress` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressWrite {
    Stored,
    /// The run already holds `MAX_EVENTS_PER_RUN` events; the line was dropped.
    Full,
}

/// Which threads a retention or erasure statement selects.
#[derive(Debug, Clone)]
pub enum Selection {
    EndedBefore { cutoff: Timestamp, limit: u32 },
    Asker { connection: String, asker: String },
    Scope(ScopeId),
}

#[async_trait]
pub trait RunStore: Send + Sync {
    async fn insert_run(&self, run: &InsertRun) -> Result<RunRecord, RunQueueError>;
    async fn insert_firing(
        &self,
        firing: &InsertFiring,
    ) -> Result<Option<RunRecord>, RunQueueError>;
    async fn thread_owner(&self, thread_id: &str) -> Result<Option<RunStarter>, RunQueueError>;
    async fn take(&self, instance: &str) -> Result<Option<TakenRun>, RunQueueError>;
    async fn renew(&self, run_id: &str, instance: &str) -> Result<LeaseRenewal, RunQueueError>;
    async fn end(&self, run_id: &str, instance: &str, end: &EndWrite) -> Result<(), RunQueueError>;
    async fn pause(
        &self,
        run_id: &str,
        instance: &str,
        pause: &RunPause,
    ) -> Result<(), RunQueueError>;
    async fn resume(
        &self,
        run_id: &str,
        decision: &serde_json::Value,
    ) -> Result<RunRecord, RunQueueError>;
    /// The outcome and the state the run was in.
    async fn cancel(&self, run_id: &str) -> Result<(CancelOutcome, &'static str), RunQueueError>;
    async fn record_cancelled(
        &self,
        run_id: &str,
        instance: &str,
        outcome: &RunOutcome,
    ) -> Result<(), RunQueueError>;
    async fn decision(&self, run_id: &str) -> Result<Option<serde_json::Value>, RunQueueError>;
    async fn get(&self, run_id: &str) -> Result<Option<RunView>, RunQueueError>;
    /// Newest first; `limit` rows with their `seq`.
    async fn list(
        &self,
        states: Option<&[RunRecordState]>,
        before: Option<i64>,
        limit: i64,
    ) -> Result<Vec<(i64, RunView)>, RunQueueError>;
    async fn in_progress(&self, filter: &InProgressFilter) -> Result<u64, RunQueueError>;
    async fn paused_with_command(
        &self,
        commands: &[String],
        except_run_id: Option<&str>,
    ) -> Result<bool, RunQueueError>;
    async fn mark_interrupted(&self) -> Result<Vec<ClosedRun>, RunQueueError>;
    async fn append_progress(
        &self,
        run_id: &str,
        instance: &str,
        line: &str,
    ) -> Result<ProgressWrite, RunQueueError>;
    async fn events_after(
        &self,
        run_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<StoredRunEvent>, RunQueueError>;
    /// Counts what the selection would delete, or deletes it.
    async fn purge(
        &self,
        selection: &Selection,
        delete: bool,
    ) -> Result<DeletedRuns, RunQueueError>;
}

/// The SQLite implementation (desktop, personal server, agent single shape).
pub struct SqliteRunStore {
    core: Core,
}

impl SqliteRunStore {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        SqliteRunStore {
            core: Core {
                store: Datastore::Sqlite(pool),
            },
        }
    }
}

/// The PostgreSQL implementation (agent scaled shape).
pub struct PgRunStore {
    core: Core,
}

impl PgRunStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        PgRunStore {
            core: Core {
                store: Datastore::Postgres(pool),
            },
        }
    }
}

macro_rules! delegate_store {
    ($ty:ty) => {
        #[async_trait]
        impl RunStore for $ty {
            async fn insert_run(&self, run: &InsertRun) -> Result<RunRecord, RunQueueError> {
                finish("enqueue", self.core.insert_run(run).await)
            }
            async fn insert_firing(
                &self,
                firing: &InsertFiring,
            ) -> Result<Option<RunRecord>, RunQueueError> {
                finish("enqueue_firing", self.core.insert_firing(firing).await)
            }
            async fn thread_owner(
                &self,
                thread_id: &str,
            ) -> Result<Option<RunStarter>, RunQueueError> {
                finish("thread_owner", self.core.thread_owner(thread_id).await)
            }
            async fn take(&self, instance: &str) -> Result<Option<TakenRun>, RunQueueError> {
                finish("take", self.core.take(instance).await)
            }
            async fn renew(
                &self,
                run_id: &str,
                instance: &str,
            ) -> Result<LeaseRenewal, RunQueueError> {
                finish("renew", self.core.renew(run_id, instance).await)
            }
            async fn end(
                &self,
                run_id: &str,
                instance: &str,
                end: &EndWrite,
            ) -> Result<(), RunQueueError> {
                finish("end", self.core.end(run_id, instance, end).await)
            }
            async fn pause(
                &self,
                run_id: &str,
                instance: &str,
                pause: &RunPause,
            ) -> Result<(), RunQueueError> {
                finish("pause", self.core.pause(run_id, instance, pause).await)
            }
            async fn resume(
                &self,
                run_id: &str,
                decision: &serde_json::Value,
            ) -> Result<RunRecord, RunQueueError> {
                finish("resume", self.core.resume(run_id, decision).await)
            }
            async fn cancel(
                &self,
                run_id: &str,
            ) -> Result<(CancelOutcome, &'static str), RunQueueError> {
                finish("cancel", self.core.cancel(run_id).await)
            }
            async fn record_cancelled(
                &self,
                run_id: &str,
                instance: &str,
                outcome: &RunOutcome,
            ) -> Result<(), RunQueueError> {
                finish(
                    "record_cancelled",
                    self.core.record_cancelled(run_id, instance, outcome).await,
                )
            }
            async fn decision(
                &self,
                run_id: &str,
            ) -> Result<Option<serde_json::Value>, RunQueueError> {
                finish("decision", self.core.decision(run_id).await)
            }
            async fn get(&self, run_id: &str) -> Result<Option<RunView>, RunQueueError> {
                finish("get", self.core.get(run_id).await)
            }
            async fn list(
                &self,
                states: Option<&[RunRecordState]>,
                before: Option<i64>,
                limit: i64,
            ) -> Result<Vec<(i64, RunView)>, RunQueueError> {
                finish("list", self.core.list(states, before, limit).await)
            }
            async fn in_progress(&self, filter: &InProgressFilter) -> Result<u64, RunQueueError> {
                finish("in_progress", self.core.in_progress(filter).await)
            }
            async fn paused_with_command(
                &self,
                commands: &[String],
                except_run_id: Option<&str>,
            ) -> Result<bool, RunQueueError> {
                finish(
                    "paused_with_command",
                    self.core.paused_with_command(commands, except_run_id).await,
                )
            }
            async fn mark_interrupted(&self) -> Result<Vec<ClosedRun>, RunQueueError> {
                finish("mark_interrupted", self.core.mark_interrupted().await)
            }
            async fn append_progress(
                &self,
                run_id: &str,
                instance: &str,
                line: &str,
            ) -> Result<ProgressWrite, RunQueueError> {
                finish(
                    "append_progress",
                    self.core.append_progress(run_id, instance, line).await,
                )
            }
            async fn events_after(
                &self,
                run_id: &str,
                after_seq: u64,
                limit: u32,
            ) -> Result<Vec<StoredRunEvent>, RunQueueError> {
                finish(
                    "events_after",
                    self.core.events_after(run_id, after_seq, limit).await,
                )
            }
            async fn purge(
                &self,
                selection: &Selection,
                delete: bool,
            ) -> Result<DeletedRuns, RunQueueError> {
                finish("purge", self.core.purge(selection, delete).await)
            }
        }
    };
}
delegate_store!(SqliteRunStore);
delegate_store!(PgRunStore);

/// An internal failure: a datastore error, or a queue error decided inside a transaction.
enum Fail {
    /// A datastore error; the sqlx text is dropped because it can hold the connection string.
    Db,
    Run(RunQueueError),
}

impl From<sqlx::Error> for Fail {
    fn from(_: sqlx::Error) -> Self {
        Fail::Db
    }
}

impl From<RunQueueError> for Fail {
    fn from(error: RunQueueError) -> Self {
        Fail::Run(error)
    }
}

/// A row that does not decode is an unreadable datastore answer.
fn corrupt<E>(_: E) -> Fail {
    Fail::Run(RunQueueError::Unavailable)
}

/// A connection or I/O error becomes `Unavailable` and is logged as `run_queue_unavailable` (WARN)
/// with the operation, never the sqlx text (it can hold the connection string).
fn finish<T>(operation: &'static str, result: Result<T, Fail>) -> Result<T, RunQueueError> {
    match result {
        Ok(value) => Ok(value),
        Err(Fail::Run(RunQueueError::Unavailable)) | Err(Fail::Db) => {
            tracing::warn!(target: "ordin_run_queue", operation, "run_queue_unavailable");
            Err(RunQueueError::Unavailable)
        }
        Err(Fail::Run(error)) => Err(error),
    }
}

/// Every column of one record, in the order `decode_view` reads them.
macro_rules! rcols {
    () => {
        "{s:id}, {s:thread_id}, state, end_kind, started_by_kind, started_by_id, started_by_asker, \
         {s:scope_id}, {s:request}, {s:gathered_context}, result, {s:outcome}, {s:stretch_summaries}, \
         lease_holder, {t:lease_expires_at}, {t:queued_at}, {t:created_at}, {t:started_at}, \
         {t:ended_at}, CASE WHEN cancel_requested_at IS NULL THEN 0 ELSE 1 END, seq"
    };
}

/// The record columns and the two that only `take` returns.
macro_rules! tcols {
    () => {
        concat!(rcols!(), ", {s:decision}, session_record")
    };
}

const EVENT_SQL: &str = "INSERT INTO run_events (run_id, seq, kind, data, at) VALUES ($1{uuid}, \
    (SELECT coalesce(max(seq), 0) + 1 FROM run_events WHERE run_id = $1{uuid}), $2, $3{json}, {now})";

fn text(value: &str) -> Arg {
    Arg::Text(value.to_owned())
}

fn opt(value: Option<&str>) -> Arg {
    Arg::OptText(value.map(str::to_owned))
}

/// The column triple a starter maps to (Internal Logic *Conversions*).
fn starter_columns(starter: &RunStarter) -> (&'static str, &str, Option<&str>) {
    match starter {
        RunStarter::Caller { caller_id } => ("caller", caller_id, None),
        RunStarter::ScheduleEntry { entry } => ("schedule", entry, None),
        RunStarter::Intake { intake } => ("intake", intake, None),
        RunStarter::ChatConnection { connection, asker } => ("chat", connection, Some(asker)),
    }
}

fn starter_from(kind: &str, id: String, asker: Option<String>) -> Result<RunStarter, Fail> {
    Ok(match (kind, asker) {
        ("caller", _) => RunStarter::Caller { caller_id: id },
        ("schedule", _) => RunStarter::ScheduleEntry { entry: id },
        ("intake", _) => RunStarter::Intake { intake: id },
        ("chat", Some(asker)) => RunStarter::ChatConnection {
            connection: id,
            asker,
        },
        _ => return Err(corrupt(())),
    })
}

fn end_kind_text(kind: RunEndKind) -> &'static str {
    match kind {
        RunEndKind::Completed => "completed",
        RunEndKind::Control => "control",
        RunEndKind::Failed => "failed",
        RunEndKind::RunLimit => "run_limit",
    }
}

fn parse_enum<T: DeserializeOwned>(raw: &str) -> Result<T, Fail> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned())).map_err(corrupt)
}

fn parse_ts(raw: &str) -> Result<Timestamp, Fail> {
    parse_rfc3339_ms_utc(raw).map(Timestamp).map_err(corrupt)
}

fn opt_ts(raw: Option<String>) -> Result<Option<Timestamp>, Fail> {
    raw.as_deref().map(parse_ts).transpose()
}

fn json_text(value: &serde_json::Value) -> Arg {
    Arg::Text(value.to_string())
}

fn outcome_text(outcome: &RunOutcome) -> Result<Arg, Fail> {
    Ok(Arg::Text(serde_json::to_string(outcome).map_err(corrupt)?))
}

/// Reads one record from the columns of `rcols!()`; the second value is its `seq`.
fn decode_view(row: &DbRow) -> Result<(RunView, i64), Fail> {
    let id = uuid::Uuid::parse_str(&row.text(0)?).map_err(corrupt)?;
    let thread_id = row.text(1)?;
    let state: RunRecordState = parse_enum(&row.text(2)?)?;
    let end_kind: Option<RunEndKind> = row.opt_text(3)?.as_deref().map(parse_enum).transpose()?;
    let started_by = starter_from(&row.text(4)?, row.text(5)?, row.opt_text(6)?)?;
    let scope_id = ScopeId(uuid::Uuid::parse_str(&row.text(7)?).map_err(corrupt)?);
    let request: RunRequest = serde_json::from_str(&row.text(8)?).map_err(corrupt)?;
    let gathered_context: serde_json::Value =
        serde_json::from_str(&row.text(9)?).map_err(corrupt)?;
    let result = row.opt_text(10)?;
    let outcome: Option<RunOutcome> = row
        .opt_text(11)?
        .map(|raw| serde_json::from_str(&raw))
        .transpose()
        .map_err(corrupt)?;
    let stretch_summaries: Vec<String> = serde_json::from_str(&row.text(12)?).map_err(corrupt)?;
    let record = RunRecord {
        id: RunRecordId(id),
        run_id: id.hyphenated().to_string(),
        thread_id,
        started_by,
        scope_id,
        request,
        gathered_context,
        state,
        end_kind,
        lease_holder: row.opt_text(13)?,
        lease_expires_at: opt_ts(row.opt_text(14)?)?,
        result,
        stretch_summaries,
        queued_at: parse_ts(&row.text(15)?)?,
        created_at: parse_ts(&row.text(16)?)?,
        started_at: opt_ts(row.opt_text(17)?)?,
        ended_at: opt_ts(row.opt_text(18)?)?,
    };
    let cancel_requested = row.int(19)? != 0;
    let seq = row.int(20)?;
    let view = RunView {
        outcome,
        created_at: record.created_at,
        started_at: record.started_at,
        ended_at: record.ended_at,
        cancel_requested,
        record,
    };
    Ok((view, seq))
}

fn decode_record(row: &DbRow) -> Result<RunRecord, Fail> {
    Ok(decode_view(row)?.0.record)
}

async fn push_event(tx: &mut Tx, run_id: &str, state: &str) -> Result<(), sqlx::Error> {
    tx.execute(
        EVENT_SQL,
        &[
            text(run_id),
            text("run_state_changed"),
            json_text(&serde_json::json!({ "state": state })),
        ],
    )
    .await?;
    Ok(())
}

async fn read_record(tx: &mut Tx, run_id: &str) -> Result<RunRecord, Fail> {
    let row = tx
        .fetch_optional(
            concat!("SELECT ", rcols!(), " FROM run_records WHERE id = $1{uuid}"),
            &[text(run_id)],
        )
        .await?
        .ok_or_else(|| corrupt(()))?;
    decode_record(&row)
}

fn view_of(row: &DbRow) -> Result<RunView, Fail> {
    Ok(decode_view(row)?.0)
}

/// The statements, written once for both engines.
struct Core {
    store: Datastore,
}

impl Core {
    async fn insert_run(&self, p: &InsertRun) -> Result<RunRecord, Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let (kind, starter_id, asker) = starter_columns(&p.started_by);
        if p.new_thread {
            tx.execute(
                "INSERT INTO run_threads (thread_id, started_by_kind, started_by_id, started_by_asker, created_at) \
                 VALUES ($1{uuid}, $2, $3, $4, {now})",
                &[text(&p.thread_id), text(kind), text(starter_id), opt(asker)],
            )
            .await?;
        } else if tx
            .fetch_optional(
                "SELECT 1 FROM run_threads WHERE thread_id = $1{uuid}",
                &[text(&p.thread_id)],
            )
            .await?
            .is_none()
        {
            return Err(RunQueueError::ThreadNotFound.into());
        }
        let command = match &p.request {
            RunRequest::Command { command, .. } => Some(command.as_str()),
            RunRequest::Message { .. } => None,
        };
        let request = serde_json::to_value(&p.request).map_err(corrupt)?;
        let inserted = tx
            .execute(
                "INSERT INTO run_records (id, seq, thread_id, state, started_by_kind, started_by_id, \
                 started_by_asker, scope_id, command, request, gathered_context, queued_at, created_at) \
                 VALUES ($1{uuid}, {seq}, $2{uuid}, 'queued', $3, $4, $5, $6{uuid}, $7, $8{json}, $9{json}, {now}, {now})",
                &[
                    text(&p.run_id),
                    text(&p.thread_id),
                    text(kind),
                    text(starter_id),
                    opt(asker),
                    Arg::Text(p.scope_id.0.hyphenated().to_string()),
                    opt(command),
                    json_text(&request),
                    json_text(&p.gathered_context),
                ],
            )
            .await;
        if let Err(error) = inserted {
            return Err(match db::unique_violation(&error) {
                Some(Unique::RunId) => RunQueueError::DuplicateId.into(),
                Some(Unique::OpenPerThread) => RunQueueError::ThreadBusy.into(),
                _ => Fail::Db,
            });
        }
        push_event(&mut tx, &p.run_id, "queued").await?;
        let record = read_record(&mut tx, &p.run_id).await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn insert_firing(&self, f: &InsertFiring) -> Result<Option<RunRecord>, Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let kind = "schedule";
        // The thread first, so the run's foreign key holds on both engines; a conflict rolls it back.
        tx.execute(
            "INSERT INTO run_threads (thread_id, started_by_kind, started_by_id, started_by_asker, created_at) \
             VALUES ($1{uuid}, $2, $3, NULL, {now})",
            &[text(&f.thread_id), text(kind), text(&f.entry_key)],
        )
        .await?;
        let request = serde_json::to_value(RunRequest::Command {
            command: f.command.clone(),
            arguments: vec![TaskArgument {
                name: "arguments".to_owned(),
                value: f.arguments.clone(),
            }],
        })
        .map_err(corrupt)?;
        let row = tx
            .fetch_optional(
                "INSERT INTO run_records (id, seq, thread_id, state, started_by_kind, started_by_id, \
                 started_by_asker, scope_id, command, request, gathered_context, firing_due, queued_at, created_at) \
                 VALUES ($1{uuid}, {seq}, $2{uuid}, 'queued', 'schedule', $3, NULL, $4{uuid}, $5, $6{json}, $7{json}, $8{ts}, {now}, {now}) \
                 ON CONFLICT (started_by_id, firing_due) WHERE started_by_kind = 'schedule' DO NOTHING \
                 RETURNING seq",
                &[
                    text(&f.run_id),
                    text(&f.thread_id),
                    text(&f.entry_key),
                    Arg::Text(f.scope_id.0.hyphenated().to_string()),
                    text(&f.command),
                    json_text(&request),
                    json_text(&serde_json::json!({})),
                    Arg::Text(f.due.to_wire()),
                ],
            )
            .await?;
        if row.is_none() {
            return Ok(None);
        }
        push_event(&mut tx, &f.run_id, "queued").await?;
        let record = read_record(&mut tx, &f.run_id).await?;
        tx.commit().await?;
        Ok(Some(record))
    }

    async fn thread_owner(&self, thread_id: &str) -> Result<Option<RunStarter>, Fail> {
        let row = db::fetch_optional(
            &self.store,
            "SELECT started_by_kind, started_by_id, started_by_asker FROM run_threads WHERE thread_id = $1{uuid}",
            &[text(thread_id)],
        )
        .await?;
        row.map(|row| starter_from(&row.text(0)?, row.text(1)?, row.opt_text(2)?))
            .transpose()
    }

    async fn take(&self, instance: &str) -> Result<Option<TakenRun>, Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let row = tx
            .fetch_optional(
                concat!(
                    "UPDATE run_records SET state = 'executing', lease_holder = $1, lease_expires_at = {lease}, \
                     started_at = coalesce(started_at, {now}) \
                     WHERE id = (SELECT id FROM run_records WHERE state = 'queued' \
                     ORDER BY queued_at, seq {skip} LIMIT 1) RETURNING ",
                    tcols!()
                ),
                &[text(instance)],
            )
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let view = view_of(&row)?;
        let decision = row
            .opt_text(21)?
            .map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(corrupt)?;
        let session_record = row.opt_bytes(22)?;
        push_event(&mut tx, &view.record.run_id, "executing").await?;
        tx.commit().await?;
        Ok(Some(TakenRun {
            view,
            decision,
            session_record,
        }))
    }

    async fn renew(&self, run_id: &str, instance: &str) -> Result<LeaseRenewal, Fail> {
        // The statement does not test `lease_expires_at` (step 11).
        let row = db::fetch_optional(
            &self.store,
            "UPDATE run_records SET lease_expires_at = {lease} \
             WHERE id = $1{uuid} AND lease_holder = $2 AND state = 'executing' \
             RETURNING CASE WHEN cancel_requested_at IS NULL THEN 0 ELSE 1 END",
            &[text(run_id), text(instance)],
        )
        .await?;
        Ok(match row {
            None => LeaseRenewal::Lost,
            Some(row) if row.int(0)? != 0 => LeaseRenewal::CancelRequested,
            Some(_) => LeaseRenewal::Kept,
        })
    }

    async fn end(&self, run_id: &str, instance: &str, end: &EndWrite) -> Result<(), Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let changed = tx
            .execute(
                "UPDATE run_records SET state = 'ended', end_kind = $1, result = $2, outcome = $3{json}, \
                 ended_at = {now}, lease_holder = NULL, lease_expires_at = NULL, session_record = NULL, \
                 stretch_summaries = {sum:4} \
                 WHERE id = $5{uuid} AND lease_holder = $6 AND state = 'executing'",
                &[
                    text(end_kind_text(end.end_kind)),
                    opt(end.outcome.reply.as_deref()),
                    outcome_text(&end.outcome)?,
                    opt(end.stretch_summary.as_deref()),
                    text(run_id),
                    text(instance),
                ],
            )
            .await?;
        if changed == 0 {
            return Err(RunQueueError::LeaseLost.into());
        }
        push_event(&mut tx, run_id, end.wire_state).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn pause(&self, run_id: &str, instance: &str, pause: &RunPause) -> Result<(), Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let changed = tx
            .execute(
                "UPDATE run_records SET state = 'paused', outcome = $1{json}, decision = NULL, \
                 session_record = $2, lease_holder = NULL, lease_expires_at = NULL, \
                 stretch_summaries = {sum:3} \
                 WHERE id = $4{uuid} AND lease_holder = $5 AND state = 'executing'",
                &[
                    outcome_text(&pause.outcome)?,
                    Arg::Bytes(Some(pause.session_record.clone())),
                    opt(pause.stretch_summary.as_deref()),
                    text(run_id),
                    text(instance),
                ],
            )
            .await?;
        if changed == 0 {
            return Err(RunQueueError::LeaseLost.into());
        }
        push_event(&mut tx, run_id, "paused").await?;
        tx.commit().await?;
        Ok(())
    }

    async fn resume(&self, run_id: &str, decision: &serde_json::Value) -> Result<RunRecord, Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let row = tx
            .fetch_optional(
                concat!(
                    "UPDATE run_records SET state = 'queued', decision = $1{json}, queued_at = {now} \
                     WHERE id = $2{uuid} AND state = 'paused' RETURNING ",
                    rcols!()
                ),
                &[json_text(decision), text(run_id)],
            )
            .await?
            .ok_or(RunQueueError::NotPaused)?;
        let record = decode_record(&row)?;
        push_event(&mut tx, run_id, "queued").await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn cancel(&self, run_id: &str) -> Result<(CancelOutcome, &'static str), Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let from = tx
            .fetch_optional(
                "SELECT state FROM run_records WHERE id = $1{uuid}{lock}",
                &[text(run_id)],
            )
            .await?
            .map(|row| row.text(0))
            .transpose()?;
        if let Some(row) = tx
            .fetch_optional(
                concat!(
                    "UPDATE run_records SET state = 'cancelled', ended_at = {now}, session_record = NULL \
                     WHERE id = $1{uuid} AND state IN ('queued', 'paused') RETURNING ",
                    rcols!()
                ),
                &[text(run_id)],
            )
            .await?
        {
            let view = view_of(&row)?;
            push_event(&mut tx, run_id, "cancelled").await?;
            tx.commit().await?;
            let from = if from.as_deref() == Some("paused") { "paused" } else { "queued" };
            return Ok((CancelOutcome::Cancelled(view), from));
        }
        if let Some(row) = tx
            .fetch_optional(
                concat!(
                    "UPDATE run_records SET cancel_requested_at = coalesce(cancel_requested_at, {now}) \
                     WHERE id = $1{uuid} AND state = 'executing' RETURNING ",
                    rcols!()
                ),
                &[text(run_id)],
            )
            .await?
        {
            let view = view_of(&row)?;
            tx.commit().await?;
            return Ok((CancelOutcome::Requested(view), "executing"));
        }
        Err(RunQueueError::NotCancellable.into())
    }

    async fn record_cancelled(
        &self,
        run_id: &str,
        instance: &str,
        outcome: &RunOutcome,
    ) -> Result<(), Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let changed = tx
            .execute(
                "UPDATE run_records SET state = 'cancelled', outcome = $1{json}, ended_at = {now}, \
                 lease_holder = NULL, lease_expires_at = NULL, session_record = NULL \
                 WHERE id = $2{uuid} AND lease_holder = $3 AND state = 'executing' \
                 AND cancel_requested_at IS NOT NULL",
                &[outcome_text(outcome)?, text(run_id), text(instance)],
            )
            .await?;
        if changed == 0 {
            return Err(RunQueueError::LeaseLost.into());
        }
        push_event(&mut tx, run_id, "cancelled").await?;
        tx.commit().await?;
        Ok(())
    }

    async fn decision(&self, run_id: &str) -> Result<Option<serde_json::Value>, Fail> {
        let row = db::fetch_optional(
            &self.store,
            "SELECT {s:decision} FROM run_records WHERE id = $1{uuid}",
            &[text(run_id)],
        )
        .await?;
        row.and_then(|row| row.opt_text(0).transpose())
            .map(|raw| {
                raw.map_err(Fail::from)
                    .and_then(|raw| serde_json::from_str(&raw).map_err(corrupt))
            })
            .transpose()
    }

    async fn get(&self, run_id: &str) -> Result<Option<RunView>, Fail> {
        let row = db::fetch_optional(
            &self.store,
            concat!("SELECT ", rcols!(), " FROM run_records WHERE id = $1{uuid}"),
            &[text(run_id)],
        )
        .await?;
        row.as_ref().map(view_of).transpose()
    }

    async fn list(
        &self,
        states: Option<&[RunRecordState]>,
        before: Option<i64>,
        limit: i64,
    ) -> Result<Vec<(i64, RunView)>, Fail> {
        let mut clauses: Vec<String> = Vec::new();
        let mut args: Vec<Arg> = Vec::new();
        if let Some(states) = states {
            let mut names: Vec<&'static str> = states
                .iter()
                .map(|s| crate::types::state_text(*s))
                .collect();
            names.sort_unstable();
            names.dedup();
            clauses.push(format!(
                "state IN ({})",
                db::placeholders(1, names.len(), "")
            ));
            args.extend(names.into_iter().map(text));
        }
        if let Some(before) = before {
            args.push(Arg::Int(before));
            clauses.push(format!("seq < ${}", args.len()));
        }
        let filter = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        args.push(Arg::Int(limit));
        let template = format!(
            "SELECT {} FROM run_records{filter} ORDER BY seq DESC LIMIT ${}",
            rcols!(),
            args.len()
        );
        let rows = db::fetch_all(&self.store, &template, &args).await?;
        rows.iter()
            .map(decode_view)
            .map(|r| r.map(|(view, seq)| (seq, view)))
            .collect()
    }

    async fn in_progress(&self, filter: &InProgressFilter) -> Result<u64, Fail> {
        let mut template = String::from(
            "SELECT count(*) FROM run_records WHERE state IN ('queued', 'executing', 'paused')",
        );
        let mut args = Vec::new();
        if let Some(command) = &filter.command {
            args.push(text(command));
            template.push_str(&format!(" AND command = ${}", args.len()));
        }
        if let Some(intake) = &filter.intake {
            args.push(text(intake));
            template.push_str(&format!(
                " AND started_by_kind = 'intake' AND started_by_id = ${}",
                args.len()
            ));
        }
        let row = db::fetch_optional(&self.store, &template, &args)
            .await?
            .ok_or_else(|| corrupt(()))?;
        Ok(row.int(0)?.max(0) as u64)
    }

    async fn paused_with_command(
        &self,
        commands: &[String],
        except_run_id: Option<&str>,
    ) -> Result<bool, Fail> {
        let mut args: Vec<Arg> = commands.iter().map(|c| text(c)).collect();
        let mut template = format!(
            "SELECT CASE WHEN EXISTS (SELECT 1 FROM run_records WHERE state = 'paused' AND command IN ({})",
            db::placeholders(1, args.len(), "")
        );
        if let Some(except) = except_run_id {
            args.push(text(except));
            template.push_str(&format!(" AND id <> ${}{{uuid}}", args.len()));
        }
        template.push_str(") THEN 1 ELSE 0 END");
        let row = db::fetch_optional(&self.store, &template, &args)
            .await?
            .ok_or_else(|| corrupt(()))?;
        Ok(row.int(0)? != 0)
    }

    async fn mark_interrupted(&self) -> Result<Vec<ClosedRun>, Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let rows = tx
            .fetch_all(
                "UPDATE run_records SET state = CASE WHEN cancel_requested_at IS NULL THEN 'interrupted' ELSE 'cancelled' END, \
                 ended_at = {now}, lease_expires_at = NULL \
                 WHERE state = 'executing' AND lease_expires_at < {now} \
                 RETURNING {s:id}, {s:thread_id}, lease_holder, state",
                &[],
            )
            .await?;
        let mut closed = Vec::with_capacity(rows.len());
        for row in &rows {
            let state = if row.text(3)? == "interrupted" {
                "interrupted"
            } else {
                "cancelled"
            };
            let run = ClosedRun {
                run_id: row.text(0)?,
                thread_id: row.text(1)?,
                instance: row.opt_text(2)?.unwrap_or_default(),
                state,
            };
            push_event(&mut tx, &run.run_id, state).await?;
            closed.push(run);
        }
        tx.commit().await?;
        Ok(closed)
    }

    async fn append_progress(
        &self,
        run_id: &str,
        instance: &str,
        line: &str,
    ) -> Result<ProgressWrite, Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        if tx
            .fetch_optional(
                "SELECT 1 FROM run_records WHERE id = $1{uuid} AND state = 'executing' AND lease_holder = $2{lock}",
                &[text(run_id), text(instance)],
            )
            .await?
            .is_none()
        {
            return Err(RunQueueError::LeaseLost.into());
        }
        let top = tx
            .fetch_optional(
                "SELECT coalesce(max(seq), 0) FROM run_events WHERE run_id = $1{uuid}",
                &[text(run_id)],
            )
            .await?
            .ok_or_else(|| corrupt(()))?
            .int(0)?;
        if top >= MAX_EVENTS_PER_RUN {
            return Ok(ProgressWrite::Full);
        }
        tx.execute(
            EVENT_SQL,
            &[
                text(run_id),
                text("run_progress"),
                json_text(&serde_json::json!({ "line": line })),
            ],
        )
        .await?;
        tx.commit().await?;
        Ok(ProgressWrite::Stored)
    }

    async fn events_after(
        &self,
        run_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<StoredRunEvent>, Fail> {
        let rows = db::fetch_all(
            &self.store,
            "SELECT {s:run_id}, seq, kind, {s:data}, {t:at} FROM run_events \
             WHERE run_id = $1{uuid} AND seq > $2 ORDER BY seq LIMIT $3",
            &[
                text(run_id),
                Arg::Int(i64::try_from(after_seq).unwrap_or(i64::MAX)),
                Arg::Int(i64::from(limit)),
            ],
        )
        .await?;
        rows.iter()
            .map(|row| {
                let kind: RunEventKind = parse_enum(&row.text(2)?)?;
                Ok(StoredRunEvent {
                    run_id: row.text(0)?,
                    seq: row.int(1)?.max(0) as u64,
                    kind,
                    data: serde_json::from_str(&row.text(3)?).map_err(corrupt)?,
                    at: parse_ts(&row.text(4)?)?,
                })
            })
            .collect()
    }

    async fn purge(&self, selection: &Selection, delete: bool) -> Result<DeletedRuns, Fail> {
        let mut tx = Tx::begin(&self.store).await?;
        let (template, args): (String, Vec<Arg>) = match selection {
            Selection::EndedBefore { cutoff, limit } => (
                format!(
                    "SELECT {{s:t.thread_id}} FROM run_threads t \
                     WHERE EXISTS (SELECT 1 FROM run_records r WHERE r.thread_id = t.thread_id AND r.ended_at < $1{{ts}}) \
                     AND NOT EXISTS (SELECT 1 FROM run_records r WHERE r.thread_id = t.thread_id \
                     AND (r.ended_at IS NULL OR r.ended_at >= $1{{ts}})) \
                     ORDER BY t.thread_id LIMIT {}",
                    (*limit).clamp(1, MAX_DELETE_LIMIT)
                ),
                vec![Arg::Text(cutoff.to_wire())],
            ),
            Selection::Asker { connection, asker } => (
                "SELECT {s:t.thread_id} FROM run_threads t \
                 WHERE (t.started_by_kind = 'chat' AND t.started_by_id = $1 AND t.started_by_asker = $2) \
                 OR EXISTS (SELECT 1 FROM run_records r WHERE r.thread_id = t.thread_id \
                 AND r.started_by_kind = 'chat' AND r.started_by_id = $1 AND r.started_by_asker = $2) \
                 ORDER BY t.thread_id"
                    .to_owned(),
                vec![text(connection), text(asker)],
            ),
            Selection::Scope(scope) => (
                "SELECT {s:t.thread_id} FROM run_threads t \
                 WHERE EXISTS (SELECT 1 FROM run_records r WHERE r.thread_id = t.thread_id AND r.scope_id = $1{uuid}) \
                 ORDER BY t.thread_id"
                    .to_owned(),
                vec![Arg::Text(scope.0.hyphenated().to_string())],
            ),
        };
        let thread_ids: Vec<String> = tx
            .fetch_all(&template, &args)
            .await?
            .iter()
            .map(|row| row.text(0))
            .collect::<Result<_, _>>()?;
        let mut run_ids = Vec::new();
        for chunk in thread_ids.chunks(500) {
            let args: Vec<Arg> = chunk.iter().map(|t| text(t)).collect();
            let rows = tx
                .fetch_all(
                    &format!(
                        "SELECT {{s:id}} FROM run_records WHERE thread_id IN ({}) ORDER BY seq",
                        db::placeholders(1, args.len(), "{uuid}")
                    ),
                    &args,
                )
                .await?;
            for row in &rows {
                run_ids.push(row.text(0)?);
            }
            if delete {
                tx.execute(
                    &format!(
                        "DELETE FROM run_threads WHERE thread_id IN ({})",
                        db::placeholders(1, args.len(), "{uuid}")
                    ),
                    &args,
                )
                .await?;
            }
        }
        if delete {
            tx.commit().await?;
        }
        Ok(DeletedRuns {
            runs: run_ids.len() as u64,
            thread_ids,
            run_ids,
        })
    }
}
