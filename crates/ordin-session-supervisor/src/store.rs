//! The `tasks` and `runs` tables (SessionSupervisor.md *Data Model*). Reads and writes only:
//! validation, events and logging belong to the supervisor.

use ordin_journal::Datastore;
use ordin_runtime_adapter::{AdapterKind, RunLimits};
use ordin_types::common::{AgentRef, InputUsed};
use ordin_types::domain::runs::{
    Run, RunOrigin, RunPauseReason, RunState, Task, TaskArgument, TaskTarget, UsageCounters,
};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{ApprovalId, ArtefactId, RunId, TaskId, WorkspaceId};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::db::{self, engine_of, placeholders, Arg, DbRow, Engine};

/// A `runs` row: the 2C `Run` plus the columns only this module reads.
#[derive(Debug, Clone)]
pub(crate) struct RunRow {
    pub run: Run,
    pub workspace_id: WorkspaceId,
    pub pause_approval_id: Option<ApprovalId>,
    pub work_key: String,
    pub command: Option<String>,
}

/// The snake_case text of a unit enum variant (its serde form).
pub(crate) fn enum_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

fn from_text<T: DeserializeOwned>(text: &str) -> Result<T, sqlx::Error> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, sqlx::Error> {
    serde_json::from_str(text).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

fn json_text<T: Serialize>(value: &T) -> Result<String, sqlx::Error> {
    serde_json::to_string(value).map_err(|e| sqlx::Error::Encode(Box::new(e)))
}

fn timestamp(text: &str) -> Result<Timestamp, sqlx::Error> {
    parse_rfc3339_ms_utc(text)
        .map(Timestamp)
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

fn opt_timestamp(text: Option<String>) -> Result<Option<Timestamp>, sqlx::Error> {
    text.as_deref().map(timestamp).transpose()
}

fn u32_of(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn u64_of(value: Option<i64>) -> Option<u64> {
    value.map(|v| u64::try_from(v.max(0)).unwrap_or(0))
}

fn i64_of(value: Option<u64>) -> Option<i64> {
    value.map(|v| i64::try_from(v).unwrap_or(i64::MAX))
}

const RUN_COLUMNS: &str =
    "{s:id}, {s:origin}, {s:workspace_id}, {s:agent}, adapter, runtime_session_id, \
     remote_run_id, remote_thread_id, state, pause_reason, {s:pause_approval_id}, wall_clock_secs, \
     request_budget, idle_timeout_secs, model_turns, tool_calls, model_level, tokens, \
     tokens_from_cache, work_key, command, {t:started_at}, {t:ended_at}, end_reason, \
     {s:inputs_used}, {t:queued_at}";

fn run_row(row: &DbRow) -> Result<RunRow, sqlx::Error> {
    let id = RunId::parse(&row.text(0)?).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let origin: RunOrigin = from_json(&row.text(1)?)?;
    let workspace_id =
        WorkspaceId::parse(&row.text(2)?).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let agent: AgentRef = from_json(&row.text(3)?)?;
    let adapter: AdapterKind = from_text(&row.text(4)?)?;
    let state: RunState = from_text(&row.text(8)?)?;
    let pause_reason: Option<RunPauseReason> =
        row.opt_text(9)?.as_deref().map(from_text).transpose()?;
    let pause_approval_id = row
        .opt_text(10)?
        .map(|t| ApprovalId::parse(&t))
        .transpose()
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let budget = row.int(12)?;
    let inputs_used: Vec<InputUsed> = from_json(&row.text(24)?)?;
    let run = Run {
        id,
        origin,
        agent,
        adapter,
        runtime_session_id: row.opt_text(5)?,
        remote_run_id: row.opt_text(6)?,
        remote_thread_id: row.opt_text(7)?,
        state,
        pause_reason,
        limits: RunLimits {
            wall_clock_secs: u32_of(row.int(11)?),
            request_budget: (budget > 0).then(|| u32_of(budget)),
            idle_timeout_secs: u32_of(row.int(13)?),
        },
        usage: UsageCounters {
            model_turns: u32_of(row.int(14)?),
            tool_calls: u32_of(row.int(15)?),
            model_level: row.opt_text(16)?,
            tokens: u64_of(row.opt_int(17)?),
            tokens_from_cache: u64_of(row.opt_int(18)?),
        },
        started_at: opt_timestamp(row.opt_text(21)?)?,
        ended_at: opt_timestamp(row.opt_text(22)?)?,
        end_reason: row.opt_text(23)?,
        inputs_used,
        queued_at: timestamp(&row.text(25)?)?,
    };
    Ok(RunRow {
        run,
        workspace_id,
        pause_approval_id,
        work_key: row.text(19)?,
        command: row.opt_text(20)?,
    })
}

/// Records a task. There is no update statement for `tasks` (`TASK_IMMUTABLE`).
pub(crate) async fn insert_task(store: &Datastore, task: &Task) -> Result<(), sqlx::Error> {
    db::execute(
        store,
        "INSERT INTO tasks (id, workspace_id, target, command, free_text, arguments, \
         attached_artefacts, run_again_of, requested_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        &[
            Arg::Text(task.id.to_string()),
            Arg::Text(task.workspace_id.to_string()),
            Arg::Text(json_text(&task.target)?),
            Arg::OptText(task.command.clone()),
            Arg::OptText(task.free_text.clone()),
            Arg::Text(json_text(&task.arguments)?),
            Arg::Text(json_text(&task.attached_artefacts)?),
            Arg::OptText(task.run_again_of.map(|t| t.to_string())),
            Arg::Text(task.requested_at.to_wire()),
        ],
    )
    .await?;
    Ok(())
}

/// A recorded task (SQLite only: the agent profile has no `tasks` table).
pub(crate) async fn task(store: &Datastore, id: TaskId) -> Result<Option<Task>, sqlx::Error> {
    if engine_of(store) == Engine::Postgres {
        return Ok(None);
    }
    let Some(row) = db::fetch_optional(
        store,
        "SELECT id, workspace_id, target, command, free_text, arguments, attached_artefacts, \
         run_again_of, requested_at FROM tasks WHERE id = $1",
        &[Arg::Text(id.to_string())],
    )
    .await?
    else {
        return Ok(None);
    };
    let decode = |e: ordin_types::TypesError| sqlx::Error::Decode(Box::new(e));
    let target: TaskTarget = from_json(&row.text(2)?)?;
    let arguments: Vec<TaskArgument> = from_json(&row.text(5)?)?;
    let attached_artefacts: Vec<ArtefactId> = from_json(&row.text(6)?)?;
    Ok(Some(Task {
        id: TaskId::parse(&row.text(0)?).map_err(decode)?,
        workspace_id: WorkspaceId::parse(&row.text(1)?).map_err(decode)?,
        target,
        command: row.opt_text(3)?,
        free_text: row.opt_text(4)?,
        arguments,
        attached_artefacts,
        run_again_of: row
            .opt_text(7)?
            .map(|t| TaskId::parse(&t))
            .transpose()
            .map_err(decode)?,
        requested_at: timestamp(&row.text(8)?)?,
    }))
}

fn origin_columns(origin: &RunOrigin) -> (Option<String>, Option<String>, Option<i64>) {
    match origin {
        RunOrigin::Task { task_id } => (Some(task_id.to_string()), None, None),
        RunOrigin::CrewRound {
            crew_run_id,
            round_number,
        } => (
            None,
            Some(crew_run_id.to_string()),
            Some(i64::from(*round_number)),
        ),
        RunOrigin::Conversation { .. } => (None, None, None),
    }
}

pub(crate) async fn insert_run(store: &Datastore, row: &RunRow) -> Result<(), sqlx::Error> {
    let run = &row.run;
    let (task_id, crew_run_id, round) = origin_columns(&run.origin);
    db::execute(
        store,
        "INSERT INTO runs (id, origin, task_id, crew_run_id, round_number, workspace_id, agent, \
         adapter, runtime_session_id, remote_run_id, remote_thread_id, state, pause_reason, \
         pause_approval_id, wall_clock_secs, request_budget, idle_timeout_secs, model_turns, \
         tool_calls, model_level, tokens, tokens_from_cache, work_key, command, started_at, \
         ended_at, end_reason, inputs_used, queued_at) VALUES ($1{uuid}, $2{json}, $3{uuid}, \
         $4{uuid}, $5, $6{uuid}, $7{json}, $8, $9, $10, $11, $12, $13, $14{uuid}, $15, $16, $17, \
         $18, $19, $20, $21, $22, $23, $24, $25{ts}, $26{ts}, $27, $28{json}, $29{ts})",
        &[
            Arg::Text(run.id.to_string()),
            Arg::Text(json_text(&run.origin)?),
            Arg::OptText(task_id),
            Arg::OptText(crew_run_id),
            Arg::OptInt(round),
            Arg::Text(row.workspace_id.to_string()),
            Arg::Text(json_text(&run.agent)?),
            Arg::Text(enum_text(&run.adapter)),
            Arg::OptText(run.runtime_session_id.clone()),
            Arg::OptText(run.remote_run_id.clone()),
            Arg::OptText(run.remote_thread_id.clone()),
            Arg::Text(enum_text(&run.state)),
            Arg::OptText(run.pause_reason.map(|r| enum_text(&r))),
            Arg::OptText(row.pause_approval_id.map(|a| a.to_string())),
            Arg::Int(i64::from(run.limits.wall_clock_secs)),
            Arg::Int(i64::from(run.limits.request_budget.unwrap_or(0))),
            Arg::Int(i64::from(run.limits.idle_timeout_secs)),
            Arg::Int(i64::from(run.usage.model_turns)),
            Arg::Int(i64::from(run.usage.tool_calls)),
            Arg::OptText(run.usage.model_level.clone()),
            Arg::OptInt(i64_of(run.usage.tokens)),
            Arg::OptInt(i64_of(run.usage.tokens_from_cache)),
            Arg::Text(row.work_key.clone()),
            Arg::OptText(row.command.clone()),
            Arg::OptText(run.started_at.map(|t| t.to_wire())),
            Arg::OptText(run.ended_at.map(|t| t.to_wire())),
            Arg::OptText(run.end_reason.clone()),
            Arg::Text(json_text(&run.inputs_used)?),
            Arg::Text(run.queued_at.to_wire()),
        ],
    )
    .await?;
    Ok(())
}

/// Writes every column an event can change.
pub(crate) async fn save_run(store: &Datastore, row: &RunRow) -> Result<(), sqlx::Error> {
    let run = &row.run;
    db::execute(
        store,
        "UPDATE runs SET runtime_session_id = $2, remote_run_id = $3, remote_thread_id = $4, \
         state = $5, pause_reason = $6, pause_approval_id = $7{uuid}, model_turns = $8, \
         tool_calls = $9, model_level = $10, tokens = $11, tokens_from_cache = $12, \
         started_at = $13{ts}, ended_at = $14{ts}, end_reason = $15 WHERE id = $1{uuid}",
        &[
            Arg::Text(run.id.to_string()),
            Arg::OptText(run.runtime_session_id.clone()),
            Arg::OptText(run.remote_run_id.clone()),
            Arg::OptText(run.remote_thread_id.clone()),
            Arg::Text(enum_text(&run.state)),
            Arg::OptText(run.pause_reason.map(|r| enum_text(&r))),
            Arg::OptText(row.pause_approval_id.map(|a| a.to_string())),
            Arg::Int(i64::from(run.usage.model_turns)),
            Arg::Int(i64::from(run.usage.tool_calls)),
            Arg::OptText(run.usage.model_level.clone()),
            Arg::OptInt(i64_of(run.usage.tokens)),
            Arg::OptInt(i64_of(run.usage.tokens_from_cache)),
            Arg::OptText(run.started_at.map(|t| t.to_wire())),
            Arg::OptText(run.ended_at.map(|t| t.to_wire())),
            Arg::OptText(run.end_reason.clone()),
        ],
    )
    .await?;
    Ok(())
}

pub(crate) async fn get_run(store: &Datastore, id: RunId) -> Result<Option<RunRow>, sqlx::Error> {
    db::fetch_optional(
        store,
        &format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = $1{{uuid}}"),
        &[Arg::Text(id.to_string())],
    )
    .await?
    .as_ref()
    .map(run_row)
    .transpose()
}

/// Runs in `queued`, `running` or `paused`, oldest first (G.1).
pub(crate) async fn in_progress(store: &Datastore) -> Result<Vec<RunRow>, sqlx::Error> {
    db::fetch_all(
        store,
        &format!(
            "SELECT {RUN_COLUMNS} FROM runs WHERE state IN ('queued', 'running', 'paused') \
             ORDER BY queued_at, id"
        ),
        &[],
    )
    .await?
    .iter()
    .map(run_row)
    .collect()
}

/// One keyset page, newest first by `(queued_at, id)`; `after` is the last row's key.
pub(crate) async fn list(
    store: &Datastore,
    workspace_id: Option<WorkspaceId>,
    after: Option<(String, String)>,
    limit: usize,
) -> Result<Vec<RunRow>, sqlx::Error> {
    let mut clauses = Vec::new();
    let mut args = Vec::new();
    if let Some(ws) = workspace_id {
        args.push(Arg::Text(ws.to_string()));
        clauses.push(format!("workspace_id = ${}{{uuid}}", args.len()));
    }
    if let Some((queued_at, id)) = after {
        args.push(Arg::Text(queued_at));
        let at = args.len();
        args.push(Arg::Text(id));
        let id_at = args.len();
        clauses.push(format!(
            "(queued_at < ${at}{{ts}} OR (queued_at = ${at}{{ts}} AND id < ${id_at}{{uuid}}))"
        ));
    }
    let filter = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    args.push(Arg::Int(i64::try_from(limit).unwrap_or(i64::MAX)));
    let limit_at = args.len();
    db::fetch_all(
        store,
        &format!(
            "SELECT {RUN_COLUMNS} FROM runs {filter} ORDER BY queued_at DESC, id DESC LIMIT ${limit_at}"
        ),
        &args,
    )
    .await?
    .iter()
    .map(run_row)
    .collect()
}

/// J.4: runs queued, running or paused with `command`.
pub(crate) async fn count_with_command(
    store: &Datastore,
    command: &str,
) -> Result<u32, sqlx::Error> {
    let row = db::fetch_optional(
        store,
        "SELECT count(*) FROM runs WHERE command = $1 AND state IN ('queued', 'running', 'paused')",
        &[Arg::Text(command.to_owned())],
    )
    .await?;
    Ok(row.map(|r| r.int(0)).transpose()?.map_or(0, u32_of))
}

/// J.4: whether a paused run other than `except` has one of `commands`.
pub(crate) async fn paused_with_command(
    store: &Datastore,
    commands: &[String],
    except: RunId,
) -> Result<bool, sqlx::Error> {
    if commands.is_empty() {
        return Ok(false);
    }
    let mut args = vec![Arg::Text(except.to_string())];
    args.extend(commands.iter().map(|c| Arg::Text(c.clone())));
    let row = db::fetch_optional(
        store,
        &format!(
            "SELECT {{s:id}} FROM runs WHERE state = 'paused' AND id <> $1{{uuid}} AND command IN ({}) LIMIT 1",
            placeholders(2, commands.len(), "")
        ),
        &args,
    )
    .await?;
    Ok(row.is_some())
}

/// H.4: the paused run waiting for `approval_id`.
pub(crate) async fn paused_with_approval(
    store: &Datastore,
    approval_id: ApprovalId,
) -> Result<Option<RunId>, sqlx::Error> {
    let row = db::fetch_optional(
        store,
        "SELECT {s:id} FROM runs WHERE state = 'paused' AND pause_approval_id = $1{uuid} LIMIT 1",
        &[Arg::Text(approval_id.to_string())],
    )
    .await?;
    row.map(|r| {
        r.text(0)
            .and_then(|t| RunId::parse(&t).map_err(|e| sqlx::Error::Decode(Box::new(e))))
    })
    .transpose()
}

/// Data Model *Pruning*: runs ended before `cutoff` (never one in progress or listed in
/// `keep`), then tasks whose runs are all deleted.
pub(crate) async fn prune(
    store: &Datastore,
    cutoff: Timestamp,
    keep: &[RunId],
) -> Result<u64, sqlx::Error> {
    let mut args = vec![Arg::Text(cutoff.to_wire())];
    args.extend(keep.iter().map(|id| Arg::Text(id.to_string())));
    let keep_clause = if keep.is_empty() {
        String::new()
    } else {
        format!(" AND id NOT IN ({})", placeholders(2, keep.len(), "{uuid}"))
    };
    let mut removed = db::execute(
        store,
        &format!(
            "DELETE FROM runs WHERE ended_at IS NOT NULL AND ended_at < $1{{ts}} \
             AND state NOT IN ('queued', 'running', 'paused'){keep_clause}"
        ),
        &args,
    )
    .await?;
    if engine_of(store) == Engine::Sqlite {
        removed += db::execute(
            store,
            "DELETE FROM tasks WHERE requested_at < $1 \
             AND NOT EXISTS (SELECT 1 FROM runs WHERE runs.task_id = tasks.id)",
            &[Arg::Text(cutoff.to_wire())],
        )
        .await?;
    }
    Ok(removed)
}
