//! Ordin's minutes and transcription tools (OrdinHost.md OH-MIN-4 to OH-MIN-11): one test per
//! Gherkin scenario and per Error Table row, with an in-memory Minutes Service behind the port.

use std::sync::{Arc, Mutex};

use ordin_mcp_front::ToolCaller;
use ordin_types::id::{MinutesId, MinutesJobId, PlaceId};
use ordin_types::paging::{Cursor, Page};
use ordin_types::time::Timestamp;
use ordin_types::wire::minutes::{
    Minutes, MinutesJob, MinutesJobPurpose, MinutesJobState, MinutesSource, MinutesSummary,
    Transcript, TranscriptSourceKind, Utterance,
};
use ordin_types::{RunId, WorkspaceId};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::support::*;
use crate::ports::{AgentFolder, DepError};
use crate::{LocalFile, MinutesExportFormat, MinutesPort, WriteMinutesRun};

/// The Minutes Service as the port sees it, in memory.
#[derive(Default)]
struct FakeMinutes {
    minutes: Mutex<Vec<Minutes>>,
    jobs: Mutex<Vec<MinutesJob>>,
    /// Every `LocalFile` the port was given: (tool, file).
    files: Mutex<Vec<(&'static str, LocalFile)>>,
    /// The model is present and consent given: new recording jobs queue, else they wait.
    model_ready: Mutex<bool>,
}

fn dep(code: &str, message: &str) -> DepError {
    DepError {
        code: code.to_owned(),
        retryable: false,
        message: message.to_owned(),
    }
}

impl FakeMinutes {
    fn job(&self, ws: WorkspaceId, name: &str, purpose: MinutesJobPurpose) -> MinutesJob {
        let state = if *self.model_ready.lock().unwrap() {
            MinutesJobState::Queued
        } else {
            MinutesJobState::WaitingForModel
        };
        let job = MinutesJob {
            id: MinutesJobId::new(),
            workspace_id: ws,
            source_name: name.to_owned(),
            source: MinutesSource::AudioFile,
            state,
            created_at: Timestamp::now(),
            purpose,
        };
        self.jobs.lock().unwrap().push(job.clone());
        job
    }

    fn set_state(&self, id: MinutesJobId, state: MinutesJobState) {
        for j in self.jobs.lock().unwrap().iter_mut() {
            if j.id == id {
                j.state = state.clone();
            }
        }
    }
}

fn transcript() -> Transcript {
    Transcript {
        format: 1,
        source_kind: TranscriptSourceKind::AudioFile,
        source_name: "standup.m4a".to_owned(),
        language: Some("en".to_owned()),
        duration_ms: 65_000,
        utterances: vec![Utterance {
            start_ms: 61_000,
            end_ms: 64_000,
            speaker: None,
            text: "We ship on Friday.".to_owned(),
        }],
    }
}

#[async_trait::async_trait]
impl MinutesPort for FakeMinutes {
    async fn list(
        &self,
        ws: WorkspaceId,
        _: Option<Cursor>,
    ) -> Result<Page<MinutesSummary>, DepError> {
        Ok(Page {
            items: self
                .minutes
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.workspace_id == ws)
                .map(|m| MinutesSummary {
                    id: m.id,
                    title: m.title.clone(),
                    source: m.source,
                    kept_at: m.kept_at,
                })
                .collect(),
            next_cursor: None,
        })
    }
    async fn get(&self, id: MinutesId) -> Result<Minutes, DepError> {
        self.minutes
            .lock()
            .unwrap()
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| dep("MIN_NOT_FOUND", "Those minutes are no longer here."))
    }
    async fn transcript(&self, _: MinutesId) -> Result<Transcript, DepError> {
        Ok(transcript())
    }
    async fn save_edit(&self, id: MinutesId, body: String, base: u32) -> Result<Minutes, DepError> {
        let mut all = self.minutes.lock().unwrap();
        let m = all
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or_else(|| dep("MIN_NOT_FOUND", "Those minutes are no longer here."))?;
        if m.revision != base {
            return Err(dep(
                "MIN_EDIT_CONFLICT",
                "These minutes changed since you opened them.",
            ));
        }
        m.body = body;
        m.revision += 1;
        Ok(m.clone())
    }
    async fn jobs(&self, ws: WorkspaceId) -> Result<Vec<MinutesJob>, DepError> {
        Ok(self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .filter(|j| j.workspace_id == ws)
            .cloned()
            .collect())
    }
    async fn cancel_job(&self, id: MinutesJobId) -> Result<MinutesJob, DepError> {
        self.set_state(id, MinutesJobState::Cancelled);
        Ok(self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .find(|j| j.id == id)
            .cloned()
            .unwrap())
    }
    async fn retry(&self, id: MinutesJobId) -> Result<MinutesJob, DepError> {
        self.set_state(id, MinutesJobState::Queued);
        Ok(self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .find(|j| j.id == id)
            .cloned()
            .unwrap())
    }
    async fn import(&self, ws: WorkspaceId, file: LocalFile) -> Result<MinutesJob, DepError> {
        let name = file.path.clone();
        self.files.lock().unwrap().push(("import", file));
        Ok(self.job(ws, &name, MinutesJobPurpose::Minutes))
    }
    async fn transcribe(&self, ws: WorkspaceId, file: LocalFile) -> Result<MinutesJob, DepError> {
        let name = file.path.clone();
        self.files.lock().unwrap().push(("transcribe", file));
        Ok(self.job(ws, &name, MinutesJobPurpose::Transcript))
    }
    async fn transcription(&self, _: MinutesJobId) -> Result<Transcript, DepError> {
        Ok(transcript())
    }
    async fn export(
        &self,
        _: MinutesId,
        _: MinutesExportFormat,
        target: LocalFile,
    ) -> Result<String, DepError> {
        let path = target.path.clone();
        self.files.lock().unwrap().push(("export", target));
        Ok(path)
    }
}

struct Rig {
    f: Fixture,
    port: Arc<FakeMinutes>,
    run: RunId,
    docs: PlaceId,
}

async fn rig() -> Rig {
    let f = fixture().await;
    let port = Arc::new(FakeMinutes::default());
    f.host.set_minutes_port(port.clone());
    let docs = PlaceId::new();
    *f.workspaces.folders.lock().unwrap() = vec![
        AgentFolder {
            place_id: docs,
            name: "docs".to_owned(),
            read_only: false,
            granted: false,
        },
        AgentFolder {
            place_id: PlaceId::new(),
            name: "PROC-12".to_owned(),
            read_only: true,
            granted: false,
        },
    ];
    let run = f.open().await;
    Rig { f, port, run, docs }
}

impl Rig {
    fn ctx(&self) -> ordin_mcp_front::ToolCallContext {
        ordin_mcp_front::ToolCallContext {
            run_id: self.run,
            workspace_id: self.f.ws,
            agent_instance_id: ordin_types::AgentInstanceId::new(),
            caller: ToolCaller::Ordin,
            scope_id: None,
            coding_task_id: None,
            call_id: ordin_types::ToolRequestId::new(),
            correlation_id: "test".to_owned(),
            deadline: tokio::time::Instant::now().into_std() + std::time::Duration::from_secs(30),
            cancel: CancellationToken::new(),
            progress: ordin_mcp_front::ProgressSink::detached(),
            only_needed_parts: false,
            via: ordin_mcp_front::CallVia::Harness,
        }
    }

    async fn call(&self, tool: &str, args: Value) -> Result<String, (String, String)> {
        self.f
            .host
            .tool(&self.ctx(), tool, args)
            .await
            .map(|o| o.text)
            .map_err(|e| (e.code, e.message))
    }

    fn minutes(&self, title: &str, revision: u32) -> MinutesId {
        let id = MinutesId::new();
        self.port.minutes.lock().unwrap().push(Minutes {
            id,
            workspace_id: self.f.ws,
            title: title.to_owned(),
            source: MinutesSource::TranscriptFile,
            source_name: "call.vtt".to_owned(),
            body: "## Summary\nThe call.".to_owned(),
            recording: None,
            run_id: None,
            revision,
            kept_at: Timestamp::now(),
            edited_at: None,
        });
        id
    }

    async fn offered(&self) -> Vec<String> {
        let grant = ordin_mcp_front::SessionGrant {
            run_id: self.run,
            workspace_id: self.f.ws,
            agent_instance_id: self.f.host.config.ordin_agent_instance_id,
            harness: ordin_types::domain::identity::HarnessKind::ClaudeCode,
            profile: crate::types::ProgramProfile::Desktop,
            caller: ToolCaller::Ordin,
            scope_id: None,
            coding_task_id: None,
            only_servers: None,
        };
        self.f
            .host
            .tool_provider()
            .offer(&grant)
            .await
            .unwrap()
            .into_iter()
            .map(|k| k.tool)
            .collect()
    }
}

// Scenario: Happy path — Ordin reads and edits minutes
// spec: ordin-desktop/components/OrdinHost#OH-MIN-7
#[tokio::test]
async fn happy_path_ordin_reads_and_edits_minutes() {
    let r = rig().await;
    let id = r.minutes("Supplier call", 1);
    let listed = r.call("list_minutes", json!({})).await.unwrap();
    assert!(
        listed.contains(&format!("Supplier call (minutes_id {id}")),
        "{listed}"
    );
    let read = r
        .call("read_minutes", json!({ "minutes_id": id }))
        .await
        .unwrap();
    assert!(
        read.starts_with("Title: Supplier call\nRevision: 1\n"),
        "{read}"
    );
    let saved = r
        .call(
            "edit_minutes",
            json!({ "minutes_id": id, "body": "## Summary\nChanged.", "base_revision": 1 }),
        )
        .await
        .unwrap();
    assert_eq!(saved, "Saved as revision 2.");
    let said = r
        .call("read_minutes_transcript", json!({ "minutes_id": id }))
        .await
        .unwrap();
    assert_eq!(said, "[00:01:01] We ship on Friday.");
}

// Scenario: Error path — an edit on an old revision
#[tokio::test]
async fn error_an_edit_on_an_old_revision() {
    let r = rig().await;
    let id = r.minutes("Supplier call", 2);
    let (code, message) = r
        .call(
            "edit_minutes",
            json!({ "minutes_id": id, "body": "x", "base_revision": 1 }),
        )
        .await
        .unwrap_err();
    assert_eq!(code, "MIN_EDIT_CONFLICT");
    assert!(
        message.contains("Read the minutes again with read_minutes"),
        "{message}"
    );
}

// Scenario: Happy path — a recording becomes text for Ordin
#[tokio::test]
async fn happy_path_a_recording_becomes_text_for_ordin() {
    let r = rig().await;
    *r.port.model_ready.lock().unwrap() = true;
    let started = r
        .call(
            "transcribe_recording",
            json!({ "folder": "docs", "path": "calls/standup.m4a" }),
        )
        .await
        .unwrap();
    assert!(
        started.contains("Call read_transcription with this job id"),
        "{started}"
    );
    let job = r.port.jobs.lock().unwrap()[0].clone();
    assert_eq!(job.purpose, MinutesJobPurpose::Transcript);
    let (_, file) = r.port.files.lock().unwrap()[0].clone();
    assert_eq!(
        file,
        LocalFile {
            place_id: r.docs,
            path: "calls/standup.m4a".to_owned()
        }
    );
    let listed = r.call("list_minutes_jobs", json!({})).await.unwrap();
    assert!(
        listed.contains("calls/standup.m4a (text for Ordin) — Waiting to start"),
        "{listed}"
    );
    // Error Table: OH_TRANSCRIPTION_NOT_READY before the text is ready.
    let (code, _) = r
        .call("read_transcription", json!({ "job_id": job.id }))
        .await
        .unwrap_err();
    assert_eq!(code, "OH_TRANSCRIPTION_NOT_READY");
    r.port.set_state(job.id, MinutesJobState::TranscriptReady);
    let text = r
        .call("read_transcription", json!({ "job_id": job.id }))
        .await
        .unwrap();
    assert_eq!(text, "[00:01:01] We ship on Friday.");
    assert!(
        r.port.minutes.lock().unwrap().is_empty(),
        "no minutes are kept"
    );
}

// Error Table: OH_TRANSCRIPTION_NOT_READY for a job that makes minutes
#[tokio::test]
async fn error_table_oh_transcription_not_ready_for_a_minutes_job() {
    let r = rig().await;
    r.call(
        "import_minutes",
        json!({ "folder": "docs", "path": "call.m4a" }),
    )
    .await
    .unwrap();
    let job = r.port.jobs.lock().unwrap()[0].clone();
    r.port.set_state(job.id, MinutesJobState::TranscriptReady);
    let (code, message) = r
        .call("read_transcription", json!({ "job_id": job.id }))
        .await
        .unwrap_err();
    assert_eq!(code, "OH_TRANSCRIPTION_NOT_READY");
    assert!(
        message.contains("makes minutes, not text for Ordin"),
        "{message}"
    );
}

// Scenario: Edge — the speech model waits for the person
#[tokio::test]
async fn edge_the_speech_model_waits_for_the_person() {
    let r = rig().await;
    let started = r
        .call(
            "transcribe_recording",
            json!({ "folder": "docs", "path": "standup.m4a" }),
        )
        .await
        .unwrap();
    assert!(
        started.contains("only the person can agree to download it"),
        "{started}"
    );
}

// Scenario: Error path — a path that leaves the folder
#[tokio::test]
async fn error_table_oh_minutes_path_invalid() {
    let r = rig().await;
    for path in [
        "../secrets/notes.txt",
        "/etc/passwd",
        "C:\\notes.txt",
        "a/./b.vtt",
        "a//b.vtt",
    ] {
        let (code, _) = r
            .call("import_minutes", json!({ "folder": "docs", "path": path }))
            .await
            .unwrap_err();
        assert_eq!(code, "OH_MINUTES_PATH_INVALID", "{path}");
    }
    assert!(r.port.jobs.lock().unwrap().is_empty(), "no job starts");
}

// Error Table: OH_MINUTES_FOLDER_UNKNOWN
#[tokio::test]
async fn error_table_oh_minutes_folder_unknown() {
    let r = rig().await;
    let (code, _) = r
        .call(
            "import_minutes",
            json!({ "folder": "elsewhere", "path": "a.vtt" }),
        )
        .await
        .unwrap_err();
    assert_eq!(code, "OH_MINUTES_FOLDER_UNKNOWN");
}

// Error Table: OH_MINUTES_FOLDER_READ_ONLY
#[tokio::test]
async fn error_table_oh_minutes_folder_read_only() {
    let r = rig().await;
    let id = r.minutes("Supplier call", 1);
    let (code, _) = r
        .call(
            "export_minutes",
            json!({ "minutes_id": id, "format": "pdf", "folder": "PROC-12" }),
        )
        .await
        .unwrap_err();
    assert_eq!(code, "OH_MINUTES_FOLDER_READ_ONLY");
}

// Scenario: Edge — export never replaces a file (the name; the program adds " (n)", DSK-MIN-6)
#[tokio::test]
async fn edge_export_names_the_file_after_the_title() {
    let r = rig().await;
    let id = r.minutes("Supplier call: Q3/Q4", 1);
    let saved = r
        .call(
            "export_minutes",
            json!({ "minutes_id": id, "format": "docx", "folder": "docs" }),
        )
        .await
        .unwrap();
    assert_eq!(saved, "Saved Supplier call- Q3-Q4.docx in docs.");
    // A path whose extension is not the format's.
    let (code, _) = r
        .call(
            "export_minutes",
            json!({ "minutes_id": id, "format": "pdf", "folder": "docs", "path": "x.docx" }),
        )
        .await
        .unwrap_err();
    assert_eq!(code, "OH_MINUTES_PATH_INVALID");
}

// Scenario: Edge — what stays the person's (OH-MIN-9), and the eleven are offered (OH-MIN-4)
#[tokio::test]
async fn edge_what_stays_the_persons() {
    let r = rig().await;
    let offered = r.offered().await;
    for tool in crate::minutes_tools::MINUTES_TOOLS {
        assert!(offered.contains(&tool.to_string()), "{tool} offered");
    }
    // OH-MIN-9: the minutes and transcription tools offered are exactly the eleven, none of which
    // records, gives a consent, saves the OBS password, deletes a recording or removes minutes.
    let mut minutes_tools: Vec<&str> = offered
        .iter()
        .map(String::as_str)
        .filter(|n| n.contains("minutes") || n.contains("transcri"))
        .collect();
    minutes_tools.sort_unstable();
    let mut expected = crate::minutes_tools::MINUTES_TOOLS.to_vec();
    expected.sort_unstable();
    assert_eq!(minutes_tools, expected);
}

// Scenario: Edge — the tools are absent without the port (the build without `minutes` has none)
#[tokio::test]
async fn edge_the_tools_are_absent_without_the_port() {
    let f = fixture().await;
    let run = f.open().await;
    let grant = ordin_mcp_front::SessionGrant {
        run_id: run,
        workspace_id: f.ws,
        agent_instance_id: f.host.config.ordin_agent_instance_id,
        harness: ordin_types::domain::identity::HarnessKind::ClaudeCode,
        profile: crate::types::ProgramProfile::Desktop,
        caller: ToolCaller::Ordin,
        scope_id: None,
        coding_task_id: None,
        only_servers: None,
    };
    let offered: Vec<String> = f
        .host
        .tool_provider()
        .offer(&grant)
        .await
        .unwrap()
        .into_iter()
        .map(|k| k.tool)
        .collect();
    assert!(offered.contains(&"delegate".to_owned()));
    for tool in crate::minutes_tools::MINUTES_TOOLS {
        assert!(
            !offered.contains(&tool.to_string()),
            "{tool} offered without a port"
        );
    }
}

// Scenario: Security — a minutes run sees only its own tool (OH-MIN-10): the host asks for server
// `ordin` only; the MCP Front's filter is tested in `ordin-mcp-front` (SA-MCP-SRV-01).
// spec: ordin-desktop/components/OrdinHost#OH-MIN-10
#[tokio::test]
async fn security_a_minutes_run_asks_for_server_ordin_only() {
    let r = rig().await;
    let host = Arc::downgrade(&r.f.host);
    let ws = r.f.ws;
    r.f.supervisor.respond(Arc::new(move |run_id: RunId, _| {
        let host = host.clone();
        Box::pin(async move {
            let host = host.upgrade()?;
            let ctx = ordin_mcp_front::ToolCallContext {
                run_id,
                workspace_id: ws,
                agent_instance_id: ordin_types::AgentInstanceId::new(),
                caller: ToolCaller::Ordin,
                scope_id: None,
                coding_task_id: None,
                call_id: ordin_types::ToolRequestId::new(),
                correlation_id: "test".to_owned(),
                deadline: tokio::time::Instant::now().into_std() + std::time::Duration::from_secs(30),
                cancel: CancellationToken::new(),
                progress: ordin_mcp_front::ProgressSink::detached(),
                only_needed_parts: false,
                via: ordin_mcp_front::CallVia::Harness,
            };
            // A minutes run cannot call the minutes tools either (OH-MIN-4).
            let refused = host.tool(&ctx, "list_minutes", json!({})).await;
            assert!(refused.is_err());
            let _ = host
                .tool(
                    &ctx,
                    "keep_minutes",
                    json!({ "title": "T", "summary": "S", "decisions": [], "actions": [], "open_questions": [] }),
                )
                .await;
            Some("Done.".to_owned())
        })
    }));
    r.f.host
        .write_minutes(
            WriteMinutesRun {
                workspace_id: ws,
                source: MinutesSource::TranscriptFile,
                source_name: "meeting.vtt".to_owned(),
                recording: None,
                transcript_text: "Alex: hello".to_owned(),
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let starts = r.f.supervisor.starts.lock().unwrap();
    let minutes_run = starts.last().expect("the minutes run started");
    assert_eq!(minutes_run.only_servers, Some(vec!["ordin".to_owned()]));
    assert!(
        starts[..starts.len() - 1]
            .iter()
            .all(|s| s.only_servers.is_none()),
        "the conversation run has no limit"
    );
}

#[test]
fn the_path_check() {
    for good in ["a.vtt", "calls/standup.m4a", "calls\\standup.m4a"] {
        assert!(crate::check_path(good), "{good}");
    }
    for bad in ["", "..", "../a", "a/../b", "/a", "\\a", "C:a", "a/", "./a"] {
        assert!(!crate::check_path(bad), "{bad}");
    }
}
