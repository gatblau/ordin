//! The service object: `new`, `shutdown`, `sign_in_status`, `harness_models`, and `RuntimeAdapter`
//! (ClaudeLocalAdapter.md *Public Interface*, `start` steps 1–5 and 12, `resume`, `read`, `cancel`).

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::BoxStream;
use ordin_hook_relay::HookRelay;
use ordin_mcp_front::{McpFront, ProgramProfile};
use ordin_rules::RulesEngine;
use ordin_runtime_adapter::{
    check_resume, check_start, AdapterContext, AdapterError, AdapterKind, AdapterVerb,
    HarnessModels, PauseReason, ResumeDecision, RunEndReason, RunEvent, RunHandle, RunSnapshot,
    RunTarget, RuntimeAdapter, StartRun,
};
use ordin_types::domain::agent_profile::{RESERVED_ENVIRONMENT_NAMES, RESERVED_ENVIRONMENT_PREFIX};
use ordin_types::{AgentInstanceId, RunId, ScopeId};
use tokio::sync::oneshot;

use crate::cli::{check_auth, check_version, AuthCheck, AuthStatus, VersionCheck};
use crate::config::{
    ClaudeAdapterConfig, ClaudeEffort, SessionRecordStore, SessionSetup, SessionSetupSource,
    SignInMode, SignInStatus,
};
use crate::env::{child_environment, ParentEnv};
use crate::error::{ClaudeAdapterError, Failure};
use crate::files;
use crate::models::ClaudeModels;
use crate::ports::{HookPort, McpPort, RulesPort};
use crate::run::{lock, run_log, Run, RunCommand, RunIdentity, SessionPlan, MAX_ENDED_RUNS};
use crate::session::answer_turn;
use crate::task::{run_task, TaskStart};

/// `resume`: the longest `Answer.text`, in characters (the Runtime Adapter interface's bound).
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-ANS-1
const MAX_ANSWER: usize = ordin_runtime_adapter::MAX_ANSWER_CHARS;

/// The durations of the adapter. [`Timings::SPEC`] holds the spec's values; the crate's own tests
/// build shorter sets so that a 15-minute sign-in does not take 15 minutes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timings {
    /// `claude --version` and `claude auth status --json`: 10 s.
    pub cli_check: Duration,
    /// Sign-in status poll: 5 s (*chosen here*).
    pub sign_in_poll: Duration,
    /// Sign-in attempt: 15 minutes (*chosen here*).
    pub sign_in_give_up: Duration,
    /// Idle harness process kept while paused: 30 minutes (2D-4).
    pub idle_process: Duration,
}

impl Timings {
    pub(crate) const SPEC: Timings = Timings {
        cli_check: Duration::from_secs(10),
        sign_in_poll: Duration::from_secs(5),
        sign_in_give_up: Duration::from_secs(15 * 60),
        idle_process: Duration::from_secs(30 * 60),
    };
}

/// What this process gives the adapter, replaced in tests.
pub(crate) struct Host {
    pub timings: Timings,
    pub parent_env: ParentEnv,
    /// Claude Code's `projects` folder; `None` is `<HOME>/.claude/projects`.
    pub projects_root: Option<PathBuf>,
}

impl Host {
    fn real() -> Host {
        Host {
            timings: Timings::SPEC,
            parent_env: Arc::new(|name: &str| -> Option<OsString> { std::env::var_os(name) }),
            projects_root: None,
        }
    }
}

/// The services a run's task needs.
pub(crate) struct Inner {
    pub config: ClaudeAdapterConfig,
    pub mcp: Arc<dyn McpPort>,
    pub hooks: Arc<dyn HookPort>,
    pub rules: Arc<dyn RulesPort>,
    pub setup: Arc<dyn SessionSetupSource>,
    pub records: Option<Arc<dyn SessionRecordStore>>,
    pub host: Host,
    pub runs: Mutex<HashMap<RunId, Arc<Run>>>,
    /// `run_id`s between step 1 and the run entry of step 5.
    pub starting: Mutex<HashSet<RunId>>,
}

impl Inner {
    pub(crate) fn agent_profile(&self) -> bool {
        self.config.profile == ProgramProfile::AgentProfile
    }

    fn run(&self, run_id: RunId) -> Option<Arc<Run>> {
        lock(&self.runs).get(&run_id).cloned()
    }

    /// A run to adopt has a session record this instance can load: the Run Queue hands one over only
    /// with a paused run taken after its approval was decided.
    async fn check_record(&self, run_id: RunId, runtime_session_id: &str) -> Result<(), Failure> {
        let Some(records) = &self.records else {
            return Err(unknown_run());
        };
        match records.load(run_id, runtime_session_id).await {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(unknown_run()),
            Err(error) => Err(Failure::unreachable(
                "CLA_SESSION_RECORD_MISSING",
                error.retryable,
            )),
        }
    }

    /// Adds a run, first evicting the longest-ended runs past [`MAX_ENDED_RUNS`]. A subscriber keeps
    /// its own reference, so eviction takes nothing from a stream already open.
    fn hold_run(&self, run: Arc<Run>) {
        let mut runs = lock(&self.runs);
        let mut ended: Vec<_> = runs
            .iter()
            .filter_map(|(id, held)| held.ended_at().map(|at| (at, *id)))
            .collect();
        if ended.len() >= MAX_ENDED_RUNS {
            ended.sort();
            for (_, id) in &ended[..=ended.len() - MAX_ENDED_RUNS] {
                runs.remove(id);
            }
        }
        runs.insert(run.run_id, run);
    }

    /// The environment of the CLI checks: step 8's, without hook variables.
    fn check_env(&self) -> Vec<(String, OsString)> {
        child_environment(
            &self.host.parent_env,
            &self.config.sign_in,
            None,
            &self.config.claude_path,
        )
    }

    /// Step 3.
    async fn check_version(&self, run_id: RunId) -> Result<String, Failure> {
        let found = check_version(
            &self.config.claude_path,
            &self.check_env(),
            self.host.timings.cli_check,
        )
        .await;
        let version = match found {
            VersionCheck::Found(version) => version,
            VersionCheck::NotFound => {
                return Err(Failure::unreachable("CLA_HARNESS_NOT_FOUND", false))
            }
            VersionCheck::Failed => {
                return Err(Failure::unreachable("CLA_VERSION_CHECK_FAILED", true))
            }
        };
        tracing::info!(
            target: "ordin_claude_adapter",
            run_id = %run_id,
            harness_version = version.as_str(),
            "claude.version.checked"
        );
        let tested = &self.config.tested_versions;
        if !tested.is_empty() && !tested.contains(&version) {
            return Err(Failure::new(
                "CLA_HARNESS_UNTESTED",
                false,
                AdapterError::HarnessUntested { found: version },
            ));
        }
        Ok(version)
    }

    /// Step 4: `true` when the harness is signed in. Never in the agent profile.
    async fn check_signed_in(&self, run_id: RunId) -> Result<bool, Failure> {
        if !matches!(self.config.sign_in, SignInMode::PersonSignIn) {
            return Ok(true);
        }
        let check = check_auth(
            &self.config.claude_path,
            &self.check_env(),
            self.host.timings.cli_check,
        )
        .await;
        let AuthCheck::Read(status) = check else {
            return Err(Failure::unreachable("CLA_AUTH_STATUS_UNREADABLE", true));
        };
        log_sign_in(run_id, &status);
        Ok(status.logged_in)
    }

    async fn start_steps(
        self: &Arc<Self>,
        ctx: &AdapterContext,
        req: StartRun,
    ) -> Result<RunHandle, Failure> {
        let run_id = req.run_id;
        let agent_instance_id = check_request(&req)?;
        tracing::info!(
            target: "ordin_claude_adapter",
            run_id = %run_id,
            agent_instance_id = agent_instance_id.to_string().as_str(),
            workspace_id = req.workspace_id.to_string().as_str(),
            correlation_id = ctx.correlation_id.as_str(),
            "claude.start.received"
        );
        // The start request's attachments, passed as received (CLA-REC-2).
        // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-REC-2
        let mut setup = self
            .setup
            .session_setup(run_id, agent_instance_id, &req.attachments)
            .await
            .map_err(|error| {
                tracing::warn!(
                    target: "ordin_claude_adapter",
                    run_id = %run_id,
                    code = "CLA_SETUP_FAILED",
                    setup_code = error.code.as_str(),
                    "claude.start.setup_failed"
                );
                Failure::unreachable("CLA_SETUP_FAILED", true)
            })?;
        // The Usage Planner's model from the start request, when the session setup names none.
        // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-REC-1
        if setup.model.is_none() {
            setup.model = req.model.as_ref().and_then(|choice| choice.model.clone());
        }
        // The person's effort for this kind of work (Settings › Agents and models), when the session
        // setup names none; a value Claude Code does not know is dropped.
        // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-REC-3
        if setup.effort.is_none() {
            setup.effort = req.effort.as_deref().and_then(ClaudeEffort::parse);
        }
        if !setup.agent_folder.is_dir() {
            return Err(Failure::unreachable("CLA_AGENT_FOLDER_MISSING", false));
        }
        let harness_version = self.check_version(run_id).await?;
        let signed_in = self.check_signed_in(run_id).await?;

        // Step 5.
        let continued = setup.resume_session_id.is_some();
        let runtime_session_id = setup
            .resume_session_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let (run, inbox) = Run::new(
            RunIdentity {
                run_id,
                agent_instance_id,
                workspace_id: req.workspace_id,
                runtime_session_id: runtime_session_id.clone(),
                correlation_id: ctx.correlation_id.clone(),
                caller: req.caller,
                scope_id: req.scope_id,
                coding_task_id: req.coding_task_id,
                only_servers: req.only_servers.clone(),
                started_by_person: req.started_by_person,
                harness_version,
                setup,
            },
            continued,
        );
        self.hold_run(run.clone());
        run.emit(RunEvent::Started {
            runtime_session_id: Some(runtime_session_id.clone()),
            remote_thread_id: None,
        });
        let handle = RunHandle {
            run_id,
            adapter: AdapterKind::ClaudeLocal,
            runtime_session_id: Some(runtime_session_id),
            remote_thread_id: None,
        };
        if !signed_in {
            // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA_SIGNED_OUT
            run.emit(RunEvent::Paused {
                reason: PauseReason::SignIn,
            });
            run_log!(
                info,
                run,
                "claude.run.paused",
                reason_kind = "sign_in",
                code = "CLA_SIGNED_OUT"
            );
            tokio::spawn(run_task(
                self.clone(),
                run,
                inbox,
                TaskStart::SignIn { input: req.input },
            ));
            return Ok(handle);
        }
        match self.launch(&run, &req.input).await {
            Ok(Some(process)) => {
                tokio::spawn(run_task(
                    self.clone(),
                    run,
                    inbox,
                    TaskStart::Live(Box::new(process)),
                ));
                Ok(handle)
            }
            Ok(None) => Ok(handle),
            Err(failure) => {
                lock(&self.runs).remove(&run_id);
                Err(failure)
            }
        }
    }
}

fn log_sign_in(run_id: RunId, status: &AuthStatus) {
    tracing::info!(
        target: "ordin_claude_adapter",
        run_id = %run_id,
        logged_in = status.logged_in,
        auth_method = status.auth_method.as_deref(),
        api_provider = status.api_provider.as_deref(),
        "claude.sign_in.status"
    );
}

/// Step 1: the 2C checks of `req`; the agent instance of the target.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA_TARGET_MISMATCH
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA_INPUT_INVALID
fn check_request(req: &StartRun) -> Result<AgentInstanceId, Failure> {
    check_start(AdapterKind::ClaudeLocal, req).map_err(|error| {
        let code = if error.code == "RA_TARGET_MISMATCH" {
            "CLA_TARGET_MISMATCH"
        } else {
            "CLA_INPUT_INVALID"
        };
        Failure::invalid(code, error.code)
    })?;
    match req.target {
        RunTarget::AgentInstance { agent_instance_id } => Ok(agent_instance_id),
        RunTarget::RemoteAgent { .. } => Err(Failure::invalid(
            "CLA_TARGET_MISMATCH",
            "RA_TARGET_MISMATCH",
        )),
    }
}

fn log_failure(run_id: RunId, failure: &Failure) {
    tracing::warn!(
        target: "ordin_claude_adapter",
        run_id = %run_id,
        code = failure.code,
        retryable = failure.retryable,
        "claude.call.failed"
    );
}

/// A session identifier joins a path (`session_record_path`), so a value from a handle must be the
/// canonical text of a UUID, the only form the adapter writes.
fn is_session_id(text: &str) -> bool {
    uuid::Uuid::try_parse(text).is_ok_and(|id| id.hyphenated().to_string() == text)
}

fn unknown_run() -> Failure {
    Failure::new("CLA_UNKNOWN_RUN", false, AdapterError::UnknownRun)
}

fn mismatch() -> Failure {
    Failure::invalid("CLA_RESUME_MISMATCH", "RA_RESUME_MISMATCH")
}

/// The user turn of a tool approval (*wording chosen here*).
fn decision_turn(decision: &ResumeDecision) -> Option<String> {
    match decision {
        ResumeDecision::Approve { .. } => Some(
            "The person approved the action you asked for. Make the same call again now."
                .to_owned(),
        ),
        ResumeDecision::Reject { reason, .. } => Some(format!(
            "The person did not approve the action. Reason: {}. Continue without it.",
            reason.as_deref().unwrap_or("none given")
        )),
        ResumeDecision::Answer { .. } => None,
    }
}

pub struct ClaudeLocalAdapter {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for ClaudeLocalAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeLocalAdapter").finish_non_exhaustive()
    }
}

/// The checks of `new`.
fn check_config(
    config: &ClaudeAdapterConfig,
    records: Option<&Arc<dyn SessionRecordStore>>,
) -> Result<(), ClaudeAdapterError> {
    for (path, field) in [
        (&config.claude_path, "claude_path"),
        (&config.hook_program, "hook_program"),
        (&config.app_data_root, "app_data_root"),
    ] {
        if !path.is_absolute() {
            return Err(ClaudeAdapterError::ConfigInvalid(field));
        }
    }
    let agent_profile = config.profile == ProgramProfile::AgentProfile;
    match &config.sign_in {
        SignInMode::PersonSignIn if agent_profile => {
            Err(ClaudeAdapterError::ConfigInvalid("sign_in"))
        }
        SignInMode::PersonSignIn => Ok(()),
        SignInMode::OrganisationKey { .. } if !agent_profile => {
            Err(ClaudeAdapterError::ConfigInvalid("sign_in"))
        }
        SignInMode::OrganisationKey { models, .. } => {
            if models
                .key
                .as_ref()
                .is_some_and(|key| key.expose().is_empty())
            {
                return Err(ClaudeAdapterError::ConfigInvalid("models.key"));
            }
            if records.is_none() {
                return Err(ClaudeAdapterError::ConfigInvalid("records"));
            }
            for variable in &models.environment {
                // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA_ENV_RESERVED
                let reserved = RESERVED_ENVIRONMENT_NAMES
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&variable.name))
                    || variable.name.starts_with(RESERVED_ENVIRONMENT_PREFIX);
                if reserved {
                    return Err(ClaudeAdapterError::EnvReserved(variable.name.clone()));
                }
            }
            Ok(())
        }
    }
}

impl ClaudeLocalAdapter {
    /// Fails with CLA_CONFIG_INVALID when a path is not absolute, the profile/sign-in pair is
    /// inconsistent (OrganisationKey outside the agent profile, PersonSignIn inside it), or the
    /// key is present and empty. Fails with CLA_ENV_RESERVED (ADR-107) when a `models.environment`
    /// name is in `ordin_types::agent_profile::RESERVED_ENVIRONMENT_NAMES` (ignoring case) or begins
    /// `RESERVED_ENVIRONMENT_PREFIX` — a second check of the rule the configuration check already applies.
    pub fn new(
        config: ClaudeAdapterConfig,
        mcp: Arc<McpFront>,
        hooks: Arc<HookRelay>,
        rules: Arc<RulesEngine>,
        setup: Arc<dyn SessionSetupSource>,
        records: Option<Arc<dyn SessionRecordStore>>,
    ) -> Result<Self, ClaudeAdapterError> {
        Self::build(config, mcp, hooks, rules, setup, records, Host::real())
    }

    pub(crate) fn build(
        config: ClaudeAdapterConfig,
        mcp: Arc<dyn McpPort>,
        hooks: Arc<dyn HookPort>,
        rules: Arc<dyn RulesPort>,
        setup: Arc<dyn SessionSetupSource>,
        records: Option<Arc<dyn SessionRecordStore>>,
        host: Host,
    ) -> Result<Self, ClaudeAdapterError> {
        check_config(&config, records.as_ref())?;
        // *Data Model*: what a crash left behind.
        files::sweep_sessions(&config.app_data_root);
        Ok(ClaudeLocalAdapter {
            inner: Arc::new(Inner {
                config,
                mcp,
                hooks,
                rules,
                setup,
                records,
                host,
                runs: Mutex::new(HashMap::new()),
                starting: Mutex::new(HashSet::new()),
            }),
        })
    }

    /// Stops every session within the stop deadline; runs still going end `Interrupted` (SA-XC-13).
    pub async fn shutdown(&self) {
        let runs: Vec<Arc<Run>> = lock(&self.inner.runs).values().cloned().collect();
        let mut waits = Vec::new();
        for run in runs {
            if run.is_ended() {
                continue;
            }
            let (ack, done) = oneshot::channel();
            if run
                .commands
                .send(RunCommand::Stop {
                    reason: RunEndReason::Interrupted,
                    ack,
                })
                .is_ok()
            {
                waits.push(done);
            }
        }
        let limit = self.inner.config.stop_deadline + Duration::from_secs(1);
        let _ = tokio::time::timeout(limit, futures::future::join_all(waits)).await;
    }

    /// Whether Claude Code is signed in on this machine: `claude auth status --json`, exit 0 and
    /// `loggedIn: true` → `SignedIn`; non-zero exit or `loggedIn: false` → `SignedOut` (row 247).
    /// Timeout 10 s → `CLA_SIGN_IN_STATUS_FAILED` (retryable). Personal server and desktop only;
    /// with `SignInMode::OrganisationKey` → `CLA_CONFIG_INVALID`.
    pub async fn sign_in_status(&self) -> Result<SignInStatus, ClaudeAdapterError> {
        let inner = &self.inner;
        if !matches!(inner.config.sign_in, SignInMode::PersonSignIn) {
            return Err(ClaudeAdapterError::ConfigInvalid("sign_in"));
        }
        let check = check_auth(
            &inner.config.claude_path,
            &inner.check_env(),
            inner.host.timings.cli_check,
        )
        .await;
        match check {
            AuthCheck::Read(status) if status.exit_ok && status.logged_in => {
                Ok(SignInStatus::SignedIn)
            }
            AuthCheck::Read(_) => Ok(SignInStatus::SignedOut),
            AuthCheck::Unreadable => Err(ClaudeAdapterError::SignInStatusFailed),
        }
    }

    /// The `HarnessModels` the program gives the Usage Planner (RuntimeAdapterInterface.md, O63).
    pub fn harness_models(&self) -> Arc<dyn HarnessModels> {
        Arc::new(ClaudeModels)
    }

    /// How many runs are in memory.
    // Used by the `tests` module, which is built on Unix alone.
    #[cfg(all(test, unix))]
    pub(crate) fn held_runs(&self) -> usize {
        lock(&self.inner.runs).len()
    }

    /// How many package servers a run still holds in memory.
    // Used by the `tests` module, which is built on Unix alone.
    #[cfg(all(test, unix))]
    pub(crate) fn package_server_count(&self, run_id: RunId) -> Option<usize> {
        self.inner
            .run(run_id)
            .map(|run| lock(&run.package_servers).len())
    }

    /// The tail of a failed run's stderr (`start` step 10).
    // Used by the `tests` module, which is built on Unix alone.
    #[cfg(all(test, unix))]
    pub(crate) fn stderr_tail(&self, run_id: RunId) -> Option<Vec<u8>> {
        self.inner
            .run(run_id)
            .map(|run| lock(&run.stderr_tail).clone())
    }

    /// `start`, keeping the internal code and retryability of a failure.
    pub(crate) async fn start_checked(
        &self,
        ctx: &AdapterContext,
        req: StartRun,
    ) -> Result<RunHandle, Failure> {
        let inner = &self.inner;
        let run_id = req.run_id;
        let held = inner.run(run_id).is_some_and(|run| !run.is_ended());
        if held || !lock(&inner.starting).insert(run_id) {
            return Err(Failure::invalid("CLA_SESSION_EXISTS", "RA_HANDLE_INVALID"));
        }
        let deadline = tokio::time::Instant::from_std(ctx.deadline);
        let outcome = tokio::time::timeout_at(deadline, inner.start_steps(ctx, req)).await;
        lock(&inner.starting).remove(&run_id);
        match outcome {
            Ok(result) => result,
            Err(_) => {
                // Step 12: everything registered so far goes; dropping the steps killed the child.
                let run = lock(&inner.runs).remove(&run_id);
                if let Some(run) = run {
                    inner.close_session(&run).await;
                }
                Err(Failure::new("CLA_DEADLINE", true, AdapterError::Deadline))
            }
        }
    }

    /// `resume`, keeping the internal code of a failure.
    pub(crate) async fn resume_checked(
        &self,
        ctx: &AdapterContext,
        handle: &RunHandle,
        decision: ResumeDecision,
    ) -> Result<(), Failure> {
        let run = match self.inner.run(handle.run_id) {
            Some(run) => run,
            None => {
                // One adoption at a time: a second would replace the first run's entry, and its
                // process could no longer be stopped.
                if !lock(&self.inner.starting).insert(handle.run_id) {
                    return Err(Failure::invalid("CLA_NOT_PAUSED", "RA_RESUME_MISMATCH"));
                }
                let adopted = match self.inner.run(handle.run_id) {
                    Some(run) => Ok(run),
                    None => self.adopt_paused_run(ctx, handle, &decision).await,
                };
                lock(&self.inner.starting).remove(&handle.run_id);
                adopted?
            }
        };
        let Some(paused_on) = run.paused_on() else {
            return Err(Failure::invalid("CLA_NOT_PAUSED", "RA_RESUME_MISMATCH"));
        };
        match paused_on {
            PauseReason::SignIn => match decision {
                ResumeDecision::Answer { text } => {
                    if !(1..=MAX_ANSWER).contains(&text.chars().count()) {
                        return Err(Failure::invalid("CLA_INPUT_INVALID", "RA_INPUT_TOO_LARGE"));
                    }
                    let _ = run.commands.send(RunCommand::RestartSignIn);
                    Ok(())
                }
                _ => Err(mismatch()),
            },
            PauseReason::Approval { .. } => {
                check_resume(&decision, &paused_on).map_err(|error| {
                    if error.code == "RA_RESUME_MISMATCH" {
                        mismatch()
                    } else {
                        Failure::invalid("CLA_INPUT_INVALID", error.code)
                    }
                })?;
                let turn = decision_turn(&decision).ok_or_else(mismatch)?;
                let (ack, done) = oneshot::channel();
                run.commands
                    .send(RunCommand::Resume { turn, ack })
                    .map_err(|_| unknown_run())?;
                done.await.map_err(|_| unknown_run())?
            }
            // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-3
            PauseReason::Person { .. } => {
                let ResumeDecision::Answer { text } = decision else {
                    return Err(mismatch());
                };
                if !(1..=MAX_ANSWER).contains(&text.chars().count()) {
                    return Err(Failure::invalid("CLA_INPUT_INVALID", "RA_INPUT_TOO_LARGE"));
                }
                let turn = answer_turn(&text);
                let (ack, done) = oneshot::channel();
                run.commands
                    .send(RunCommand::Resume { turn, ack })
                    .map_err(|_| unknown_run())?;
                done.await.map_err(|_| unknown_run())?
            }
            _ => Err(mismatch()),
        }
    }

    /// Agent profile: this instance took a paused run another instance paused, so the run is not in
    /// memory. A new entry holds it, paused on the decision's approval, with no process; `resume`'s
    /// "process ended" branch does the rest. Who the run is, and what it was narrowed to, come from
    /// the Rules Engine's record of the run; that it is paused, on which approval and for which
    /// caller, from the program's `SessionSetup.paused`; and that this instance took it, from the
    /// session record. A run missing any of them is `CLA_UNKNOWN_RUN`, and a decision for another
    /// approval is `CLA_RESUME_MISMATCH`. Everything else → `CLA_UNKNOWN_RUN`.
    // spec: ordin-desktop/components/ClaudeLocalAdapter#SA-RECON-08
    async fn adopt_paused_run(
        &self,
        ctx: &AdapterContext,
        handle: &RunHandle,
        decision: &ResumeDecision,
    ) -> Result<Arc<Run>, Failure> {
        let inner = &self.inner;
        let Some(runtime_session_id) = handle
            .runtime_session_id
            .clone()
            .filter(|id| inner.agent_profile() && is_session_id(id))
        else {
            return Err(unknown_run());
        };
        let approval_id = match decision {
            ResumeDecision::Approve { approval_id }
            | ResumeDecision::Reject { approval_id, .. } => *approval_id,
            ResumeDecision::Answer { .. } => return Err(unknown_run()),
        };
        let run_id = handle.run_id;
        let record = inner
            .rules
            .adopted_run(run_id)
            .await
            .map_err(|_| Failure::unreachable("CLA_RULES_UNAVAILABLE", true))?
            .ok_or_else(unknown_run)?;
        let scope_id = record
            .scope
            .as_deref()
            .map(ScopeId::parse)
            .transpose()
            .map_err(|_| unknown_run())?;
        let agent_instance_id = record.agent_instance_id;
        // An adopted run has no start request, so no attachments reach its setup.
        let setup: SessionSetup = inner
            .setup
            .session_setup(run_id, agent_instance_id, &[])
            .await
            .map_err(|_| Failure::unreachable("CLA_SETUP_FAILED", true))?;
        let paused = setup.paused.ok_or_else(unknown_run)?;
        if paused.approval_id != approval_id {
            return Err(mismatch());
        }
        inner.check_record(run_id, &runtime_session_id).await?;
        let harness_version = inner.check_version(run_id).await?;
        let (run, inbox) = Run::new(
            RunIdentity {
                run_id,
                agent_instance_id,
                workspace_id: record.workspace_id,
                runtime_session_id: runtime_session_id.clone(),
                correlation_id: ctx.correlation_id.clone(),
                caller: paused.caller,
                scope_id,
                // Adoption is agent-profile only, where no run has a coding task.
                coding_task_id: None,
                // An adopted run has no server limit (only a minutes run has one, never adopted).
                only_servers: None,
                // An adopted run writes no first turn.
                started_by_person: false,
                harness_version,
                setup,
            },
            true,
        );
        // The run's own time limit, counted in full from the adoption.
        *lock(&run.plan) = SessionPlan {
            tool_subset: record.tool_subset,
            single_call: record.single_call,
            limit_seconds: record.limit_seconds,
        };
        run.emit(RunEvent::Started {
            runtime_session_id: Some(runtime_session_id),
            remote_thread_id: None,
        });
        run.mark_paused(PauseReason::Approval {
            approval_id: paused.approval_id,
        });
        inner.hold_run(run.clone());
        tokio::spawn(run_task(
            inner.clone(),
            run.clone(),
            inbox,
            TaskStart::Parked,
        ));
        Ok(run)
    }
}

#[async_trait::async_trait]
impl RuntimeAdapter for ClaudeLocalAdapter {
    fn kind(&self) -> AdapterKind {
        AdapterKind::ClaudeLocal
    }

    /// 2C `AdapterVerb` table: `claude_local` offers every verb.
    fn unsupported_verbs(&self) -> &'static [AdapterVerb] {
        &[]
    }

    async fn start(&self, ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError> {
        let run_id = req.run_id;
        self.start_checked(ctx, req).await.map_err(|failure| {
            log_failure(run_id, &failure);
            failure.error
        })
    }

    fn events(
        &self,
        _ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<BoxStream<'static, RunEvent>, AdapterError> {
        match self.inner.run(run.run_id) {
            Some(run) => Ok(run.subscribe()),
            None => {
                log_failure(run.run_id, &unknown_run());
                Err(AdapterError::UnknownRun)
            }
        }
    }

    async fn resume(
        &self,
        ctx: &AdapterContext,
        run: &RunHandle,
        decision: ResumeDecision,
    ) -> Result<(), AdapterError> {
        self.resume_checked(ctx, run, decision)
            .await
            .map_err(|failure| {
                log_failure(run.run_id, &failure);
                failure.error
            })
    }

    async fn read(
        &self,
        _ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<RunSnapshot, AdapterError> {
        match self.inner.run(run.run_id) {
            Some(run) => Ok(run.snapshot()),
            None => {
                // A question, not a refused request: after a restart the Session Supervisor reads
                // every run the journal shows in progress, and none is known to this process.
                // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-LOG-1
                tracing::info!(
                    target: "ordin_claude_adapter",
                    run_id = %run.run_id,
                    code = "CLA_UNKNOWN_RUN",
                    "claude.read.unknown_run"
                );
                Err(AdapterError::UnknownRun)
            }
        }
    }

    async fn cancel(&self, _ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError> {
        let Some(run) = self.inner.run(run.run_id) else {
            log_failure(run.run_id, &unknown_run());
            return Err(AdapterError::UnknownRun);
        };
        if run.is_ended() {
            return Ok(());
        }
        let (ack, done) = oneshot::channel();
        if run
            .commands
            .send(RunCommand::Stop {
                reason: RunEndReason::Cancelled,
                ack,
            })
            .is_ok()
        {
            let _ = done.await;
        }
        Ok(())
    }
}
