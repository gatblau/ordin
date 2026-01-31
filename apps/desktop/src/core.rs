//! `Core::start`'s construction (OrdinDesktop.md *Internal Logic* step 3): the steps of
//! [`crate::plan::start_steps`] run one after another, and the first error stops start with nothing
//! later constructed.
//!
//! The runner that calls each owner's constructor is given to [`construct`]; the order is not the
//! runner's to choose. The real runner is [`crate::wiring::ModuleRunner`]; the Hook Relay's write
//! attribution is checked right after row 3.23 there (DSK-WIRE-2), and the program's
//! `SessionSetupSource` of rows 3.32–3.33 sets `SessionSetup.paused` to `None` (DSK-WIRE-3).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use ordin_session_supervisor::ShutdownReason;
use ordin_setup::UpdateFinish;
use ordin_types::CodedError;
use tokio_util::sync::CancellationToken;

use crate::error::{StartError, SHUTDOWN_STEP_TIMEOUT, UPDATE_INSTALL_FAILED};
use crate::hosts::Late;
use crate::paths::DesktopPaths;
use crate::plan::{start_steps, Row, Step};
use crate::shell::QuitControl;
use crate::wiring::{CoreInputs, ModuleRunner, Modules};

/// Why the core stops (distinct from the Session Supervisor's `ShutdownReason`); `Update` added by
/// the amendment January 2026 (ADR-114).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitReason {
    PersonQuit,
    OperatingSystemEnding,
    Update,
}

impl QuitReason {
    /// The `reason` of the `shutdown` log line.
    pub fn log_name(self) -> &'static str {
        match self {
            QuitReason::PersonQuit => "person_quit",
            QuitReason::OperatingSystemEnding => "operating_system_ending",
            QuitReason::Update => "update",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownSummary {
    pub sessions_stopped: u32,
    pub sessions_interrupted: u32,
    pub journal_shut_down: bool,
}

/// What the shell does once the core has stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterQuit {
    /// `app.exit(0)`.
    Exit,
    /// `app.restart()` (step 15.4: the person is never left without ordin).
    Restart,
}

/// The bound of the Session Supervisor's shutdown (step 10.3).
pub const SUPERVISOR_STOP_BOUND: Duration = Duration::from_secs(15);
/// The bound of every other shutdown step (step 10.3).
pub const STEP_BOUND: Duration = Duration::from_secs(2);
/// The bound of `finish_update` (DSK-UP-12, DSK-UP-13: the installer's own run).
pub const FINISH_UPDATE_BOUND: Duration = Duration::from_secs(120);

/// The started core: one handle per constructed module, in construction order, and the tokens of
/// step 4 and step 10.
pub struct Core {
    pub modules: Modules,
    pub late: Arc<Late>,
    pub paths: DesktopPaths,
    /// The core's shutdown token (step 10.3: Mail, Catalogue, Memory, MCP Front, Ordin Host and the
    /// other token-driven modules).
    pub shutdown_token: CancellationToken,
    /// The Server Connector's token (step 4; cancelled in step 10.3).
    pub connector_cancel: CancellationToken,
    /// Step 10.1, 10.2 and step 15's flag.
    pub quit: QuitControl,
}

impl Core {
    /// Steps 3 and 4: constructs every row of this build in order, then starts the background work.
    pub async fn start(inputs: CoreInputs) -> Result<Arc<Core>, StartError> {
        let mut runner = ModuleRunner::new(inputs);
        construct(&mut runner).await?;
        Core::from_runner(runner).await
    }

    /// Step 4 over a runner that has run every step of step 3.
    pub async fn from_runner(runner: ModuleRunner) -> Result<Arc<Core>, StartError> {
        let core = Core {
            modules: runner.modules,
            late: runner.late,
            paths: runner.inputs.paths,
            shutdown_token: runner.shutdown,
            connector_cancel: CancellationToken::new(),
            quit: QuitControl::default(),
        };
        core.start_background().await?;
        Ok(Arc::new(core))
    }

    /// Step 4, in order. `recover_external_actions` and `hook_relay.start()` stop start; every
    /// other failure is logged and start continues.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-36
    async fn start_background(&self) -> Result<(), StartError> {
        let m = &self.modules;
        if let Some(journal) = &m.journal {
            // DSK-REC-36.1: it runs "before any session resumes", and `supervisor.recover()` resumes.
            journal
                .recover_external_actions()
                .await
                .map_err(|error| StartError::ModuleFailed {
                    module: Row::Journal.module(),
                    code: error.code().to_owned(),
                })?;
            let max_days = m
                .profile
                .as_ref()
                .and_then(|p| p.current().document.history.max_days);
            step_logged(Row::Journal, journal.set_company_max_days(max_days).await);
        }
        if let Some(setup) = &m.setup {
            record_started(setup).await;
        }
        if let Some(folders) = &m.agent_folders {
            step_logged(Row::AgentFolders, folders.recover().await);
        }
        if let Some(inspector) = &m.inspector {
            match inspector.reload_rule_sets().await {
                Ok(failures) => {
                    for failure in failures {
                        tracing::warn!(
                            component = "desktop",
                            agent_instance_id = failure.agent_instance_id.to_string().as_str(),
                            code = failure.code.as_str(),
                            "rule_set_reload_failed"
                        );
                    }
                }
                Err(error) => tracing::warn!(
                    component = "desktop",
                    code = error.code(),
                    "rule_set_reload_failed"
                ),
            }
        }
        if let Some(relay) = &m.hook_relay {
            relay
                .start()
                .await
                .map_err(|error| StartError::ModuleFailed {
                    module: Row::HookRelay.module(),
                    code: error.code().to_owned(),
                })?;
        }
        if let Some(supervisor) = &m.supervisor {
            step_logged(Row::SessionSupervisor, supervisor.recover().await);
        }
        if let Some(orchestrator) = &m.orchestrator {
            step_logged(Row::CrewOrchestrator, orchestrator.recover().await);
        }
        if let Some(catalogue) = &m.catalogue {
            let started = catalogue.start(self.shutdown_token.clone()).await;
            let ok = started.is_ok();
            step_logged(Row::Catalogue, started);
            // The Package Catalogue's notices loop, whose start sweep re-plans every `ready` instance
            // against the catalogue `catalogue.start` has just indexed (PackageCatalogue.md §C), so a
            // new application version updates the agents of the bundled repository (PKG-REC-3).
            // spec: ordin-desktop/components/OrdinDesktop#DSK-PKG-1
            if let (true, Some(packages)) = (ok, m.packages.clone()) {
                let shutdown = self.shutdown_token.clone();
                tokio::spawn(async move {
                    if let Err(e) = packages.run(shutdown).await {
                        tracing::warn!(component = "desktop", code = e.code, "packages.run_failed");
                    }
                });
            }
            // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-24
            if let (true, Some(packages), Some(localisation)) =
                (ok, m.packages.clone(), m.localisation.clone())
            {
                let shutdown = self.shutdown_token.clone();
                let workspaces = m.workspaces.clone();
                // spec: ordin-desktop/components/OrdinDesktop#DSK-DOC-1
                tokio::spawn(async move {
                    crate::bundled::add_bundled_agents(
                        packages.clone(),
                        localisation.clone(),
                        shutdown.clone(),
                    )
                    .await;
                    if let Some(workspaces) = workspaces {
                        crate::bundled::add_starter_agents_everywhere(
                            packages,
                            workspaces,
                            localisation,
                            shutdown,
                        )
                        .await;
                    }
                });
            }
        }
        if let Some(host) = &m.ordin_host {
            step_logged(
                Row::OrdinHost,
                host.start(self.shutdown_token.clone()).await,
            );
        }
        if let Some(files) = &m.files {
            step_logged(Row::Files, files.start().await);
        }
        if let Some(workspaces) = &m.workspaces {
            step_logged(Row::Workspaces, workspaces.start().await);
        }
        if let Some(projects) = &m.projects {
            step_logged(Row::Projects, projects.start().await);
        }
        if let Some(craft) = &m.craft {
            step_logged(Row::CraftToolkit, craft.start().await);
        }
        if let Some(home) = &m.home {
            home.start().await;
        }
        // The Knowledge Service's freshness listeners, memory-index task and start-up scan; they
        // stop when step 10.3 cancels the core's shutdown token.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-KS-1
        if let Some(knowledge) = m.knowledge.clone() {
            tokio::spawn(knowledge.run(self.shutdown_token.clone()));
        }
        // The Push Service's recovery and decision loops: an approved **Send back** is carried out
        // only while this runs; it stops when step 10.3 cancels the core's shutdown token.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-PUSH-1
        if let Some(push) = m.push.clone() {
            let shutdown = self.shutdown_token.clone();
            tokio::spawn(async move {
                if let Err(e) = push.run(shutdown).await {
                    tracing::warn!(
                        component = "desktop",
                        code = ordin_types::CodedError::code(&e),
                        "push.run_failed"
                    );
                }
            });
        }
        // The Team Knowledge Service's checks, timer, listeners and run hook (TeamKnowledgeService.md
        // §A, TW-F); they stop when step 10.3 cancels the core's shutdown token.
        #[cfg(feature = "team-knowledge")]
        if let Some(team) = m.team_knowledge.clone() {
            tokio::spawn(team.run(self.shutdown_token.clone()));
        }
        // The Minutes Service's job loop (MinutesService.md A2); it stops when step 10.3 cancels the
        // core's shutdown token.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-1
        #[cfg(feature = "minutes")]
        if let Some(minutes) = m.minutes.clone() {
            let shutdown = self.shutdown_token.clone();
            tokio::spawn(async move { minutes.run(shutdown).await });
        }
        if let (Some(connector), Some(sender)) = (m.connector.clone(), m.server_events.clone()) {
            let cancel = self.connector_cancel.clone();
            tokio::spawn(async move {
                step_logged(Row::ServerConnector, connector.start(sender, cancel).await);
            });
        }
        if let Some(setup) = m.setup.clone() {
            // Each change of the guided run's progress becomes a `setup_progress` event
            // (SetupAssistant.md, Event `setup_progress`: emitted by the program from `progress()`
            // through `Journal::append_event`), which the first-start screen draws.
            if let Some(journal) = m.journal.clone() {
                let mut progress = setup.progress();
                tokio::spawn(async move {
                    while progress.changed().await.is_ok() {
                        let current = progress.borrow_and_update().clone();
                        let data = serde_json::json!({ "progress": current });
                        if let Err(e) = journal.append_event("setup_progress", data).await {
                            tracing::warn!(
                                component = "desktop",
                                code = e.code(),
                                "setup.progress_event_failed"
                            );
                        }
                    }
                });
            }
            // The start checks, then the background checks, on one task start does not wait for. The
            // start report reaches the screens as the progress the listener above turns into
            // `setup_progress` events; a failed check, or an error, never stops start.
            // spec: ordin-desktop/components/OrdinDesktop#DSK-SC-1
            tokio::spawn(async move {
                if let Err(e) = setup.run_checks(ordin_setup::CheckTrigger::Start).await {
                    tracing::warn!(
                        component = "desktop",
                        code = e.code(),
                        "setup.start_checks_failed"
                    );
                }
                setup.start_background_checks().await;
            });
        }
        self.paths.log_missing_sidecars();
        Ok(())
    }

    /// The number of local sessions running now (step 10.2).
    pub fn running_local_sessions(&self) -> u32 {
        self.modules
            .supervisor
            .as_ref()
            .map_or(0, |s| s.running_local_sessions())
    }

    /// Step 10.3: every step in order, each bounded; an overrun is logged and the next step runs.
    pub async fn shutdown(&self, reason: QuitReason) -> ShutdownSummary {
        let m = &self.modules;
        let mut summary = ShutdownSummary {
            sessions_stopped: 0,
            sessions_interrupted: 0,
            journal_shut_down: false,
        };
        if let Some(bridge) = &m.bridge {
            bounded("command_bridge.close", STEP_BOUND, bridge.close()).await;
        }
        if let Some(supervisor) = &m.supervisor {
            if let Some(report) = bounded(
                "session_supervisor.shutdown",
                SUPERVISOR_STOP_BOUND,
                supervisor.shutdown(ShutdownReason::Quit),
            )
            .await
            {
                summary.sessions_stopped = report.stopped;
                summary.sessions_interrupted = report.interrupted;
            }
        }
        // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-8
        self.late.runs.release_all();
        self.connector_cancel.cancel();
        if let Some(setup) = &m.setup {
            bounded("setup_assistant.close", STEP_BOUND, setup.close()).await;
        }
        if let Some(workspaces) = &m.workspaces {
            bounded("workspaces.shutdown", STEP_BOUND, workspaces.shutdown()).await;
        }
        if let Some(projects) = &m.projects {
            bounded("projects.shutdown", STEP_BOUND, projects.shutdown()).await;
        }
        if let Some(craft) = &m.craft {
            bounded("craft.shutdown", STEP_BOUND, craft.shutdown()).await;
        }
        if let Some(files) = &m.files {
            bounded("files.shutdown", STEP_BOUND, files.shutdown()).await;
        }
        if let Some(profile) = &m.profile {
            bounded("profile.shutdown", STEP_BOUND, profile.shutdown()).await;
        }
        // DSK-MIN-5: the Minutes Service before the Journal; on sleep nothing is done.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-5
        #[cfg(feature = "minutes")]
        if let Some(minutes) = &m.minutes {
            bounded("minutes.shutdown", STEP_BOUND, minutes.shutdown()).await;
        }
        self.shutdown_token.cancel();
        if let Some(relay) = &m.hook_relay {
            bounded("hook_relay.shutdown", STEP_BOUND, relay.shutdown()).await;
        }
        if let Some(journal) = &m.journal {
            summary.journal_shut_down = bounded("journal.shutdown", STEP_BOUND, journal.shutdown())
                .await
                .is_some();
            bounded("datastore.close", STEP_BOUND, journal.datastore().close()).await;
        }
        let _ = reason;
        summary
    }

    /// Steps 10.3 and 10.4 (with DSK-UP-12) or step 15 (DSK-UP-13): stops the core, finishes an
    /// update as the reason says, logs `shutdown` and tells the shell whether to exit or restart.
    pub async fn quit(&self, reason: QuitReason) -> AfterQuit {
        let summary = self.shutdown(reason).await;
        let after = match reason {
            // 10.5: nothing is installed while the operating system ends the session.
            QuitReason::OperatingSystemEnding => AfterQuit::Exit,
            // spec: ordin-desktop/components/OrdinDesktop#DSK-UP-12
            QuitReason::PersonQuit => {
                self.finish_update(UpdateFinish::OnExit, "update_install_on_exit")
                    .await;
                AfterQuit::Exit
            }
            // spec: ordin-desktop/components/OrdinDesktop#DSK-UP-13
            QuitReason::Update => {
                self.finish_update(UpdateFinish::Restart, "update_restart")
                    .await;
                AfterQuit::Restart
            }
        };
        tracing::info!(
            component = "desktop",
            reason = reason.log_name(),
            sessions_stopped = summary.sessions_stopped,
            sessions_interrupted = summary.sessions_interrupted,
            journal_shut_down = summary.journal_shut_down,
            "shutdown"
        );
        after
    }

    /// `setup_assistant.finish_update(how)`, bounded at 120 s; an error or an overrun is logged with
    /// `DSK_UPDATE_INSTALL_FAILED`.
    async fn finish_update(&self, how: UpdateFinish, line: &'static str) {
        let Some(setup) = &self.modules.setup else {
            return;
        };
        let version = setup.update_offer().map(|o| o.version).unwrap_or_default();
        match tokio::time::timeout(FINISH_UPDATE_BOUND, setup.finish_update(how)).await {
            Ok(Ok(installed)) => tracing::info!(
                component = "desktop",
                version = version.as_str(),
                outcome = if installed {
                    "installed"
                } else {
                    "nothing_ready"
                },
                "{line}"
            ),
            Ok(Err(error)) => tracing::error!(
                component = "desktop",
                version = version.as_str(),
                code = UPDATE_INSTALL_FAILED,
                setup_code = error.code(),
                outcome = "failed",
                "{line}"
            ),
            Err(_) => tracing::error!(
                component = "desktop",
                version = version.as_str(),
                code = UPDATE_INSTALL_FAILED,
                outcome = "timed_out",
                "{line}"
            ),
        }
    }
}

/// One step of step 10.3 within `bound`; an overrun is logged `DSK_SHUTDOWN_STEP_TIMEOUT` and
/// `None` is returned.
pub async fn bounded<T>(
    step: &'static str,
    bound: Duration,
    work: impl Future<Output = T>,
) -> Option<T> {
    match tokio::time::timeout(bound, work).await {
        Ok(out) => Some(out),
        Err(_) => {
            tracing::warn!(
                component = "desktop",
                code = SHUTDOWN_STEP_TIMEOUT,
                step,
                "shutdown_step_timeout"
            );
            None
        }
    }
}

/// DSK-REC-44: the highest version started before this one; a lower running version is logged
/// `update_downgrade_seen` (DSK-UP-14). A failure is logged and start continues.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-44
async fn record_started(setup: &ordin_setup::SetupAssistant) {
    match setup.record_started().await {
        Ok(Some(last_started)) if downgraded(env!("CARGO_PKG_VERSION"), &last_started) => {
            tracing::warn!(
                component = "desktop",
                running = env!("CARGO_PKG_VERSION"),
                last_started = last_started.as_str(),
                "update_downgrade_seen"
            );
        }
        Ok(_) => {}
        Err(error) => step_logged::<(), _>(Row::Setup, Err(error)),
    }
}

/// `running` is lower than `last_started` by semantic-version order; an unreadable version is
/// never a downgrade.
pub(crate) fn downgraded(running: &str, last_started: &str) -> bool {
    match (
        semver::Version::parse(running),
        semver::Version::parse(last_started),
    ) {
        (Ok(running), Ok(last)) => running < last,
        _ => false,
    }
}

/// A step-4 call whose failure is logged and start continues (DSK-REC-36.2).
fn step_logged<T, E: CodedError>(row: Row, result: Result<T, E>) {
    if let Err(error) = result {
        tracing::warn!(
            component = "desktop",
            module = row.module(),
            code = error.code(),
            "background_step_failed"
        );
    }
}

/// Runs one construction step: constructs a row's module with its owner's constructor, or does the
/// wiring the step names.
#[async_trait::async_trait]
pub trait StepRunner: Send {
    async fn run(&mut self, step: Step) -> Result<(), StartError>;
}

/// The counts the `wired` log line carries after row 3.49.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wired {
    pub modules: usize,
    pub tool_providers: usize,
    pub adapters: usize,
}

/// Step 3: runs every step of this build in order. A failure stops start; no later step runs.
// spec: ordin-desktop/components/OrdinDesktop#DSK-UP-5
pub async fn construct(runner: &mut dyn StepRunner) -> Result<Wired, StartError> {
    let mut wired = Wired {
        modules: 0,
        tool_providers: 0,
        adapters: 0,
    };
    for step in start_steps() {
        runner.run(step).await?;
        match step {
            Step::Construct(_) => wired.modules += 1,
            Step::RegisterProvider(_) => wired.tool_providers += 1,
            Step::RegisterAdapter(_) => wired.adapters += 1,
            _ => {}
        }
    }
    tracing::info!(
        component = "desktop",
        modules = wired.modules,
        tool_providers = wired.tool_providers,
        adapters = wired.adapters,
        "wired"
    );
    Ok(wired)
}

/// Logs `start_failed` with the code and the module (step 9).
pub fn log_start_failed(error: &StartError) {
    let module_code = match error {
        StartError::ModuleFailed { code, .. } => Some(code.as_str()),
        StartError::DataRootUnwritable { .. } => None,
    };
    tracing::error!(
        component = "desktop",
        code = error.code(),
        module = error.module(),
        module_code,
        "start_failed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{rows, Row};

    /// Fake modules: records each step and fails the row it is told to.
    struct Recorder {
        steps: Vec<Step>,
        fail: Option<(Row, &'static str)>,
    }

    #[async_trait::async_trait]
    impl StepRunner for Recorder {
        async fn run(&mut self, step: Step) -> Result<(), StartError> {
            if let (Step::Construct(row), Some((failing, code))) = (step, self.fail) {
                if row == failing {
                    return Err(StartError::ModuleFailed {
                        module: row.module(),
                        code: code.to_owned(),
                    });
                }
            }
            self.steps.push(step);
            Ok(())
        }
    }

    fn constructed(steps: &[Step]) -> Vec<Row> {
        steps
            .iter()
            .filter_map(|s| match s {
                Step::Construct(row) => Some(*row),
                _ => None,
            })
            .collect()
    }

    // Integration stub (§6.4): construction with fake modules records the order of 3.1–3.49.
    #[tokio::test]
    async fn construction_with_fake_modules_records_the_order_of_step_3() {
        let mut recorder = Recorder {
            steps: Vec::new(),
            fail: None,
        };
        let wired = construct(&mut recorder).await.expect("start");
        assert_eq!(recorder.steps, start_steps());
        assert_eq!(constructed(&recorder.steps), rows());
        assert_eq!(wired.modules, rows().len());
        assert_eq!(
            wired.tool_providers,
            recorder
                .steps
                .iter()
                .filter(|s| matches!(s, Step::RegisterProvider(_)))
                .count()
        );
        assert!(wired.adapters >= 1);
    }

    #[tokio::test]
    async fn error_path_a_module_fails_to_start() {
        let mut recorder = Recorder {
            steps: Vec::new(),
            fail: Some((Row::Journal, "JRNL_SCHEMA_NEWER")),
        };
        let err = construct(&mut recorder).await.expect_err("stops");
        log_start_failed(&err);
        assert!(recorder.steps.is_empty(), "no module after the Journal");
        assert_eq!(err.code(), "DSK_START_FAILED");
        assert_eq!(err.module(), Some("ordin-journal"));
    }

    #[tokio::test]
    async fn error_path_the_sharepoint_adapter_refuses_to_start() {
        let mut recorder = Recorder {
            steps: Vec::new(),
            fail: Some((Row::SharePoint, "SP_DATASTORE_UNAVAILABLE")),
        };
        let err = construct(&mut recorder).await.expect_err("stops");
        assert_eq!(err.code(), "DSK_START_FAILED");
        assert_eq!(err.module(), Some("ordin-sharepoint"));
        assert!(matches!(
            &err,
            StartError::ModuleFailed { code, .. } if code == "SP_DATASTORE_UNAVAILABLE"
        ));
        let built = constructed(&recorder.steps);
        assert_eq!(built.last(), Some(&Row::Resolver), "nothing after 3.9");
    }
}
