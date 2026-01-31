//! The `minutes.*` commands over the real modules of a build with the `minutes` switch and a fake
//! window shell (OrdinDesktop.md DSK-MIN-1 to DSK-MIN-5; ServiceApi.md SVC-MIN-1; plan 07 FU-011).

use std::collections::VecDeque;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ordin_journal::NewMinutes;
use ordin_service_api::{command, dispatch, ServiceApi};
use ordin_types::wire::minutes::MinutesSource;
use serde_json::{json, Value};

use crate::service_api::{ApiModules, DesktopErrorSentences, DesktopServiceApi};
use crate::shell::{PickerUnavailable, QuitTrigger, Shell};
use crate::test_support::TempDir;
use crate::wiring::ModuleRunner;
use crate::wiring_tests::{context, wire_all};

/// A window shell whose file picker answers in turn (a `None` is a cancelled dialog) and whose save
/// dialog answers `save_to`.
#[derive(Default)]
struct DialogShell {
    files: Mutex<VecDeque<Option<PathBuf>>>,
    save_to: Option<PathBuf>,
    filters: Mutex<Vec<(String, Vec<String>)>>,
}

#[async_trait::async_trait]
impl Shell for DialogShell {
    async fn pick_folder(&self) -> Result<Option<PathBuf>, PickerUnavailable> {
        Ok(None)
    }
    async fn pick_file(&self, _title: &str) -> Result<Option<PathBuf>, PickerUnavailable> {
        Ok(self.files.lock().unwrap().pop_front().flatten())
    }
    async fn pick_file_of(
        &self,
        _title: &str,
        filter_name: &str,
        extensions: &[&str],
    ) -> Result<Option<PathBuf>, PickerUnavailable> {
        self.filters.lock().unwrap().push((
            filter_name.to_owned(),
            extensions.iter().map(|e| (*e).to_owned()).collect(),
        ));
        Ok(self.files.lock().unwrap().pop_front().flatten())
    }
    async fn save_path(&self, _file_name: &str) -> Option<PathBuf> {
        self.save_to.clone()
    }
    fn quit(&self, _trigger: QuitTrigger) {}
    fn appearance_changed(&self, _appearance: crate::prefs::Appearance) {}
}

fn api(runner: &ModuleRunner, shell: Arc<DialogShell>) -> DesktopServiceApi {
    let m = &runner.modules;
    DesktopServiceApi::new(
        ApiModules {
            backend: m.backend,
            journal: m.journal.clone(),
            localisation: m.localisation.clone(),
            identity: m.identity.clone(),
            workspaces: m.workspaces.clone(),
            minutes: m.minutes.clone(),
            shell: Some(shell),
            ..ApiModules::default()
        },
        Arc::new(DesktopErrorSentences {
            localisation: m.localisation.clone().expect("localisation"),
        }),
    )
}

async fn call(api: &DesktopServiceApi, name: &str, input: Value) -> Result<Value, String> {
    dispatch(api, &context(), command(name).expect("row"), input)
        .await
        .map_err(|e| e.code)
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-1
// spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-6
#[tokio::test]
async fn happy_path_a_build_with_minutes_wires_the_three_modules() {
    let dir = TempDir::new("minutes-wired");
    let runner = wire_all(&dir).await;
    assert!(runner.modules.transcription.is_some());
    assert!(runner.modules.obs.is_some());
    assert!(runner.modules.minutes.is_some());
    let features = crate::plan::features();
    assert!(features.contains(&"minutes"));
    // DSK-MIN-6: minutes does not turn the dictation switch on.
    assert_eq!(features.contains(&"dictation"), cfg!(feature = "dictation"));
    let api = api(&runner, Arc::new(DialogShell::default()));
    assert!(api.groups().contains(&"minutes"));
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-4
#[tokio::test]
async fn happy_path_minutes_saved_as_word() {
    let dir = TempDir::new("minutes-export");
    let runner = wire_all(&dir).await;
    let workspace = runner
        .modules
        .workspaces
        .clone()
        .unwrap()
        .create("Suppliers".to_owned())
        .await
        .expect("workspace");
    let body = "## Summary\n\nPrices agreed.\n\n## Decisions\n\n- Sign in May.\n\n## Actions\n\n- Send the contract — Sam\n\n## Open questions\n\n- None recorded.\n";
    let kept = runner
        .modules
        .journal
        .clone()
        .unwrap()
        .keep_minutes(NewMinutes {
            workspace_id: workspace.id,
            title: "Supplier call".to_owned(),
            source: MinutesSource::TranscriptFile,
            source_name: "meeting.vtt".to_owned(),
            body: body.to_owned(),
            recording: None,
            run_id: None,
        })
        .await
        .expect("kept");
    let saved = dir.path().join("Supplier call.docx");
    let shell = Arc::new(DialogShell {
        save_to: Some(saved.clone()),
        ..DialogShell::default()
    });
    let api = api(&runner, shell);
    let reply = call(
        &api,
        "minutes.export",
        json!({ "minutes_id": kept.id, "format": "docx" }),
    )
    .await
    .expect("exported");
    assert_eq!(reply["saved_path"], json!(saved.to_string_lossy()));

    let bytes = std::fs::read(&saved).expect("written");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a Word package");
    let mut core = String::new();
    archive
        .by_name("docProps/core.xml")
        .unwrap()
        .read_to_string(&mut core)
        .unwrap();
    assert!(core.contains("<dc:title>Supplier call</dc:title>"));
    let mut document = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut document)
        .unwrap();
    assert_eq!(
        document.matches(r#"<w:pStyle w:val="Heading2"/>"#).count(),
        4
    );
    for heading in ["Summary", "Decisions", "Actions", "Open questions"] {
        assert!(
            document.contains(&format!(
                r#"<w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t xml:space="preserve">{heading}</w:t>"#
            )),
            "{heading}"
        );
    }

    // Minutes the Journal does not hold.
    let missing = call(
        &api,
        "minutes.export",
        json!({ "minutes_id": ordin_types::MinutesId::new(), "format": "pdf" }),
    )
    .await;
    assert_eq!(missing, Err("MIN_NOT_FOUND".to_owned()));
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-2
#[tokio::test]
async fn error_the_import_dialog_is_cancelled() {
    let dir = TempDir::new("minutes-import");
    let runner = wire_all(&dir).await;
    let workspace = runner
        .modules
        .workspaces
        .clone()
        .unwrap()
        .create("Suppliers".to_owned())
        .await
        .expect("workspace");
    let shell = Arc::new(DialogShell {
        files: Mutex::new(VecDeque::from([None])),
        ..DialogShell::default()
    });
    let api = api(&runner, shell.clone());
    let reply = call(
        &api,
        "minutes.import",
        json!({ "workspace_id": workspace.id }),
    )
    .await;
    assert_eq!(reply, Err("DSK_DIALOG_CANCELLED".to_owned()));
    let jobs = call(
        &api,
        "minutes.jobs",
        json!({ "workspace_id": workspace.id }),
    )
    .await
    .expect("jobs");
    assert_eq!(jobs, json!([]));
    assert_eq!(
        *shell.filters.lock().unwrap(),
        [(
            "Transcripts and recordings".to_owned(),
            ["vtt", "docx", "txt", "m4a", "mp3", "wav"]
                .map(str::to_owned)
                .to_vec()
        )]
    );
}

// Plan 07 FU-011: the program checks the workspace before the Minutes Service sees it.
// spec: ordin-desktop/components/MinutesService#MIN_WORKSPACE_UNKNOWN
#[tokio::test]
async fn error_import_for_a_removed_workspace_is_workspace_unknown() {
    let dir = TempDir::new("minutes-removed");
    let runner = wire_all(&dir).await;
    let workspaces = runner.modules.workspaces.clone().unwrap();
    let workspace = workspaces
        .create("Old project".to_owned())
        .await
        .expect("workspace");
    workspaces.move_to_bin(workspace.id).await.expect("binned");
    workspaces
        .delete_from_bin(workspace.id)
        .await
        .expect("deleted");
    let shell = Arc::new(DialogShell::default());
    let api = api(&runner, shell.clone());
    for name in ["minutes.import", "minutes.status", "minutes.record"] {
        let reply = call(&api, name, json!({ "workspace_id": workspace.id })).await;
        assert_eq!(reply, Err("MIN_WORKSPACE_UNKNOWN".to_owned()), "{name}");
    }
    // No dialog was opened for a workspace that is gone.
    assert!(shell.filters.lock().unwrap().is_empty());
    // An unknown workspace is refused the same way.
    let unknown = call(
        &api,
        "minutes.status",
        json!({ "workspace_id": ordin_types::WorkspaceId::new() }),
    )
    .await;
    assert_eq!(unknown, Err("MIN_WORKSPACE_UNKNOWN".to_owned()));
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-3
#[test]
fn error_an_empty_obs_password_is_refused_at_the_boundary() {
    let backend = ordin_types::BackendId::new();
    for input in [
        json!({ "password": "" }),
        json!({ "password": "fixture-pass", "colour": 1 }),
    ] {
        let body = json!({
            "api_version": {"major": 1, "minor": 0},
            "request_id": ordin_types::CommandRequestId::new(),
            "command": "minutes.save_obs_password",
            "backend": backend,
            "input": input,
        })
        .to_string();
        let refusal = ordin_service_api::accept(&ordin_service_api::Incoming {
            body: body.as_bytes(),
            routed_name: "minutes.save_obs_password",
            origin: ordin_service_api::CallOrigin::LocalScreens,
            this_backend: backend,
            groups: crate::groups::groups(),
        })
        .expect_err("refused");
        assert_eq!(refusal.code, "SVC_INPUT_INVALID");
        assert!(!format!("{refusal:?}").contains("fixture-pass"));
    }
}
