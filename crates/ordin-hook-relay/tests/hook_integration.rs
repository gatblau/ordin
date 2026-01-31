//! Cross-component tests (plan 01, Phase 5): the REAL `ordin-hook` binary sends its payload to a live
//! relay on a real socket, and the verdict follows the REAL Rules Engine, Approval Service, Session
//! Supervisor and Journal over a real SQLite file. Only the harness (a channel-driven adapter) and
//! the programs' hosts are stand-ins. The phase's behavioural check is
//! `behavioural_check_a_forbidden_bash_call_is_denied_and_journalled`.
//!
//! The test builds `ordin-hook` with Cargo (the same target folder, so a binary the workspace build
//! already made is reused) and runs the file it finds next to this test's own binary.

#![cfg(unix)]

mod support;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use ordin_rules::AgentToolPolicy;
use ordin_runtime_adapter::{PauseReason, ResumeDecision, RunEvent};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::runs::{ToolRequestDecision, ToolRequestState};
use ordin_types::wire::hook::{HarnessKind, HookMessage, HookReply};
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::{ApprovalId, RunId};
use serde_json::json;
use sqlx::Row;
use support::{rule_set, Session, Stack};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// `target/<profile>/ordin-hook`, built on first use.
fn hook_binary() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let exe = std::env::current_exe().expect("this test's path");
        let profile_dir = exe
            .parent()
            .and_then(|deps| deps.parent())
            .expect("target/<profile>")
            .to_path_buf();
        let target_dir = profile_dir
            .parent()
            .expect("the target folder")
            .to_path_buf();
        let mut build =
            std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        build
            .args(["build", "-p", "ordin-hook", "--target-dir"])
            .arg(&target_dir)
            .env("CARGO_INCREMENTAL", "0")
            .current_dir(env!("CARGO_MANIFEST_DIR"));
        if profile_dir
            .file_name()
            .is_some_and(|name| name == "release")
        {
            build.arg("--release");
        }
        let status = build.status().expect("run cargo");
        assert!(status.success(), "cargo build -p ordin-hook failed");
        let binary = profile_dir.join("ordin-hook");
        assert!(binary.exists(), "{} was not built", binary.display());
        binary
    })
}

struct Hooked {
    status: i32,
    stdout: String,
}

/// Runs the real hook the way the harness does: its environment from the relay, the payload on
/// standard input.
async fn run_hook(session: &Session, event: &str, payload: &str) -> Hooked {
    run_hook_with(session, event, payload, |_| {}).await
}

async fn run_hook_with(
    session: &Session,
    event: &str,
    payload: &str,
    tweak: impl FnOnce(&mut Command),
) -> Hooked {
    let binary = hook_binary().clone();
    let mut command = Command::new(binary);
    command
        .args([
            "--harness",
            "claude-code",
            "--event",
            event,
            "--agent-instance",
            &session.agent.to_string(),
        ])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in session.env.env_vars() {
        command.env(key, value);
    }
    tweak(&mut command);
    let mut child = command.spawn().expect("spawn ordin-hook");
    let mut stdin = child.stdin.take().expect("stdin");
    stdin
        .write_all(payload.as_bytes())
        .await
        .expect("write stdin");
    drop(stdin);
    let out = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .expect("the hook finishes")
        .expect("wait");
    Hooked {
        status: out.status.code().expect("exit status"),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
    }
}

fn pre_payload(tool: &str, input: serde_json::Value, id: &str) -> String {
    json!({"session_id": "s", "hook_event_name": "PreToolUse", "tool_name": tool,
           "tool_input": input, "tool_use_id": id})
    .to_string()
}

fn post_payload(tool: &str, id: &str) -> String {
    json!({"session_id": "s", "hook_event_name": "PostToolUse", "tool_name": tool,
           "tool_input": {}, "tool_use_id": id, "tool_response": {"is_error": false}})
    .to_string()
}

fn verdict(hooked: &Hooked) -> serde_json::Value {
    serde_json::from_str(&hooked.stdout)
        .unwrap_or_else(|e| panic!("verdict {:?}: {e}", hooked.stdout))
}

/// The tool requests journalled for a run: (tool, decision, state, deciding rule).
async fn requests(stack: &Stack, run: RunId) -> Vec<(String, String, String, Option<String>)> {
    sqlx::query("SELECT tool, decision, state, deciding_rule FROM tool_requests WHERE run_id = ?1 ORDER BY requested_at")
        .bind(run.to_string())
        .fetch_all(stack.pool())
        .await
        .expect("tool requests")
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("tool"),
                r.get::<Option<String>, _>("decision").unwrap_or_default(),
                r.get::<String, _>("state"),
                r.get::<Option<String>, _>("deciding_rule"),
            )
        })
        .collect()
}

async fn poll<T, F: std::future::Future<Output = Option<T>>>(
    what: &str,
    mut check: impl FnMut() -> F,
) -> T {
    for _ in 0..500 {
        if let Some(found) = check().await {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never: {what}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn happy_path_an_allowed_call_is_journalled_by_the_real_engine() {
    let stack = Stack::start().await;
    let folder = stack.folder("analyst-1");
    let session = stack
        .session(rule_set(Authority::DocumentOnly, &folder))
        .await;
    let notes = folder.join("notes.md").display().to_string();
    let hooked = run_hook(
        &session,
        "pre-tool-use",
        &pre_payload("Read", json!({"file_path": notes}), "toolu_1"),
    )
    .await;
    assert_eq!(hooked.status, 0);
    assert_eq!(
        verdict(&hooked)["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    let rows = requests(&stack, session.run_id).await;
    assert_eq!(
        rows.len(),
        1,
        "the Rules Engine journalled the tool request"
    );
    assert_eq!(rows[0].0, "Read");
    assert_eq!(rows[0].1, "allow");
    assert_eq!(rows[0].2, "decided");

    // The after-tool event: acknowledged, and the tool request is recorded as executed.
    let hooked = run_hook(&session, "post-tool-use", &post_payload("Read", "toolu_1")).await;
    assert_eq!(hooked.status, 0);
    assert_eq!(hooked.stdout, "", "the hook prints nothing for an ack");
    let rows = poll("the request is executed", || async {
        let rows = requests(&stack, session.run_id).await;
        (rows[0].2 == "executed").then_some(rows)
    })
    .await;
    assert_eq!(rows[0].1, "allow");
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_verdict_follows_the_rules_engine_for_a_document_only_agent() {
    let stack = Stack::start().await;
    let session = stack
        .session(rule_set(
            Authority::DocumentOnly,
            &stack.folder("analyst-2"),
        ))
        .await;
    let hooked = run_hook(
        &session,
        "pre-tool-use",
        &pre_payload("Bash", json!({"command": "ls"}), "toolu_2"),
    )
    .await;
    assert_eq!(hooked.status, 0);
    let out = &verdict(&hooked)["hookSpecificOutput"];
    assert_eq!(out["permissionDecision"], "deny");
    assert!(!out["permissionDecisionReason"]
        .as_str()
        .unwrap_or("")
        .is_empty());
    let rows = requests(&stack, session.run_id).await;
    assert_eq!(
        (rows[0].1.as_str(), rows[0].3.as_deref()),
        ("deny", Some("app_shell_policy:app.shell"))
    );
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ask_and_approve_within_the_deadline() {
    let stack = Stack::start().await;
    let session = stack
        .session(rule_set(Authority::SystemChanging, &stack.folder("eng-1")))
        .await;
    let call = pre_payload("Bash", json!({"command": "make deploy"}), "toolu_3");
    let hook = run_hook(&session, "pre-tool-use", &call);
    let person = async {
        // The engine opened an approval; the person approves a moment later.
        let approval = poll("an approval is pending", || async {
            stack.pending_approvals().await.into_iter().next()
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        stack.approve(approval).await;
        approval
    };
    let (hooked, approval) = tokio::join!(hook, person);
    assert_eq!(hooked.status, 0);
    assert_eq!(
        verdict(&hooked)["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    let rows = requests(&stack, session.run_id).await;
    assert_eq!(rows[0].1, "ask");
    let _ = approval;
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wrong_mac() {
    let stack = Stack::start().await;
    let session = stack
        .session(rule_set(
            Authority::DocumentOnly,
            &stack.folder("analyst-3"),
        ))
        .await;
    let other_secret = ordin_types::encoding::b64url_encode(&[7u8; 32]);
    let hooked = run_hook_with(
        &session,
        "pre-tool-use",
        &pre_payload(
            "Read",
            json!({"file_path": "/x/secret-arguments.txt"}),
            "toolu_4",
        ),
        |command| {
            command.env("ORDIN_HOOK_SECRET", other_secret);
        },
    )
    .await;
    assert_eq!(hooked.status, 0);
    assert_eq!(
        verdict(&hooked)["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    let entry = sqlx::query(
        "SELECT data, summary, run_id FROM journal_entries WHERE kind = 'hook_refused'",
    )
    .fetch_one(stack.pool())
    .await
    .expect("a hook_refused entry");
    let data: String = entry.get("data");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&data).unwrap(),
        json!({"code": "HOOK_UNAUTHENTICATED"})
    );
    assert!(
        !data.contains("secret-arguments"),
        "the journal holds no arguments"
    );
    assert!(
        requests(&stack, session.run_id).await.is_empty(),
        "the engine was never asked"
    );
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_session() {
    let stack = Stack::start().await;
    let session = stack
        .session(rule_set(
            Authority::DocumentOnly,
            &stack.folder("analyst-4"),
        ))
        .await;
    stack.relay.close_session(session.run_id);
    let hooked = run_hook(
        &session,
        "pre-tool-use",
        &pre_payload("Read", json!({"file_path": "/x"}), "toolu_5"),
    )
    .await;
    let out = &verdict(&hooked)["hookSpecificOutput"];
    assert_eq!(out["permissionDecision"], "deny");
    assert_eq!(
        out["permissionDecisionReason"],
        "this session is not known to ordin"
    );
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stop_with_no_gate_open_ends_the_turn_and_is_journalled() {
    let stack = Stack::start().await;
    let session = stack
        .session(rule_set(
            Authority::DocumentOnly,
            &stack.folder("analyst-5"),
        ))
        .await;
    let hooked = run_hook(
        &session,
        "stop",
        &json!({"session_id": "s", "hook_event_name": "Stop"}).to_string(),
    )
    .await;
    assert_eq!(hooked.status, 0);
    assert_eq!(hooked.stdout, "", "an ack lets the harness stop");
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM journal_entries WHERE kind = 'hook_stop' AND run_id = ?1",
    )
    .bind(session.run_id.to_string())
    .fetch_one(stack.pool())
    .await
    .unwrap();
    assert_eq!(n, 1);
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copilot_decision_in_process_follows_the_real_engine() {
    let stack = Stack::start().await;
    let session = stack
        .session(rule_set(
            Authority::DocumentOnly,
            &stack.folder("analyst-6"),
        ))
        .await;
    let mut events = stack.relay.session_events(session.run_id);
    let reply = stack
        .relay
        .decide_in_process(
            session.run_id,
            HookMessage::PreToolUse {
                harness: HarnessKind::CopilotCli,
                tool: "Bash".into(),
                origin: ToolOrigin::BuiltIn,
                arguments: json!({"command": "ls"}),
                harness_tool_use_id: None,
                subagent: None,
            },
        )
        .await;
    let HookReply::Deny { reason, .. } = reply else {
        panic!("expected a deny, got {reply:?}");
    };
    assert!(!reason.is_empty());
    use futures::StreamExt;
    let event = tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({"kind": "decided", "tool": "Bash", "allowed": false})
    );
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_late_decision_resumes_the_run_through_the_real_supervisor() {
    let stack = Stack::start().await;
    let session = stack
        .session(rule_set(Authority::SystemChanging, &stack.folder("eng-2")))
        .await;
    // The engine asks a person for a call (a real pending approval, as the hook would have caused).
    let outcome = stack
        .engine
        .decide_tool_call(
            &ordin_types::wire::rules::ToolCall {
                run_id: session.run_id,
                agent_instance_id: session.agent,
                workspace_id: session.workspace,
                point: ordin_types::wire::rules::DecisionPoint::BeforeToolCall,
                tool: "Bash".into(),
                origin: ToolOrigin::BuiltIn,
                arguments: json!({"command": "make deploy"}),
            },
            &ordin_rules::CallContext::default(),
        )
        .await;
    let approval: ApprovalId = outcome.approval_id.expect("an approval");
    // The hook gave up at its deadline: the relay's row stands for the wait, and the harness
    // session pauses on the approval.
    sqlx::query("INSERT INTO hook_late_decisions (approval_id, run_id, kind, created_at) VALUES (?1, ?2, 'tool_call', ?3)")
        .bind(approval.to_string())
        .bind(session.run_id.to_string())
        .bind(ordin_types::time::Timestamp::now().to_wire())
        .execute(stack.pool())
        .await
        .unwrap();
    stack.adapter.push(
        session.run_id,
        RunEvent::Paused {
            reason: PauseReason::Approval {
                approval_id: approval,
            },
        },
    );
    poll("the supervisor records the pause", || async {
        let state: Option<String> = sqlx::query_scalar("SELECT state FROM runs WHERE id = ?1")
            .bind(session.run_id.to_string())
            .fetch_optional(stack.pool())
            .await
            .unwrap();
        (state.as_deref() == Some("paused")).then_some(())
    })
    .await;
    // The person approves three minutes later (here: now); the Journal sink delivers the decision.
    stack.approve(approval).await;
    let resumed = poll("the adapter is resumed", || async {
        let resumes = stack.adapter.resumes.lock().unwrap().clone();
        resumes.into_iter().next()
    })
    .await;
    assert_eq!(resumed.0, session.run_id);
    assert!(
        matches!(resumed.1, ResumeDecision::Approve { approval_id } if approval_id == approval)
    );
    poll("the late-decision row is deleted", || async {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM hook_late_decisions")
            .fetch_one(stack.pool())
            .await
            .unwrap();
        (n == 0).then_some(())
    })
    .await;
    stack.shutdown().await;
}

/// The phase's behavioural check: `ordin-hook` -> Hook Relay -> Rules Engine -> Approval Service,
/// end to end on SQLite, denies a forbidden Bash call and journals the denial.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn behavioural_check_a_forbidden_bash_call_is_denied_and_journalled() {
    let stack = Stack::start().await;
    let mut set = rule_set(Authority::SystemChanging, &stack.folder("eng-3"));
    set.tool_policy = Some(AgentToolPolicy {
        forbidden_flags: vec!["--no-verify".to_owned()],
        ..AgentToolPolicy::default()
    });
    let session = stack.session(set).await;

    // A forbidden call: the hook prints the harness's deny form and exits 0.
    let forbidden = pre_payload(
        "Bash",
        json!({"command": "git commit -m 'x' --no-verify"}),
        "toolu_forbidden",
    );
    let hooked = run_hook(&session, "pre-tool-use", &forbidden).await;
    assert_eq!(hooked.status, 0);
    let out = &verdict(&hooked)["hookSpecificOutput"];
    assert_eq!(out["permissionDecision"], "deny");
    assert!(
        out["permissionDecisionReason"]
            .as_str()
            .unwrap_or("")
            .contains("--no-verify"),
        "{out}"
    );

    // The denial is journalled by the Rules Engine: decided, denied, by the agent's own policy.
    let rows = requests(&stack, session.run_id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "Bash");
    assert_eq!(rows[0].1, "deny");
    assert_eq!(rows[0].2, "decided");
    assert!(
        rows[0].3.as_deref().unwrap_or("").contains("--no-verify"),
        "{:?}",
        rows[0]
    );
    let journalled = stack
        .journal
        .tool_request(
            ordin_types::ToolRequestId::parse(
                &sqlx::query_scalar::<_, String>("SELECT id FROM tool_requests WHERE run_id = ?1")
                    .bind(session.run_id.to_string())
                    .fetch_one(stack.pool())
                    .await
                    .unwrap(),
            )
            .unwrap(),
        )
        .await
        .unwrap()
        .expect("the tool request through the Journal's own read");
    assert_eq!(journalled.decision, Some(ToolRequestDecision::Deny));
    assert_eq!(journalled.state, ToolRequestState::Decided);

    // Nothing was left waiting on a person, and the same session still gets a call that needs one
    // through to the Approval Service.
    assert!(stack.pending_approvals().await.is_empty());
    let deploy = pre_payload("Bash", json!({"command": "make deploy"}), "toolu_deploy");
    let hook = run_hook(&session, "pre-tool-use", &deploy);
    let person = async {
        let approval = poll("an approval is pending", || async {
            stack.pending_approvals().await.into_iter().next()
        })
        .await;
        stack.approve(approval).await;
    };
    let (hooked, ()) = tokio::join!(hook, person);
    assert_eq!(
        verdict(&hooked)["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    stack.shutdown().await;
}
