//! Language plug-ins, the language-server instances and the editor relay, against the stand-in in
//! `tests/lsp_stand_in.rs`.

use std::sync::atomic::Ordering;
use std::time::Duration;

use ordin_types::CodedError;
use serde_json::json;

use super::support::{fixture, pin, settings, Fixture};
use crate::lsp::{uri_of, InstanceKey};
use crate::service::lock;
use crate::{ApprovedServers, LanguagePluginStatus, ProjectId};

async fn eventually(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

async fn ready() -> Fixture {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    f
}

#[tokio::test]
async fn reconcile_adds_updates_and_withdraws_languages() {
    let f = ready().await;
    let plugins = f.service.language_plugins().await.unwrap();
    assert_eq!(plugins.len(), 2);
    let rust = plugins.iter().find(|p| p.language == "rust").unwrap();
    assert_eq!(rust.status, LanguagePluginStatus::Available);
    assert_eq!(rust.approved_servers, ["rust-analyzer", "other"]);
    assert_eq!(rust.pinned_version.as_deref(), Some("2026-01-01"));
    f.service
        .configure_language("rust", Some("other".to_owned()), true, json!({"x": 1}))
        .await
        .unwrap();

    let mut changed = settings();
    changed.language_servers.remove("go");
    changed.language_servers.insert(
        "rust".to_owned(),
        ApprovedServers {
            approved: vec![pin("rust-analyzer", "2026-02-02")],
            default: "rust-analyzer".to_owned(),
        },
    );
    f.settings.send(changed).unwrap();
    f.service.reconcile_settings().await.unwrap();
    let plugins = f.service.language_plugins().await.unwrap();
    let rust = plugins.iter().find(|p| p.language == "rust").unwrap();
    assert_eq!(rust.chosen_server, None, "no longer approved");
    assert_eq!(rust.pinned_version.as_deref(), Some("2026-02-02"));
    assert_eq!(rust.server_settings, json!({"x": 1}));
    let go = plugins.iter().find(|p| p.language == "go").unwrap();
    assert_eq!(go.status, LanguagePluginStatus::Removed);

    f.settings.send(settings()).unwrap();
    f.service.reconcile_settings().await.unwrap();
    let go = crate::store::plugin(&f.service.pool, "go")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(go.status, LanguagePluginStatus::Available);
}

#[tokio::test]
async fn configuring_adds_and_journals_and_first_use_installs() {
    let f = ready().await;
    let (project, _) = f.project().await;
    let plugin = f
        .service
        .configure_language("go", None, true, json!({"gofumpt": true}))
        .await
        .unwrap();
    assert_eq!(plugin.status, LanguagePluginStatus::Added);
    assert_eq!(f.entries_of("language_configured").await.len(), 1);
    let session = f.service.editor_open(project.id, "main.go").await.unwrap();
    assert_eq!(session.server_line, "Go · gopls · Running");
    assert_eq!(session.language.as_deref(), Some("go"));
    let plugin = crate::store::plugin(&f.service.pool, "go")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(plugin.status, LanguagePluginStatus::Installed);
    assert_eq!(
        f.installer.calls.lock().unwrap().clone(),
        [("go".to_owned(), "gopls".to_owned())]
    );
    assert_eq!(
        f.service
            .project(project.id)
            .await
            .unwrap()
            .language_servers_in_use,
        ["gopls"]
    );
    f.service.close_project(project.id).await.unwrap();
    assert!(lock(&f.service.lsp.instances).is_empty());
}

#[tokio::test]
async fn a_file_with_no_language_or_a_server_off_opens_highlighted() {
    let f = ready().await;
    let (project, _) = f.project().await;
    let plain = f
        .service
        .editor_open(project.id, "notes.txt")
        .await
        .unwrap();
    assert_eq!(plain.language, None);
    assert_eq!(plain.server_line, "highlighted · no language server");
    assert_eq!(
        f.service
            .lsp_send(plain.stream, json!({"method": "textDocument/hover"}))
            .await
            .unwrap_err()
            .code(),
        "PROJ_LSP_UNAVAILABLE"
    );
    let unknown = crate::StreamId(uuid::Uuid::now_v7());
    assert_eq!(
        f.service
            .lsp_send(unknown, json!({"method": "textDocument/hover"}))
            .await
            .unwrap_err()
            .code(),
        "PROJ_LSP_UNAVAILABLE"
    );
    let opened = f.service.editor_open(project.id, "main.ts").await.unwrap();
    assert_eq!(opened.language.as_deref(), Some("typescript"));
    assert_eq!(opened.server_line, "highlighted · no language server");
}

#[tokio::test]
async fn editors_share_an_instance_and_answers_keep_their_own_ids() {
    let f = ready().await;
    let (project, folder) = f.project().await;
    let first = f.service.editor_open(project.id, "main.rs").await.unwrap();
    let second = f.service.editor_open(project.id, "lib.rs").await.unwrap();
    assert_eq!(lock(&f.service.lsp.instances).len(), 1);
    let uri = uri_of(&folder.join("main.rs"));
    let hover = |id: i64| {
        json!({
            "jsonrpc": "2.0", "id": id, "method": "textDocument/hover",
            "params": {"textDocument": {"uri": uri}, "position": {"line": 0, "character": 0}},
        })
    };
    f.service.lsp_send(first.stream, hover(1)).await.unwrap();
    f.service.lsp_send(second.stream, hover(1)).await.unwrap();
    assert!(
        eventually(|| {
            f.streams.lsp_messages(first.stream).len() == 1
                && f.streams.lsp_messages(second.stream).len() == 1
        })
        .await
    );
    let answer = &f.streams.lsp_messages(first.stream)[0];
    assert_eq!(answer["id"], 1);
    assert_eq!(answer["result"]["contents"], "The rates client.");

    // Diagnostics reach every session of the instance.
    f.service
        .lsp_send(
            first.stream,
            json!({"method": "textDocument/didOpen", "params": {"textDocument": {
                "uri": uri, "languageId": "rust", "version": 1, "text": "// ERROR x\n",
            }}}),
        )
        .await
        .unwrap();
    assert!(eventually(|| f.streams.lsp_messages(second.stream).len() == 2).await);
    f.service.editor_close(second.stream).await;
    assert!(f.service.lsp_send(second.stream, hover(2)).await.is_err());
}

#[tokio::test]
async fn a_document_outside_the_project_is_refused() {
    let f = ready().await;
    let (project, _) = f.project().await;
    let session = f.service.editor_open(project.id, "main.rs").await.unwrap();
    let error = f
        .service
        .lsp_send(
            session.stream,
            json!({
                "id": 1, "method": "textDocument/hover",
                "params": {"textDocument": {"uri": "file:///etc/passwd"}},
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PROJ_LSP_METHOD_NOT_ALLOWED");
}

#[tokio::test]
async fn server_requests_are_answered_and_settings_reach_a_running_server() {
    let f = ready().await;
    let (project, folder) = f.project().await;
    let session = f.service.editor_open(project.id, "main.rs").await.unwrap();
    let key = InstanceKey::Editor(project.id, "rust".to_owned());
    let instance = lock(&f.service.lsp.instances).get(&key).unwrap().clone();
    f.service
        .lsp_send(
            session.stream,
            json!({"method": "textDocument/didSave", "params": {"textDocument": {
                "uri": uri_of(&folder.join("main.rs")),
            }}}),
        )
        .await
        .unwrap();
    // The stand-in keeps running after its two questions are answered.
    let answer = instance
        .request("textDocument/completion", json!({}), Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(answer[0]["label"], "fetch");

    f.service
        .configure_language("rust", None, true, json!({"cargo": true}))
        .await
        .unwrap();
    assert!(eventually(|| !instance.diagnostics_of("file:///settings").is_empty()).await);
    let echoed = &instance.diagnostics_of("file:///settings")[0];
    assert!(echoed["message"].as_str().unwrap().contains("cargo"));

    f.service
        .configure_language("rust", None, false, json!({}))
        .await
        .unwrap();
    assert!(lock(&f.service.lsp.instances).is_empty());
    assert!(!instance.alive() || eventually(|| !instance.alive()).await);
}

#[tokio::test]
async fn a_malformed_frame_kills_the_instance_and_the_next_use_restarts_it() {
    let f = ready().await;
    let (project, folder) = f.project().await;
    let session = f.service.editor_open(project.id, "main.rs").await.unwrap();
    let key = InstanceKey::Editor(project.id, "rust".to_owned());
    let first = lock(&f.service.lsp.instances).get(&key).unwrap().clone();
    let uri = uri_of(&folder.join("main.rs"));
    f.service
        .lsp_send(
            session.stream,
            json!({"method": "textDocument/didClose", "params": {"textDocument": {"uri": uri}}}),
        )
        .await
        .unwrap();
    assert!(eventually(|| !lock(&f.service.lsp.instances).contains_key(&key)).await);
    assert!(
        !first.stopping.load(Ordering::SeqCst),
        "a crash, not a stop"
    );
    f.service
        .lsp_send(
            session.stream,
            json!({"id": 3, "method": "textDocument/hover",
                   "params": {"textDocument": {"uri": uri}}}),
        )
        .await
        .unwrap();
    let second = lock(&f.service.lsp.instances).get(&key).unwrap().clone();
    assert_ne!(first.number, second.number);
}

#[tokio::test]
async fn a_restart_starts_the_editor_instances_again() {
    let f = ready().await;
    let (project, _) = f.project().await;
    f.service.editor_open(project.id, "main.rs").await.unwrap();
    let key = InstanceKey::Editor(project.id, "rust".to_owned());
    let before = lock(&f.service.lsp.instances).get(&key).unwrap().number;
    f.service.restart_language_server("rust").await.unwrap();
    let after = lock(&f.service.lsp.instances).get(&key).unwrap().number;
    assert_ne!(before, after);
}

#[tokio::test]
async fn at_the_limit_the_least_recently_used_editor_instance_is_stopped() {
    let f = ready().await;
    let folder = f.dir.0.clone();
    let first = ProjectId::new();
    f.service
        .start_instance(InstanceKey::Editor(first, "go".to_owned()), &folder, "go")
        .await
        .unwrap();
    let store_row = ProjectId::new();
    for _ in 1..crate::MAX_LANGUAGE_INSTANCES {
        let key = InstanceKey::Task(crate::CodingTaskId::new(), "go".to_owned());
        f.service.start_instance(key, &folder, "go").await.unwrap();
    }
    f.service
        .start_instance(
            InstanceKey::Editor(store_row, "go".to_owned()),
            &folder,
            "go",
        )
        .await
        .unwrap();
    {
        let running = lock(&f.service.lsp.instances);
        assert_eq!(running.len(), crate::MAX_LANGUAGE_INSTANCES);
        assert!(!running.contains_key(&InstanceKey::Editor(first, "go".to_owned())));
    }
    // With only task instances left there is nothing to stop.
    f.service
        .stop_instance(&InstanceKey::Editor(store_row, "go".to_owned()))
        .await;
    let key = InstanceKey::Task(crate::CodingTaskId::new(), "go".to_owned());
    f.service.start_instance(key, &folder, "go").await.unwrap();
    let full = InstanceKey::Task(crate::CodingTaskId::new(), "go".to_owned());
    let error = f
        .service
        .start_instance(full, &folder, "go")
        .await
        .err()
        .unwrap();
    assert_eq!(CodedError::code(&error), "PROJ_LSP_UNAVAILABLE");
    f.service.shutdown().await;
    assert!(lock(&f.service.lsp.instances).is_empty());
}

// Unix only: the silent server is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn a_server_that_never_answers_is_unavailable() {
    let f = ready().await;
    let silent = super::standin::linked_script(&f.dir.0, "silent-server", "exec sleep 30");
    *f.installer.program.lock().unwrap() = Some(silent);
    lock(&f.service.tunables).lsp_start_timeout = Duration::from_millis(200);
    let folder = f.dir.0.clone();
    let key = InstanceKey::Editor(ProjectId::new(), "go".to_owned());
    let error = f
        .service
        .start_instance(key, &folder, "go")
        .await
        .err()
        .unwrap();
    assert_eq!(CodedError::code(&error), "PROJ_LSP_UNAVAILABLE");
}

/// Gherkin "Happy path — a document of the project reaches the language server on Windows": the
/// place is added with the verbatim form `\\?\C:\...`, and an address `file:///C:/...` of a file in
/// the project is passed to the language server, which answers it.
#[cfg(windows)]
// spec: ordin-desktop/components/ProjectService#PROJ-PATH-1
#[tokio::test]
async fn happy_path_a_document_of_the_project_reaches_the_language_server_on_windows() {
    let f = ready().await;
    let folder = f.dir.0.join("supplier-portal");
    super::support::repository(&folder, &[("src/main.rs", "fn main() {}\n")]);
    let verbatim = std::fs::canonicalize(&folder).unwrap();
    assert!(
        verbatim.to_string_lossy().starts_with(r"\\?\"),
        "{verbatim:?}"
    );
    let place = f
        .files
        .add_place(ordin_files::AddPlace {
            path: verbatim,
            friendly_name: None,
            kind: ordin_files::PlaceKind::PersonsFolder,
        })
        .await
        .unwrap()
        .id;
    f.scanned(place).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();
    f.service
        .answer_language_servers(project.id, true)
        .await
        .unwrap();
    let session = f
        .service
        .editor_open(project.id, "src/main.rs")
        .await
        .unwrap();
    // `file:///C:/Users/.../supplier-portal/src/main.rs`, as an editor writes it.
    let uri = format!(
        "file:///{}/src/main.rs",
        folder.to_string_lossy().replace('\\', "/")
    );
    assert!(
        uri.starts_with("file:///") && uri.as_bytes()[9] == b':',
        "{uri}"
    );
    f.service
        .lsp_send(
            session.stream,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "textDocument/hover",
                "params": {"textDocument": {"uri": uri}, "position": {"line": 0, "character": 0}},
            }),
        )
        .await
        .expect("not refused with PROJ_LSP_METHOD_NOT_ALLOWED");
    assert!(eventually(|| f.streams.lsp_messages(session.stream).len() == 1).await);
    assert_eq!(f.streams.lsp_messages(session.stream)[0]["id"], 1);
}
