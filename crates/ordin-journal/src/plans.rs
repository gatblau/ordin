//! Plans and step marks (Internal Logic 13; ADR-068) and the run index reads (Internal Logic 6.4).

use ordin_types::domain::runs::{MarkSet, RunStep, StepMark};
use ordin_types::text::check_len;
use ordin_types::time::Timestamp;
use ordin_types::{RunId, RunStepId};

use crate::codec;
use crate::db::{self, Arg, DbRow, Tx};
use crate::error::JournalError;
use crate::journal::{check_event, transition_allowed, Journal};
use crate::types::{PlannedStep, RunIndexEntry};

/// A plan holds 3 to 7 steps (2C RunStep).
const MIN_STEPS: usize = 3;
const MAX_STEPS: usize = 7;
/// At most 20 plan versions per run (LW-35).
const MAX_PLAN_VERSIONS: u32 = 20;
/// The run index reads return at most 500 rows (Internal Logic 6.4).
const MAX_RUN_ROWS: i64 = 500;

pub(crate) const STEP_COLUMNS: &str =
    "{s:id}, {s:run_id}, plan_version, position, line, came_from, \
    mark, {s:marks_set}, {s:events_seen}";

pub(crate) fn step_from(row: &DbRow) -> Result<RunStep, JournalError> {
    Ok(RunStep {
        id: RunStepId(codec::uuid(&row.text(0)?)?),
        run_id: RunId(codec::uuid(&row.text(1)?)?),
        plan_version: codec::u32_of(row.int(2)?)?,
        position: codec::u32_of(row.int(3)?)?,
        line: row.text(4)?,
        came_from: codec::from_name(&row.text(5)?)?,
        mark: codec::from_name(&row.text(6)?)?,
        marks_set: codec::from_json(&row.text(7)?)?,
        events_seen: codec::from_json(&row.text(8)?)?,
    })
}

const RUN_INDEX_COLUMNS: &str = "{s:run_id}, state, {t:first_seen_at}, {t:ended_at}";

fn run_index_from(row: &DbRow) -> Result<RunIndexEntry, JournalError> {
    Ok(RunIndexEntry {
        run_id: RunId(codec::uuid(&row.text(0)?)?),
        state: codec::from_name(&row.text(1)?)?,
        first_seen_at: codec::ts(&row.text(2)?)?,
        ended_at: codec::opt_ts(row.opt_text(3)?)?,
    })
}

/// The steps of the run's highest plan version, in position order.
async fn highest_plan(tx: &mut Tx, run_id: RunId) -> Result<Vec<RunStep>, JournalError> {
    let rows = tx
        .fetch_all(
            &format!(
                "SELECT {STEP_COLUMNS} FROM run_steps WHERE run_id = $1{{uuid}} \
                 AND plan_version = (SELECT MAX(plan_version) FROM run_steps WHERE run_id = $1{{uuid}}) \
                 ORDER BY position"
            ),
            &[Arg::Text(run_id.to_string())],
        )
        .await?;
    rows.iter().map(step_from).collect()
}

/// `true` when the run index says the run has ended.
async fn run_ended(tx: &mut Tx, run_id: RunId) -> Result<bool, JournalError> {
    let row = tx
        .fetch_optional(
            "SELECT {t:ended_at} FROM journal_runs WHERE run_id = $1{uuid}",
            &[Arg::Text(run_id.to_string())],
        )
        .await?;
    Ok(match row {
        Some(row) => row.opt_text(0)?.is_some(),
        None => false,
    })
}

impl Journal {
    /// Emits `plan_changed`. The new plan version is `RunStep.plan_version` of the result.
    pub async fn state_plan(
        &self,
        run_id: RunId,
        steps: Vec<PlannedStep>,
    ) -> Result<Vec<RunStep>, JournalError> {
        if !(MIN_STEPS..=MAX_STEPS).contains(&steps.len()) {
            return Err(JournalError::invalid("RSTEP_PLAN_SIZE_OUT_OF_RANGE"));
        }
        for step in &steps {
            check_len(&step.line, 1, 80, "RSTEP_LINE_TOO_LONG")?;
        }
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        if run_ended(&mut tx, run_id).await? {
            return Err(JournalError::invalid("RSTEP_IMMUTABLE"));
        }
        let highest = tx
            .fetch_optional(
                "SELECT MAX(plan_version) FROM run_steps WHERE run_id = $1{uuid}",
                &[Arg::Text(run_id.to_string())],
            )
            .await?
            .map(|row| row.opt_int(0))
            .transpose()?
            .flatten()
            .map(codec::u32_of)
            .transpose()?
            .unwrap_or(0);
        let plan_version = highest + 1;
        // The 21st version reuses this code (Internal Logic 13.1).
        if plan_version > MAX_PLAN_VERSIONS {
            return Err(JournalError::invalid("RSTEP_PLAN_VERSION_GAP"));
        }
        let now = self.now();
        let mut stated = Vec::with_capacity(steps.len());
        for (position, step) in (1u32..).zip(steps) {
            let run_step = RunStep {
                id: RunStepId(uuid::Uuid::now_v7()),
                run_id,
                plan_version,
                position,
                line: step.line,
                came_from: step.came_from,
                mark: StepMark::Next,
                marks_set: vec![MarkSet {
                    mark: StepMark::Next,
                    at: now,
                }],
                events_seen: Vec::new(),
            };
            insert_step(&mut tx, &run_step).await?;
            stated.push(run_step);
        }
        // Step 13.2: the event in the same transaction.
        let data = serde_json::json!({
            "run_id": run_id,
            "plan_version": plan_version,
            "steps": stated,
        });
        let text = check_event("plan_changed", &data)?;
        let event = self.append_in_tx(&mut tx, "plan_changed", data, text).await;
        let event = match event {
            Ok(event) => event,
            Err(error) => return Err(self.append_failed("plan_changed", error)),
        };
        tx.commit().await?;
        self.published(event).await;
        Ok(stated)
    }

    /// Acts on the highest plan version; emits `step_marked`.
    pub async fn mark_step(
        &self,
        run_id: RunId,
        position: u32,
        mark: StepMark,
    ) -> Result<RunStep, JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        let plan = highest_plan(&mut tx, run_id).await?;
        let mut step = plan
            .iter()
            .find(|s| s.position == position)
            .cloned()
            .ok_or(JournalError::PlanNotFound)?;
        if run_ended(&mut tx, run_id).await? || !transition_allowed(step.mark, mark) {
            return Err(JournalError::invalid("RSTEP_IMMUTABLE"));
        }
        let another_now = plan
            .iter()
            .any(|s| s.position != position && s.mark == StepMark::Now);
        if mark == StepMark::Now && another_now {
            return Err(JournalError::invalid("RSTEP_IMMUTABLE"));
        }
        step.mark = mark;
        step.marks_set.push(MarkSet {
            mark,
            at: self.now(),
        });
        tx.execute(
            "UPDATE run_steps SET mark = $1, marks_set = $2{json} WHERE id = $3{uuid}",
            &[
                Arg::Text(codec::name_of(&mark)?),
                Arg::Text(codec::to_json(&step.marks_set)?),
                Arg::Text(step.id.to_string()),
            ],
        )
        .await?;
        let position_u8 = u8::try_from(position).map_err(|_| JournalError::PlanNotFound)?;
        let data = serde_json::json!({
            "run_id": run_id,
            "plan_version": step.plan_version,
            "position": position_u8,
            "mark": mark,
        });
        let text = check_event("step_marked", &data)?;
        let event = match self.append_in_tx(&mut tx, "step_marked", data, text).await {
            Ok(event) => event,
            Err(error) => return Err(self.append_failed("step_marked", error)),
        };
        // The event's own side effect may have added its `seq` to this step's `events_seen`.
        let refreshed = tx
            .fetch_optional(
                &format!("SELECT {STEP_COLUMNS} FROM run_steps WHERE id = $1{{uuid}}"),
                &[Arg::Text(step.id.to_string())],
            )
            .await?
            .map(|row| step_from(&row))
            .transpose()?
            .unwrap_or(step);
        tx.commit().await?;
        self.published(event).await;
        Ok(refreshed)
    }

    /// The highest plan version's steps in position order; empty when no plan was stated.
    pub async fn current_plan(&self, run_id: RunId) -> Result<Vec<RunStep>, JournalError> {
        let rows = db::fetch_all(
            &self.inner.store,
            &format!(
                "SELECT {STEP_COLUMNS} FROM run_steps WHERE run_id = $1{{uuid}} \
                 AND plan_version = (SELECT MAX(plan_version) FROM run_steps WHERE run_id = $1{{uuid}}) \
                 ORDER BY position"
            ),
            &[Arg::Text(run_id.to_string())],
        )
        .await?;
        rows.iter().map(step_from).collect()
    }

    // ---- run index (read side, step 6.3)

    /// Runs still in progress, from the Journal's own run index.
    pub async fn runs_in_progress(&self) -> Result<Vec<RunId>, JournalError> {
        let rows = db::fetch_all(
            &self.inner.store,
            "SELECT {s:run_id} FROM journal_runs WHERE ended_at IS NULL ORDER BY run_id",
            &[],
        )
        .await?;
        rows.iter()
            .map(|row| Ok(RunId(codec::uuid(&row.text(0)?)?)))
            .collect()
    }

    /// The same runs with their state and first-seen time, newest first, at most 500.
    pub async fn running_runs(&self) -> Result<Vec<RunIndexEntry>, JournalError> {
        let rows = db::fetch_all(
            &self.inner.store,
            &format!(
                "SELECT {RUN_INDEX_COLUMNS} FROM journal_runs WHERE ended_at IS NULL \
                 ORDER BY first_seen_at DESC LIMIT $1"
            ),
            &[Arg::Int(MAX_RUN_ROWS)],
        )
        .await?;
        rows.iter().map(run_index_from).collect()
    }

    /// Runs that ended at or after `since`, newest first, `limit` clamped to 1–500.
    pub async fn runs_ended_since(
        &self,
        since: Timestamp,
        limit: u32,
    ) -> Result<Vec<RunIndexEntry>, JournalError> {
        let limit = i64::from(limit).clamp(1, MAX_RUN_ROWS);
        let rows = db::fetch_all(
            &self.inner.store,
            &format!(
                "SELECT {RUN_INDEX_COLUMNS} FROM journal_runs WHERE ended_at >= $1{{ts}} \
                 ORDER BY ended_at DESC, run_id DESC LIMIT $2"
            ),
            &[Arg::Text(since.to_wire()), Arg::Int(limit)],
        )
        .await?;
        rows.iter().map(run_index_from).collect()
    }
}

async fn insert_step(tx: &mut Tx, step: &RunStep) -> Result<(), JournalError> {
    tx.execute(
        "INSERT INTO run_steps (id, run_id, plan_version, position, line, came_from, mark, \
         marks_set, events_seen) \
         VALUES ($1{uuid}, $2{uuid}, $3, $4, $5, $6, $7, $8{json}, $9{json})",
        &[
            Arg::Text(step.id.to_string()),
            Arg::Text(step.run_id.to_string()),
            Arg::Int(i64::from(step.plan_version)),
            Arg::Int(i64::from(step.position)),
            Arg::Text(step.line.clone()),
            Arg::Text(codec::name_of(&step.came_from)?),
            Arg::Text(codec::name_of(&step.mark)?),
            Arg::Text(codec::to_json(&step.marks_set)?),
            Arg::Text(codec::to_json(&step.events_seen)?),
        ],
    )
    .await?;
    Ok(())
}
