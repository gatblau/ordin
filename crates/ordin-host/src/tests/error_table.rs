//! X1: one test per row of the Error Table (OrdinHost.md *Error Table* and the rows the amendments
//! added), named after the row's code or condition; X4: the registry check.
#![allow(unused_imports)]

use std::sync::Arc;

use ordin_mcp_front::ToolFailure;
use ordin_service_api::commands::ordin::BlockAnswer;
use ordin_service_api::commands::voice::VoiceUpdateInput;
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::wire::ContentRef;
use ordin_types::{ArtefactId, BlockId, CodedError, RepairAction, RunId, WorkspaceId};
use serde_json::{json, Value};

use super::support::*;
use crate::error::{OhError, ALL_CODES};
use crate::ids::ConversationId;

// ---------------------------------------------------------------- helpers

/// Ordin never answers from now on: a turn stays running.
fn silent() -> Responder {
    Arc::new(|_, _| Box::pin(async { None }))
}

/// Ordin answers `text` to a turn containing `needle`, else "Ready.".
fn answer_to(needle: &'static str, text: &'static str) -> Responder {
    Arc::new(move |_, turn| {
        Box::pin(async move {
            Some(if turn.contains(needle) {
                text.to_owned()
            } else {
                "Ready.".to_owned()
            })
        })
    })
}

/// Ordin shows a question with two choices; returns its block id.
async fn show_question(f: &Fixture) -> BlockId {
    let out = f
        .tool(
            "show",
            json!({ "block": { "kind": "question", "question": "Which one?", "choices": ["A", "B"], "allow_other": false } }),
        )
        .await
        .expect("the question is shown");
    let id = out.structured.expect("structured")["block_id"].clone();
    serde_json::from_value(id).expect("a block id")
}

fn assert_tool(
    r: Result<ordin_mcp_front::ToolOutput, ToolFailure>,
    code: &str,
    retryable: bool,
) -> ToolFailure {
    let e = r.expect_err("the tool refuses");
    assert_eq!(e.code, code, "{e:?}");
    assert_eq!(e.retryable, retryable, "{e:?}");
    e
}

/// Sends one message of the person and waits for Ordin's turn on it to end, so a tool call that
/// follows is made in the person's own turn (step 52.1).
async fn persons_turn(f: &Fixture) {
    f.host
        .send_message(f.ws, "Please change my setting".to_owned(), vec![])
        .await
        .expect("sent");
    let host = f.host.clone();
    let ws = f.ws;
    assert!(
        eventually(|| {
            crate::host::lock(&host.state)
                .convs
                .get(&ws)
                .map(|c| !c.busy && c.persons_turn)
                .unwrap_or(false)
        })
        .await
    );
}

/// Ends the fixture's current conversation by Clear so an earlier, ended conversation exists.
async fn ended_conversation(f: &Fixture) -> ConversationId {
    f.open().await;
    let first = crate::host::lock(&f.host.state)
        .convs
        .get(&f.ws)
        .unwrap()
        .id;
    f.host.start_fresh(f.ws, vec![]).await.expect("start fresh");
    first
}

// ---------------------------------------------------------------- conversation rows

// Workspace unknown.
#[tokio::test]
async fn oh_workspace_unknown() {
    let f = fixture().await;
    f.f.always("workspaces.ensure_active", "WS_UNKNOWN", false);
    let e = f
        .host
        .send_message(f.ws, "Hello".to_owned(), vec![])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_WORKSPACE_UNKNOWN");
    assert!(!e.retryable());
    assert_eq!(e.message(), "This workspace isn't here any more.");
}

// Message empty or over 8,000 characters.
#[tokio::test]
async fn oh_message_invalid() {
    let f = fixture().await;
    for text in [String::new(), "x".repeat(8_001)] {
        let e = f.host.send_message(f.ws, text, vec![]).await.unwrap_err();
        assert_eq!(e.code(), "OH_MESSAGE_INVALID");
        assert!(!e.retryable());
        assert_eq!(e.message(), "That message is too long; send it in parts.");
    }
    // 8,000 characters is accepted.
    f.open().await;
    f.host
        .send_message(f.ws, "x".repeat(8_000), vec![])
        .await
        .unwrap();
}

// More than 5 messages queued.
#[tokio::test]
async fn oh_too_many_waiting() {
    let f = fixture().await;
    f.open().await;
    f.supervisor.respond(silent());
    let first = f
        .host
        .send_message(f.ws, "one".to_owned(), vec![])
        .await
        .unwrap();
    assert_eq!(first.position_in_queue, 0);
    for n in 1..=5u8 {
        let r = f
            .host
            .send_message(f.ws, format!("queued {n}"), vec![])
            .await
            .unwrap();
        assert_eq!(r.position_in_queue, n);
    }
    let e = f
        .host
        .send_message(f.ws, "seventh".to_owned(), vec![])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_TOO_MANY_WAITING");
    assert!(e.retryable());
    assert_eq!(CodedError::repair(&e), Some(RepairAction::TryAgain));
    assert_eq!(
        e.message(),
        "Ordin is still answering; try again in a moment."
    );
}

// Ordin's session cannot start (retryable or permanent as the Supervisor says).
#[tokio::test]
async fn oh_session_start_failed() {
    for retryable in [true, false] {
        let f = fixture().await;
        f.f.always(
            "supervisor.start_conversation",
            "SUP_START_FAILED",
            retryable,
        );
        let e = f
            .host
            .send_message(f.ws, "Hello".to_owned(), vec![])
            .await
            .unwrap_err();
        assert_eq!(e.code(), "OH_SESSION_START_FAILED");
        assert_eq!(e.retryable(), retryable);
        assert_eq!(e.message(), "Ordin couldn't start.");
        assert_eq!(CodedError::repair(&e), Some(RepairAction::TryAgain));
        assert_eq!(f.count("SELECT COUNT(*) FROM ordin_conversations").await, 0);
    }
}

// Start fresh while a crew Ordin coordinates runs.
#[tokio::test]
async fn oh_crew_running_on_start_fresh() {
    let f = fixture().await;
    f.open().await;
    *f.orchestrator.running.lock().unwrap() = Some(ordin_types::CrewRunId::new());
    let e = f.host.start_fresh(f.ws, vec![]).await.unwrap_err();
    assert_eq!(e.code(), "OH_CREW_RUNNING");
    assert!(!e.retryable());
    assert_eq!(
        e.message(),
        "Wait for the crew to finish before starting fresh."
    );
}

// Summary turn fails.
#[tokio::test]
async fn oh_summary_failed() {
    let f = fixture().await;
    f.open().await;
    f.supervisor.respond(answer_to("Write a summary", ""));
    let e = f.host.summarise(f.ws).await.unwrap_err();
    assert_eq!(e.code(), "OH_SUMMARY_FAILED");
    assert!(e.retryable());
    assert_eq!(e.message(), "Ordin couldn't summarise just now.");
    // The conversation stays current.
    assert_eq!(
        f.count("SELECT COUNT(*) FROM ordin_conversations WHERE state = 'current'")
            .await,
        1
    );
}

// Conversation unknown.
#[tokio::test]
async fn oh_conversation_unknown() {
    let f = fixture().await;
    let e = f
        .host
        .ask_about_conversation(f.ws, ConversationId::new(), "What was decided?".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CONVERSATION_UNKNOWN");
    assert!(!e.retryable());
    let e = f
        .host
        .read_conversation(ConversationId::new(), None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CONVERSATION_UNKNOWN");
}

// More than 20 attachments on a message.
#[tokio::test]
async fn oh_attachments_invalid() {
    let f = fixture().await;
    let many: Vec<ContentRef> = (0..21)
        .map(|_| ContentRef::Artefact {
            artefact_id: ArtefactId::new(),
        })
        .collect();
    let e = f
        .host
        .send_message(f.ws, "See these".to_owned(), many)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_ATTACHMENTS_INVALID");
    assert!(!e.retryable());
    assert_eq!(e.message(), "Attach at most 20 files to one message.");
}

// Block unknown in the workspace.
#[tokio::test]
async fn oh_block_unknown() {
    let f = fixture().await;
    f.open().await;
    let e = f
        .host
        .answer_block(
            f.ws,
            BlockId::new(),
            BlockAnswer::Choice {
                choice: "A".to_owned(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_BLOCK_UNKNOWN");
    assert!(!e.retryable());
    // A block of another workspace is unknown here too.
    let block = show_question(&f).await;
    let e = f.host.block(WorkspaceId::new(), block).await.unwrap_err();
    assert_eq!(e.code(), "OH_BLOCK_UNKNOWN");
}

// Answer to a block already answered.
#[tokio::test]
async fn oh_block_answered() {
    let f = fixture().await;
    f.open().await;
    let block = show_question(&f).await;
    f.host
        .answer_block(
            f.ws,
            block,
            BlockAnswer::Choice {
                choice: "A".to_owned(),
            },
        )
        .await
        .unwrap();
    let e = f
        .host
        .answer_block(
            f.ws,
            block,
            BlockAnswer::Choice {
                choice: "B".to_owned(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_BLOCK_ANSWERED");
    assert!(!e.retryable());
    assert_eq!(e.message(), "This was already answered.");
}

// Answer kind not fitting the block (OH_INPUT_INVALID).
#[tokio::test]
async fn oh_input_invalid_answer_kind() {
    let f = fixture().await;
    f.open().await;
    let block = show_question(&f).await;
    for answer in [
        BlockAnswer::Choice {
            choice: "C".to_owned(),
        },
        BlockAnswer::Other {
            text: "something else".to_owned(),
        },
        BlockAnswer::Offer { accept: true },
    ] {
        let e = f.host.answer_block(f.ws, block, answer).await.unwrap_err();
        assert_eq!(e.code(), "OH_INPUT_INVALID");
        assert!(!e.retryable());
    }
}

// Datastore unavailable.
#[tokio::test]
async fn oh_datastore_unavailable() {
    let f = fixture().await;
    f.pool.close().await;
    let e = f
        .host
        .block(f.ws, ordin_types::BlockId::new())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DATASTORE_UNAVAILABLE");
    assert!(e.retryable());
    assert_eq!(e.message(), "Something went wrong; try again.");
    assert_eq!(CodedError::repair(&e), Some(RepairAction::TryAgain));
}

// Datastore unavailable: the log line names the error's class, never its text, so a decode error
// quoting stored workflow or charter text does not reach the log (Logging.md; FU-122).
#[tokio::test]
async fn oh_datastore_unavailable_logs_the_class_not_the_text() {
    init_logging();
    let quoted = "charter-purpose-7f3a9c Compare the confidential supplier quotes";
    let decode = sqlx::Error::ColumnDecode {
        index: "purpose".to_owned(),
        source: Box::new(std::io::Error::other(quoted)),
    };
    let e = OhError::from(decode);
    assert_eq!(e.code(), "OH_DATASTORE_UNAVAILABLE");
    assert!(
        log_lines("charter-purpose-7f3a9c").is_empty(),
        "the stored text is not logged"
    );
    let e = OhError::from(sqlx::Error::PoolTimedOut);
    assert_eq!(e.code(), "OH_DATASTORE_UNAVAILABLE");
    assert!(log_lines("ordin_host.datastore")
        .iter()
        .any(|l| l.contains("\"error_class\":\"pool\"")));
}

// ---------------------------------------------------------------- voice rows

// Voice field set by the company.
#[tokio::test]
async fn voice_fixed_by_company() {
    let mut p = policy();
    p.voice_defaults.tone = Some(ordin_types::domain::VoiceTone::Direct);
    let f = fixture_with(config(), p).await;
    let e = f
        .host
        .voice_update(VoiceUpdateInput {
            tone: Some(ordin_types::domain::VoiceTone::Friendly),
            ..VoiceUpdateInput::default()
        })
        .await
        .unwrap_err();
    assert_eq!(e.code(), "VOICE_FIXED_BY_COMPANY");
    assert!(!e.retryable());
    assert_eq!(e.message(), "Your company sets this.");
}

// Voice notes over 300 / name over 60.
#[tokio::test]
async fn voice_notes_too_long_and_voice_name_too_long() {
    let f = fixture().await;
    let e = f
        .host
        .voice_update(VoiceUpdateInput {
            notes: Some("n".repeat(301)),
            ..VoiceUpdateInput::default()
        })
        .await
        .unwrap_err();
    assert_eq!(e.code(), "VOICE_NOTES_TOO_LONG");
    assert!(!e.retryable());
    let e = f
        .host
        .voice_update(VoiceUpdateInput {
            name_to_use: Some("n".repeat(61)),
            ..VoiceUpdateInput::default()
        })
        .await
        .unwrap_err();
    assert_eq!(e.code(), "VOICE_NAME_TOO_LONG");
    // The limits themselves are accepted.
    f.host
        .voice_update(VoiceUpdateInput {
            notes: Some("n".repeat(300)),
            name_to_use: Some("n".repeat(60)),
            ..VoiceUpdateInput::default()
        })
        .await
        .unwrap();
}

// ---------------------------------------------------------------- X4

// ErrorHandling.md: every owned code is registered to this crate under a prefix it owns.
#[test]
fn every_owned_code_is_registered_and_the_prefix_is_owned() {
    assert!(!ALL_CODES.is_empty());
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-host", "{code}");
        let prefix = code.split('_').next().unwrap();
        assert!(owns_prefix("ordin-host", prefix), "{prefix}");
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted, ALL_CODES,
        "ALL_CODES is sorted and has no duplicates"
    );
}

// Every code an `OhError` answers through `CodedError` is one of `ALL_CODES`.
#[test]
fn every_returned_code_is_in_all_codes() {
    let errors = [
        OhError::WorkspaceUnknown,
        OhError::input("OH_MESSAGE_INVALID", "/text"),
        OhError::SessionStartFailed {
            cause: "X".to_owned(),
            retryable: true,
        },
        OhError::TooManyWaiting,
        OhError::CrewRunning,
        OhError::InputUnreadable {
            label: "x".to_owned(),
        },
        OhError::NotCoordinating,
        OhError::ConversationUnknown,
        OhError::NotContinuable,
        OhError::NotDeletable,
        OhError::CharterUnknown,
        OhError::WorkflowUnknown,
        OhError::VersionUnknown,
        OhError::StaleVersion { latest: 2 },
        OhError::FileRefused {
            code: "WFF_FILE_TOO_LARGE",
        },
        OhError::BlockUnknown,
        OhError::BlockAnswered,
        OhError::DashboardUnknown,
        OhError::CrewRunUnknown,
        OhError::InputMissing {
            label: "x".to_owned(),
        },
        OhError::NeedsAgents,
        OhError::TemplateUnknown,
        OhError::NotConfirmed,
        OhError::FolderNotLinked,
        OhError::DependencyFailed {
            dependency: "usage",
            code: "USAGE_PAUSED".to_owned(),
            retryable: true,
        },
        OhError::DatastoreUnavailable,
        OhError::VoiceFixed,
        OhError::NotPersonsWords,
        OhError::DraftRunning,
        OhError::NothingToDraft,
        OhError::RunUnknown,
        OhError::RunNotInterrupted,
        OhError::RunNotRepeatable,
    ];
    for e in errors {
        let coded = CodedError::code(&e);
        assert!(ALL_CODES.contains(&coded), "{coded}");
        assert_eq!(coded, e.code(), "{e:?}");
    }
}

// ---------------------------------------------------------------- tool and coordinator helpers

type ToolResult = Result<ordin_mcp_front::ToolOutput, ToolFailure>;

fn ctx_of(ws: WorkspaceId, run_id: RunId) -> ordin_mcp_front::ToolCallContext {
    ordin_mcp_front::ToolCallContext {
        run_id,
        workspace_id: ws,
        agent_instance_id: ordin_types::AgentInstanceId::new(),
        caller: ordin_mcp_front::ToolCaller::Ordin,
        scope_id: None,
        coding_task_id: None,
        call_id: ordin_types::ToolRequestId::new(),
        correlation_id: "test".to_owned(),
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
        cancel: tokio_util::sync::CancellationToken::new(),
        progress: ordin_mcp_front::ProgressSink::detached(),
        only_needed_parts: false,
        via: ordin_mcp_front::CallVia::Harness,
    }
}

/// Ordin calls `tool(args)` on a turn containing `needle`, records the result, and answers "Done.".
fn calls_tool(
    f: &Fixture,
    needle: &'static str,
    tool: &'static str,
    args: Value,
) -> Arc<std::sync::Mutex<Vec<ToolResult>>> {
    let results: Arc<std::sync::Mutex<Vec<ToolResult>>> = Arc::default();
    let host = Arc::downgrade(&f.host);
    let ws = f.ws;
    let seen = results.clone();
    f.supervisor.respond(Arc::new(move |run_id, turn| {
        let host = host.clone();
        let seen = seen.clone();
        let args = args.clone();
        Box::pin(async move {
            if turn.contains(needle) {
                if let Some(h) = host.upgrade() {
                    let r = h.tool(&ctx_of(ws, run_id), tool, args).await;
                    seen.lock().unwrap().push(r);
                }
            }
            Some("Done.".to_owned())
        })
    }));
    results
}

fn view(ws: WorkspaceId) -> ordin_crew_orchestrator::CrewView {
    ordin_crew_orchestrator::CrewView {
        crew_run_id: ordin_types::CrewRunId::new(),
        workspace_id: ws,
        purpose: "Compare quotes".to_owned(),
        members: vec![],
        round: 1,
        rounds_left: 4,
        seconds_left: 600,
        requests_left: None,
        inputs: vec![],
        latest_results: vec![],
        position: None,
        cancel: tokio_util::sync::CancellationToken::new(),
    }
}

fn paths() -> Vec<String> {
    vec!["yes".to_owned(), "no".to_owned()]
}

// ---------------------------------------------------------------- tool rows

// Crew with more than 8 members.
#[tokio::test]
async fn oh_crew_too_many_members() {
    let f = fixture().await;
    f.open().await;
    let members: Vec<Value> = (0..9)
        .map(|i| json!({ "agent": format!("a{i}"), "role": "helps" }))
        .collect();
    let e = assert_tool(
        f.tool(
            "create_crew",
            json!({ "purpose": "Compare quotes", "members": members }),
        )
        .await,
        "OH_CREW_TOO_MANY_MEMBERS",
        false,
    );
    assert_eq!(e.message, "A crew has at most 8 members.");
    assert_eq!(f.count("SELECT COUNT(*) FROM crew_charters").await, 0);
}

// Agent key not in the directory.
#[tokio::test]
async fn oh_agent_unknown() {
    let f = fixture().await;
    f.open().await;
    let e = assert_tool(
        f.tool(
            "delegate",
            json!({ "agent": "not-a-key", "brief": "Read it" }),
        )
        .await,
        "OH_AGENT_UNKNOWN",
        false,
    );
    assert_eq!(
        e.message,
        "No agent with that key is in this workspace; use a key from the agent directory."
    );
    assert!(f.supervisor.tasks.lock().unwrap().is_empty());
}

// `delegate` with a command the agent does not list (OH-DLG-1).
// spec: ordin-desktop/components/OrdinHost#OH-DLG-1
#[tokio::test]
async fn oh_command_unknown() {
    let f = fixture().await;
    let a = agent("Document agent");
    *f.inspector.agents.lock().unwrap() = vec![a.clone()];
    f.open().await;
    let e = assert_tool(
        f.tool(
            "delegate",
            json!({
                "agent": a.agent_instance_id.to_string(),
                "brief": "Read it",
                "command": "Summarise documents"
            }),
        )
        .await,
        "OH_COMMAND_UNKNOWN",
        false,
    );
    assert_eq!(
        e.message,
        "Document agent has no command \"Summarise documents\". It has no commands. Give the work as a brief, without a command."
    );
    assert!(f.supervisor.tasks.lock().unwrap().is_empty());
}

// Tool call from a run that is not Ordin's current run.
#[tokio::test]
async fn oh_not_ordin_session() {
    let f = fixture().await;
    let run = f.open().await;
    let e = f
        .host
        .tool(
            &f.ctx_for(RunId::new()),
            "find_agent",
            json!({ "need": "x" }),
        )
        .await;
    let e = assert_tool(e, "OH_NOT_ORDIN_SESSION", false);
    assert_eq!(e.message, "These tools are for Ordin's conversation only.");
    let mut ctx = f.ctx_for(run);
    ctx.caller = ordin_mcp_front::ToolCaller::Agent;
    assert_tool(
        f.host
            .tool(&ctx, "find_agent", json!({ "need": "x" }))
            .await,
        "OH_NOT_ORDIN_SESSION",
        false,
    );
}

// `finish`/`escalate`/`decide` with no crew waiting.
#[tokio::test]
async fn oh_not_coordinating() {
    let f = fixture().await;
    f.open().await;
    for (tool, args) in [
        ("finish", json!({ "summary": "All done." })),
        ("escalate", json!({ "question": "Which supplier?" })),
        (
            "decide",
            json!({ "kind": "question", "answer": "The cheaper one." }),
        ),
    ] {
        let e = assert_tool(f.tool(tool, args).await, "OH_NOT_COORDINATING", false);
        assert_eq!(e.message, "No crew is waiting for this.");
    }
}

// `decide` kind differs from the request.
#[tokio::test]
async fn oh_decision_kind_mismatch() {
    let f = fixture().await;
    f.open().await;
    let seen = calls_tool(
        &f,
        "Paths:",
        "decide",
        json!({ "kind": "check", "verdict": "carry_on", "reason": "fine" }),
    );
    let coordinator = f.host.coordinator();
    let r = coordinator
        .decide_condition(&view(f.ws), "Is the quote below budget", &paths())
        .await;
    assert!(r.is_err());
    let results = seen.lock().unwrap();
    let e = results[0].as_ref().unwrap_err();
    assert_eq!(e.code, "OH_DECISION_KIND_MISMATCH");
    assert!(!e.retryable);
    assert!(
        e.message.contains("decide(kind: condition)"),
        "{}",
        e.message
    );
}

// `decide` condition path not offered.
#[tokio::test]
async fn oh_path_unknown() {
    let f = fixture().await;
    f.open().await;
    let seen = calls_tool(
        &f,
        "Paths:",
        "decide",
        json!({ "kind": "condition", "path": "maybe", "reason": "unsure" }),
    );
    let coordinator = f.host.coordinator();
    let _ = coordinator
        .decide_condition(&view(f.ws), "Is the quote below budget", &paths())
        .await;
    let results = seen.lock().unwrap();
    let e = results[0].as_ref().unwrap_err();
    assert_eq!(e.code, "OH_PATH_UNKNOWN");
    assert!(!e.retryable);
    assert!(e.message.contains("yes, no"), "{}", e.message);
}

// Remote agent in a workflow draft.
#[tokio::test]
async fn oh_remote_agent_in_workflow() {
    let f = fixture().await;
    f.open().await;
    let e = assert_tool(
        f.tool(
            "save_workflow",
            json!({
                "name": "Quotes", "purpose": "Compare quotes", "inputs": [],
                "agents": [{ "key": "buyer", "agent": ordin_types::RemoteAgentId::new().to_string(), "role": "buys" }],
                "steps": []
            }),
        )
        .await,
        "OH_REMOTE_AGENT_IN_WORKFLOW",
        false,
    );
    assert_eq!(e.message, "Workflows name agents from the library only.");
}

// `show` block refused by the catalogue check.
#[tokio::test]
async fn block_refused() {
    let f = fixture().await;
    f.open().await;
    *f.blocks.reject.lock().unwrap() = vec![
        ordin_service_api::blocks::BlockRejection {
            code: "BLOCK_REF_UNKNOWN".to_owned(),
            at: "/parts/0".to_owned(),
            reason: "The first reason.".to_owned(),
        },
        ordin_service_api::blocks::BlockRejection {
            code: "BLOCK_REF_UNKNOWN".to_owned(),
            at: "/parts/1".to_owned(),
            reason: "The second reason.".to_owned(),
        },
    ];
    let e = assert_tool(
        f.tool("show", json!({ "block": { "kind": "question", "question": "Which?", "choices": ["A", "B"], "allow_other": false } })).await,
        "BLOCK_REFUSED",
        false,
    );
    assert!(e.message.starts_with("The first reason."), "{}", e.message);
    assert!(
        e.message.contains("/parts/1"),
        "every rejection is listed: {}",
        e.message
    );
    assert_eq!(f.host.waiting_blocks().len(), 0);
    // No block but Ordin's message replies is stored: the refused block is not.
    assert_eq!(
        f.count(r#"SELECT COUNT(*) FROM ordin_blocks WHERE body NOT LIKE '%"kind":"message"%'"#)
            .await,
        0
    );
}

// Agent instance not ready before its session starts: permanent ToolFailure at 8.3.
#[tokio::test]
async fn pkg_instance_not_ready_at_delegate() {
    let f = fixture().await;
    f.open().await;
    let a = agent("Reader");
    f.inspector.agents.lock().unwrap().push(a.clone());
    f.f.always("catalogue.ensure_ready", "PKG_INSTANCE_NOT_READY", false);
    assert_tool(
        f.tool(
            "delegate",
            json!({ "agent": a.agent_instance_id.to_string(), "brief": "Read it" }),
        )
        .await,
        "PKG_INSTANCE_NOT_READY",
        false,
    );
    assert!(f.supervisor.tasks.lock().unwrap().is_empty());
}

// Agent instance not ready at 3.1: Ordin's own package; the message waits (OH_DEPENDENCY_FAILED).
#[tokio::test]
async fn pkg_folder_unusable_at_session_start() {
    let f = fixture().await;
    f.f.always("catalogue.ensure_ready", "PKG_FOLDER_UNUSABLE", false);
    let e = f
        .host
        .send_message(f.ws, "Hello".to_owned(), vec![])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DEPENDENCY_FAILED");
    assert!(matches!(&e, OhError::DependencyFailed { code, .. } if code == "PKG_FOLDER_UNUSABLE"));
    assert!(f.supervisor.starts.lock().unwrap().is_empty());
}

// Pack questions cannot be read before a delegated task.
#[tokio::test]
async fn pending_choices_unreadable() {
    for (code, retryable) in [("PKG_INSTANCE_UNKNOWN", false), ("PKG_STORE_FAILED", true)] {
        let f = fixture().await;
        f.open().await;
        let a = agent("Reader");
        f.inspector.agents.lock().unwrap().push(a.clone());
        f.f.always("catalogue.pending_choices", code, retryable);
        assert_tool(
            f.tool(
                "delegate",
                json!({ "agent": a.agent_instance_id.to_string(), "brief": "Read it" }),
            )
            .await,
            code,
            retryable,
        );
        assert!(f.supervisor.tasks.lock().unwrap().is_empty());
    }
}

// The person's pack answer cannot be stored: the answer stands; no task; a fixed turn names the code.
#[tokio::test]
async fn pack_answer_not_stored() {
    let f = fixture().await;
    f.open().await;
    let a = agent("Reader");
    f.inspector.agents.lock().unwrap().push(a.clone());
    f.catalogue
        .pending
        .lock()
        .unwrap()
        .push(ordin_package_catalogue::PendingChoice {
            agent_instance_id: a.agent_instance_id,
            rule_index: 0,
            at_least: 1,
            class_label: "style guide".to_owned(),
            choices: vec![
                ordin_package_catalogue::PendingChoiceOption {
                    pack: "house".to_owned(),
                    label: "House".to_owned(),
                },
                ordin_package_catalogue::PendingChoiceOption {
                    pack: "plain".to_owned(),
                    label: "Plain".to_owned(),
                },
            ],
        });
    f.tool(
        "delegate",
        json!({ "agent": a.agent_instance_id.to_string(), "brief": "Read it" }),
    )
    .await
    .expect("the question is asked");
    let waiting = f.host.waiting_blocks();
    assert_eq!(waiting.len(), 1);
    f.f.always("catalogue.record_answer", "PKG_CHOICE_NOT_ALLOWED", false);
    f.host
        .answer_block(
            f.ws,
            waiting[0].block_id,
            BlockAnswer::Choice {
                choice: "House".to_owned(),
            },
        )
        .await
        .unwrap();
    let sup = f.supervisor.clone();
    assert!(
        eventually(|| sup
            .turn_texts()
            .iter()
            .any(|t| t.ends_with("The choice could not be saved (PKG_CHOICE_NOT_ALLOWED).")))
        .await
    );
    assert!(f.supervisor.tasks.lock().unwrap().is_empty());
    assert_eq!(
        f.count("SELECT COUNT(*) FROM ordin_messages WHERE author = 'person' AND text = 'House'")
            .await,
        1
    );
}

// Memory, Knowledge, Package Catalogue, Push Service or Supervisor error in a tool.
#[tokio::test]
async fn oh_dependency_failed_in_a_tool() {
    let f = fixture().await;
    f.open().await;
    for (call, code, retryable, tool, args) in [
        (
            "catalogue.find_agents",
            "PKG_STORE_FAILED",
            true,
            "find_agent",
            json!({ "need": "a reader" }),
        ),
        (
            "memory.recall",
            "MEM_STORE_UNAVAILABLE",
            true,
            "recall",
            json!({ "query": "quotes" }),
        ),
        (
            "memory.remember_facts",
            "MEM_STORE_UNAVAILABLE",
            false,
            "remember",
            json!({ "source": "conversation", "facts": [{ "subject": { "name": "Quotes" }, "predicate": "endDate", "object": { "date": "2026-02-06" } }] }),
        ),
        (
            "knowledge.write_page",
            "KNOW_STORE_UNAVAILABLE",
            true,
            "write_page",
            json!({ "title": "Quotes", "body": "# Quotes", "sources": [], "description": "How quotes work." }),
        ),
        (
            "push.propose",
            "PUSH_STORE_FAILED",
            true,
            "propose_push",
            json!({ "request": "Send it", "items": [{ "item": ordin_types::WorkingCopyItemId::new(), "summary": "Quote" }] }),
        ),
    ] {
        f.f.always(call, code, retryable);
        let e = assert_tool(f.tool(tool, args).await, "OH_DEPENDENCY_FAILED", retryable);
        assert_eq!(e.message, "That did not work; try again.");
    }
}

// Memory before its one-time download (OH-DEP-2): Ordin is told it is not ready, not that it failed.
// A model that answered too late (the retryable form of the code) keeps OH-DEP-1's text.
// spec: ordin-desktop/components/OrdinHost#OH-DEP-2
#[tokio::test]
async fn oh_dependency_failed_memory_not_ready() {
    for (retryable, message) in [
        (false, crate::tools::MEMORY_NOT_READY),
        (true, "That did not work; try again."),
    ] {
        let f = fixture().await;
        f.open().await;
        f.f.always("memory.recall", "MEM_MODEL_UNAVAILABLE", retryable);
        let e = assert_tool(
            f.tool("recall", json!({ "query": "the strategy" })).await,
            "OH_DEPENDENCY_FAILED",
            retryable,
        );
        assert_eq!(e.message, message);
    }
    assert!(crate::tools::MEMORY_NOT_READY.starts_with("Memory is not ready yet"));
}

// `delegate` with `code_in` to a repository not linked to the workspace or not open as a project.
#[tokio::test]
async fn oh_input_invalid_code_in() {
    let f = fixture().await;
    f.open().await;
    let a = agent("Coder");
    f.inspector.agents.lock().unwrap().push(a.clone());
    let args = json!({ "agent": a.agent_instance_id.to_string(), "brief": "Fix it", "code_in": "acme/app" });
    let e = assert_tool(
        f.tool("delegate", args.clone()).await,
        "OH_INPUT_INVALID",
        false,
    );
    assert_eq!(
        e.message,
        "That repository is not linked to this workspace."
    );
    let place = ordin_types::PlaceId::new();
    f.workspaces.repos.lock().unwrap().push(LinkedRepo {
        host: ordin_types::domain::ShareHost::Github,
        repository: "acme/app".to_owned(),
        host_name: "github.com".to_owned(),
        place_id: place,
    });
    let e = assert_tool(
        f.tool("delegate", args.clone()).await,
        "OH_INPUT_INVALID",
        false,
    );
    assert_eq!(e.message, "That repository is not open as a project here.");
    f.projects
        .projects
        .lock()
        .unwrap()
        .insert(place, (ordin_types::ProjectId::new(), false));
    assert_tool(f.tool("delegate", args).await, "OH_INPUT_INVALID", false);
    assert!(f.supervisor.tasks.lock().unwrap().is_empty());
}

// Coding task cannot start: the Project Service's code.
#[tokio::test]
async fn coding_task_cannot_start() {
    let f = fixture().await;
    f.open().await;
    let a = agent("Coder");
    f.inspector.agents.lock().unwrap().push(a.clone());
    let place = ordin_types::PlaceId::new();
    f.workspaces.repos.lock().unwrap().push(LinkedRepo {
        host: ordin_types::domain::ShareHost::Github,
        repository: "acme/app".to_owned(),
        host_name: "github.com".to_owned(),
        place_id: place,
    });
    f.projects
        .projects
        .lock()
        .unwrap()
        .insert(place, (ordin_types::ProjectId::new(), true));
    f.f.always("projects.start_coding_task", "CODE_WORKTREE_FAILED", false);
    assert_tool(
        f.tool("delegate", json!({ "agent": a.agent_instance_id.to_string(), "brief": "Fix it", "code_in": "acme/app" })).await,
        "CODE_WORKTREE_FAILED",
        false,
    );
    assert!(f.supervisor.tasks.lock().unwrap().is_empty());
}

// Usage Planner unavailable for reuse or estimate: logged `warn`, the work goes on.
#[tokio::test]
async fn usage_planner_unavailable_work_goes_on() {
    let f = fixture().await;
    f.open().await;
    let a = agent("Reader");
    f.inspector.agents.lock().unwrap().push(a.clone());
    f.f.always("usage.find_reusable", "USAGE_TEST_REUSE_DOWN", true);
    f.f.always("usage.estimate", "USAGE_TEST_ESTIMATE_DOWN", true);
    let out = f
        .tool(
            "delegate",
            json!({ "agent": a.agent_instance_id.to_string(), "brief": "Read it" }),
        )
        .await
        .expect("the task starts");
    assert!(out.text.starts_with("Started"), "{}", out.text);
    let many: Vec<ArtefactId> = (0..21).map(|_| ArtefactId::new()).collect();
    f.tool(
        "delegate",
        json!({ "agent": a.agent_instance_id.to_string(), "brief": "Read all", "artefacts": many }),
    )
    .await
    .expect("the task starts without an estimate");
    assert_eq!(f.supervisor.tasks.lock().unwrap().len(), 2);
    assert!(!log_lines("USAGE_TEST_REUSE_DOWN").is_empty());
    assert!(log_lines("USAGE_TEST_REUSE_DOWN")
        .iter()
        .all(|l| l.contains("WARN") || l.contains("warn")));
    assert!(!log_lines("USAGE_TEST_ESTIMATE_DOWN").is_empty());
}

// `propose_workflow_run` with an empty or over-long request.
#[tokio::test]
async fn propose_workflow_run_message_invalid() {
    let f = fixture().await;
    f.open().await;
    for request in [String::new(), "   ".to_owned(), "r".repeat(4_001)] {
        let e = assert_tool(
            f.tool(
                "propose_workflow_run",
                json!({ "workflow_id": ordin_types::WorkflowId::new(), "request": request }),
            )
            .await,
            "OH_MESSAGE_INVALID",
            false,
        );
        assert_eq!(
            e.message,
            "Say in 1 to 4,000 characters what the run is for."
        );
    }
}

// `propose_workflow_run` for a workflow not saved in this workspace.
#[tokio::test]
async fn propose_workflow_run_workflow_unknown() {
    let f = fixture().await;
    f.open().await;
    let e = assert_tool(
        f.tool(
            "propose_workflow_run",
            json!({ "workflow_id": ordin_types::WorkflowId::new(), "request": "Run it" }),
        )
        .await,
        "OH_WORKFLOW_UNKNOWN",
        false,
    );
    assert_eq!(
        e.message,
        "There's no saved workflow with that id in this workspace."
    );
}

// ---------------------------------------------------------------- coordinator rows

// Ordin makes no move after one reminder.
#[tokio::test]
async fn oh_coordinator_no_move() {
    let f = fixture().await;
    f.open().await;
    let before = f.supervisor.turn_texts().len();
    let r = f
        .host
        .coordinator()
        .decide_condition(&view(f.ws), "Is it below budget", &paths())
        .await;
    assert!(
        matches!(r, Err(ordin_crew_orchestrator::CoordinatorError::Unusable(ref w)) if w == "no move"),
        "{r:?}"
    );
    let turns = f.supervisor.turn_texts();
    assert_eq!(turns.len() - before, 2, "the call and one reminder");
    assert!(turns
        .last()
        .unwrap()
        .ends_with("Reply with decide(kind: condition)."));
    assert!(!log_lines("OH_COORDINATOR_NO_MOVE").is_empty());
}

// Ordin's turn exceeds `coordinator_turn_timeout`.
#[tokio::test]
async fn oh_coordinator_timeout() {
    let mut c = config();
    c.coordinator_turn_timeout = std::time::Duration::from_millis(200);
    let f = fixture_with(c, policy()).await;
    f.open().await;
    f.supervisor.respond(silent());
    let r = f
        .host
        .coordinator()
        .check_result(
            &view(f.ws),
            &ordin_crew_orchestrator::StepOutcome {
                member: ordin_types::common::AgentRef::AgentInstance {
                    agent_instance_id: ordin_types::AgentInstanceId::new(),
                },
                label: "1".to_owned(),
                run_id: RunId::new(),
                state: ordin_types::domain::runs::RunState::Completed,
                summary: Some("Done".to_owned()),
                artefacts: vec![],
            },
        )
        .await;
    assert!(
        matches!(r, Err(ordin_crew_orchestrator::CoordinatorError::Unusable(ref w)) if w == "timeout"),
        "{r:?}"
    );
    assert!(!log_lines("OH_COORDINATOR_TIMEOUT").is_empty());
}

// Ordin's run not available during a coordinator call.
#[tokio::test]
async fn oh_coordinator_session_lost() {
    let f = fixture().await;
    let r = f.host.coordinator().round(&view(f.ws)).await;
    assert!(
        matches!(
            r,
            Err(ordin_crew_orchestrator::CoordinatorError::Unavailable)
        ),
        "{r:?}"
    );
    // The run ends while the call waits.
    let run = f.open().await;
    f.supervisor.respond(silent());
    let host = f.host.clone();
    let ws = f.ws;
    let waiting = tokio::spawn(async move {
        host.coordinator()
            .decide_condition(&view(ws), "Is it", &paths())
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    f.supervisor.end(run);
    let r = waiting.await.unwrap();
    assert!(
        matches!(
            r,
            Err(ordin_crew_orchestrator::CoordinatorError::Unavailable)
        ),
        "{r:?}"
    );
}

// Crew stopped while a coordinator call waits.
#[tokio::test]
async fn crew_stopped_while_coordinator_waits() {
    let f = fixture().await;
    f.open().await;
    f.supervisor.respond(silent());
    let v = view(f.ws);
    let cancel = v.cancel.clone();
    let host = f.host.clone();
    let waiting = tokio::spawn(async move { host.coordinator().round(&v).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.cancel();
    let r = waiting.await.unwrap();
    assert!(
        matches!(
            r,
            Err(ordin_crew_orchestrator::CoordinatorError::StoppedByPerson)
        ),
        "{r:?}"
    );
}

// ---------------------------------------------------------------- workflow helpers

use ordin_types::wire::workflow_file::{
    AgentRepositoryRef, IfItFails, WorkflowFileAgent, WorkflowFileInput, WorkflowFileLimits,
    WorkflowFileStep, WorkflowInputKind,
};
use ordin_types::wire::WorkflowFile;
use ordin_types::WorkflowId;

const BUYER_ID: &str = "urn:ordin:agent:buyer";

fn file3(name: &str) -> WorkflowFile {
    WorkflowFile {
        format: 3,
        name: name.to_owned(),
        purpose: "Compare supplier quotes".to_owned(),
        inputs: vec![],
        agents: vec![WorkflowFileAgent {
            key: "buyer".to_owned(),
            repository: AgentRepositoryRef::ProfileKey("company".to_owned()),
            id: BUYER_ID.to_owned(),
            version: "^1.0".to_owned(),
            name: "Buyer".to_owned(),
            role: "Finds quotes".to_owned(),
        }],
        steps: vec![WorkflowFileStep::Agent {
            position: "1".to_owned(),
            agent: "buyer".to_owned(),
            line: "Find three quotes".to_owned(),
            uses: vec![],
            asks_first: false,
            ordin_checks: false,
            if_it_fails: IfItFails::OrdinTakesOver,
        }],
        limits: WorkflowFileLimits {
            max_rounds: 10,
            wall_clock_minutes: 30,
            request_budget: None,
            agents_at_once: 2,
            failure_repeat_limit: 3,
            external_action_agents: vec![],
        },
        origin: String::new(),
    }
}

/// Installs the Buyer agent the workflow files name.
fn install_buyer(f: &Fixture) -> DirectoryAgent {
    let a = agent("Buyer");
    f.inspector.agents.lock().unwrap().push(a.clone());
    f.catalogue.install(&a, BUYER_ID, "1.0.0");
    a
}

async fn import(f: &Fixture, file: &WorkflowFile) -> (WorkflowId, crate::WorkflowCheckReport) {
    f.host
        .workflows_import(
            f.ws,
            "quotes.workflow.yaml".to_owned(),
            crate::wff::to_yaml(file),
        )
        .await
        .expect("the file is imported")
}

/// A workflow saved once (version 1).
async fn saved(f: &Fixture, file: WorkflowFile) -> WorkflowId {
    let (id, _) = import(f, &file).await;
    let stored = f.host.workflows_get(id, None).await.unwrap().file;
    f.host
        .workflows_save(id, None, stored, "First".to_owned())
        .await
        .expect("saved");
    id
}

fn start_req(
    f: &Fixture,
    workflow_id: WorkflowId,
    inputs: Vec<crate::types::RunInput>,
) -> crate::types::StartWorkflowRun {
    crate::types::StartWorkflowRun {
        workspace_id: f.ws,
        workflow_id,
        version: 1,
        inputs,
        started_by: crate::types::RunStartedBy::Person {
            channel: "chat".to_owned(),
        },
    }
}

// ---------------------------------------------------------------- workflow rows

// Workflow unknown.
#[tokio::test]
async fn oh_workflow_unknown() {
    let f = fixture().await;
    for e in [
        f.host
            .workflows_get(WorkflowId::new(), None)
            .await
            .unwrap_err(),
        f.host
            .workflows_remove(WorkflowId::new(), true)
            .await
            .unwrap_err(),
        f.host
            .workflows_run(WorkflowId::new(), None, None)
            .await
            .unwrap_err(),
    ] {
        assert_eq!(e.code(), "OH_WORKFLOW_UNKNOWN");
        assert!(!e.retryable());
    }
}

// Version unknown.
#[tokio::test]
async fn oh_version_unknown() {
    let f = fixture().await;
    install_buyer(&f);
    let id = saved(&f, file3("Quotes")).await;
    for e in [
        f.host.workflows_get(id, Some(9)).await.unwrap_err(),
        f.host.workflows_export(id, 9).await.unwrap_err(),
        f.host.workflows_restore(id, 9).await.unwrap_err(),
    ] {
        assert_eq!(e.code(), "OH_VERSION_UNKNOWN");
        assert!(!e.retryable());
    }
}

// Save on an older base version.
#[tokio::test]
async fn oh_stale_version() {
    let f = fixture().await;
    install_buyer(&f);
    let id = saved(&f, file3("Quotes")).await;
    f.host
        .workflows_save(id, Some(1), file3("Quotes"), "Second".to_owned())
        .await
        .unwrap();
    let e = f
        .host
        .workflows_save(id, Some(1), file3("Quotes"), "Third".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_STALE_VERSION");
    assert!(matches!(e, OhError::StaleVersion { latest: 2 }));
    assert!(!e.retryable());
    assert_eq!(
        e.message(),
        "This workflow changed since you opened it; reopen it to see the latest."
    );
}

// Name taken in the workspace.
#[tokio::test]
async fn wff_name_duplicate() {
    let f = fixture().await;
    install_buyer(&f);
    saved(&f, file3("Quotes")).await;
    let other = saved(&f, file3("Invoices")).await;
    let e = f
        .host
        .workflows_rename(other, "Quotes".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "WFF_NAME_DUPLICATE");
    assert!(!e.retryable());
    let e = f
        .host
        .workflows_save(other, Some(1), file3("Quotes"), "Rename".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "WFF_NAME_DUPLICATE");
}

// Check finds problems on save.
#[tokio::test]
async fn oh_check_failed() {
    let f = fixture().await;
    install_buyer(&f);
    let id = saved(&f, file3("Quotes")).await;
    let mut bad = file3("Quotes");
    bad.purpose = String::new();
    let e = f
        .host
        .workflows_save(id, Some(1), bad, "Broken".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CHECK_FAILED");
    assert!(!e.retryable());
    let OhError::CheckFailed { report } = e else {
        unreachable!()
    };
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.code == "WFF_PURPOSE_INVALID"),
        "{report:?}"
    );
}

// Agent missing at Save: inside the report, with the agent's name.
#[tokio::test]
async fn oh_agent_missing() {
    let f = fixture().await;
    install_buyer(&f);
    let id = saved(&f, file3("Quotes")).await;
    f.catalogue.matches.lock().unwrap().clear();
    let e = f
        .host
        .workflows_save(id, Some(1), file3("Quotes"), "Again".to_owned())
        .await
        .unwrap_err();
    let OhError::CheckFailed { report } = e else {
        panic!("{e:?}")
    };
    let p = report
        .problems
        .iter()
        .find(|p| p.code == "OH_AGENT_MISSING")
        .expect("OH_AGENT_MISSING");
    assert_eq!(p.args.get("name").map(String::as_str), Some("Buyer"));
    assert_eq!(p.at, "/agents/0");
}

// Restore of the latest version.
#[tokio::test]
async fn oh_restore_same() {
    let f = fixture().await;
    install_buyer(&f);
    let id = saved(&f, file3("Quotes")).await;
    let e = f.host.workflows_restore(id, 1).await.unwrap_err();
    assert_eq!(e.code(), "OH_RESTORE_SAME");
    assert!(!e.retryable());
}

// Remove not confirmed.
#[tokio::test]
async fn oh_remove_not_confirmed() {
    let f = fixture().await;
    install_buyer(&f);
    let id = saved(&f, file3("Quotes")).await;
    let e = f.host.workflows_remove(id, false).await.unwrap_err();
    assert_eq!(e.code(), "OH_REMOVE_NOT_CONFIRMED");
    assert!(!e.retryable());
    assert!(f.host.workflows_get(id, None).await.is_ok());
}

// Import text over 256 KB.
#[tokio::test]
async fn wff_file_too_large() {
    let f = fixture().await;
    let yaml = format!(
        "format: 3\nname: Big\npurpose: \"{}\"\n",
        "x".repeat(300 * 1024)
    );
    let e = f
        .host
        .workflows_import(f.ws, "big.workflow.yaml".to_owned(), yaml)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "WFF_FILE_TOO_LARGE");
    assert!(!e.retryable());
    assert_eq!(e.message(), "This file isn't a workflow ordin can read.");
}

// Import file name empty, over 255 characters or holding a path separator.
#[tokio::test]
async fn oh_input_invalid_import_file_name() {
    let f = fixture().await;
    let yaml = crate::wff::to_yaml(&file3("Quotes"));
    for name in [
        String::new(),
        "n".repeat(256),
        "a/b.yaml".to_owned(),
        "a\\b.yaml".to_owned(),
    ] {
        let e = f
            .host
            .workflows_import(f.ws, name, yaml.clone())
            .await
            .unwrap_err();
        assert_eq!(e.code(), "OH_INPUT_INVALID");
        assert!(!e.retryable());
    }
}

// Import file whose `format` is missing, not an integer or below 1.
#[tokio::test]
async fn wff_format_unsupported() {
    let f = fixture().await;
    for yaml in [
        "name: Quotes\npurpose: x\n",
        "format: three\nname: Quotes\n",
        "format: 0\nname: Quotes\n",
    ] {
        let e = f
            .host
            .workflows_import(f.ws, "q.workflow.yaml".to_owned(), yaml.to_owned())
            .await
            .unwrap_err();
        assert_eq!(e.code(), "WFF_FORMAT_UNSUPPORTED", "{yaml}");
        assert_eq!(e.message(), "This file isn't a workflow ordin can read.");
    }
}

// Import file malformed.
#[tokio::test]
async fn wff_file_malformed() {
    let f = fixture().await;
    for yaml in [
        "format: 3\nname: [unclosed\n",
        "format: 3\nname: &a Quotes\npurpose: *a\n",
        "format: 3\nname: Quotes\n",
    ] {
        let e = f
            .host
            .workflows_import(f.ws, "q.workflow.yaml".to_owned(), yaml.to_owned())
            .await
            .unwrap_err();
        assert_eq!(e.code(), "WFF_FILE_MALFORMED", "{yaml}");
        assert!(!e.retryable());
        assert_eq!(e.message(), "This file isn't a workflow ordin can read.");
    }
}

// Import file of a newer format.
#[tokio::test]
async fn wff_format_newer() {
    let f = fixture().await;
    let mut file = file3("Quotes");
    file.format = 4;
    let e = f
        .host
        .workflows_import(
            f.ws,
            "q.workflow.yaml".to_owned(),
            crate::wff::to_yaml(&file),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "WFF_FORMAT_NEWER");
    assert_eq!(
        e.message(),
        "This workflow was made by a newer ordin. Update ordin to import it."
    );
}

// Format 1 or 2 file naming an agent not in the workspace: imported; problem shown; Save blocked.
#[tokio::test]
async fn wff_agent_unmatched() {
    let f = fixture().await;
    let yaml = "format_version: 1\nname: Old quotes\npurpose: Compare quotes\ninputs: []\nagents:\n  - name: Ghost\n    version: \"^1.0\"\n    role: Finds quotes\nsteps:\n  - position: \"1\"\n    agent: Ghost\n    line: Find quotes\n    uses: []\n    asks_first: false\n    ordin_checks: false\nlimits:\n  max_rounds: 10\n  wall_clock_minutes: 30\n  agents_at_once: 2\n  failure_repeat_limit: 3\n  external_action_agents: []\norigin: \"\"\n";
    let (id, report) = f
        .host
        .workflows_import(f.ws, "old.workflow.yaml".to_owned(), yaml.to_owned())
        .await
        .expect("imported");
    let p = report
        .problems
        .iter()
        .find(|p| p.code == "WFF_AGENT_UNMATCHED")
        .expect("WFF_AGENT_UNMATCHED");
    assert_eq!(p.args.get("name").map(String::as_str), Some("Ghost"));
    let draft = f.host.workflows_get(id, None).await.unwrap().file;
    let e = f
        .host
        .workflows_save(id, None, draft, "Keep".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CHECK_FAILED");
}

// Any other 2C `WFF_…` row: inside the `WorkflowCheckReport` with the 2C code.
#[tokio::test]
async fn other_wff_rows_inside_the_report() {
    let f = fixture().await;
    install_buyer(&f);
    type Break = Box<dyn Fn(&mut WorkflowFile)>;
    let cases: Vec<(&str, Break)> = vec![
        ("WFF_NAME_INVALID", Box::new(|w| w.name = String::new())),
        (
            "WFF_PURPOSE_INVALID",
            Box::new(|w| w.purpose = "p".repeat(201)),
        ),
        (
            "WFF_AGENT_KEY_INVALID",
            Box::new(|w| w.agents[0].key = "Bad Key".to_owned()),
        ),
        (
            "WFF_AGENT_ID_INVALID",
            Box::new(|w| w.agents[0].id = "agent:buyer".to_owned()),
        ),
        (
            "WFF_AGENT_VERSION_INVALID",
            Box::new(|w| w.agents[0].version = "not a version".to_owned()),
        ),
        (
            "WFF_LIMIT_INVALID",
            Box::new(|w| w.limits.failure_repeat_limit = 9),
        ),
        (
            "WFF_LINE_INVALID",
            Box::new(|w| {
                if let WorkflowFileStep::Agent { line, .. } = &mut w.steps[0] {
                    line.clear();
                }
            }),
        ),
        (
            "WFF_AGENT_KEY_UNKNOWN",
            Box::new(|w| {
                if let WorkflowFileStep::Agent { agent, .. } = &mut w.steps[0] {
                    *agent = "nobody".to_owned();
                }
            }),
        ),
        (
            "WFF_POSITION_INVALID",
            Box::new(|w| {
                if let WorkflowFileStep::Agent { position, .. } = &mut w.steps[0] {
                    *position = "x".to_owned();
                }
            }),
        ),
    ];
    for (code, change) in cases {
        let mut file = file3("Quotes");
        change(&mut file);
        let report = f.host.workflows_check(f.ws, file).await.unwrap();
        assert!(
            report.problems.iter().any(|p| p.code == code),
            "{code}: {report:?}"
        );
    }
}

// Limit above the company's maximum: a notice, the limit lowered.
#[tokio::test]
async fn wff_limit_lowered() {
    let f = fixture().await;
    install_buyer(&f);
    let mut file = file3("Quotes");
    file.limits.max_rounds = 30;
    let (id, report) = import(&f, &file).await;
    let n = report
        .notices
        .iter()
        .find(|p| p.code == "WFF_LIMIT_LOWERED")
        .expect("notice");
    assert_eq!(n.args.get("asked").map(String::as_str), Some("30"));
    assert_eq!(n.args.get("maximum").map(String::as_str), Some("20"));
    assert!(report.problems.is_empty(), "{report:?}");
    assert_eq!(
        f.host
            .workflows_get(id, None)
            .await
            .unwrap()
            .file
            .limits
            .max_rounds,
        20
    );
}

// Name already used on import: a notice, the name suffixed.
#[tokio::test]
async fn oh_name_suffixed() {
    let f = fixture().await;
    install_buyer(&f);
    import(&f, &file3("Quotes")).await;
    let (id, report) = import(&f, &file3("Quotes")).await;
    let n = report
        .notices
        .iter()
        .find(|p| p.code == "OH_NAME_SUFFIXED")
        .expect("notice");
    assert_eq!(
        n.args.get("new_name").map(String::as_str),
        Some("Quotes (2)")
    );
    assert_eq!(
        f.host.workflows_get(id, None).await.unwrap().workflow.name,
        "Quotes (2)"
    );
}

// Share to a folder not linked.
#[tokio::test]
async fn oh_folder_not_linked() {
    let f = fixture().await;
    install_buyer(&f);
    let id = saved(&f, file3("Quotes")).await;
    let folder = crate::RepositoryFolder {
        host: ordin_types::domain::ShareHost::Github,
        repository: "acme/flows".to_owned(),
        folder: String::new(),
    };
    let e = f.host.workflows_share(id, 1, folder).await.unwrap_err();
    assert_eq!(e.code(), "OH_FOLDER_NOT_LINKED");
    assert!(!e.retryable());
    assert!(f.push.pulls.lock().unwrap().is_empty());
}

// Repository folder unreadable: partial reply; log `warn`.
#[tokio::test]
async fn oh_repository_unreachable() {
    let mut p = policy();
    p.company_workflow_repository = Some(crate::RepositoryFolder {
        host: ordin_types::domain::ShareHost::Gitlab,
        repository: "acme/unreachable-flows".to_owned(),
        folder: "flows".to_owned(),
    });
    let f = fixture_with(config(), p).await;
    f.f.always("projects.list_files", "PROJ_TEST_UNREACHABLE", true);
    let files = f
        .host
        .workflows_list_in_repositories(f.ws)
        .await
        .expect("a partial reply");
    assert!(files.is_empty());
    let lines = log_lines("PROJ_TEST_UNREACHABLE");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("ordin_host.repository_unreachable")
                && l.contains("acme/unreachable-flows")),
        "{lines:?}"
    );
}

// ---------------------------------------------------------------- crews, immediate start, dashboards, authoring

// Charter unknown or not this workspace's.
#[tokio::test]
async fn oh_charter_unknown() {
    let f = fixture().await;
    f.open().await;
    let e = f
        .host
        .crews_start(ordin_types::CrewCharterId::new())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CHARTER_UNKNOWN");
    assert!(!e.retryable());
    let e = f
        .host
        .crews_discard_plan(ordin_types::CrewCharterId::new())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CHARTER_UNKNOWN");
    assert_tool(
        f.tool("show", json!({ "block": { "kind": "crew_plan", "crew_charter_id": ordin_types::CrewCharterId::new() } })).await,
        "OH_CHARTER_UNKNOWN",
        false,
    );
}

// Start pressed on a charter not `valid`.
#[tokio::test]
async fn chrt_not_started_by_person() {
    let f = fixture().await;
    f.open().await;
    let a = agent("Buyer");
    f.inspector.agents.lock().unwrap().push(a.clone());
    f.orchestrator.plan.lock().unwrap().fits = false;
    let out = f
        .tool("create_crew", json!({ "purpose": "Compare quotes", "members": [{ "agent": a.agent_instance_id.to_string(), "role": "buys" }] }))
        .await
        .unwrap();
    let id: ordin_types::CrewCharterId =
        serde_json::from_value(out.structured.unwrap()["crew_charter_id"].clone()).unwrap();
    let e = f.host.crews_start(id).await.unwrap_err();
    assert_eq!(e.code(), "CHRT_NOT_STARTED_BY_PERSON");
    assert!(!e.retryable());
    assert!(f.orchestrator.started.lock().unwrap().is_empty());
}

// Immediate start: a place-file input cannot be read.
#[tokio::test]
async fn oh_input_unreadable() {
    let f = fixture().await;
    install_buyer(&f);
    let mut file = file3("Quotes");
    file.inputs = vec![WorkflowFileInput {
        label: "Brief".to_owned(),
        kind: WorkflowInputKind::Document,
        required: true,
    }];
    let id = saved(&f, file).await;
    f.f.always("files.read_file", "FILE_NOT_FOUND", false);
    let input = crate::types::RunInput {
        label: "Brief".to_owned(),
        value: crate::types::RunInputValue::Document {
            content: ContentRef::PlaceFile {
                place_id: ordin_types::PlaceId::new(),
                relative_path: "brief.md".to_owned(),
            },
        },
    };
    let e = f
        .host
        .start_workflow_run(start_req(&f, id, vec![input]))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_INPUT_UNREADABLE");
    assert!(matches!(&e, OhError::InputUnreadable { label } if label == "Brief"));
    assert!(!e.retryable());
    assert!(f.orchestrator.started.lock().unwrap().is_empty());
}

// Dashboard unknown.
#[tokio::test]
async fn oh_dashboard_unknown() {
    let f = fixture().await;
    let e = f
        .host
        .dashboards_refresh(f.ws, ordin_types::DashboardId::new())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DASHBOARD_UNKNOWN");
    assert!(!e.retryable());
}

// Immediate start of a workflow that needs agents.
#[tokio::test]
async fn oh_needs_agents() {
    let f = fixture().await;
    // The agent is missing: the import's first save keeps the workflow as needing agents.
    let id = saved(&f, file3("Quotes")).await;
    let e = f
        .host
        .start_workflow_run(start_req(&f, id, vec![]))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_NEEDS_AGENTS");
    assert!(!e.retryable());
}

// Immediate start without a required input.
#[tokio::test]
async fn oh_input_missing() {
    let f = fixture().await;
    install_buyer(&f);
    let mut file = file3("Quotes");
    file.inputs = vec![WorkflowFileInput {
        label: "Budget".to_owned(),
        kind: WorkflowInputKind::Text,
        required: true,
    }];
    let id = saved(&f, file).await;
    let e = f
        .host
        .start_workflow_run(start_req(&f, id, vec![]))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_INPUT_MISSING");
    assert!(matches!(&e, OhError::InputMissing { label } if label == "Budget"));
    assert!(!e.retryable());
}

// Crew run unknown for Run again.
#[tokio::test]
async fn oh_crew_run_unknown() {
    let f = fixture().await;
    let e = f
        .host
        .run_again_inputs(ordin_types::CrewRunId::new())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CREW_RUN_UNKNOWN");
    assert!(!e.retryable());
}

// Generation template not known: no run starts.
#[tokio::test]
async fn oh_template_unknown() {
    use crate::types::AuthoringHost;
    let f = fixture().await;
    let e = f
        .host
        .generate(crate::types::GenerationRequest {
            template_id: "no-such-template",
            data: json!({}),
            workspace_id: f.ws,
        })
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_TEMPLATE_UNKNOWN");
    assert!(!e.retryable());
    assert!(f.supervisor.tasks.lock().unwrap().is_empty());
}

// ---------------------------------------------------------------- continue rows (ADR-108)

// Continue: conversation unknown or of another workspace.
#[tokio::test]
async fn continue_conversation_unknown() {
    let f = fixture().await;
    let earlier = ended_conversation(&f).await;
    let e = f
        .host
        .continue_conversation(f.ws, ConversationId::new(), vec![])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CONVERSATION_UNKNOWN");
    assert!(!e.retryable());
    assert_eq!(e.message(), "That conversation isn't here any more.");
    let e = f
        .host
        .continue_conversation(WorkspaceId::new(), earlier, vec![])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CONVERSATION_UNKNOWN");
}

// Continue: the current conversation, or one already continued.
#[tokio::test]
async fn oh_conversation_not_continuable() {
    let f = fixture().await;
    let earlier = ended_conversation(&f).await;
    let current = crate::host::lock(&f.host.state)
        .convs
        .get(&f.ws)
        .unwrap()
        .id;
    let e = f
        .host
        .continue_conversation(f.ws, current, vec![])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CONVERSATION_NOT_CONTINUABLE");
    assert!(!e.retryable());
    assert_eq!(
        e.message(),
        "This conversation was continued later; continue the later one."
    );
    f.host
        .continue_conversation(f.ws, earlier, vec![])
        .await
        .expect("continued once");
    let e = f
        .host
        .continue_conversation(f.ws, earlier, vec![])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CONVERSATION_NOT_CONTINUABLE");
}

// Continue while a crew Ordin coordinates runs.
#[tokio::test]
async fn continue_while_crew_running() {
    let f = fixture().await;
    let earlier = ended_conversation(&f).await;
    *f.orchestrator.running.lock().unwrap() = Some(ordin_types::CrewRunId::new());
    let e = f
        .host
        .continue_conversation(f.ws, earlier, vec![])
        .await
        .unwrap_err();
    // The Error Table words this row "…before continuing another conversation."; the sentence comes
    // from Localisation by code, so only the code is this crate's.
    assert_eq!(e.code(), "OH_CREW_RUNNING");
    assert!(!e.retryable());
    assert_eq!(
        f.count("SELECT COUNT(*) FROM ordin_conversations WHERE continues_id IS NOT NULL")
            .await,
        0
    );
}

// Continue: more than 10 kept points, or a point empty or over 500 characters.
#[tokio::test]
async fn continue_kept_points_invalid() {
    let f = fixture().await;
    let earlier = ended_conversation(&f).await;
    for points in [
        (0..11).map(|i| format!("Point {i}")).collect::<Vec<_>>(),
        vec!["  ".to_owned()],
        vec!["p".repeat(501)],
    ] {
        let e = f
            .host
            .continue_conversation(f.ws, earlier, points)
            .await
            .unwrap_err();
        assert_eq!(e.code(), "OH_INPUT_INVALID");
        assert!(matches!(&e, OhError::InputInvalid { at, .. } if at == "kept_points"));
        assert!(!e.retryable());
    }
    assert!(f.memory.remembered.lock().unwrap().is_empty());
}

// Continue: plan paused or Ordin's package not ready. A kept point is no longer stored by the host
// (OH-TR-1), so the memory row of this test is gone.
#[tokio::test]
async fn continue_dependency_failed() {
    for (call, code, retryable) in [
        ("usage.may_start_ordin_work", "USAGE_PLAN_PAUSED", true),
        ("catalogue.ensure_ready", "PKG_INSTANCE_NOT_READY", false),
    ] {
        let f = fixture().await;
        let earlier = ended_conversation(&f).await;
        f.f.always(call, code, retryable);
        let e = f
            .host
            .continue_conversation(f.ws, earlier, vec!["Budget is 10k".to_owned()])
            .await
            .unwrap_err();
        assert_eq!(e.code(), "OH_DEPENDENCY_FAILED", "{call}");
        assert!(matches!(&e, OhError::DependencyFailed { code: c, .. } if c == code));
        assert_eq!(e.retryable(), retryable, "{call}");
        assert_eq!(e.message(), "That did not work; try again.");
        assert_eq!(CodedError::repair(&e), Some(RepairAction::TryAgain));
        assert_eq!(
            f.count("SELECT COUNT(*) FROM ordin_conversations WHERE continues_id IS NOT NULL")
                .await,
            0
        );
    }
}

// Continue: the new session cannot start; nothing changes state.
#[tokio::test]
async fn continue_session_start_failed() {
    let f = fixture().await;
    let earlier = ended_conversation(&f).await;
    let current = crate::host::lock(&f.host.state)
        .convs
        .get(&f.ws)
        .unwrap()
        .id;
    f.f.always("supervisor.start_conversation", "SUP_START_FAILED", true);
    let e = f
        .host
        .continue_conversation(f.ws, earlier, vec![])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_SESSION_START_FAILED");
    assert!(e.retryable());
    assert_eq!(e.message(), "Ordin couldn't start.");
    let still: String =
        sqlx::query_scalar("SELECT id FROM ordin_conversations WHERE state = 'current'")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(still, current.to_string());
    assert_eq!(f.count("SELECT COUNT(*) FROM ordin_conversations").await, 2);
}

// Continue: datastore unavailable at the switch; the new run is cancelled.
#[tokio::test]
async fn continue_datastore_unavailable_at_switch() {
    let f = fixture().await;
    let earlier = ended_conversation(&f).await;
    sqlx::query("CREATE TRIGGER refuse_insert BEFORE INSERT ON ordin_conversations BEGIN SELECT RAISE(ABORT, 'refused'); END")
        .execute(&f.pool)
        .await
        .unwrap();
    let e = f
        .host
        .continue_conversation(f.ws, earlier, vec![])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DATASTORE_UNAVAILABLE");
    assert!(e.retryable());
    assert_eq!(e.message(), "Something went wrong; try again.");
    let new_run = f.supervisor.starts.lock().unwrap().len();
    assert_eq!(new_run, 3, "the first, the fresh and the continuing run");
    let cancelled = f.supervisor.cancelled.lock().unwrap().clone();
    let runs: Vec<RunId> = f.supervisor.runs.lock().unwrap().keys().copied().collect();
    assert!(runs.iter().any(|r| cancelled.contains(r)));
    assert_eq!(
        f.count("SELECT COUNT(*) FROM ordin_conversations WHERE continues_id IS NOT NULL")
            .await,
        0
    );
}

// Continue: journal entry not written; logged `error`, the switch stands.
#[tokio::test]
async fn continue_journal_entry_not_written() {
    let f = fixture().await;
    let earlier = ended_conversation(&f).await;
    f.f.always("journal.append_entry", "JRNL_TEST_CONTINUE_FAILED", true);
    let id = f
        .host
        .continue_conversation(f.ws, earlier, vec![])
        .await
        .expect("the switch stands");
    assert_eq!(
        crate::host::lock(&f.host.state)
            .convs
            .get(&f.ws)
            .unwrap()
            .id,
        id
    );
    let lines = log_lines("JRNL_TEST_CONTINUE_FAILED");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("ordin_host.journal_failed") && l.contains("ERROR")),
        "{lines:?}"
    );
}

// ---------------------------------------------------------------- Slack rows (ADR-102)

fn kept(n: usize) -> Vec<ordin_types::KeptSlackMessageId> {
    (0..n)
        .map(|_| ordin_types::KeptSlackMessageId::new())
        .collect()
}

// `react_to_slack` with 0 or more than 50 message ids.
#[tokio::test]
async fn react_to_slack_input_invalid() {
    let f = fixture().await;
    for n in [0, 51] {
        let e = f
            .host
            .react_to_slack(
                f.ws,
                ordin_types::SlackLinkId::new(),
                "general".to_owned(),
                kept(n),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code(), "OH_INPUT_INVALID");
        assert!(matches!(&e, OhError::InputInvalid { at, .. } if at == "kept"));
        assert!(!e.retryable());
    }
}

// `react_to_slack` while a crew Ordin coordinates runs in the workspace.
#[tokio::test]
async fn react_to_slack_crew_running() {
    let f = fixture().await;
    *f.orchestrator.running.lock().unwrap() = Some(ordin_types::CrewRunId::new());
    let e = f
        .host
        .react_to_slack(
            f.ws,
            ordin_types::SlackLinkId::new(),
            "general".to_owned(),
            kept(2),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CREW_RUNNING");
    // Permanent, as the row says (OrdinHost.md step 46.3).
    assert!(!e.retryable());
    assert!(f.supervisor.turn_texts().is_empty());
}

// `react_to_slack` while the plan is paused.
#[tokio::test]
async fn react_to_slack_plan_paused() {
    let f = fixture().await;
    f.f.always("usage.may_start_ordin_work", "USAGE_PLAN_PAUSED", false);
    let e = f
        .host
        .react_to_slack(
            f.ws,
            ordin_types::SlackLinkId::new(),
            "general".to_owned(),
            kept(2),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DEPENDENCY_FAILED");
    assert!(matches!(
        &e,
        OhError::DependencyFailed {
            dependency: "usage",
            ..
        }
    ));
    assert!(e.retryable());
}

// ---------------------------------------------------------------- language rows (ADR-109)

// `change_setting` called inside a Slack turn.
#[tokio::test]
async fn oh_not_persons_words() {
    let f = fixture().await;
    f.open().await;
    let seen = calls_tool(
        &f,
        "SlackTurn",
        "change_setting",
        json!({ "setting": "tone", "value": "warm" }),
    );
    f.host
        .react_to_slack(
            f.ws,
            ordin_types::SlackLinkId::new(),
            "general".to_owned(),
            kept(1),
        )
        .await
        .unwrap();
    let s = seen.clone();
    assert!(eventually(|| !s.lock().unwrap().is_empty()).await);
    let results = seen.lock().unwrap();
    let e = results[0].as_ref().unwrap_err();
    assert_eq!(e.code, "OH_NOT_PERSONS_WORDS");
    assert!(!e.retryable);
    assert_eq!(e.message, "Settings change only when the person asks.");
}

// `change_setting` called in a turn the person's own message did not start: here a crew round,
// whose member results are quoted data (step 52.1, FU-120).
#[tokio::test]
async fn oh_not_persons_words_in_a_crew_round() {
    let f = fixture().await;
    f.open().await;
    let before = f.host.voice_settings().await.unwrap();
    let seen = calls_tool(
        &f,
        "Crew round",
        "change_setting",
        json!({ "setting": "tone", "value": "direct" }),
    );
    let host = f.host.clone();
    let ws = f.ws;
    let round = tokio::spawn(async move { host.coordinator().round(&view(ws)).await });
    let s = seen.clone();
    assert!(eventually(|| !s.lock().unwrap().is_empty()).await);
    round.abort();
    let e = seen.lock().unwrap()[0].clone().unwrap_err();
    assert_eq!(e.code, "OH_NOT_PERSONS_WORDS");
    assert!(!e.retryable);
    assert_eq!(e.message, "Settings change only when the person asks.");
    let after = f.host.voice_settings().await.unwrap();
    assert_eq!(after.tone.value, before.tone.value, "the tone is unchanged");
}

// `change_setting` in the turn of the person's own message is carried out (step 52.1).
#[tokio::test]
async fn change_setting_in_the_persons_turn_is_carried_out() {
    let f = fixture().await;
    f.open().await;
    let seen = calls_tool(
        &f,
        "be more direct",
        "change_setting",
        json!({ "setting": "tone", "value": "direct" }),
    );
    f.host
        .send_message(f.ws, "Please be more direct".to_owned(), vec![])
        .await
        .unwrap();
    let s = seen.clone();
    assert!(eventually(|| !s.lock().unwrap().is_empty()).await);
    let result = seen.lock().unwrap()[0].clone();
    assert!(result.is_ok(), "{result:?}");
}

// `take_over` of a crew of another workspace is answered as an unknown crew, and the Crew
// Orchestrator is not asked (step 13, FU-120).
#[tokio::test]
async fn take_over_of_another_workspaces_crew_is_refused() {
    let f = fixture().await;
    f.open().await;
    let other = ordin_types::CrewRunId::new();
    f.orchestrator
        .states
        .lock()
        .unwrap()
        .insert(other, ordin_types::domain::runs::CrewRunState::Running);
    let e = assert_tool(
        f.tool(
            "take_over",
            json!({ "crew_run_id": other, "from_position": "3", "reason": "The steps do not fit." }),
        )
        .await,
        "CREW_NOT_FOUND",
        false,
    );
    assert!(!e.message.is_empty());
    assert!(
        f.orchestrator.take_overs.lock().unwrap().is_empty(),
        "the orchestrator is not asked"
    );
}

// `change_setting` on a voice field the company sets, or a name over 60 characters.
#[tokio::test]
async fn change_setting_voice_refused() {
    let mut p = policy();
    p.voice_defaults.tone = Some(ordin_types::domain::VoiceTone::Direct);
    let f = fixture_with(config(), p).await;
    f.open().await;
    persons_turn(&f).await;
    let e = assert_tool(
        f.tool(
            "change_setting",
            json!({ "setting": "tone", "value": "warm" }),
        )
        .await,
        "VOICE_FIXED_BY_COMPANY",
        false,
    );
    assert_eq!(e.message, "Your company sets this.");
    let e = assert_tool(
        f.tool(
            "change_setting",
            json!({ "setting": "name_to_use", "value": "n".repeat(61) }),
        )
        .await,
        "VOICE_NAME_TOO_LONG",
        false,
    );
    assert_eq!(e.message, "Use a name of at most 60 characters.");
}

// `change_setting` to a shipped language the company does not offer.
#[tokio::test]
async fn loc_not_offered() {
    let f = fixture().await;
    f.open().await;
    persons_turn(&f).await;
    f.localisation.matches.lock().unwrap().insert(
        "de".to_owned(),
        crate::ports::Match::NotOffered("de".to_owned()),
    );
    assert_tool(
        f.tool(
            "change_setting",
            json!({ "setting": "interface_language", "locale": "de", "name": "German" }),
        )
        .await,
        "LOC_NOT_OFFERED",
        false,
    );
    assert!(f.localisation.set.lock().unwrap().is_empty());
}

// `change_setting` with a tag that is not BCP 47, or `set_locale` fails.
#[tokio::test]
async fn loc_tag_invalid_or_store_unavailable() {
    let f = fixture().await;
    f.open().await;
    persons_turn(&f).await;
    f.f.once("localisation.match_language", "LOC_TAG_INVALID", false);
    assert_tool(
        f.tool(
            "change_setting",
            json!({ "setting": "interface_language", "locale": "zz_!", "name": "Nonsense" }),
        )
        .await,
        "LOC_TAG_INVALID",
        false,
    );
    f.f.always("localisation.set_locale", "LOC_STORE_UNAVAILABLE", true);
    assert_tool(
        f.tool(
            "change_setting",
            json!({ "setting": "interface_language", "locale": "fr", "name": "French" }),
        )
        .await,
        "LOC_STORE_UNAVAILABLE",
        true,
    );
}

fn sources(n: usize) -> Vec<crate::ports::Source> {
    (0..n)
        .map(|i| crate::ports::Source {
            message_id: format!("m-{i}"),
            english: format!("Message {i}."),
        })
        .collect()
}

// `ordin.draft_language` while another draft runs on this installation.
#[tokio::test]
async fn oh_draft_running() {
    let f = fixture().await;
    *f.localisation.sources.lock().unwrap() = sources(2);
    crate::host::lock(&f.host.state).draft = Some(crate::language::DraftJob {
        locale: "cy".to_owned(),
        stop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    let e = f
        .host
        .draft_language("fr".to_owned(), "French".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DRAFT_RUNNING");
    assert!(!e.retryable());
    assert_eq!(
        e.message(),
        "Ordin is already drafting a language. Wait for it to finish or stop it."
    );
    assert!(f.localisation.began.lock().unwrap().is_empty());
}

// `ordin.draft_language` for a language with no message to draft.
#[tokio::test]
async fn oh_nothing_to_draft() {
    let f = fixture().await;
    let e = f
        .host
        .draft_language("fr".to_owned(), "French".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_NOTHING_TO_DRAFT");
    assert!(!e.retryable());
    assert_eq!(
        e.message(),
        "Every message of that language already has its wording."
    );
}

// `ordin.draft_language` with a name empty or over 60 characters.
#[tokio::test]
async fn draft_language_name_invalid() {
    let f = fixture().await;
    *f.localisation.sources.lock().unwrap() = sources(2);
    for name in [String::new(), "n".repeat(61)] {
        let e = f
            .host
            .draft_language("fr".to_owned(), name)
            .await
            .unwrap_err();
        assert_eq!(e.code(), "OH_INPUT_INVALID");
        assert!(matches!(&e, OhError::InputInvalid { at, .. } if at == "name"));
    }
}

// `draft_language` or `estimate_language_draft` while the plan is paused, or Localisation refuses.
#[tokio::test]
async fn draft_language_dependency_failed() {
    let f = fixture().await;
    *f.localisation.sources.lock().unwrap() = sources(2);
    f.f.once("usage.may_start_ordin_work", "USAGE_PLAN_PAUSED", false);
    let e = f
        .host
        .draft_language("fr".to_owned(), "French".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DEPENDENCY_FAILED");
    assert!(matches!(&e, OhError::DependencyFailed { code, .. } if code == "USAGE_PLAN_PAUSED"));
    assert_eq!(e.message(), "That did not work; try again.");
    f.f.always("localisation.draft_sources", "LOC_SHIPPED_LANGUAGE", false);
    for e in [
        f.host
            .draft_language("fr".to_owned(), "French".to_owned())
            .await
            .unwrap_err(),
        f.host
            .estimate_language_draft("fr".to_owned())
            .await
            .unwrap_err(),
    ] {
        assert_eq!(e.code(), "OH_DEPENDENCY_FAILED");
        assert!(
            matches!(&e, OhError::DependencyFailed { code, retryable: false, .. } if code == "LOC_SHIPPED_LANGUAGE")
        );
    }
    f.f.clear("localisation.draft_sources");
    f.f.always("localisation.begin_draft", "LOC_STORE_UNAVAILABLE", true);
    let e = f
        .host
        .draft_language("fr".to_owned(), "French".to_owned())
        .await
        .unwrap_err();
    assert!(
        matches!(&e, OhError::DependencyFailed { code, retryable: true, .. } if code == "LOC_STORE_UNAVAILABLE")
    );
}

// A draft batch's turn fails twice: the batch is left out and counted, the job goes on; logged `warn`.
#[tokio::test]
async fn draft_batch_fails_twice() {
    let f = fixture().await;
    f.open().await;
    *f.localisation.sources.lock().unwrap() = sources(2);
    f.f.always("supervisor.submit_task", "SUP_TEST_DRAFT_TURN_FAILED", true);
    assert_eq!(
        f.host
            .draft_language("fr".to_owned(), "French".to_owned())
            .await
            .unwrap(),
        2
    );
    let l = f.localisation.clone();
    assert!(eventually(|| !l.ended.lock().unwrap().is_empty()).await);
    assert_eq!(
        f.localisation.ended.lock().unwrap()[0],
        ("fr".to_owned(), crate::ports::DraftEnding::Finished)
    );
    assert!(f.localisation.drafted.lock().unwrap().is_empty());
    assert_eq!(
        f.f.count("supervisor.submit_task"),
        2,
        "one batch, tried twice"
    );
    // Step 54.3.4: logged once, when the second failure leaves the batch out.
    let lines = log_lines("SUP_TEST_DRAFT_TURN_FAILED");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines
        .iter()
        .all(|l| l.contains("language_draft_batch_failed") && l.contains("WARN")));
    assert!(crate::host::lock(&f.host.state).draft.is_none());
}

// ---------------------------------------------------------------- memory and knowledge rows

// `remember` whose facts the Memory Service refuses (amendment ADR-118, step 15.3): the Memory
// Service's code and sentence go back to the model, retryable false.
// spec: ordin-desktop/components/OrdinHost#OH-TR-2
#[tokio::test]
async fn remember_refused_by_the_memory_service() {
    let f = fixture().await;
    f.open().await;
    let facts = json!({ "source": "conversation", "facts": [{ "subject": { "name": "Quotes" }, "predicate": "endDate", "object": { "date": "2026-02-06" } }] });
    for code in [
        "MEM_FACTS_INVALID",
        "MEM_TRIPLE_INVALID",
        "MEM_TERM_UNKNOWN",
        "MEM_TERM_PROPOSAL_INVALID",
        "MEM_TERM_PROPOSAL_REFUSED",
        "MEM_TERM_OBJECT_MISMATCH",
        "MEM_FLAGS_INVALID",
        "MEM_TOO_MANY_TERMS",
        "MEM_SOURCE_REQUIRED",
        "MEM_SOURCE_INVALID",
        "MEM_NOT_A_FACT",
        // spec: ordin-desktop/components/OrdinHost#OH-TOP-1
        "MEM_TOPIC_INVALID",
        "MEM_TOPIC_REFUSED",
    ] {
        f.f.once("memory.remember_facts", code, true);
        assert_tool(f.tool("remember", facts.clone()).await, code, false);
    }
    let ids = [
        ordin_types::MemoryFactId::new(),
        ordin_types::MemoryFactId::new(),
    ];
    let summary = json!({ "source": "conversation", "summary": "Quotes are due on Fridays.", "made_from": ids });
    for code in ["MEM_MADE_FROM_NOT_ALLOWED", "MEM_MADE_FROM_INVALID"] {
        f.f.once("memory.remember", code, true);
        assert_tool(f.tool("remember", summary.clone()).await, code, false);
    }
}

// Counting a run's facts fails at run end: logged, no consolidation turn.
#[tokio::test]
async fn counting_a_runs_facts_fails() {
    let f = fixture().await;
    f.open().await;
    let before = f.supervisor.turn_texts().len();
    f.f.always("memory.remembered_in_run", "MEM_TEST_COUNT_FAILED", true);
    let run = RunId::new();
    f.host.count_run(f.ws, run).await;
    f.host.consolidate(f.ws, run).await;
    assert!(log_lines("MEM_TEST_COUNT_FAILED")
        .iter()
        .any(|l| l.contains("ordin_host.memory_count_failed")));
    assert!(crate::host::lock(&f.host.state).counts.is_empty());
    assert_eq!(
        f.supervisor.turn_texts().len(),
        before,
        "no consolidation turn"
    );
}

// `write_page` without a description, or with a type outside the five: caught by `check_arguments`.
#[test]
fn write_page_arguments_invalid() {
    use ordin_mcp_front::ToolProvider;
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let f = fixture().await;
        let provider = f.host.tool_provider();
        let key = ordin_mcp_front::ToolKey {
            server: ordin_mcp_front::ServerName::new("ordin").unwrap(),
            tool: "write_page".to_owned(),
        };
        let p = provider
            .check_arguments(&key, &json!({ "title": "Quotes", "body": "x", "sources": [] }))
            .unwrap_err();
        assert_eq!(p.at, "/description");
        let p = provider
            .check_arguments(&key, &json!({ "title": "Quotes", "body": "x", "sources": [], "description": "How quotes work.", "type": "Essay" }))
            .unwrap_err();
        assert_eq!(p.at, "/type");
    });
}

// The Knowledge Service refuses the description or type.
#[tokio::test]
async fn write_page_refused_by_the_knowledge_service() {
    let f = fixture().await;
    f.open().await;
    for code in ["KNOW_DESCRIPTION_INVALID", "KNOW_TYPE_INVALID"] {
        f.f.once("knowledge.write_page", code, true);
        assert_tool(
            f.tool("write_page", json!({ "title": "Quotes", "body": "# Quotes", "sources": [], "description": "How quotes work." })).await,
            code,
            false,
        );
    }
}

// OH-KP-1: found on the 0.1.1 Mac build — Ordin could not save a knowledge page: its sources were refused and
// a second write under the same title was refused, and each time it was told only "That did not work; try
// again." It is now told what to change, and a page it wrote can be changed by its id.
// spec: ordin-desktop/components/OrdinHost#OH-KP-1
#[tokio::test]
async fn write_page_says_what_to_change_when_a_source_or_a_title_is_refused() {
    let f = fixture().await;
    f.open().await;
    let args = json!({ "title": "Quotes", "body": "# Quotes", "sources": [], "description": "How quotes work." });
    f.f.once("knowledge.write_page", "KNOW_SOURCE_INVALID", false);
    let e = assert_tool(
        f.tool("write_page", args.clone()).await,
        "KNOW_SOURCE_INVALID",
        false,
    );
    assert!(
        e.message.contains("place:<place id>:<path>"),
        "{}",
        e.message
    );
    assert!(e.message.contains("no sources"), "{}", e.message);
    f.f.once("knowledge.write_page", "KNOW_TITLE_DUPLICATE", false);
    let e = assert_tool(
        f.tool("write_page", args).await,
        "KNOW_TITLE_DUPLICATE",
        false,
    );
    assert!(e.message.contains("page_id"), "{}", e.message);
    assert!(!e.message.contains("try again"), "{}", e.message);
}

#[tokio::test]
async fn write_page_with_a_page_id_changes_that_page_and_a_bad_one_is_refused() {
    let f = fixture().await;
    f.open().await;
    let id = ordin_types::KnowledgePageId::new();
    f.tool(
        "write_page",
        json!({ "title": "Quotes", "body": "# Quotes", "sources": [], "description": "How quotes work.", "page_id": id.to_string() }),
    )
    .await
    .expect("the page is written");
    assert_eq!(
        f.knowledge.written.lock().unwrap().last().unwrap().page_id,
        Some(id)
    );
    f.tool(
        "write_page",
        json!({ "title": "Quotes", "body": "# Quotes", "sources": [], "description": "How quotes work." }),
    )
    .await
    .expect("a new page is written");
    assert_eq!(
        f.knowledge.written.lock().unwrap().last().unwrap().page_id,
        None
    );
    let e = assert_tool(
        f.tool(
            "write_page",
            json!({ "title": "Quotes", "body": "# Quotes", "sources": [], "description": "How quotes work.", "page_id": "not an id" }),
        )
        .await,
        "MCP_ARGUMENTS_INVALID",
        false,
    );
    assert!(e.message.contains("/page_id"), "{}", e.message);
}

// Every front door failed or timed out on a wider recall: not an error; the local facts are answered.
#[tokio::test]
async fn wider_recall_without_answers_is_not_an_error() {
    let f = fixture().await;
    f.open().await;
    *f.memory.recall.lock().unwrap() = Some(ordin_memory_service::RecallResult {
        facts: vec![],
        abstained: false,
        recall_id: ordin_types::MemoryRecallId::new(),
        links: vec![],
        wider: vec![],
        order: vec![],
        wider_status: ordin_memory_service::WiderStatus {
            asked: true,
            reason: ordin_memory_service::WiderReason::Failed,
            answered: vec![],
            unanswered: vec![
                ordin_types::wire::knowledge_query::UnansweredSource {
                    agent: "Finance".to_owned(),
                    reason: ordin_types::wire::knowledge_query::UnansweredReason::NotInTime,
                },
                ordin_types::wire::knowledge_query::UnansweredSource {
                    agent: "Legal".to_owned(),
                    reason: ordin_types::wire::knowledge_query::UnansweredReason::Unreachable,
                },
            ],
            more_found_pending: false,
            unchecked_sources: false,
        },
    });
    let out = f
        .tool("recall", json!({ "query": "quotes", "wider": true }))
        .await
        .expect("answered");
    assert!(
        out.text.contains("Did not answer in time: Finance."),
        "{}",
        out.text
    );
    assert!(
        out.text.contains("Could not be reached: Legal."),
        "{}",
        out.text
    );
}
