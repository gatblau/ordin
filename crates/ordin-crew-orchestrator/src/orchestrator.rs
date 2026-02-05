//! The Crew Orchestrator's public surface (CrewOrchestrator.md *Public Interface*): start, kill,
//! continue, read, take-over requests, recovery and the pruning hook.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_approval::ApprovalService;
use ordin_journal::{
    Datastore, Journal, ModuleMigrations, NewJournalEntry, PruneFailure, PruneParticipant,
    PruneRequest,
};
use ordin_organisation_profile::{CrewLimits, OrganisationProfileService};
use ordin_rules::RulesEngine;
use ordin_session_supervisor::SessionSupervisor;
use ordin_types::domain::runs::{CrewCharter, CrewPauseReason, CrewRun, CrewRunState, Round};
use ordin_types::time::Timestamp;
use ordin_types::wire::events::EventKind;
use ordin_types::{CrewRunId, RunId, WorkspaceId};
use ordin_usage_planner::UsagePlanner;
use sqlx::SqlitePool;
use tracing::Instrument;

use crate::driver::{crew_log, lock, startup_report, startup_subject, Crew, Launch, Live, Resume};
use crate::error::CrewError;
use crate::guard::{lower_guardrails, validate_charter, Counters, MAX_END_REASON};
use crate::plan::{check_plan, problem_code};
use crate::store::{self, StoredCrew};
use crate::types::{
    Coordinator, CrewHost, CrewInput, CrewMode, CrewStart, PlanCheck, PlanProblem, StepOutcome,
};

/// The module name: the crate name without `ordin-`, hyphens as underscores.
pub(crate) const MODULE: &str = "crew_orchestrator";

/// How long `kill` waits for the driver to finish ending the crew.
const KILL_WAIT: Duration = Duration::from_secs(30);

/// What the orchestrator and its drivers share.
pub(crate) struct Inner {
    pub(crate) store: Datastore,
    pub(crate) journal: Arc<Journal>,
    pub(crate) supervisor: Arc<SessionSupervisor>,
    pub(crate) rules: Arc<RulesEngine>,
    pub(crate) approvals: Arc<ApprovalService>,
    pub(crate) usage: Arc<UsagePlanner>,
    pub(crate) profile: Arc<OrganisationProfileService>,
    pub(crate) host: Arc<dyn CrewHost>,
    pub(crate) live: Mutex<HashMap<CrewRunId, Arc<Live>>>,
}

impl Inner {
    /// The SQLite pool. The tables are built for SQLite only (programs D and P; no PostgreSQL
    /// variant, Data Model), so another datastore is an unavailable store.
    pub(crate) fn pool(&self) -> Result<&SqlitePool, sqlx::Error> {
        self.store.sqlite().ok_or_else(|| {
            sqlx::Error::Configuration("the crew store is built for SQLite only".into())
        })
    }

    async fn emit(&self, kind: EventKind, data: serde_json::Value) {
        if let Err(e) = self.journal.append_event(kind.as_str(), data).await {
            crew_log!(
                warn,
                code = ordin_types::CodedError::code(&e),
                "crew_emit_failed"
            );
        }
    }

    fn company(&self) -> CrewLimits {
        self.profile.current().document.limits.crew.clone()
    }
}

pub struct CrewOrchestrator {
    inner: Arc<Inner>,
}

impl CrewOrchestrator {
    pub const MODULE: &'static str = MODULE;

    /// Module `crew_orchestrator`: SQLite only (programs D and P).
    // spec: ordin-desktop/components/CrewOrchestrator#SA-RECON-03
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: Self::MODULE,
            sqlite: &[(1, "crew orchestrator", include_str!("sql/sqlite_v1.sql"))],
            postgres: &[],
        }
    }

    /// Called by: the desktop and personal-server programs at start.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        datastore: Datastore,
        journal: Arc<Journal>,
        supervisor: Arc<SessionSupervisor>,
        rules: Arc<RulesEngine>,
        approvals: Arc<ApprovalService>,
        usage: Arc<UsagePlanner>,
        profile: Arc<OrganisationProfileService>,
        host: Arc<dyn CrewHost>,
    ) -> Arc<Self> {
        Arc::new(CrewOrchestrator {
            inner: Arc::new(Inner {
                store: datastore,
                journal,
                supervisor,
                rules,
                approvals,
                usage,
                profile,
                host,
                live: Mutex::new(HashMap::new()),
            }),
        })
    }

    /// Does the workflow or crew fit (03 *Run a saved workflow*). Called by: Ordin Host, after it
    /// writes the charter (a workflow file is converted to 2C `WorkflowStep`s in `charter.steps` first).
    pub async fn check_plan(&self, charter: &CrewCharter, inputs: &[CrewInput]) -> PlanCheck {
        check_plan(&*self.inner.host, &self.inner.company(), charter, inputs).await
    }

    /// The person pressed Start: validates, records the crew run, and starts driving it.
    /// Called by: Ordin Host (there is no `start_crew_run`).
    pub async fn start(&self, start: CrewStart) -> Result<CrewRunId, CrewError> {
        let o = &self.inner;
        let CrewStart {
            mut charter,
            inputs,
            coordinator,
            started_by_person,
            trigger_id,
        } = start;
        // B.1: a trigger starts saved workflows only (SA-RECON-02, SA-CLOSE-01).
        // spec: ordin-desktop/components/CrewOrchestrator#SA-RECON-02
        // spec: ordin-desktop/components/CrewOrchestrator#SA-CLOSE-01
        if !started_by_person && (trigger_id.is_none() || charter.workflow_id.is_none()) {
            crew_log!(
                info,
                code = "CREW_NOT_STARTED_BY_PERSON",
                "crew_start_refused"
            );
            return Err(CrewError::NotStartedByPerson);
        }
        // B.2
        validate_charter(&charter).map_err(|code| CrewError::CharterInvalid { code })?;
        let plan = self.check_plan(&charter, &inputs).await;
        if !plan.fits {
            let first = plan
                .problems
                .iter()
                .find(|p| !matches!(p, PlanProblem::AgentChanged { .. }));
            let code = first.map(problem_code).unwrap_or("CREW_CHARTER_INVALID");
            return Err(CrewError::CharterInvalid { code });
        }
        let (lowered, _) = lower_guardrails(&charter.guardrails, &o.company());
        charter.guardrails = lowered;
        // B.3
        let mode = if charter.steps.is_empty() {
            CrewMode::Coordinated
        } else {
            CrewMode::SavedSteps
        };
        // B.4
        let id = CrewRunId::new();
        let pool = o.pool()?;
        store::insert_run(pool, id, &charter, mode, &inputs, Timestamp::now()).await?;
        let members: Vec<String> = charter.members.iter().map(|m| m.role.clone()).collect();
        let entry = NewJournalEntry {
            kind: "crew_started".to_owned(),
            component: MODULE.to_owned(),
            run_id: None,
            crew_run_id: Some(id),
            workspace_id: Some(charter.workspace_id),
            summary: "A crew started.".to_owned(),
            data: serde_json::json!({
                "crew_run_id": id,
                "mode": mode,
                "members": members,
                "trigger_id": trigger_id,
            }),
        };
        if let Err(e) = o.journal.append_entry(entry).await {
            let _ = store::set_state(
                pool,
                id,
                CrewRunState::Failed,
                None,
                Some(("The crew could not be recorded.", Timestamp::now())),
            )
            .await;
            return Err(CrewError::Journal(e));
        }
        crew_log!(
            info,
            crew_run_id = id.to_string().as_str(),
            mode = store::enum_text(&mode).as_str(),
            members = charter.members.len() as u64,
            "crew_started"
        );
        // B.5
        let items = o.host.startup_items(&charter).await;
        if let Err(e) = o
            .journal
            .store_startup_report(startup_report(id, &items), false)
            .await
        {
            crew_log!(
                warn,
                crew_run_id = id.to_string().as_str(),
                code = ordin_types::CodedError::code(&e),
                "crew_startup_not_kept"
            );
        }
        o.emit(
            EventKind::StartUpReportReady,
            serde_json::json!({ "subject": startup_subject(id), "items": items }),
        )
        .await;
        // B.6
        store::set_state(pool, id, CrewRunState::Running, None, None).await?;
        o.emit(
            EventKind::CrewRoundChanged,
            serde_json::json!({ "crew_run_id": id, "round": 0, "state": CrewRunState::Running }),
        )
        .await;
        let saved_versions = match (charter.workflow_id, charter.workflow_version) {
            (Some(workflow), Some(version)) => o.host.saved_agent_versions(workflow, version).await,
            _ => Vec::new(),
        };
        let crew = Crew::new(
            o.clone(),
            Live::new(),
            Launch {
                id,
                charter,
                inputs,
                mode,
                coordinator,
                counters: Counters::default(),
                saved_versions,
                resume: Resume::default(),
            },
        );
        self.launch(crew, None);
        Ok(id)
    }

    /// Registers the crew and spawns its driver.
    fn launch(&self, crew: Crew, settle: Option<bool>) {
        let id = crew.id;
        let crew = Arc::new(crew);
        lock(&self.inner.live).insert(id, crew.live.clone());
        let span = tracing::info_span!(target: "ordin_crew_orchestrator", "crew", crew_run_id = id.to_string().as_str());
        tokio::spawn(
            async move {
                let stop = crew.run(settle).await;
                crew.conclude(stop).await;
            }
            .instrument(span),
        );
    }

    /// Kill switch / **Stop crew** (after the person's confirmation, 11 section 5.5): every member
    /// run is cancelled. Called by: the program for Service API `crews.stop`; Ordin Host
    /// (`stop_workflow_run`, for the Trigger Service).
    pub async fn kill(&self, crew_run_id: CrewRunId) -> Result<(), CrewError> {
        let o = &self.inner;
        let stored = store::get_crew(o.pool()?, crew_run_id)
            .await?
            .ok_or(CrewError::NotFound)?;
        if is_ended(stored.run.state) {
            return Err(CrewError::Ended);
        }
        let live = lock(&o.live).get(&crew_run_id).cloned();
        match live {
            Some(live) => {
                // I: signal the driver first, then cancel every member run; the driver ends the crew.
                live.killed.store(true, std::sync::atomic::Ordering::SeqCst);
                live.cancel.cancel();
                let runs: Vec<RunId> = lock(&live.runs).iter().copied().collect();
                for run in runs {
                    let _ = o.supervisor.cancel(run).await;
                }
                let mut done = live.done.subscribe();
                let _ = tokio::time::timeout(KILL_WAIT, done.wait_for(|d| *d)).await;
            }
            None => self.end_without_driver(stored).await?,
        }
        Ok(())
    }

    /// A crew with no driver (paused by a restart) is ended here, with no coordinator to tell.
    async fn end_without_driver(&self, stored: StoredCrew) -> Result<(), CrewError> {
        let o = &self.inner;
        let id = stored.run.id;
        let pool = o.pool()?;
        let rounds = store::list_rounds(pool, id).await?;
        for run in rounds.iter().flat_map(|r| r.member_runs.iter()) {
            let _ = o.supervisor.cancel(*run).await;
        }
        let _ = o.approvals.expire_for_crew_run(id).await;
        store::set_state(
            pool,
            id,
            CrewRunState::Killed,
            None,
            Some(("Stopped by you.", Timestamp::now())),
        )
        .await?;
        o.emit(
            EventKind::CrewRoundChanged,
            serde_json::json!({ "crew_run_id": id, "round": stored.run.round_counter, "state": CrewRunState::Killed }),
        )
        .await;
        let entry = NewJournalEntry {
            kind: "crew_ended".to_owned(),
            component: MODULE.to_owned(),
            run_id: None,
            crew_run_id: Some(id),
            workspace_id: Some(stored.run.workspace_id),
            summary: "The crew ended.".to_owned(),
            data: serde_json::json!({
                "crew_run_id": id,
                "state": CrewRunState::Killed,
                "end_reason": "Stopped by you.",
                "rounds": stored.run.round_counter,
                "requests_used": stored.run.budget.requests_used,
            }),
        };
        o.journal.append_entry(entry).await?;
        Ok(())
    }

    /// After a restart the person presses **Continue**. Called by: the program for Service API
    /// `crews.continue`, with the coordinator it obtains from the Ordin Host's `coordinator()`.
    pub async fn continue_crew(
        &self,
        crew_run_id: CrewRunId,
        coordinator: Arc<dyn Coordinator>,
    ) -> Result<(), CrewError> {
        let o = &self.inner;
        let pool = o.pool()?;
        let stored = store::get_crew(pool, crew_run_id)
            .await?
            .ok_or(CrewError::NotFound)?;
        let paused_for_restart = stored.run.state == CrewRunState::Paused
            && stored.run.pause_reason == Some(CrewPauseReason::Restart);
        if !paused_for_restart || lock(&o.live).contains_key(&crew_run_id) {
            return Err(CrewError::NotPaused);
        }
        // J.2: rebuild from the store.
        let rows = store::list_steps(pool, crew_run_id).await?;
        let mut sorted: Vec<_> = rows.iter().filter(|r| r.outcome.is_some()).collect();
        sorted.sort_by(|a, b| b.position.cmp(&a.position));
        let mut latest: Vec<StepOutcome> = Vec::new();
        let mut by_position = HashMap::new();
        for row in sorted {
            let Some(outcome) = &row.outcome else {
                continue;
            };
            if !latest.iter().any(|o| o.member == outcome.member) {
                latest.push(outcome.clone());
            }
            by_position.insert(row.position.clone(), outcome.clone());
            if let Some(plain) = row.position.split(['#', '@']).next() {
                by_position
                    .entry(plain.to_owned())
                    .or_insert_with(|| outcome.clone());
            }
        }
        let taken_over = stored.run.taken_over_from_step.is_some();
        // A round opened just before the stop may not have reached the counter yet.
        let last_round = store::list_rounds(pool, crew_run_id)
            .await?
            .last()
            .map(|r| r.number)
            .unwrap_or(0);
        let counters = Counters {
            round_counter: stored.run.round_counter.max(last_round),
            requests_used: stored.run.budget.requests_used,
            wall_clock_used_secs: stored.run.budget.wall_clock_used_secs,
            repeated_failures: stored.run.budget.repeated_failures,
        };
        let saved_versions = match (stored.charter.workflow_id, stored.charter.workflow_version) {
            (Some(workflow), Some(version)) => o.host.saved_agent_versions(workflow, version).await,
            _ => Vec::new(),
        };
        store::set_state(pool, crew_run_id, CrewRunState::Running, None, None).await?;
        o.emit(
            EventKind::CrewRoundChanged,
            serde_json::json!({ "crew_run_id": crew_run_id, "round": counters.round_counter, "state": CrewRunState::Running }),
        )
        .await;
        // F2: leaving `paused` for `running`.
        o.emit(
            EventKind::NeedsYouChanged,
            serde_json::json!({ "workspace_id": stored.run.workspace_id }),
        )
        .await;
        let coordinated = stored.mode == CrewMode::Coordinated || taken_over;
        let crew = Crew::new(
            o.clone(),
            Live::new(),
            Launch {
                id: crew_run_id,
                charter: stored.charter,
                inputs: stored.inputs,
                mode: stored.mode,
                coordinator,
                counters,
                saved_versions,
                resume: Resume {
                    latest,
                    by_position,
                    taken_over,
                    rows: rows.into_iter().map(|r| (r.position.clone(), r)).collect(),
                },
            },
        );
        self.launch(crew, Some(coordinated));
        Ok(())
    }

    /// Called by: the program for Service API `crews.get` (`{crew_run, rounds}`); Ordin Host
    /// (`crew_run_state`, for the Trigger Service); the desktop and server programs'
    /// `SessionSetupSource` for the Claude and Copilot Local Adapters (a crew member run's
    /// `crew_charter_id`).
    pub async fn get(&self, crew_run_id: CrewRunId) -> Result<(CrewRun, Vec<Round>), CrewError> {
        let pool = self.inner.pool()?;
        let stored = store::get_crew(pool, crew_run_id)
            .await?
            .ok_or(CrewError::NotFound)?;
        let rounds = store::list_rounds(pool, crew_run_id).await?;
        Ok((stored.run, rounds))
    }

    /// The crew run of a workspace that is starting, running or paused, if any (one indexed read).
    /// Called by: Ordin Host (one crew at a time per workspace, `OH_CREW_RUNNING`).
    pub async fn running_crew(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Option<CrewRunId>, CrewError> {
        Ok(store::running_crew(self.inner.pool()?, workspace_id).await?)
    }

    /// Ordin asks to take over a saved-steps crew from `from_position` (R10): the orchestrator
    /// decides (D.8) and, when it accepts, carries the take-over out at the next step boundary.
    /// Called by: Ordin Host (Ordin's `take_over` tool).
    // spec: ordin-desktop/components/CrewOrchestrator#SA-RECON-01
    pub async fn request_take_over(
        &self,
        crew_run_id: CrewRunId,
        from_position: String,
        reason: String,
    ) -> Result<(), CrewError> {
        let o = &self.inner;
        let pool = o.pool()?;
        let stored = store::get_crew(pool, crew_run_id)
            .await?
            .ok_or(CrewError::NotFound)?;
        if is_ended(stored.run.state) {
            return Err(CrewError::Ended);
        }
        let refusal = self
            .take_over_refusal(&stored, &from_position, &reason)
            .await?;
        let accepted = refusal.is_none();
        let live = lock(&o.live).get(&crew_run_id).cloned();
        let result = match (refusal, live) {
            (Some(reason), _) => Err(CrewError::TakeOverRefused { reason }),
            (None, None) => Err(CrewError::TakeOverRefused {
                reason: "the crew is not running",
            }),
            (None, Some(live)) => {
                *lock(&live.pending_take_over) = Some((from_position.clone(), reason.clone()));
                Ok(())
            }
        };
        let entry = NewJournalEntry {
            kind: "crew_take_over_requested".to_owned(),
            component: MODULE.to_owned(),
            run_id: None,
            crew_run_id: Some(crew_run_id),
            workspace_id: Some(stored.run.workspace_id),
            summary: "Ordin asked to take over a crew.".to_owned(),
            data: serde_json::json!({
                "crew_run_id": crew_run_id,
                "position": from_position,
                "accepted": accepted && result.is_ok(),
            }),
        };
        if let Err(e) = o.journal.append_entry(entry).await {
            crew_log!(
                warn,
                crew_run_id = crew_run_id.to_string().as_str(),
                code = ordin_types::CodedError::code(&e),
                "crew_emit_failed"
            );
        }
        result
    }

    /// D.8: why a take-over request is refused, if it is.
    async fn take_over_refusal(
        &self,
        stored: &StoredCrew,
        from_position: &str,
        reason: &str,
    ) -> Result<Option<&'static str>, CrewError> {
        if stored.mode == CrewMode::Coordinated || stored.run.taken_over_from_step.is_some() {
            return Ok(Some("Ordin already coordinates this crew"));
        }
        if reason.is_empty() || reason.chars().count() > MAX_END_REASON {
            return Ok(Some("a reason is needed"));
        }
        let mut positions = Vec::new();
        collect_positions(&stored.charter.steps, &mut positions);
        let rows = store::list_steps(self.inner.pool()?, stored.run.id).await?;
        let behind = rows
            .iter()
            .any(|r| r.position == from_position && matches!(r.state.as_str(), "done" | "skipped"));
        if !positions.iter().any(|p| p == from_position) || behind {
            return Ok(Some("that step is not ahead of the crew"));
        }
        Ok(None)
    }

    /// At start, before any crew is started: every crew in progress becomes paused (restart).
    /// Called by: the desktop and personal-server programs, after the Session Supervisor's `recover`.
    pub async fn recover(&self) -> Result<u32, CrewError> {
        let o = &self.inner;
        let pool = o.pool()?;
        let ids = store::in_progress(pool).await?;
        let mut count = 0;
        for id in ids {
            let Some(stored) = store::get_crew(pool, id).await? else {
                continue;
            };
            store::set_state(
                pool,
                id,
                CrewRunState::Paused,
                Some(CrewPauseReason::Restart),
                None,
            )
            .await?;
            o.emit(
                EventKind::CrewRoundChanged,
                serde_json::json!({ "crew_run_id": id, "round": stored.run.round_counter, "state": CrewRunState::Paused }),
            )
            .await;
            // F2: a crew paused for a restart needs the person.
            o.emit(
                EventKind::NeedsYouChanged,
                serde_json::json!({ "workspace_id": stored.run.workspace_id }),
            )
            .await;
            count += 1;
        }
        crew_log!(info, count = count, "crew_recovered");
        Ok(count)
    }

    /// The Journal's pruning hook (Data Model). Called by: the programs.
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn PruneParticipant> {
        Arc::new(CrewPrune)
    }
}

fn is_ended(state: CrewRunState) -> bool {
    !matches!(
        state,
        CrewRunState::Starting | CrewRunState::Running | CrewRunState::Paused
    )
}

/// Every step and block position of an outline (not those of called workflows).
fn collect_positions(steps: &[ordin_types::domain::runs::WorkflowStep], into: &mut Vec<String>) {
    use ordin_types::domain::runs::WorkflowStep;
    for step in steps {
        match step {
            WorkflowStep::Agent(a) => into.push(a.position.clone()),
            WorkflowStep::If(b) => {
                into.push(b.position.clone());
                for path in &b.paths {
                    collect_positions(&path.steps, into);
                }
            }
            WorkflowStep::Each(b) => {
                into.push(b.position.clone());
                collect_positions(&b.steps, into);
            }
            WorkflowStep::Until(b) => {
                into.push(b.position.clone());
                collect_positions(&b.steps, into);
            }
            WorkflowStep::Call(b) => into.push(b.position.clone()),
        }
    }
}

struct CrewPrune;

#[async_trait::async_trait]
impl PruneParticipant for CrewPrune {
    fn module(&self) -> &'static str {
        MODULE
    }

    /// Deletes ended crew runs with `ended_at < cutoff` and their rounds and step states.
    async fn prune(&self, store: &Datastore, request: &PruneRequest) -> Result<u64, PruneFailure> {
        let failed = || PruneFailure {
            code: "CREW_STORE_UNAVAILABLE",
        };
        let pool = store.sqlite().ok_or_else(failed)?;
        store::prune(pool, request.cutoff)
            .await
            .map_err(|_| failed())
    }
}
