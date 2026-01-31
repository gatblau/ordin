//! The SQLite statements of the `crew_runs`, `rounds` and `crew_step_states` tables
//! (CrewOrchestrator.md *Data Model*). Every statement is written once, with bound values; no
//! statement updates a round after `closed_at` is set (`ROUND_IMMUTABLE`).

use ordin_types::common::InputUsed;
use ordin_types::domain::runs::{
    CrewBudgetCounters, CrewCharter, CrewPauseReason, CrewRun, CrewRunState, Round,
};
use ordin_types::time::Timestamp;
use ordin_types::{CrewCharterId, CrewRunId, RunId, WorkspaceId};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::{Row, SqlitePool};

use crate::guard::Counters;
use crate::types::{CrewInput, CrewMode, StepOutcome};

/// A text column holding a serde enum or timestamp value.
pub(crate) fn enum_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

pub(crate) fn from_text<T: DeserializeOwned>(text: &str) -> Result<T, sqlx::Error> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, sqlx::Error> {
    serde_json::from_str(text).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

fn to_json<T: Serialize>(value: &T) -> Result<String, sqlx::Error> {
    serde_json::to_string(value).map_err(|e| sqlx::Error::Encode(Box::new(e)))
}

/// A crew run with everything the driver needs to rebuild itself.
#[derive(Debug, Clone)]
pub(crate) struct StoredCrew {
    pub run: CrewRun,
    pub charter: CrewCharter,
    pub mode: CrewMode,
    pub inputs: Vec<CrewInput>,
}

/// One row of `crew_step_states`.
#[derive(Debug, Clone)]
pub(crate) struct StepRow {
    pub position: String,
    pub state: String,
    pub attempts: u32,
    pub run_id: Option<RunId>,
    pub outcome: Option<StepOutcome>,
    pub decided_path: Option<String>,
    pub passes: Option<u32>,
}

fn crew_from(row: &sqlx::sqlite::SqliteRow) -> Result<StoredCrew, sqlx::Error> {
    let id: String = row.try_get("id")?;
    let workspace: String = row.try_get("workspace_id")?;
    let charter_id: String = row.try_get("crew_charter_id")?;
    let charter: String = row.try_get("charter")?;
    let mode: String = row.try_get("mode")?;
    let workflow_id: Option<String> = row.try_get("workflow_id")?;
    let workflow_version: Option<i64> = row.try_get("workflow_version")?;
    let inputs: String = row.try_get("inputs")?;
    let state: String = row.try_get("state")?;
    let pause: Option<String> = row.try_get("pause_reason")?;
    let started: String = row.try_get("started_at")?;
    let ended: Option<String> = row.try_get("ended_at")?;
    let decode = |e: ordin_types::TypesError| sqlx::Error::Decode(Box::new(e));
    let inputs: Vec<CrewInput> = from_json(&inputs)?;
    let used: Vec<InputUsed> = inputs.iter().flat_map(|i| i.used.clone()).collect();
    let run = CrewRun {
        id: CrewRunId::parse(&id).map_err(decode)?,
        workspace_id: WorkspaceId::parse(&workspace).map_err(decode)?,
        crew_charter_id: CrewCharterId::parse(&charter_id).map_err(decode)?,
        charter_version: u32::try_from(row.try_get::<i64, _>("charter_version")?).unwrap_or(0),
        workflow_id: match workflow_id {
            Some(w) => Some(ordin_types::WorkflowId::parse(&w).map_err(decode)?),
            None => None,
        },
        workflow_version: workflow_version.and_then(|v| u32::try_from(v).ok()),
        taken_over_from_step: row.try_get("taken_over_from_step")?,
        inputs: used,
        state: from_text(&state)?,
        pause_reason: match pause {
            Some(p) => Some(from_text::<CrewPauseReason>(&p)?),
            None => None,
        },
        round_counter: u32::try_from(row.try_get::<i64, _>("round_counter")?).unwrap_or(0),
        budget: CrewBudgetCounters {
            requests_used: u32::try_from(row.try_get::<i64, _>("requests_used")?).unwrap_or(0),
            wall_clock_used_secs: u32::try_from(row.try_get::<i64, _>("wall_clock_used_secs")?)
                .unwrap_or(0),
            repeated_failures: u32::try_from(row.try_get::<i64, _>("repeated_failures")?)
                .unwrap_or(0),
        },
        end_reason: row.try_get("end_reason")?,
        started_at: from_text::<Timestamp>(&started)?,
        ended_at: match ended {
            Some(e) => Some(from_text::<Timestamp>(&e)?),
            None => None,
        },
    };
    Ok(StoredCrew {
        run,
        charter: from_json(&charter)?,
        mode: from_text(&mode)?,
        inputs,
    })
}

/// B.4: inserts the `starting` row.
pub(crate) async fn insert_run(
    pool: &SqlitePool,
    id: CrewRunId,
    charter: &CrewCharter,
    mode: CrewMode,
    inputs: &[CrewInput],
    started_at: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO crew_runs (id, workspace_id, crew_charter_id, charter_version, charter, mode, \
         workflow_id, workflow_version, inputs, state, started_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'starting', ?10)",
    )
    .bind(id.to_string())
    .bind(charter.workspace_id.to_string())
    .bind(charter.id.to_string())
    .bind(i64::from(charter.charter_version))
    .bind(to_json(charter)?)
    .bind(enum_text(&mode))
    .bind(charter.workflow_id.map(|w| w.to_string()))
    .bind(charter.workflow_version.map(i64::from))
    .bind(to_json(&inputs)?)
    .bind(started_at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn get_crew(
    pool: &SqlitePool,
    id: CrewRunId,
) -> Result<Option<StoredCrew>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, workspace_id, crew_charter_id, charter_version, charter, mode, \
         workflow_id, workflow_version, taken_over_from_step, inputs, state, pause_reason, \
         round_counter, requests_used, wall_clock_used_secs, repeated_failures, end_reason, \
         started_at, ended_at FROM crew_runs WHERE id = ?1",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;
    match row {
        Some(row) => Ok(Some(crew_from(&row)?)),
        None => Ok(None),
    }
}

/// Sets the state, the pause reason and (for an ended state) the end reason and time.
pub(crate) async fn set_state(
    pool: &SqlitePool,
    id: CrewRunId,
    state: CrewRunState,
    pause: Option<CrewPauseReason>,
    end: Option<(&str, Timestamp)>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE crew_runs SET state = ?2, pause_reason = ?3, \
         end_reason = COALESCE(?4, end_reason), ended_at = COALESCE(?5, ended_at) WHERE id = ?1",
    )
    .bind(id.to_string())
    .bind(enum_text(&state))
    .bind(pause.map(|p| enum_text(&p)))
    .bind(end.map(|(reason, _)| reason.to_owned()))
    .bind(end.map(|(_, at)| at.to_wire()))
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn set_counters(
    pool: &SqlitePool,
    id: CrewRunId,
    counters: &Counters,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE crew_runs SET round_counter = ?2, requests_used = ?3, wall_clock_used_secs = ?4, \
         repeated_failures = ?5 WHERE id = ?1",
    )
    .bind(id.to_string())
    .bind(i64::from(counters.round_counter))
    .bind(i64::from(counters.requests_used))
    .bind(i64::from(counters.wall_clock_used_secs))
    .bind(i64::from(counters.repeated_failures))
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn set_taken_over(
    pool: &SqlitePool,
    id: CrewRunId,
    position: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE crew_runs SET taken_over_from_step = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(position)
        .execute(pool)
        .await?;
    Ok(())
}

/// I2: the crew run of a workspace that is starting, running or paused, if any.
pub(crate) async fn running_crew(
    pool: &SqlitePool,
    workspace: WorkspaceId,
) -> Result<Option<CrewRunId>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id FROM crew_runs WHERE workspace_id = ?1 AND state IN ('starting','running','paused') \
         ORDER BY started_at DESC LIMIT 1",
    )
    .bind(workspace.to_string())
    .fetch_optional(pool)
    .await?;
    match row {
        Some(row) => {
            let id: String = row.try_get("id")?;
            CrewRunId::parse(&id)
                .map(Some)
                .map_err(|e| sqlx::Error::Decode(Box::new(e)))
        }
        None => Ok(None),
    }
}

/// J.1: the identifiers of every crew run in progress.
pub(crate) async fn in_progress(pool: &SqlitePool) -> Result<Vec<CrewRunId>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id FROM crew_runs WHERE state IN ('starting','running','paused') ORDER BY started_at",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let id: String = row.try_get("id")?;
            CrewRunId::parse(&id).map_err(|e| sqlx::Error::Decode(Box::new(e)))
        })
        .collect()
}

pub(crate) async fn insert_round(
    pool: &SqlitePool,
    crew: CrewRunId,
    number: u32,
    decision: &str,
    opened_at: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO rounds (crew_run_id, number, coordinator_decision, guardrail_decisions, \
         member_runs, opened_at) VALUES (?1, ?2, ?3, '[]', '[]', ?4)",
    )
    .bind(crew.to_string())
    .bind(i64::from(number))
    .bind(decision)
    .bind(opened_at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

/// Records the member runs of a round that is still open, so a restart can find them (J.2).
pub(crate) async fn set_round_runs(
    pool: &SqlitePool,
    crew: CrewRunId,
    number: u32,
    runs: &[RunId],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE rounds SET member_runs = ?3 WHERE crew_run_id = ?1 AND number = ?2 AND closed_at IS NULL",
    )
    .bind(crew.to_string())
    .bind(i64::from(number))
    .bind(to_json(&runs)?)
    .execute(pool)
    .await?;
    Ok(())
}

/// Closes a round once; the `closed_at IS NULL` guard is what keeps a closed round immutable.
pub(crate) async fn close_round(
    pool: &SqlitePool,
    crew: CrewRunId,
    number: u32,
    decisions: &[String],
    runs: &[RunId],
    closed_at: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE rounds SET guardrail_decisions = ?3, member_runs = ?4, closed_at = ?5 \
         WHERE crew_run_id = ?1 AND number = ?2 AND closed_at IS NULL",
    )
    .bind(crew.to_string())
    .bind(i64::from(number))
    .bind(to_json(&decisions)?)
    .bind(to_json(&runs)?)
    .bind(closed_at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn list_rounds(
    pool: &SqlitePool,
    crew: CrewRunId,
) -> Result<Vec<Round>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT number, coordinator_decision, guardrail_decisions, member_runs, opened_at, closed_at \
         FROM rounds WHERE crew_run_id = ?1 ORDER BY number",
    )
    .bind(crew.to_string())
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let decisions: String = row.try_get("guardrail_decisions")?;
            let runs: String = row.try_get("member_runs")?;
            let opened: String = row.try_get("opened_at")?;
            let closed: Option<String> = row.try_get("closed_at")?;
            Ok(Round {
                crew_run_id: crew,
                number: u32::try_from(row.try_get::<i64, _>("number")?).unwrap_or(0),
                coordinator_decision: row.try_get("coordinator_decision")?,
                guardrail_decisions: from_json(&decisions)?,
                member_runs: from_json(&runs)?,
                opened_at: from_text(&opened)?,
                closed_at: match closed {
                    Some(c) => Some(from_text(&c)?),
                    None => None,
                },
            })
        })
        .collect()
}

/// The upsert of one step's cursor row (D: persisted after every step).
pub(crate) async fn put_step(
    pool: &SqlitePool,
    crew: CrewRunId,
    row: &StepRow,
) -> Result<(), sqlx::Error> {
    let outcome = match &row.outcome {
        Some(o) => Some(to_json(o)?),
        None => None,
    };
    sqlx::query(
        "INSERT INTO crew_step_states (crew_run_id, position, state, attempts, run_id, outcome, \
         decided_path, passes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT (crew_run_id, position) DO UPDATE SET state = excluded.state, \
         attempts = excluded.attempts, run_id = excluded.run_id, outcome = excluded.outcome, \
         decided_path = excluded.decided_path, passes = excluded.passes",
    )
    .bind(crew.to_string())
    .bind(&row.position)
    .bind(&row.state)
    .bind(i64::from(row.attempts))
    .bind(row.run_id.map(|r| r.to_string()))
    .bind(outcome)
    .bind(&row.decided_path)
    .bind(row.passes.map(i64::from))
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn list_steps(
    pool: &SqlitePool,
    crew: CrewRunId,
) -> Result<Vec<StepRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT position, state, attempts, run_id, outcome, decided_path, passes \
         FROM crew_step_states WHERE crew_run_id = ?1",
    )
    .bind(crew.to_string())
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let run_id: Option<String> = row.try_get("run_id")?;
            let outcome: Option<String> = row.try_get("outcome")?;
            let passes: Option<i64> = row.try_get("passes")?;
            Ok(StepRow {
                position: row.try_get("position")?,
                state: row.try_get("state")?,
                attempts: u32::try_from(row.try_get::<i64, _>("attempts")?).unwrap_or(0),
                run_id: match run_id {
                    Some(r) => {
                        Some(RunId::parse(&r).map_err(|e| sqlx::Error::Decode(Box::new(e)))?)
                    }
                    None => None,
                },
                outcome: match outcome {
                    Some(o) => Some(from_json(&o)?),
                    None => None,
                },
                decided_path: row.try_get("decided_path")?,
                passes: passes.and_then(|p| u32::try_from(p).ok()),
            })
        })
        .collect()
}

/// Data Model: deletes ended crew runs older than `cutoff` with their rounds and step states.
pub(crate) async fn prune(pool: &SqlitePool, cutoff: Timestamp) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "DELETE FROM crew_step_states WHERE crew_run_id IN \
         (SELECT id FROM crew_runs WHERE ended_at IS NOT NULL AND ended_at < ?1)",
    )
    .bind(cutoff.to_wire())
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM rounds WHERE crew_run_id IN \
         (SELECT id FROM crew_runs WHERE ended_at IS NOT NULL AND ended_at < ?1)",
    )
    .bind(cutoff.to_wire())
    .execute(&mut *tx)
    .await?;
    let removed = sqlx::query("DELETE FROM crew_runs WHERE ended_at IS NOT NULL AND ended_at < ?1")
        .bind(cutoff.to_wire())
        .execute(&mut *tx)
        .await?
        .rows_affected();
    tx.commit().await?;
    Ok(removed)
}
