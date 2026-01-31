//! One supervisor task per crew run (CrewOrchestrator.md *Approach*): the state machine
//! `starting -> running <-> paused -> finished | escalated | stopped_at_limit | killed | failed`,
//! its counters, its rounds, and the ending (G). The two modes live in `coordinated.rs` and
//! `saved.rs`.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ordin_journal::NewJournalEntry;
use ordin_types::common::AgentRef;
use ordin_types::domain::runs::{
    AgentStep, CrewCharter, CrewPauseReason, CrewRunState, RuleNotEnforced, StartupReport,
    StartupSubject,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::events::EventKind;
use ordin_types::wire::startup::{StartUpReportItem, StartUpSubject};
use ordin_types::{CrewRunId, RunId, StartupReportId};
use tokio::sync::{watch, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::guard::{
    check_crew_run, check_limits, check_round, cut, Counters, LimitHit, MAX_DECISION,
    MAX_END_REASON,
};
use crate::orchestrator::Inner;
use crate::store::{self, enum_text};
use crate::types::{
    Coordinator, CoordinatorError, CrewInput, CrewMode, CrewOutcome, CrewView, StepOutcome,
    TakeOverReason,
};

/// Registers the target `ordin_crew_orchestrator` on every event (CrewOrchestrator.md
/// *Observability*); the event name is the message.
macro_rules! crew_log {
    ($level:ident, $($rest:tt)*) => {
        tracing::$level!(target: "ordin_crew_orchestrator", $($rest)*)
    };
}
pub(crate) use crew_log;

/// What the orchestrator shares with the running driver.
pub(crate) struct Live {
    /// Cancelled when the crew is killed or ends.
    pub(crate) cancel: CancellationToken,
    /// The member runs going now, so a kill can cancel each of them.
    pub(crate) runs: Mutex<HashSet<RunId>>,
    /// D.8: a take-over Ordin asked for, `(position, reason)`, taken at the next step boundary.
    pub(crate) pending_take_over: Mutex<Option<(String, String)>>,
    /// True once the driver has finished.
    pub(crate) done: watch::Sender<bool>,
    pub(crate) killed: AtomicBool,
}

impl Live {
    pub(crate) fn new() -> Arc<Live> {
        let (done, _) = watch::channel(false);
        Arc::new(Live {
            cancel: CancellationToken::new(),
            runs: Mutex::new(HashSet::new()),
            pending_take_over: Mutex::new(None),
            done,
            killed: AtomicBool::new(false),
        })
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Running time only: time paused for an approval, a person's answer or a restart does not count
/// (SA-CREW-06).
// spec: ordin-desktop/components/CrewOrchestrator#SA-CREW-06
#[derive(Debug)]
pub(crate) struct RunClock {
    used: Duration,
    since: Option<Instant>,
}

impl RunClock {
    pub(crate) fn started(used_secs: u32) -> RunClock {
        RunClock {
            used: Duration::from_secs(u64::from(used_secs)),
            since: Some(Instant::now()),
        }
    }

    pub(crate) fn used_secs(&self) -> u32 {
        let total = self.used + self.since.map(|s| s.elapsed()).unwrap_or_default();
        u32::try_from(total.as_secs()).unwrap_or(u32::MAX)
    }

    pub(crate) fn pause(&mut self) {
        if let Some(since) = self.since.take() {
            self.used += since.elapsed();
        }
    }

    pub(crate) fn resume(&mut self) {
        if self.since.is_none() {
            self.since = Some(Instant::now());
        }
    }
}

/// How a crew ends (G).
#[derive(Debug, Clone)]
pub(crate) struct EndKind {
    pub(crate) state: CrewRunState,
    pub(crate) reason: String,
    /// The registry code journalled with the ending, where the Error Table gives one.
    pub(crate) code: Option<&'static str>,
    pub(crate) recommendation: Option<String>,
}

impl EndKind {
    pub(crate) fn new(state: CrewRunState, reason: impl Into<String>) -> EndKind {
        EndKind {
            state,
            reason: reason.into(),
            code: None,
            recommendation: None,
        }
    }

    pub(crate) fn with_code(mut self, code: &'static str) -> EndKind {
        self.code = Some(code);
        self
    }

    pub(crate) fn killed() -> EndKind {
        EndKind::new(CrewRunState::Killed, "Stopped by you.")
    }

    pub(crate) fn limit(hit: LimitHit, charter: &CrewCharter) -> EndKind {
        EndKind::new(
            CrewRunState::StoppedAtLimit,
            hit.end_reason(&charter.guardrails),
        )
        .with_code("CREW_LIMIT_REACHED")
    }
}

/// Why a driver function stops its caller.
#[derive(Debug)]
pub(crate) enum Stop {
    /// The crew ends in this way (G).
    Ended(EndKind),
    /// D.6: the saved steps do not fit; Ordin takes over from `position`.
    TakeOver {
        position: String,
        reason: TakeOverReason,
    },
    /// Ordin's answer could not be used; the caller decides what that means.
    Unusable(String),
    /// The datastore (or the Approval Service) failed: the crew pauses with reason `escalation`.
    Store,
    /// Another path is ending the crew; this one returns without ending it again.
    Gone,
}

impl From<sqlx::Error> for Stop {
    fn from(e: sqlx::Error) -> Stop {
        crew_log!(error, code = "CREW_STORE_UNAVAILABLE", error = %e, "crew_store_failed");
        Stop::Store
    }
}

pub(crate) type Flow<T> = Result<T, Stop>;

/// Two counters of paused waits: the clock stops for either; the crew shows `paused` for approvals.
#[derive(Debug, Default)]
pub(crate) struct Waits {
    pub(crate) clock: u32,
    pub(crate) approval: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PauseKind {
    /// A person's answer: the clock stops, the crew stays `running`.
    Answer,
    /// An approval card: the clock stops and the crew is `paused` (reason `approval`).
    Approval,
}

/// Everything the driver changes while the crew runs.
pub(crate) struct State {
    pub(crate) counters: Counters,
    pub(crate) clock: RunClock,
    pub(crate) waits: Waits,
    /// The latest result of each member, newest first.
    pub(crate) latest: Vec<StepOutcome>,
    /// Outcomes by step key (position plus item or pass suffix) and by plain position.
    pub(crate) by_position: HashMap<String, StepOutcome>,
    pub(crate) shortfalls: HashSet<String>,
    pub(crate) skipped: HashSet<String>,
    pub(crate) agent_steps: HashMap<String, AgentStep>,
    pub(crate) last_failed: Option<AgentRef>,
    pub(crate) changed_by_ordin: Vec<String>,
    pub(crate) position: Option<String>,
    pub(crate) mode: CrewMode,
    pub(crate) taken_over: bool,
    pub(crate) recommendation: Option<String>,
    /// The next step index inside a coordinated round, for `r<round>.<k>` keys.
    pub(crate) step_seq: u32,
    /// The cursor rows as stored (D), for a restart's replay.
    pub(crate) rows: HashMap<String, crate::store::StepRow>,
}

/// A round being filled while its steps run.
pub(crate) struct RoundCtx {
    pub(crate) number: u32,
    pub(crate) decisions: Mutex<Vec<String>>,
    pub(crate) runs: Mutex<Vec<RunId>>,
    pub(crate) codes: Mutex<Vec<&'static str>>,
}

impl RoundCtx {
    pub(crate) fn note(&self, decision: String, code: Option<&'static str>) {
        lock(&self.decisions).push(decision);
        if let Some(code) = code {
            lock(&self.codes).push(code);
        }
    }
}

pub(crate) struct Crew {
    pub(crate) o: Arc<Inner>,
    pub(crate) id: CrewRunId,
    /// The charter as started: guardrails already lowered (B.2).
    pub(crate) charter: CrewCharter,
    pub(crate) inputs: Vec<CrewInput>,
    pub(crate) coordinator: Arc<dyn Coordinator>,
    pub(crate) live: Arc<Live>,
    pub(crate) slots: Arc<Semaphore>,
    pub(crate) saved_versions: Vec<(AgentRef, String)>,
    pub(crate) st: Mutex<State>,
}

/// Everything a new driver is built from.
pub(crate) struct Launch {
    pub(crate) id: CrewRunId,
    pub(crate) charter: CrewCharter,
    pub(crate) inputs: Vec<CrewInput>,
    pub(crate) mode: CrewMode,
    pub(crate) coordinator: Arc<dyn Coordinator>,
    pub(crate) counters: Counters,
    pub(crate) saved_versions: Vec<(AgentRef, String)>,
    pub(crate) resume: Resume,
}

/// What a restarted crew carries into its driver.
#[derive(Default)]
pub(crate) struct Resume {
    pub(crate) latest: Vec<StepOutcome>,
    pub(crate) by_position: HashMap<String, StepOutcome>,
    pub(crate) taken_over: bool,
    pub(crate) rows: HashMap<String, crate::store::StepRow>,
}

impl Crew {
    pub(crate) fn new(o: Arc<Inner>, live: Arc<Live>, launch: Launch) -> Crew {
        let Launch {
            id,
            charter,
            inputs,
            mode,
            coordinator,
            counters,
            saved_versions,
            resume,
        } = launch;
        let wanted = charter.guardrails.max_members_running.max(1);
        // D.1: saved steps start at most min(members at once, the person's agents at once).
        // The Usage Planner keeps agents at once at 3 or fewer.
        let permits = match mode {
            CrewMode::SavedSteps => wanted.min(o.usage.agents_at_once().max(1)),
            CrewMode::Coordinated => wanted,
        };
        let taken_over = resume.taken_over;
        Crew {
            o,
            id,
            charter,
            inputs,
            coordinator,
            live,
            slots: Arc::new(Semaphore::new(permits as usize)),
            saved_versions,
            st: Mutex::new(State {
                clock: RunClock::started(counters.wall_clock_used_secs),
                counters,
                waits: Waits::default(),
                latest: resume.latest,
                by_position: resume.by_position,
                shortfalls: HashSet::new(),
                skipped: HashSet::new(),
                agent_steps: HashMap::new(),
                last_failed: None,
                changed_by_ordin: Vec::new(),
                position: None,
                mode: if taken_over {
                    CrewMode::Coordinated
                } else {
                    mode
                },
                taken_over,
                recommendation: None,
                step_seq: 0,
                rows: resume.rows,
            }),
        }
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.st)
    }

    pub(crate) fn workspace(&self) -> ordin_types::WorkspaceId {
        self.charter.workspace_id
    }

    /// The role a member goes by in messages ("analyst failed twice in a row.").
    pub(crate) fn role_of(&self, member: &AgentRef) -> String {
        self.charter
            .members
            .iter()
            .find(|m| m.member == *member)
            .map(|m| m.role.clone())
            .unwrap_or_else(|| "A member".to_owned())
    }

    /// The `work_key` family of the crew: `crew:<charter id>` or `workflow:<id>:<version>`.
    pub(crate) fn work_key(&self) -> String {
        match (self.charter.workflow_id, self.charter.workflow_version) {
            (Some(id), Some(version)) => format!("workflow:{id}:{version}"),
            _ => format!("crew:{}", self.charter.id),
        }
    }

    // ---- views and counters

    pub(crate) fn counters(&self) -> Counters {
        let st = self.state();
        Counters {
            wall_clock_used_secs: st.clock.used_secs(),
            ..st.counters
        }
    }

    pub(crate) fn view(&self) -> CrewView {
        let counters = self.counters();
        let g = &self.charter.guardrails;
        let st = self.state();
        CrewView {
            crew_run_id: self.id,
            workspace_id: self.charter.workspace_id,
            purpose: self.charter.purpose.clone(),
            members: self.charter.members.clone(),
            round: counters.round_counter,
            rounds_left: g.max_rounds.saturating_sub(counters.round_counter),
            seconds_left: g
                .wall_clock_secs
                .saturating_sub(counters.wall_clock_used_secs),
            requests_left: (g.request_budget > 0)
                .then(|| g.request_budget.saturating_sub(counters.requests_used)),
            inputs: self.inputs.clone(),
            latest_results: st.latest.clone(),
            position: st.position.clone(),
            cancel: self.live.cancel.clone(),
        }
    }

    pub(crate) async fn persist(&self) -> Flow<()> {
        let counters = self.counters();
        let pool = self.o.pool()?;
        store::set_counters(pool, self.id, &counters).await?;
        Ok(())
    }

    pub(crate) fn add_requests(&self, turns: u32) {
        let mut st = self.state();
        st.counters.requests_used = st.counters.requests_used.saturating_add(turns);
    }

    /// F.1: a limit reached ends the crew `stopped_at_limit` with its state kept.
    pub(crate) fn stop_if_limit(&self) -> Flow<()> {
        match check_limits(&self.charter.guardrails, &self.counters()) {
            Some(hit) => {
                crew_log!(info, crew_run_id = %self.id, decision = hit.name(), "crew_guardrail");
                Err(Stop::Ended(EndKind::limit(hit, &self.charter)))
            }
            None => Ok(()),
        }
    }

    pub(crate) fn check_cancelled(&self) -> Flow<()> {
        if self.live.cancel.is_cancelled() {
            return Err(Stop::Ended(EndKind::killed()));
        }
        Ok(())
    }

    /// Runs `fut` unless the crew is cancelled first.
    pub(crate) async fn unless_cancelled<T>(
        &self,
        fut: impl std::future::Future<Output = T>,
    ) -> Flow<T> {
        tokio::select! {
            biased;
            _ = self.live.cancel.cancelled() => Err(Stop::Ended(EndKind::killed())),
            out = fut => Ok(out),
        }
    }

    // ---- Ordin's calls

    /// One call to Ordin. `Unavailable` is tried once more; twice in a row fails the crew
    /// (C.2, `CREW_COORDINATOR_UNAVAILABLE`). `Unusable` is `retry_unusable`'s to retry, else it
    /// goes back to the caller as [`Stop::Unusable`]; `StoppedByPerson` kills the crew.
    pub(crate) async fn ask<T, F, Fut>(&self, retry_unusable: bool, call: F) -> Flow<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, CoordinatorError>>,
    {
        for _ in 0..2 {
            match self.unless_cancelled(call()).await? {
                Ok(value) => return Ok(value),
                Err(CoordinatorError::StoppedByPerson) => {
                    return Err(Stop::Ended(EndKind::killed()))
                }
                Err(CoordinatorError::Unusable(why)) => {
                    crew_log!(warn, crew_run_id = %self.id, code = "CREW_COORDINATOR_UNUSABLE", "crew_coordinator_unusable");
                    if !retry_unusable {
                        return Err(Stop::Unusable(why));
                    }
                }
                Err(CoordinatorError::Unavailable) => {
                    crew_log!(warn, crew_run_id = %self.id, code = "CREW_COORDINATOR_UNAVAILABLE", "crew_coordinator_unavailable");
                }
            }
        }
        Err(Stop::Ended(
            EndKind::new(CrewRunState::Failed, "Ordin could not continue the crew.")
                .with_code("CREW_COORDINATOR_UNAVAILABLE"),
        ))
    }

    // ---- rounds

    /// Opens round `round_counter + 1` (C.3) and counts it.
    pub(crate) async fn open_round(&self, decision: &str) -> Flow<Arc<RoundCtx>> {
        let number = {
            let mut st = self.state();
            st.counters.round_counter += 1;
            st.counters.round_counter
        };
        let text = if decision.is_empty() {
            "round"
        } else {
            decision
        };
        let decision = cut(text, MAX_DECISION);
        let probe = ordin_types::domain::runs::Round {
            crew_run_id: self.id,
            number,
            coordinator_decision: decision.clone(),
            guardrail_decisions: vec![],
            member_runs: vec![],
            opened_at: Timestamp::now(),
            closed_at: None,
        };
        if let Err(code) = check_round(number - 1, &probe) {
            crew_log!(error, crew_run_id = %self.id, code = code, "crew_round_invalid");
            return Err(Stop::Ended(EndKind::new(CrewRunState::Failed, code)));
        }
        let pool = self.o.pool()?;
        store::insert_round(pool, self.id, number, &decision, probe.opened_at).await?;
        crew_log!(info, crew_run_id = %self.id, round = number, "crew_round_opened");
        self.persist().await?;
        Ok(Arc::new(RoundCtx {
            number,
            decisions: Mutex::new(Vec::new()),
            runs: Mutex::new(Vec::new()),
            codes: Mutex::new(Vec::new()),
        }))
    }

    /// C.5: closes the round, journals it, emits the change and persists the counters.
    pub(crate) async fn close_round(&self, round: &RoundCtx) -> Flow<()> {
        let decisions = lock(&round.decisions).clone();
        let runs = lock(&round.runs).clone();
        let codes = lock(&round.codes).clone();
        let pool = self.o.pool()?;
        store::close_round(
            pool,
            self.id,
            round.number,
            &decisions,
            &runs,
            Timestamp::now(),
        )
        .await?;
        self.journal(
            "crew_round_closed",
            "A crew round closed.",
            serde_json::json!({
                "crew_run_id": self.id,
                "round": round.number,
                "guardrail_decisions": decisions,
                "guardrail_codes": codes,
                "member_runs": runs,
            }),
        )
        .await;
        self.emit_round_changed(CrewRunState::Running).await;
        self.persist().await
    }

    // ---- journal and events (a failure is logged and never stops the crew)

    pub(crate) async fn journal(&self, kind: &str, summary: &str, data: serde_json::Value) {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: crate::orchestrator::MODULE.to_owned(),
            run_id: None,
            crew_run_id: Some(self.id),
            workspace_id: Some(self.charter.workspace_id),
            summary: summary.to_owned(),
            data,
        };
        if let Err(e) = self.o.journal.append_entry(entry).await {
            crew_log!(warn, crew_run_id = %self.id, code = ordin_types::CodedError::code(&e), kind = kind, "crew_emit_failed");
        }
    }

    pub(crate) async fn emit(&self, kind: EventKind, data: serde_json::Value) {
        if let Err(e) = self.o.journal.append_event(kind.as_str(), data).await {
            crew_log!(warn, crew_run_id = %self.id, code = ordin_types::CodedError::code(&e), "crew_emit_failed");
        }
    }

    pub(crate) async fn emit_round_changed(&self, state: CrewRunState) {
        let round = self.state().counters.round_counter;
        self.emit(
            EventKind::CrewRoundChanged,
            serde_json::json!({ "crew_run_id": self.id, "round": round, "state": state }),
        )
        .await;
    }

    /// F2: the person has something to look at.
    pub(crate) async fn needs_you(&self) {
        self.emit(
            EventKind::NeedsYouChanged,
            serde_json::json!({ "workspace_id": self.charter.workspace_id }),
        )
        .await;
    }

    // ---- paused waits

    /// Starts a wait that does not count against the crew's time; an approval also shows the crew
    /// `paused` with reason `approval`.
    pub(crate) async fn enter_pause(&self, kind: PauseKind) -> Flow<()> {
        let first_approval = {
            let mut st = self.state();
            st.waits.clock += 1;
            if st.waits.clock == 1 {
                st.clock.pause();
            }
            if kind == PauseKind::Approval {
                st.waits.approval += 1;
                st.waits.approval == 1
            } else {
                false
            }
        };
        if first_approval {
            let pool = self.o.pool()?;
            store::set_state(
                pool,
                self.id,
                CrewRunState::Paused,
                Some(CrewPauseReason::Approval),
                None,
            )
            .await?;
            self.emit_round_changed(CrewRunState::Paused).await;
        }
        Ok(())
    }

    pub(crate) async fn leave_pause(&self, kind: PauseKind) -> Flow<()> {
        let back_to_running = {
            let mut st = self.state();
            st.waits.clock = st.waits.clock.saturating_sub(1);
            if st.waits.clock == 0 {
                st.clock.resume();
            }
            if kind == PauseKind::Approval {
                st.waits.approval = st.waits.approval.saturating_sub(1);
                st.waits.approval == 0
            } else {
                false
            }
        };
        if back_to_running && !self.live.cancel.is_cancelled() {
            let pool = self.o.pool()?;
            store::set_state(pool, self.id, CrewRunState::Running, None, None).await?;
            self.emit_round_changed(CrewRunState::Running).await;
            // F2: leaving `paused` for `running`.
            self.needs_you().await;
        }
        Ok(())
    }

    // ---- ending (G)

    pub(crate) fn outcome(&self, kind: &EndKind) -> CrewOutcome {
        let st = self.state();
        CrewOutcome {
            state: kind.state,
            end_reason: Some(cut(&kind.reason, MAX_END_REASON)),
            results: st.latest.clone(),
            changed_by_ordin: st.changed_by_ordin.clone(),
            recommendation: kind
                .recommendation
                .clone()
                .or_else(|| st.recommendation.clone()),
        }
    }

    /// G. Ends the crew in the way `kind` says.
    pub(crate) async fn finish(&self, kind: EndKind) {
        // The crew token is cancelled first: a coordinator method still waiting returns.
        self.live.cancel.cancel();
        let runs: Vec<RunId> = lock(&self.live.runs).iter().copied().collect();
        for run in runs {
            // A run that has already ended answers `SUP_RUN_ENDED`; nothing more to do for it.
            let _ = self.o.supervisor.cancel(run).await;
        }
        if let Err(e) = self.o.approvals.expire_for_crew_run(self.id).await {
            crew_log!(warn, crew_run_id = %self.id, code = e.code(), "crew_approvals_not_expired");
        }
        let counters = self.counters();
        let reason = cut(&kind.reason, MAX_END_REASON);
        let checked = check_crew_run(
            &self.charter.guardrails,
            &counters,
            kind.state,
            None,
            Some(&reason),
            (
                self.charter.workflow_id.is_some(),
                self.charter.workflow_version.is_some(),
            ),
        );
        if let Err(code) = checked {
            crew_log!(error, crew_run_id = %self.id, code = code, "crew_run_invalid");
        }
        let now = Timestamp::now();
        let stored = async {
            let pool = self.o.pool()?;
            store::set_counters(pool, self.id, &counters).await?;
            store::set_state(pool, self.id, kind.state, None, Some((&reason, now))).await
        }
        .await;
        if let Err(e) = stored {
            crew_log!(error, crew_run_id = %self.id, code = "CREW_STORE_UNAVAILABLE", error = %e, "crew_end_not_stored");
        }
        self.emit_round_changed(kind.state).await;
        if matches!(
            kind.state,
            CrewRunState::Escalated | CrewRunState::StoppedAtLimit
        ) {
            self.needs_you().await;
        }
        let total = counters.requests_used;
        match RunId::parse(&self.id.to_string()) {
            Ok(as_run) => {
                if let Err(e) = self
                    .o
                    .usage
                    .record_run_finished(&self.work_key(), as_run, total)
                    .await
                {
                    crew_log!(warn, crew_run_id = %self.id, code = ordin_types::CodedError::code(&e), "crew_usage_not_recorded");
                }
            }
            Err(_) => {
                crew_log!(warn, crew_run_id = %self.id, "crew_usage_not_recorded");
            }
        }
        let outcome = self.outcome(&kind);
        let mut view = self.view();
        view.cancel = CancellationToken::new();
        if let Err(e) = self.coordinator.crew_ended(&view, &outcome).await {
            crew_log!(warn, crew_run_id = %self.id, error = %e, "crew_ended_call_failed");
        }
        self.journal(
            "crew_ended",
            "The crew ended.",
            serde_json::json!({
                "crew_run_id": self.id,
                "state": kind.state,
                "end_reason": reason,
                "code": kind.code,
                "rounds": counters.round_counter,
                "requests_used": counters.requests_used,
            }),
        )
        .await;
        crew_log!(
            info,
            crew_run_id = %self.id,
            state = enum_text(&kind.state).as_str(),
            rounds = counters.round_counter,
            requests_used = counters.requests_used,
            "crew_ended"
        );
    }

    /// Error Table, *Store unavailable*: in a driver the crew pauses with reason `escalation`.
    pub(crate) async fn pause_for_store(&self) {
        self.live.cancel.cancel();
        let stored = match self.o.pool() {
            Ok(pool) => {
                store::set_state(
                    pool,
                    self.id,
                    CrewRunState::Paused,
                    Some(CrewPauseReason::Escalation),
                    None,
                )
                .await
            }
            Err(e) => Err(e),
        };
        if let Err(e) = stored {
            crew_log!(error, crew_run_id = %self.id, code = "CREW_STORE_UNAVAILABLE", error = %e, "crew_pause_not_stored");
        }
        self.emit_round_changed(CrewRunState::Paused).await;
        self.needs_you().await;
    }

    /// The end of the driver task: whatever stopped it, the orchestrator forgets the crew.
    pub(crate) async fn conclude(&self, stop: Stop) {
        match stop {
            Stop::Ended(kind) => self.finish(kind).await,
            Stop::Store => self.pause_for_store().await,
            Stop::Gone => {}
            Stop::TakeOver { position, .. } => {
                // A take-over is handled before the walk returns here; reaching this is a defect.
                crew_log!(error, crew_run_id = %self.id, position = position.as_str(), "crew_take_over_unhandled");
                self.finish(EndKind::new(
                    CrewRunState::Failed,
                    "The crew could not go on.",
                ))
                .await;
            }
            Stop::Unusable(why) => {
                crew_log!(error, crew_run_id = %self.id, code = "CREW_COORDINATOR_UNUSABLE", why = why.as_str(), "crew_coordinator_unusable");
                self.finish(
                    EndKind::new(CrewRunState::Failed, "Ordin could not continue the crew.")
                        .with_code("CREW_COORDINATOR_UNUSABLE"),
                )
                .await;
            }
        }
        lock(&self.o.live).remove(&self.id);
        // `send_replace` stores the value even when nobody is subscribed yet (a `kill` may arrive late).
        self.live.done.send_replace(true);
    }
}

/// B.5: the Journal's flat start-up report from the worded items.
pub(crate) fn startup_report(crew_run_id: CrewRunId, items: &[StartUpReportItem]) -> StartupReport {
    let mut report = StartupReport {
        id: StartupReportId::new(),
        subject: StartupSubject::CrewRun { crew_run_id },
        enforced_rules: vec![],
        rules_not_enforced: vec![],
        unavailable_parts: vec![],
        available_tools: vec![],
        adapter_verbs_not_offered: vec![],
        generated_at: Timestamp::now(),
    };
    for item in items {
        match item {
            StartUpReportItem::EnforcedRule { plain, .. } => {
                report.enforced_rules.push(plain.clone())
            }
            StartUpReportItem::RuleNotEnforced { plain, reason, .. } => {
                report.rules_not_enforced.push(RuleNotEnforced {
                    rule: plain.clone(),
                    reason: enum_text(reason),
                })
            }
            StartUpReportItem::UnavailablePart { plain, .. } => {
                report.unavailable_parts.push(plain.clone())
            }
            StartUpReportItem::AvailableTool { tool, .. } => {
                report.available_tools.push(tool.clone())
            }
            StartUpReportItem::VerbNotOffered { verb, .. } => {
                report.adapter_verbs_not_offered.push(enum_text(verb))
            }
            StartUpReportItem::OwnScreensNotShown { .. } => {}
        }
    }
    report
}

pub(crate) fn startup_subject(crew_run_id: CrewRunId) -> StartUpSubject {
    StartUpSubject::CrewRun { crew_run_id }
}
