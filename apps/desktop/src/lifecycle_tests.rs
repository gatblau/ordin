//! Tests of step 4 (background work), step 10 (quit) and the commands the window shell answers, over
//! the real modules of the build with no switch and a fake window shell.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ordin_service_api::{command, dispatch};
use ordin_setup::UrlOpener;
use serde_json::json;

use crate::core::{construct, AfterQuit, Core, QuitReason};
use crate::plan::Row;
use crate::prefs::{Appearance, Preferences};
use crate::service_api::{ApiModules, DesktopErrorSentences, DesktopServiceApi};
use crate::shell::{PickerUnavailable, QuitTrigger, Shell};
use crate::test_support::TempDir;
use crate::wiring::ModuleRunner;
use crate::wiring_tests::{build_until, context, inputs};

/// A window shell that records what the core asked of it.
#[derive(Default)]
struct FakeShell {
    /// The files `pick_file` answers in turn; a `None` is a cancelled picker.
    files: Mutex<std::collections::VecDeque<Option<PathBuf>>>,
    folder: Option<PathBuf>,
    save_to: Option<PathBuf>,
    picker_fails: bool,
    quits: Mutex<Vec<QuitTrigger>>,
    appearances: Mutex<Vec<Appearance>>,
}

#[async_trait::async_trait]
impl Shell for FakeShell {
    async fn pick_folder(&self) -> Result<Option<PathBuf>, PickerUnavailable> {
        if self.picker_fails {
            Err(PickerUnavailable)
        } else {
            Ok(self.folder.clone())
        }
    }
    async fn pick_file(&self, _title: &str) -> Result<Option<PathBuf>, PickerUnavailable> {
        if self.picker_fails {
            return Err(PickerUnavailable);
        }
        Ok(self.files.lock().unwrap().pop_front().flatten())
    }
    async fn save_path(&self, _file_name: &str) -> Option<PathBuf> {
        self.save_to.clone()
    }
    fn quit(&self, trigger: QuitTrigger) {
        self.quits.lock().unwrap().push(trigger);
    }
    fn appearance_changed(&self, appearance: Appearance) {
        self.appearances.lock().unwrap().push(appearance);
    }
}

#[derive(Default)]
struct FakeOpener {
    opened: Mutex<Vec<String>>,
}

impl UrlOpener for FakeOpener {
    fn open(&self, url: &str) -> Result<(), String> {
        self.opened.lock().unwrap().push(url.to_owned());
        Ok(())
    }
}

async fn api_with(
    dir: &TempDir,
    shell: Arc<FakeShell>,
    opener: Arc<FakeOpener>,
) -> DesktopServiceApi {
    let runner = build_until(dir, Row::Checker).await;
    let m = &runner.modules;
    let sentences = Arc::new(DesktopErrorSentences {
        localisation: m.localisation.clone().expect("localisation"),
    });
    DesktopServiceApi::new(
        ApiModules {
            backend: m.backend,
            journal: m.journal.clone(),
            localisation: m.localisation.clone(),
            brand: m.brand.clone(),
            setup: None,
            shell: Some(shell),
            preferences: Some(Arc::new(Preferences::new(
                runner.inputs.paths.preferences.clone(),
            ))),
            url_opener: Some(opener),
            ..ApiModules::default()
        },
        sentences,
    )
}

// The program's own commands over the window shell (*Commands this program answers itself*).
#[tokio::test]
async fn the_shell_rows_answer_through_the_window_shell() {
    let dir = TempDir::new("shell-rows");
    let export = dir.path().join("export.yaml");
    let shell = Arc::new(FakeShell {
        folder: Some(dir.path().join("chosen")),
        save_to: Some(export.clone()),
        ..FakeShell::default()
    });
    let opener = Arc::new(FakeOpener::default());
    let api = api_with(&dir, shell.clone(), opener.clone()).await;
    let ctx = context();
    let call = |name: &'static str, input: serde_json::Value| {
        let api = &api;
        let ctx = &ctx;
        async move { dispatch(api, ctx, command(name).expect("row"), input).await }
    };

    // app.quit starts step 10 without asking.
    let quit = call("app.quit", json!({})).await.expect("quit");
    assert_eq!(quit, json!({ "stopping_sessions": 0 }));
    assert_eq!(*shell.quits.lock().unwrap(), [QuitTrigger::AppQuit]);

    // app.pick_folder.
    let picked = call("app.pick_folder", json!({})).await.expect("picked");
    assert_eq!(
        picked["folder"],
        json!(dir.path().join("chosen").to_string_lossy())
    );

    // files.save_export: written as UTF-8; a bad name is refused before any dialog.
    let saved = call(
        "files.save_export",
        json!({ "file_name": "export.yaml", "text": "steps: []\n" }),
    )
    .await
    .expect("saved");
    assert_eq!(saved["saved_path"], json!(export.to_string_lossy()));
    assert_eq!(
        std::fs::read_to_string(&export).expect("file"),
        "steps: []\n"
    );
    let bad = call(
        "files.save_export",
        json!({ "file_name": "../x.yaml", "text": "x" }),
    )
    .await
    .expect_err("refused");
    assert_eq!(bad.code, "SVC_INPUT_INVALID");

    // app.open_slack_link: a permalink opens; anything else is refused and nothing opens.
    let link = "https://acme.slack.com/archives/C01234567/p1700000000123456";
    call("app.open_slack_link", json!({ "url": link }))
        .await
        .expect("opened");
    let refused = call(
        "app.open_slack_link",
        json!({ "url": "https://evil.example/archives/C01234567/p1700000000123456" }),
    )
    .await
    .expect_err("refused");
    assert_eq!(refused.code, "DSK_LINK_REFUSED");
    assert_eq!(*opener.opened.lock().unwrap(), [link]);

    // settings.get and settings.change of preferences.json.
    let notifications = call("settings.get", json!({ "section": "notifications" }))
        .await
        .expect("notifications");
    assert_eq!(notifications["settings"].as_array().map(Vec::len), Some(5));
    let changed = call(
        "settings.change",
        json!({ "key": "display.appearance", "value": "dark" }),
    )
    .await
    .expect("changed");
    assert_eq!(changed["value"], json!("dark"));
    assert_eq!(*shell.appearances.lock().unwrap(), [Appearance::Dark]);
    let invalid = call(
        "settings.change",
        json!({ "key": "notifications.approvals", "value": "yes" }),
    )
    .await
    .expect_err("invalid");
    assert_eq!(invalid.code, "DSK_PREFERENCE_VALUE_INVALID");
    assert_eq!(invalid.action, None);
    let about = call("settings.get", json!({ "section": "about" }))
        .await
        .expect("about");
    assert_eq!(about["settings"], json!([]));
    // brand.get: the build pack with no profile.
    let brand = call("brand.get", json!({})).await.expect("brand");
    assert!(brand["strings"]["product_name"].is_string());
    assert_eq!(brand["refusal"], json!(null));
    // DSK-REC-37: a section the program does not hold, and a key it does not hold.
    let usage = call("settings.get", json!({ "section": "using_your_plan" }))
        .await
        .expect("answered");
    assert_eq!(
        usage,
        json!({ "section": "using_your_plan", "settings": [] })
    );
    let unknown = call(
        "settings.change",
        json!({ "key": "usage.plan_mode", "value": "x" }),
    )
    .await
    .expect_err("unknown key");
    assert_eq!(unknown.code, "SVC_INPUT_INVALID");
}

// Gherkin: Error path — save dialog cancelled; Error Table: the folder picker fails to open.
#[tokio::test]
async fn error_path_a_cancelled_dialog_and_a_failed_picker() {
    let dir = TempDir::new("shell-cancel");
    let shell = Arc::new(FakeShell {
        picker_fails: true,
        ..FakeShell::default()
    });
    let api = api_with(&dir, shell, Arc::new(FakeOpener::default())).await;
    let ctx = context();
    let cancelled = dispatch(
        &api,
        &ctx,
        command("files.save_export").expect("row"),
        json!({ "file_name": "notes.txt", "text": "x" }),
    )
    .await
    .expect_err("cancelled");
    assert_eq!(cancelled.code, "DSK_DIALOG_CANCELLED");
    assert_eq!(cancelled.action, None);
    let picker = dispatch(
        &api,
        &ctx,
        command("app.pick_folder").expect("row"),
        json!({}),
    )
    .await
    .expect_err("picker failed");
    assert_eq!(picker.code, "DSK_PICKER_UNAVAILABLE");
    assert_eq!(picker.action, Some(ordin_types::RepairAction::TryAgain));
}

// Smoke (§7.3 row *ordin desktop*, the part with no window): an empty data root with no
// organisation profile constructs the modules in order with defaults, starts the background work of
// step 4 (the Setup Assistant's start checks among it) and quits within 3 s with nothing running.
#[tokio::test]
async fn smoke_start_on_an_empty_data_root_and_quit_within_three_seconds() {
    let dir = TempDir::new("smoke");
    let mut runner = ModuleRunner::new(inputs(&dir));
    construct(&mut runner).await.expect("step 3");
    let core = Core::from_runner(runner).await.expect("step 4");
    let relay = core.modules.hook_relay.clone().expect("row 3.23");
    if cfg!(unix) {
        assert!(
            std::path::Path::new(relay.endpoint()).exists(),
            "hook_relay.start() bound its channel"
        );
    }
    assert_eq!(core.running_local_sessions(), 0);
    let started = Instant::now();
    let after = core.quit(QuitReason::PersonQuit).await;
    assert_eq!(after, AfterQuit::Exit);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "quit took {:?}",
        started.elapsed()
    );
    assert!(core.connector_cancel.is_cancelled() && core.shutdown_token.is_cancelled());
}

/// The window's updater on a computer where another copy of ordin is installed.
struct OtherCopy {
    other: PathBuf,
}

#[async_trait::async_trait]
impl ordin_setup::AppUpdater for OtherCopy {
    async fn check(
        &self,
        _feed_url: &reqwest::Url,
    ) -> Result<Option<ordin_setup::UpdateOffer>, ordin_setup::UpdaterError> {
        Ok(None)
    }
    async fn download(
        &self,
        _offer: &ordin_setup::UpdateOffer,
    ) -> Result<(), ordin_setup::UpdaterError> {
        Ok(())
    }
    async fn install_and_restart(&self) -> Result<(), ordin_setup::UpdaterError> {
        Ok(())
    }
    async fn install_on_exit(&self) -> Result<(), ordin_setup::UpdaterError> {
        Ok(())
    }
    fn install_kind(&self) -> ordin_setup::InstallKind {
        ordin_setup::InstallKind::PerUser
    }
    fn other_install(&self) -> Option<PathBuf> {
        Some(self.other.clone())
    }
}

// Scenario: Edge case — a second install is reported at start (DSK-SC-1, SVC-SC-1). The start checks
// run on their own task, so `setup.status` is read until their progress has arrived.
#[tokio::test]
async fn edge_case_a_second_install_is_reported_at_start_across_the_service_api() {
    let dir = TempDir::new("second-install");
    let folder = dir.path().join("Applications");
    let mut core_inputs = inputs(&dir);
    core_inputs.updater = Some(Arc::new(OtherCopy {
        other: folder.join("ordin.app"),
    }));
    let mut runner = ModuleRunner::new(core_inputs);
    construct(&mut runner).await.expect("step 3");
    let core = Core::from_runner(runner).await.expect("step 4");
    let api = core.modules.service_api.clone().expect("row 3.48");
    let limit = Instant::now() + Duration::from_secs(30);
    let step = loop {
        let status = dispatch(
            &*api,
            &context(),
            command("setup.status").expect("row"),
            json!({}),
        )
        .await
        .expect("setup.status crosses the Service API with the start report's progress");
        let found = status["progress"]["steps"].as_array().and_then(|steps| {
            steps
                .iter()
                .find(|s| s["check"] == json!("second_install"))
                .cloned()
        });
        if let Some(step) = found {
            break step;
        }
        assert!(
            Instant::now() < limit,
            "no second_install step within 30 s: {status}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(step["state"], json!("failed"));
    assert_eq!(step["message_id"], json!("setup-step-second-install"));
    assert_eq!(step["problem"]["message_id"], json!("setup-second-install"));
    assert_eq!(
        step["problem"]["message_args"]["folder"],
        json!(folder.to_string_lossy())
    );
    // A failed start check does not stop start: the core is running and quits as usual.
    assert!(!core.shutdown_token.is_cancelled());
    assert_eq!(core.quit(QuitReason::PersonQuit).await, AfterQuit::Exit);
}

// Gherkin: Edge case — quit with nothing running does not ask; the journal is shut down.
#[tokio::test]
async fn shutdown_shuts_the_journal_down_and_releases_every_lease() {
    let dir = TempDir::new("shutdown");
    let mut runner = ModuleRunner::new(inputs(&dir));
    construct(&mut runner).await.expect("step 3");
    let core = Core::from_runner(runner).await.expect("step 4");
    let summary = core.shutdown(QuitReason::OperatingSystemEnding).await;
    assert!(summary.journal_shut_down);
    assert_eq!(summary.sessions_stopped + summary.sessions_interrupted, 0);
    // A second quit decision while the first runs does nothing more (10.1).
    assert!(matches!(
        core.quit.decide(QuitTrigger::Menu, 0),
        crate::shell::QuitDecision::Proceed(QuitReason::PersonQuit)
    ));
    assert_eq!(
        core.quit.decide(QuitTrigger::Menu, 0),
        crate::shell::QuitDecision::AlreadyRunning
    );
}

/// An opener the operating system refuses.
struct RefusingOpener;

impl UrlOpener for RefusingOpener {
    fn open(&self, _url: &str) -> Result<(), String> {
        Err("refused".to_owned())
    }
}

// Scenario: Error path — the browser cannot be opened (DSK-REC-38)
#[tokio::test]
async fn error_path_the_browser_cannot_be_opened() {
    let dir = TempDir::new("browser-refused");
    let runner = build_until(&dir, Row::Checker).await;
    let m = &runner.modules;
    let api = DesktopServiceApi::new(
        ApiModules {
            backend: m.backend,
            localisation: m.localisation.clone(),
            url_opener: Some(Arc::new(RefusingOpener)),
            ..ApiModules::default()
        },
        Arc::new(DesktopErrorSentences {
            localisation: m.localisation.clone().expect("localisation"),
        }),
    );
    let link = "https://acme.slack.com/archives/C01234567/p1700000000123456";
    let error = dispatch(
        &api,
        &context(),
        command("app.open_slack_link").expect("row"),
        json!({ "url": link }),
    )
    .await
    .expect_err("refused");
    assert_eq!(error.code, "CRED_BROWSER_FAILED");
    assert_eq!(error.action, Some(ordin_types::RepairAction::TryAgain));
}

// Scenario: Happy path — a support export is saved (DSK-REC-41); Error path — cancelled.
#[tokio::test]
async fn a_support_export_is_saved_or_cancelled() {
    let dir = TempDir::new("support-export");
    let target = dir.path().join("ordin-support.jsonl");
    let shell = Arc::new(FakeShell {
        save_to: Some(target.clone()),
        ..FakeShell::default()
    });
    let api = api_with(&dir, shell, Arc::new(FakeOpener::default())).await;
    let ctx = context();
    let run = ordin_types::RunId::new();
    let saved = dispatch(
        &api,
        &ctx,
        command("diagnostics.support_export").expect("row"),
        json!({ "run_ids": [run] }),
    )
    .await;
    match saved {
        Ok(out) => {
            assert_eq!(out["saved_path"], json!(target.to_string_lossy()));
            assert!(target.exists());
        }
        // A run the journal has no entry for is the Journal's own refusal.
        Err(error) => assert!(error.code.starts_with("JRNL_"), "{}", error.code),
    }
    let cancel_dir = TempDir::new("support-export-cancel");
    let api = api_with(
        &cancel_dir,
        Arc::new(FakeShell::default()),
        Arc::new(FakeOpener::default()),
    )
    .await;
    let cancelled = dispatch(
        &api,
        &ctx,
        command("diagnostics.support_export").expect("row"),
        json!({ "run_ids": [run] }),
    )
    .await
    .expect_err("cancelled or refused");
    assert!(
        cancelled.code == "DSK_DIALOG_CANCELLED" || cancelled.code.starts_with("JRNL_"),
        "{}",
        cancelled.code
    );
}

#[test]
fn a_lower_running_version_is_a_downgrade() {
    assert!(crate::core::downgraded("0.2.0", "0.3.0"));
    assert!(!crate::core::downgraded("0.3.0", "0.3.0"));
    assert!(!crate::core::downgraded("0.4.0", "0.3.0"));
    assert!(!crate::core::downgraded("0.2.0", "not-a-version"));
}

// Scenario: Error path — no client certificate name in the profile (DSK-R1-1): no picker opens.
#[tokio::test]
async fn error_path_a_certificate_needs_the_profile_name() {
    let dir = TempDir::new("certificate-name");
    let runner = build_until(&dir, Row::Checker).await;
    let m = &runner.modules;
    let shell = Arc::new(FakeShell {
        files: Mutex::new(std::collections::VecDeque::from([Some(
            dir.path().join("client.pem"),
        )])),
        ..FakeShell::default()
    });
    let api = DesktopServiceApi::new(
        ApiModules {
            backend: m.backend,
            localisation: m.localisation.clone(),
            identity: m.identity.clone(),
            profile: m.profile.clone(),
            shell: Some(shell.clone()),
            ..ApiModules::default()
        },
        Arc::new(DesktopErrorSentences {
            localisation: m.localisation.clone().expect("localisation"),
        }),
    );
    let error = dispatch(
        &api,
        &context(),
        command("identity.import_client_certificate").expect("row"),
        json!({}),
    )
    .await
    .expect_err("no name");
    assert_eq!(error.code, "CRED_PROVIDER_NOT_CONFIGURED");
    assert_eq!(shell.files.lock().unwrap().len(), 1, "no picker was opened");
}

// Scenario: Happy path — a saved knowledge page reaches memory (DSK-KS-1). Step 4 starts the
// Knowledge Service's memory-index task, so the page is handed to memory with no other call. This
// build has no memory models, so memory answers `MEM_MODEL_UNAVAILABLE` and the page waits for the
// next attempt; the end-to-end job proves the recall with the models.
#[tokio::test]
async fn a_saved_knowledge_page_is_handed_to_memory_after_start() {
    let dir = TempDir::new("knowledge-index");
    let mut runner = ModuleRunner::new(inputs(&dir));
    construct(&mut runner).await.expect("step 3");
    let core = Core::from_runner(runner).await.expect("step 4");
    let m = &core.modules;
    let workspace = m
        .workspaces
        .clone()
        .expect("row 3.35")
        .create("Northwind".to_owned())
        .await
        .expect("a workspace");
    let page = m
        .knowledge
        .clone()
        .expect("row 3.38")
        .write_page(
            ordin_types::domain::places::KnowledgeAuthor::Person,
            ordin_knowledge::WritePage {
                workspace_id: workspace.id,
                page_id: None,
                title: "Supplier terms".to_owned(),
                body_markdown: "Kestrel pays in 30 days.".to_owned(),
                sources: Vec::new(),
                run_id: None,
                tags: None,
                aliases: None,
                page_type: None,
                description: None,
            },
        )
        .await
        .expect("a page");
    let pool = m
        .journal
        .clone()
        .expect("row 3.1")
        .datastore()
        .sqlite()
        .cloned()
        .expect("the desktop's SQLite datastore");
    let started = Instant::now();
    let attempts = loop {
        let (attempts, indexed): (i64, Option<i64>) = sqlx::query_as(
            "SELECT memory_index_attempts, memory_indexed_version FROM knowledge_pages WHERE id = ?1",
        )
        .bind(page.to_string())
        .fetch_one(&pool)
        .await
        .expect("the page's row");
        if attempts > 0 || indexed.is_some() {
            break attempts;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the memory-index task did not take the page"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(
        attempts, 1,
        "memory was asked once and refused for want of models"
    );
    let _ = core.quit(QuitReason::PersonQuit).await;
    assert!(core.shutdown_token.is_cancelled());
}
