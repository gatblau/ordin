//! The assistant chosen at first start (OrdinDesktop.md DSK-CHO-1 to DSK-CHO-4): one test per
//! Gherkin scenario of the amendment, over the real desktop core on a temporary data root. Each
//! assistant is a stand-in `HarnessProbe` that is found and signed in, registered with the Setup
//! Assistant in place of the real ones, so the guided run asks the question without either CLI.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_organisation_profile::HarnessName;
use ordin_service_api::{command, dispatch};
use ordin_setup::{
    CheckKind, HarnessKind, HarnessLocation, HarnessProbe, HarnessSignIn, ProbeError, ProbeRepair,
    SetupAssistant, StepState,
};
use serde_json::{json, Value};

use crate::core::{construct, Core, QuitReason};
use crate::prefs::{Appearance, Preferences};
use crate::shell::{PickerUnavailable, QuitTrigger, Shell};
use crate::test_support::TempDir;
use crate::wiring::ModuleRunner;
use crate::wiring_tests::{context, inputs};

/// The window shell, recording the quits it is asked for.
#[derive(Default)]
struct RecordingShell {
    quits: Mutex<Vec<QuitTrigger>>,
}

impl RecordingShell {
    fn quits(&self) -> Vec<QuitTrigger> {
        self.quits.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Shell for RecordingShell {
    async fn pick_folder(&self) -> Result<Option<PathBuf>, PickerUnavailable> {
        Ok(None)
    }
    async fn pick_file(&self, _title: &str) -> Result<Option<PathBuf>, PickerUnavailable> {
        Ok(None)
    }
    async fn pick_file_of(
        &self,
        _title: &str,
        _filter_name: &str,
        _extensions: &[&str],
    ) -> Result<Option<PathBuf>, PickerUnavailable> {
        Ok(None)
    }
    async fn save_path(&self, _file_name: &str) -> Option<PathBuf> {
        None
    }
    fn quit(&self, trigger: QuitTrigger) {
        self.quits.lock().unwrap().push(trigger);
    }
    fn appearance_changed(&self, _appearance: Appearance) {}
}

/// One assistant, found at a fixed path and signed in; nothing is started.
struct SignedInProbe(HarnessKind);

#[async_trait::async_trait]
impl HarnessProbe for SignedInProbe {
    fn harness(&self) -> HarnessKind {
        self.0
    }
    async fn locate(&self) -> Result<Option<HarnessLocation>, ProbeError> {
        let path = match self.0 {
            HarnessKind::ClaudeCode => "/home/p/.local/bin/claude",
            HarnessKind::CopilotCli => "/home/p/.local/bin/copilot",
        };
        Ok(Some(HarnessLocation {
            path: PathBuf::from(path),
            version: "1.0.0".to_owned(),
        }))
    }
    async fn sign_in_status(&self) -> Result<HarnessSignIn, ProbeError> {
        Ok(HarnessSignIn::SignedIn {
            method: Some("test".to_owned()),
        })
    }
    async fn start_sign_in(&self) -> Result<(), ProbeError> {
        Ok(())
    }
    async fn repair(&self) -> Result<ProbeRepair, ProbeError> {
        Ok(ProbeRepair::NothingToDo)
    }
}

/// Steps 3 and 4 over `dir`'s data root, with the recording shell and both stand-in assistants.
async fn start(dir: &TempDir, shell: Arc<RecordingShell>) -> Arc<Core> {
    let mut core_inputs = inputs(dir);
    core_inputs.shell = Some(shell);
    let mut runner = ModuleRunner::new(core_inputs);
    construct(&mut runner).await.expect("step 3");
    let core = Core::from_runner(runner).await.expect("step 4");
    let setup = setup(&core);
    for kind in [HarnessKind::ClaudeCode, HarnessKind::CopilotCli] {
        setup.register_probe(Arc::new(SignedInProbe(kind)));
    }
    core
}

fn setup(core: &Core) -> Arc<SetupAssistant> {
    core.modules.setup.clone().expect("row 3.43")
}

async fn call(
    core: &Core,
    name: &str,
    input: Value,
) -> Result<Value, ordin_service_api::ServiceError> {
    let api = core.modules.service_api.clone().expect("row 3.48");
    dispatch(&*api, &context(), command(name).expect("row"), input).await
}

/// `agents.harness` written as Settings writes it.
fn store_harness(dir: &TempDir, harness: ordin_service_api::commands::profile::HarnessName) {
    let preferences = Preferences::new(inputs_preferences(dir));
    preferences
        .change_agents(&ordin_service_api::commands::ChangeAgentSetting::Harness {
            harness: Some(harness),
        })
        .expect("stored");
}

fn inputs_preferences(dir: &TempDir) -> PathBuf {
    inputs(dir).paths.preferences
}

fn stored_harness(dir: &TempDir) -> Option<HarnessName> {
    Preferences::new(inputs_preferences(dir))
        .read()
        .agents
        .harness
}

/// Starts `setup.run_guided` on its own task; it waits at the question until it is answered.
fn run_guided(core: &Arc<Core>) -> tokio::task::JoinHandle<()> {
    let core = core.clone();
    tokio::spawn(async move {
        let _ = call(&core, "setup.run_guided", json!({})).await;
    })
}

/// Waits until the guided run's progress satisfies `seen`; panics after ten seconds.
async fn progress_until(
    setup: &SetupAssistant,
    what: &str,
    seen: impl Fn(&[ordin_setup::ProgressStep]) -> bool,
) {
    let mut progress = setup.progress();
    let reached = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if seen(&progress.borrow_and_update().steps) {
                return true;
            }
            if progress.changed().await.is_err() {
                return false;
            }
        }
    })
    .await;
    assert_eq!(reached, Ok(true), "{what}");
}

async fn question_shown(core: &Core) {
    progress_until(&setup(core), "the question was never shown", |steps| {
        steps
            .iter()
            .any(|s| s.check == CheckKind::HarnessChoice && s.state == StepState::NeedsYou)
    })
    .await;
}

/// The assistant check of the guided run has finished.
async fn harness_checked(core: &Core) {
    progress_until(&setup(core), "the assistant was never checked", |steps| {
        steps.iter().any(|s| {
            s.check == CheckKind::Harness
                && matches!(
                    s.state,
                    StepState::Done | StepState::Failed | StepState::NeedsYou
                )
        })
    })
    .await;
}

// Scenario: Happy path — choosing Copilot restarts ordin with the Copilot Local Adapter (DSK-CHO-1,
// DSK-CHO-2)
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-1
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-2
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-4
#[cfg(feature = "copilot")]
#[tokio::test]
async fn dsk_cho_choosing_copilot_restarts_ordin_with_the_copilot_local_adapter() {
    use crate::core::AfterQuit;
    let dir = TempDir::new("cho-copilot");
    let shell = Arc::new(RecordingShell::default());
    let core = start(&dir, shell.clone()).await;
    assert_eq!(core.modules.harness_in_use, Some(HarnessName::ClaudeCode));
    let guided = run_guided(&core);
    question_shown(&core).await;
    // The question is part of `setup.status` on the wire.
    let status = call(&core, "setup.status", json!({}))
        .await
        .expect("status");
    assert!(
        status.to_string().contains("\"harness_choice\""),
        "{status}"
    );
    let answer = call(
        &core,
        "setup.choose_harness",
        json!({ "harness": "copilot_cli" }),
    )
    .await
    .expect("chosen");
    // DSK-CHO-4: the answer is given, and the shutdown is only asked for.
    assert_eq!(answer, json!({ "restarting": true }));
    assert_eq!(stored_harness(&dir), Some(HarnessName::Copilot));
    assert_eq!(shell.quits(), [QuitTrigger::HarnessChosen]);
    // What the window does with the trigger: the core stops with `harness_chosen` and restarts.
    assert_eq!(
        core.quit
            .decide(QuitTrigger::HarnessChosen, core.running_local_sessions()),
        crate::shell::QuitDecision::Proceed(QuitReason::HarnessChosen)
    );
    assert_eq!(
        core.quit(QuitReason::HarnessChosen).await,
        AfterQuit::Restart
    );
    guided.abort();
    // The next start over the same data root wires Copilot at row 3.19 and does not ask again.
    let shell = Arc::new(RecordingShell::default());
    let again = start(&dir, shell.clone()).await;
    assert_eq!(again.modules.harness_in_use, Some(HarnessName::Copilot));
    assert!(again
        .modules
        .adapters
        .contains(&crate::plan::Adapter::Copilot));
    let guided = run_guided(&again);
    harness_checked(&again).await;
    let steps = setup(&again).progress().borrow().steps.clone();
    assert!(
        !steps.iter().any(|s| s.check == CheckKind::HarnessChoice),
        "{steps:?}"
    );
    assert!(shell.quits().is_empty());
    guided.abort();
    again.quit(QuitReason::PersonQuit).await;
}

// Scenario: Happy path — choosing Claude Code goes on without a restart (DSK-CHO-2)
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-2
#[tokio::test]
async fn dsk_cho_choosing_claude_code_goes_on_without_a_restart() {
    let dir = TempDir::new("cho-claude");
    let shell = Arc::new(RecordingShell::default());
    let core = start(&dir, shell.clone()).await;
    assert_eq!(core.modules.harness_in_use, Some(HarnessName::ClaudeCode));
    let guided = run_guided(&core);
    question_shown(&core).await;
    let answer = call(
        &core,
        "setup.choose_harness",
        json!({ "harness": "claude_code" }),
    )
    .await
    .expect("chosen");
    assert_eq!(answer, json!({ "restarting": false }));
    assert_eq!(stored_harness(&dir), Some(HarnessName::ClaudeCode));
    assert!(shell.quits().is_empty());
    // The guided run goes on in this process, with Claude Code.
    harness_checked(&core).await;
    guided.abort();
    core.quit(QuitReason::PersonQuit).await;
}

// Scenario: Edge case — a stored choice of Copilot is used at start (DSK-CHO-1)
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-1
#[cfg(feature = "copilot")]
#[tokio::test]
async fn dsk_cho_a_stored_choice_of_copilot_is_used_at_start() {
    let dir = TempDir::new("cho-stored");
    std::fs::create_dir_all(inputs_preferences(&dir).parent().expect("data root")).expect("root");
    store_harness(
        &dir,
        ordin_service_api::commands::profile::HarnessName::Copilot,
    );
    let core = start(&dir, Arc::new(RecordingShell::default())).await;
    assert_eq!(core.modules.harness_in_use, Some(HarnessName::Copilot));
    let view = call(&core, "app.agent_settings", json!({}))
        .await
        .expect("view");
    assert_eq!(view["harness_in_use"], json!("copilot"), "{view}");
    core.quit(QuitReason::PersonQuit).await;
}

// Scenario: Edge case — a build without copilot keeps the choice and uses Claude Code (DSK-CHO-1)
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-1
#[cfg(not(feature = "copilot"))]
#[tokio::test]
async fn dsk_cho_a_build_without_copilot_keeps_the_choice_and_uses_claude_code() {
    let dir = TempDir::new("cho-no-copilot");
    std::fs::create_dir_all(inputs_preferences(&dir).parent().expect("data root")).expect("root");
    store_harness(
        &dir,
        ordin_service_api::commands::profile::HarnessName::Copilot,
    );
    let core = start(&dir, Arc::new(RecordingShell::default())).await;
    assert_eq!(core.modules.harness_in_use, Some(HarnessName::ClaudeCode));
    let view = call(&core, "app.agent_settings", json!({}))
        .await
        .expect("view");
    assert_eq!(view["harness_in_use"], json!("claude_code"), "{view}");
    assert_eq!(view["chosen_harness"], json!("copilot"), "{view}");
    let copilot = view["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|h| h["harness"] == json!("copilot"))
        .expect("copilot");
    assert_eq!(copilot["readiness"], json!("not_in_this_version"), "{view}");
    assert_eq!(stored_harness(&dir), Some(HarnessName::Copilot));
    core.quit(QuitReason::PersonQuit).await;
}

// Scenario: Edge case — Settings still decides at the next start (DSK-CHO-3)
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-3
#[tokio::test]
async fn dsk_cho_settings_still_decides_at_the_next_start() {
    let dir = TempDir::new("cho-settings");
    std::fs::create_dir_all(inputs_preferences(&dir).parent().expect("data root")).expect("root");
    store_harness(
        &dir,
        ordin_service_api::commands::profile::HarnessName::Copilot,
    );
    let shell = Arc::new(RecordingShell::default());
    let core = start(&dir, shell.clone()).await;
    call(
        &core,
        "app.change_agent_setting",
        json!({ "change": "harness", "harness": "claude_code" }),
    )
    .await
    .expect("changed");
    assert!(shell.quits().is_empty(), "Settings never restarts");
    assert_eq!(stored_harness(&dir), Some(HarnessName::ClaudeCode));
    core.quit(QuitReason::PersonQuit).await;
    let again = start(&dir, Arc::new(RecordingShell::default())).await;
    assert_eq!(again.modules.harness_in_use, Some(HarnessName::ClaudeCode));
    again.quit(QuitReason::PersonQuit).await;
}

// Scenario: Error path — the choice cannot be saved (DSK-CHO-2)
// spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-2
#[tokio::test]
async fn dsk_cho_the_choice_cannot_be_saved() {
    let dir = TempDir::new("cho-unsaved");
    // A folder where `preferences.json` belongs: it reads as the defaults and cannot be replaced.
    std::fs::create_dir_all(inputs_preferences(&dir)).expect("blocking folder");
    let shell = Arc::new(RecordingShell::default());
    let core = start(&dir, shell.clone()).await;
    let guided = run_guided(&core);
    question_shown(&core).await;
    let error = call(
        &core,
        "setup.choose_harness",
        json!({ "harness": "copilot_cli" }),
    )
    .await
    .expect_err("not saved");
    assert_eq!(error.code, "SETUP_CHOICE_NOT_SAVED");
    assert!(shell.quits().is_empty(), "nothing restarts");
    guided.abort();
    core.quit(QuitReason::PersonQuit).await;
}
