//! `read_document` (FileService.md SA-FS-DOC-1 to SA-FS-DOC-5): a real File Service and Rules
//! Engine; the PDF is one Chrome printed from `tests/fixtures/browser-printed.html`, and a copy
//! of it re-saved with compressed object streams and a cross-reference stream.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ordin_mcp_front::{
    CallVia, ProgressSink, ServerName, ToolCallContext, ToolCaller, ToolFailure, ToolKey,
    ToolOutput, ToolProvider,
};
use ordin_rules::{GrantedPlace, InstanceRuleSet};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::places::{FolderAccess, Grantee};
use ordin_types::{AgentInstanceId, RunId, ToolRequestId, WorkspaceId};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::support::*;
use crate::{PlaceKind, DOC_PART_CHARS};

const BROWSER_PDF: &[u8] = include_bytes!("../../tests/fixtures/browser-printed.pdf");

struct Rig {
    fx: Fixture,
    provider: std::sync::Arc<dyn ToolProvider>,
    agent: AgentInstanceId,
    workspace: WorkspaceId,
    agent_folder: PathBuf,
    allowed: crate::Place,
    other: crate::Place,
}

async fn rig() -> Rig {
    let fx = fixture().await;
    let agent = AgentInstanceId::new();
    let workspace = WorkspaceId::new();
    let agent_folder = fx.dir.0.join("agents/doc");
    std::fs::create_dir_all(&agent_folder).unwrap();
    fx.rules
        .load_rule_set(InstanceRuleSet {
            agent_instance_id: agent,
            package_name: "documents".to_owned(),
            authority: Authority::DocumentOnly,
            agent_folder: agent_folder.clone(),
            agent_folder_read_only: false,
            declaration: None,
            configured: None,
            tool_policy: None,
            approval_list: Vec::new(),
            package_read_only_tools: Vec::new(),
            tools: Vec::new(),
            unmapped_operations: Vec::new(),
            external: Vec::new(),
            commands: Vec::new(),
            reviewer: None,
        })
        .expect("the rule set loads");
    let allowed = fx.place("Docs", PlaceKind::PersonsFolder).await;
    let other = fx.place("Private", PlaceKind::PersonsFolder).await;
    fx.rules.replace_grants(
        workspace,
        vec![GrantedPlace {
            grantee: Grantee::AgentInstance {
                agent_instance_id: agent,
            },
            place_id: allowed.id,
            root: fx.root(&allowed),
            access: FolderAccess::Read,
        }],
    );
    let provider = fx.service.document_tool_provider();
    Rig {
        fx,
        provider,
        agent,
        workspace,
        agent_folder,
        allowed,
        other,
    }
}

fn key() -> ToolKey {
    ToolKey {
        server: ServerName::new("documents").unwrap(),
        tool: "read_document".to_owned(),
    }
}

impl Rig {
    fn context(&self, run_id: RunId) -> ToolCallContext {
        ToolCallContext {
            run_id,
            workspace_id: self.workspace,
            agent_instance_id: self.agent,
            caller: ToolCaller::Agent,
            scope_id: None,
            coding_task_id: None,
            call_id: ToolRequestId::new(),
            correlation_id: "test".to_owned(),
            deadline: Instant::now() + Duration::from_secs(60),
            cancel: CancellationToken::new(),
            progress: ProgressSink::detached(),
            only_needed_parts: false,
            via: CallVia::Harness,
        }
    }

    async fn read_at(&self, run_id: RunId, path: &Path, from: u64) -> Result<ToolOutput, ToolFailure> {
        let arguments = json!({"path": path, "from": from});
        self.provider.check_arguments(&key(), &arguments).expect("well-formed");
        self.provider.call(self.context(run_id), &key(), arguments).await
    }

    async fn read(&self, path: &Path) -> Result<ToolOutput, ToolFailure> {
        self.read_at(RunId::new(), path, 0).await
    }
}

fn code(outcome: Result<ToolOutput, ToolFailure>) -> String {
    outcome.expect_err("a failure").code
}

/// The fixture re-saved as PDF 1.5 with object streams and a cross-reference stream, every
/// stream Flate-compressed: the "compressed" form some browsers and PDF tools write.
fn compressed_copy() -> Vec<u8> {
    let mut document = lopdf::Document::load_mem(BROWSER_PDF).expect("the fixture loads");
    document.version = "1.5".to_owned();
    document.compress();
    let mut out = Vec::new();
    document.save_modern(&mut out).expect("saved with object streams");
    out
}

// spec: ordin-desktop/components/FileService#SA-FS-DOC-2
#[tokio::test]
async fn happy_path_a_browser_printed_pdf_in_an_allowed_folder_is_read() {
    let r = rig().await;
    let pdf = r.fx.put(&r.allowed, "Agent-Platform.pdf", BROWSER_PDF);
    let out = r.read(&pdf).await.expect("read");
    for expected in [
        "Agent Platform",
        "Session supervisor",
        "Planned — Q2",
        "naïve, résumé, Zürich",
        "≤ 50 MB per file, €1,200 budget",
        "ordin agent start --name \"Document agent\"",
    ] {
        assert!(out.text.contains(expected), "missing {expected:?} in {}", out.text);
    }
    assert!(!out.text.contains('\u{FB01}'), "ligatures become letters");
    assert!(out.structured.as_ref().unwrap()["next"].is_null());
}

// spec: ordin-desktop/components/FileService#SA-FS-DOC-2
#[tokio::test]
async fn edge_a_pdf_with_compressed_object_streams_is_read() {
    let r = rig().await;
    let bytes = compressed_copy();
    assert!(
        bytes.windows(b"/ObjStm".len()).any(|w| w == b"/ObjStm"),
        "the copy uses object streams"
    );
    let pdf = r.fx.put(&r.allowed, "compressed.pdf", &bytes);
    let out = r.read(&pdf).await.expect("read");
    assert!(out.text.contains("Session supervisor"), "{}", out.text);
    assert!(out.text.contains("Zürich"));
}

// spec: ordin-desktop/components/FileService#SA-FS-DOC-3
#[tokio::test]
async fn error_a_file_outside_the_allowed_folders_is_refused() {
    let r = rig().await;
    let pdf = r.fx.put(&r.other, "secret.pdf", BROWSER_PDF);
    assert_eq!(code(r.read(&pdf).await), "FILE_NOT_GRANTED");
}

// spec: ordin-desktop/components/FileService#SA-FS-DOC-3
#[cfg(unix)]
#[tokio::test]
async fn error_a_link_inside_an_allowed_folder_to_a_file_outside_it_is_refused() {
    let r = rig().await;
    let secret = r.fx.put(&r.other, "secret.md", b"# Secret");
    let link = r.fx.root(&r.allowed).join("link.md");
    std::os::unix::fs::symlink(&secret, &link).unwrap();
    assert_eq!(code(r.read(&link).await), "FILE_NOT_GRANTED");
}

// spec: ordin-desktop/components/FileService#SA-FS-DOC-3
#[tokio::test]
async fn edge_a_file_in_the_agents_own_folder_is_read() {
    let r = rig().await;
    let note = r.agent_folder.join("notes.md");
    std::fs::write(&note, "# Notes\n\n| a | b |\n|---|---|\n| 1 | 2 |\n").unwrap();
    let out = r.read(&note).await.expect("read");
    assert!(out.text.starts_with("# Notes"), "Markdown keeps its source");
}

// spec: ordin-desktop/components/FileService#SA-FS-DOC-4
#[tokio::test]
async fn edge_a_long_document_is_read_in_parts() {
    let r = rig().await;
    let text = "é".repeat(DOC_PART_CHARS * 2 + 7);
    let file = r.fx.put(&r.allowed, "long.txt", text.as_bytes());
    let run = RunId::new();
    let first = r.read_at(run, &file, 0).await.unwrap();
    assert_eq!(first.structured.as_ref().unwrap()["next"], DOC_PART_CHARS);
    assert!(first.text.contains(&format!("from: {DOC_PART_CHARS}")));
    let second = r.read_at(run, &file, DOC_PART_CHARS as u64).await.unwrap();
    assert_eq!(second.structured.as_ref().unwrap()["next"], DOC_PART_CHARS * 2);
    let third = r.read_at(run, &file, (DOC_PART_CHARS * 2) as u64).await.unwrap();
    assert_eq!(third.text, "é".repeat(7));
    assert_eq!(third.structured.as_ref().unwrap()["characters"], DOC_PART_CHARS * 2 + 7);
}

// spec: ordin-desktop/components/FileService#SA-FS-DOC-4
#[tokio::test]
async fn edge_a_changed_file_is_read_again() {
    let r = rig().await;
    let file = r.fx.put(&r.allowed, "plan.md", b"first");
    let run = RunId::new();
    assert_eq!(r.read_at(run, &file, 0).await.unwrap().text, "first");
    // A later modification time, whatever the file system's resolution.
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&file, "second").unwrap();
    assert_eq!(r.read_at(run, &file, 0).await.unwrap().text, "second");
}

// spec: ordin-desktop/components/FileService#SA-FS-DOC-3
#[tokio::test]
async fn error_rows() {
    let r = rig().await;
    let image = r.fx.put(&r.allowed, "photo.png", b"\x89PNG");
    assert_eq!(code(r.read(&image).await), "FILE_DOC_KIND_UNSUPPORTED");
    let broken = r.fx.put(&r.allowed, "broken.pdf", b"not a pdf at all");
    assert_eq!(code(r.read(&broken).await), "FILE_DOC_UNREADABLE");
    let empty = r.fx.put(&r.allowed, "empty.md", b"  \n ");
    assert_eq!(code(r.read(&empty).await), "FILE_DOC_NO_TEXT");
    let missing = r.fx.root(&r.allowed).join("missing.pdf");
    assert_eq!(code(r.read(&missing).await), "FILE_NOT_FOUND");
}

// spec: ordin-desktop/components/FileService#SA-FS-DOC-1
#[tokio::test]
async fn the_tool_is_offered_and_a_relative_path_is_refused_before_the_call() {
    let r = rig().await;
    assert_eq!(r.provider.servers()[0].tools[0].name, "read_document");
    let problem = r
        .provider
        .check_arguments(&key(), &json!({"path": "Agent-Platform.pdf"}))
        .unwrap_err();
    assert_eq!(problem.at, "/path");
}
