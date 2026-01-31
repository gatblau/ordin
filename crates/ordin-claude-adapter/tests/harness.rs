//! The harness test job: tests that drive the REAL Claude Code CLI at the pinned version. They are
//! separate from the ordinary suite because Claude Code updates itself and must not break every
//! commit; `make harness-test` runs them. When run they fail, never skip, if the CLI is missing or
//! its version differs.
//!
//! The model is a local `wiremock` stand-in reached through `ANTHROPIC_BASE_URL`: no live model
//! call, no real sign-in and no spending. The CLI's own configuration and credentials are never
//! touched: a test wrapper around the CLI points `HOME` and `CLAUDE_CONFIG_DIR` at a temporary
//! folder. The adapter never sets those itself (`start` step 8), so the wrapper is the adapter's
//! `claude_path`. A live session stays with the Phase 13 ship gate.

#![cfg(unix)]

mod support;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::stream::StreamExt;
use ordin_claude_adapter::{
    ClaudeAdapterConfig, ClaudeLocalAdapter, KeyRoute, PackageMcpServer, SessionRecordStore,
    SignInMode, DESKTOP_BUILT_IN_TOOLS,
};
use ordin_hook_relay::HookRelay;
use ordin_mcp_front::{McpFront, ProgramProfile, SessionGrant};
use ordin_rules::{DecidingOutcome, Declaration, MessageRule};
use ordin_runtime_adapter::{
    AdapterContext, AdapterKind, HarnessKind, RunEvent, RunHandle, RunState, RuntimeAdapter,
    StartInput, ToolCaller,
};
use ordin_types::domain::agent_profile::{
    HarnessEnvVar, HarnessModelSettings, ModelProvider, OrganisationKey,
};
use ordin_types::domain::agents::Authority;
use ordin_types::{RunId, WorkspaceId};
use serde_json::{json, Value};
use support::{hook_binary, write_script, Options, Stack, TempDir};
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// The Claude Code version this job holds the adapter to.
const PINNED: &str = "2.1.316";
const FAKE_KEY: &str = "sk-ant-not-a-real-key";
const FINAL_TEXT: &str = "All done.";

/// The real CLI on `PATH`. A missing CLI or another version is a failure of the job.
fn pinned_claude() -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH");
    let claude = std::env::split_paths(&path)
        .map(|dir| dir.join("claude"))
        .find(|candidate| candidate.is_file())
        .expect("the harness job needs Claude Code on PATH");
    let output = std::process::Command::new(&claude)
        .arg("--version")
        .env_clear()
        .output()
        .expect("run claude --version");
    let version = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        version.trim(),
        format!("{PINNED} (Claude Code)"),
        "the harness job is pinned to Claude Code {PINNED}"
    );
    claude
}

/// The adapter's `claude_path`: the real CLI in a temporary home, with no traffic but the model's.
/// `stand_in` also gives the CLI a key and an address, for a desktop run, where the adapter sets
/// neither. Each session's stdout and arguments are copied beside it.
fn wrapper(dir: &Path, stand_in: Option<&str>) -> PathBuf {
    let real = pinned_claude();
    let wrapper = dir.join("claude-wrapper");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).expect("the temporary home");
    let model_env = stand_in.map_or(String::new(), |address| {
        format!("export ANTHROPIC_API_KEY='{FAKE_KEY}' ANTHROPIC_BASE_URL='{address}'\n")
    });
    let script = format!(
        r#"#!/bin/sh
export HOME='{home}' CLAUDE_CONFIG_DIR='{home}/config'
export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_AUTOUPDATER=1 DISABLE_TELEMETRY=1
{model_env}if [ "$1" = "-p" ]; then
  printf '%s\n' "$@" > '{dir}/args'
  '{real}' "$@" | tee '{dir}/stream.jsonl'
else
  exec '{real}' "$@"
fi
"#,
        home = home.display(),
        dir = dir.display(),
        real = real.display(),
    );
    write_script(&wrapper, &script);
    wrapper
}

/// `ordin-hook` in a folder with a space in its name, like macOS's "Application Support".
fn hook_with_a_space(dir: &Path) -> PathBuf {
    let folder = dir.join("Application Support");
    std::fs::create_dir_all(&folder).expect("the folder");
    let copy = folder.join("ordin-hook");
    std::fs::copy(hook_binary(), &copy).expect("copy the hook");
    copy
}

fn models_at(address: &str, environment: Vec<HarnessEnvVar>) -> HarnessModelSettings {
    HarnessModelSettings {
        provider: ModelProvider(ModelProvider::DEFAULT.to_owned()),
        primary_model: "claude-sonnet-4-6".to_owned(),
        fallback_model: None,
        gateway_url: Some(address.to_owned()),
        key: Some(OrganisationKey::new(FAKE_KEY.to_owned())),
        environment,
    }
}

fn sse_message(content: &[Value], stop_reason: &str) -> Vec<u8> {
    let mut events = vec![json!({"type": "message_start", "message": {
        "id": "msg_1", "type": "message", "role": "assistant", "model": "stand-in", "content": [],
        "stop_reason": null, "stop_sequence": null,
        "usage": {"input_tokens": 10, "output_tokens": 1, "cache_read_input_tokens": 3,
                  "cache_creation_input_tokens": 0}}})];
    for (index, block) in content.iter().enumerate() {
        if let Some(text) = block.get("text") {
            events.push(json!({"type": "content_block_start", "index": index,
                "content_block": {"type": "text", "text": ""}}));
            events.push(json!({"type": "content_block_delta", "index": index,
                "delta": {"type": "text_delta", "text": text}}));
        } else {
            events.push(json!({"type": "content_block_start", "index": index,
                "content_block": {"type": "tool_use", "id": block["id"], "name": block["name"],
                                  "input": {}}}));
            events.push(json!({"type": "content_block_delta", "index": index,
                "delta": {"type": "input_json_delta",
                          "partial_json": block["input"].to_string()}}));
        }
        events.push(json!({"type": "content_block_stop", "index": index}));
    }
    events.push(json!({"type": "message_delta",
        "delta": {"stop_reason": stop_reason, "stop_sequence": null},
        "usage": {"output_tokens": 5}}));
    events.push(json!({"type": "message_stop"}));
    let mut body = String::new();
    for event in events {
        body.push_str(&format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().expect("a type")
        ));
    }
    body.into_bytes()
}

/// The stand-in's script: each call in `calls` once, in order, then the final text.
struct ScriptedModel {
    calls: Vec<(&'static str, String)>,
    requests: AtomicUsize,
}

impl Respond for ScriptedModel {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        self.requests.fetch_add(1, Ordering::SeqCst);
        if !request.url.path().ends_with("/v1/messages") {
            return ResponseTemplate::new(200).set_body_json(json!({"input_tokens": 1}));
        }
        let body = String::from_utf8_lossy(&request.body);
        let next = self
            .calls
            .iter()
            .find(|(id, _)| !body.contains(&format!("\"tool_use_id\":\"{id}\"")));
        let message = match next {
            Some((id, path)) => sse_message(
                &[json!({"id": id, "name": "Read", "input": {"file_path": path}})],
                "tool_use",
            ),
            None => sse_message(&[json!({"text": FINAL_TEXT})], "end_turn"),
        };
        ResponseTemplate::new(200).set_body_raw(message, "text/event-stream")
    }
}

async fn stand_in(calls: Vec<(&'static str, String)>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ScriptedModel {
            calls,
            requests: AtomicUsize::new(0),
        })
        .mount(&server)
        .await;
    server
}

fn ctx() -> AdapterContext {
    AdapterContext {
        correlation_id: "harness-job".to_owned(),
        deadline: std::time::Instant::now() + Duration::from_secs(30),
        cancel: CancellationToken::new(),
    }
}

async fn events_of(stack: &Stack, run: RunId) -> Vec<RunEvent> {
    let handle = RunHandle {
        run_id: run,
        adapter: AdapterKind::ClaudeLocal,
        runtime_session_id: None,
        remote_thread_id: None,
    };
    for _ in 0..1000 {
        if let Ok(stream) = stack.adapter.events(&ctx(), &handle) {
            return tokio::time::timeout(Duration::from_secs(120), stream.collect::<Vec<_>>())
                .await
                .expect("the run ends");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the run never reached the adapter");
}

fn final_message(events: &[RunEvent]) -> Option<String> {
    match events.last() {
        Some(RunEvent::Ended { end }) => end.final_message.clone(),
        other => panic!("last event {other:?}"),
    }
}

/// The JSON lines the wrapper copied.
fn stream_lines(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("stream.jsonl"))
        .expect("the copied stream")
        .lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect()
}

async fn agent_stack(dir: &Path, models: HarnessModelSettings, hook: PathBuf) -> Stack {
    let agent_folder = dir.join("agent");
    std::fs::create_dir_all(&agent_folder).expect("the agent folder");
    Stack::start(
        dir,
        Options {
            claude: wrapper(dir, None),
            hook,
            profile: ProgramProfile::AgentProfile,
            sign_in: SignInMode::OrganisationKey {
                models,
                route: KeyRoute::Environment,
            },
            tested_versions: vec![PINNED.to_owned()],
            agent_folder,
        },
    )
    .await
}

/// A stack with a person's sign-in and the real hook, for the desktop's own command line.
async fn desktop_stack(dir: &Path, address: &str) -> Stack {
    let agent_folder = dir.join("agent");
    std::fs::create_dir_all(&agent_folder).expect("the agent folder");
    Stack::start(
        dir,
        Options {
            claude: wrapper(dir, Some(address)),
            hook: hook_binary().clone(),
            profile: ProgramProfile::Desktop,
            sign_in: SignInMode::PersonSignIn,
            tested_versions: vec![PINNED.to_owned()],
            agent_folder,
        },
    )
    .await
}

/// What the model was told about a tool call, from the copied stream.
fn tool_result(dir: &Path, id: &str) -> Value {
    stream_lines(dir)
        .iter()
        .filter(|line| line["type"] == "user")
        .flat_map(|line| {
            line["message"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .find(|block| block["tool_use_id"] == id)
        .expect("the call's result")
}

/// The text of every request the stand-in received.
async fn bodies_of(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .expect("recording is on")
        .iter()
        .map(|request| String::from_utf8_lossy(&request.body).to_string())
        .collect()
}

/// A package rule that leaves the run no built-in tool: the Rules Engine's subset has no overlap
/// with the list, so `tool_list` gives `--tools` an empty value.
fn no_tools_left() -> Option<Declaration> {
    Some(Declaration {
        format_version: 1,
        message_rules: vec![MessageRule {
            name: "no-tools".to_owned(),
            priority: 0,
            command: None,
            conditions: vec![],
            decides: Some(DecidingOutcome::Tools { tools: vec![] }),
            adds: vec![],
        }],
        ..Declaration::default()
    })
}

/// A run of `stack` whose subset leaves no tool, while the model asks to read a private file.
async fn assert_no_tool_runs(dir: &Path, stack: &Stack, server: &MockServer) {
    let (run, _agent) = stack
        .start_declared_run(Authority::DocumentOnly, no_tools_left())
        .await;
    let events = events_of(stack, run).await;
    assert_eq!(final_message(&events).as_deref(), Some(FINAL_TEXT));
    let rows = stack.requests(run).await;
    stack.stop().await;

    // The value is empty, not absent, and it reached the CLI as its own argument.
    let args = std::fs::read_to_string(dir.join("args")).expect("the arguments");
    let args: Vec<&str> = args.lines().collect();
    let at = args
        .iter()
        .position(|arg| *arg == "--tools")
        .expect("--tools");
    assert_eq!(args[at + 1], "", "{args:?}");

    let lines = stream_lines(dir);
    let init = lines
        .iter()
        .find(|line| line["type"] == "system" && line["subtype"] == "init")
        .expect("the init event");
    let built_in: Vec<&str> = init["tools"]
        .as_array()
        .expect("the tools field")
        .iter()
        .filter_map(Value::as_str)
        .filter(|tool| !tool.starts_with("mcp__"))
        .collect();
    assert!(built_in.is_empty(), "built-in tools remain: {built_in:?}");

    // The call was refused by the CLI, so no decision was asked for and no content was read.
    let refused = tool_result(dir, "toolu_read");
    assert_eq!(refused["is_error"], true);
    assert!(
        refused["content"]
            .as_str()
            .is_some_and(|text| text.contains("No such tool available: Read")),
        "{refused}"
    );
    assert!(rows.is_empty(), "no call reached the hook: {rows:?}");
    assert!(!bodies_of(server)
        .await
        .iter()
        .any(|body| body.contains("private-content-9012")));
}

/// `--tools ""` is no built-in tool at the pinned version, never the default set: a Rules Engine
/// subset with no overlap must not widen the session. The desktop's command line.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn an_empty_tools_value_leaves_a_desktop_session_no_built_in_tool() {
    let dir = TempDir::new();
    let file = dir.0.join("agent").join("private.md");
    std::fs::create_dir_all(dir.0.join("agent")).expect("the agent folder");
    std::fs::write(&file, "private-content-9012").expect("the file");
    let server = stand_in(vec![("toolu_read", file.display().to_string())]).await;
    let stack = desktop_stack(&dir.0, &server.uri()).await;
    assert_no_tool_runs(&dir.0, &stack, &server).await;
}

/// The same with the agent profile's command line, where `--disallowedTools` follows the empty value.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn an_empty_tools_value_leaves_an_agent_session_no_built_in_tool() {
    let dir = TempDir::new();
    let file = dir.0.join("agent").join("private.md");
    std::fs::create_dir_all(dir.0.join("agent")).expect("the agent folder");
    std::fs::write(&file, "private-content-9012").expect("the file");
    let server = stand_in(vec![("toolu_read", file.display().to_string())]).await;
    let stack = agent_stack(
        &dir.0,
        models_at(&server.uri(), vec![]),
        hook_binary().clone(),
    )
    .await;
    assert_no_tool_runs(&dir.0, &stack, &server).await;
}

/// A session whose hook cannot answer: the model's one call must not run, and it must say why.
async fn assert_blocked_without_a_decision(dir: &Path, hook: PathBuf, server: &MockServer) {
    let stack = agent_stack(dir, models_at(&server.uri(), vec![]), hook).await;
    let (run, _agent) = stack.start_run(Authority::DocumentOnly).await;
    let events = events_of(&stack, run).await;
    assert_eq!(final_message(&events).as_deref(), Some(FINAL_TEXT));
    let rows = stack.requests(run).await;
    stack.stop().await;

    let blocked = tool_result(dir, "toolu_read");
    assert_eq!(blocked["is_error"], true);
    assert!(
        blocked["content"]
            .as_str()
            .is_some_and(|text| text.contains("blocking because onFailure is \"block\"")),
        "{blocked}"
    );
    assert!(rows.is_empty(), "the hook decided nothing: {rows:?}");
    assert!(!bodies_of(server)
        .await
        .iter()
        .any(|body| body.contains("private-content-9012")));
}

async fn hook_failure_fixture(dir: &Path) -> MockServer {
    let file = dir.join("agent").join("private.md");
    std::fs::create_dir_all(dir.join("agent")).expect("the agent folder");
    std::fs::write(&file, "private-content-9012").expect("the file");
    stand_in(vec![("toolu_read", file.display().to_string())]).await
}

/// `"onFailure": "block"` at the pinned version: a hook program that is not there leaves no tool
/// running without a decision. Without the setting 2.1.316 runs the tool.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn a_missing_hook_program_blocks_the_tool() {
    let dir = TempDir::new();
    let server = hook_failure_fixture(&dir.0).await;
    let hook = dir.0.join("no-such-hook");
    assert_blocked_without_a_decision(&dir.0, hook, &server).await;
}

/// The same for a hook program that starts and exits non-zero with no decision.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn a_hook_program_that_exits_non_zero_blocks_the_tool() {
    let dir = TempDir::new();
    let server = hook_failure_fixture(&dir.0).await;
    let hook = dir.0.join("failing-hook");
    write_script(&hook, "#!/bin/sh\nexit 3\n");
    assert_blocked_without_a_decision(&dir.0, hook, &server).await;
}

/// Conformance — init event tool list at the pinned version (run against the real CLI in the
/// harness test job).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn conformance_init_event_tool_list_at_the_pinned_version() {
    let dir = TempDir::new();
    let server = stand_in(vec![]).await;
    let agent_folder = dir.0.join("agent");
    std::fs::create_dir_all(&agent_folder).expect("the agent folder");
    let stack = Stack::start(
        &dir.0,
        Options {
            claude: wrapper(&dir.0, Some(&server.uri())),
            hook: hook_binary().clone(),
            profile: ProgramProfile::Desktop,
            sign_in: SignInMode::PersonSignIn,
            tested_versions: vec![PINNED.to_owned()],
            agent_folder,
        },
    )
    .await;
    let (run, agent) = stack.start_run(Authority::DocumentOnly).await;
    let events = events_of(&stack, run).await;
    assert_eq!(final_message(&events).as_deref(), Some(FINAL_TEXT));

    let lines = stream_lines(&dir.0);
    let init = lines
        .iter()
        .find(|line| line["type"] == "system" && line["subtype"] == "init")
        .expect("the init event");
    let tools: Vec<String> = init["tools"]
        .as_array()
        .expect("the tools field")
        .iter()
        .map(|tool| tool.as_str().expect("a name").to_owned())
        .collect();
    let (mcp, mut built_in): (Vec<String>, Vec<String>) = tools
        .into_iter()
        .partition(|tool| tool.starts_with("mcp__"));

    let result = lines
        .iter()
        .find(|line| line["type"] == "result")
        .expect("the result event");
    assert!(result.get("is_error").is_some_and(Value::is_boolean));
    assert!(result.get("result").is_some_and(Value::is_string));
    assert!(result.get("usage").is_some_and(Value::is_object));

    // The MCP tools registered: what the MCP Front offers such a session.
    let other = RunId::new();
    stack
        .front
        .open_session(SessionGrant {
            run_id: other,
            workspace_id: WorkspaceId::new(),
            agent_instance_id: agent,
            harness: HarnessKind::ClaudeCode,
            profile: ProgramProfile::Desktop,
            caller: ToolCaller::Agent,
            scope_id: None,
            coding_task_id: None,
        })
        .await
        .expect("open a session for the list");
    let mut offered: Vec<String> = stack
        .front
        .offered_tools(other)
        .expect("the offered tools")
        .iter()
        .map(|tool| format!("mcp__{}__{}", tool.server.as_str(), tool.tool))
        .collect();
    stack.front.close_session(other).await.expect("close");
    let mut mcp = mcp;
    mcp.sort();
    offered.sort();
    built_in.sort();
    let mut expected: Vec<String> = DESKTOP_BUILT_IN_TOOLS
        .iter()
        .map(|tool| (*tool).to_owned())
        .collect();
    expected.sort();
    stack.stop().await;
    assert_eq!(
        (built_in, mcp),
        (expected, offered),
        "(built-in, MCP) tools at {PINNED}; init servers: {}",
        init["mcp_servers"]
    );
}

/// The child's environment, seen from the model server: the key, the gateway and a declared setting
/// reach it, and a reserved name never starts a session.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn the_child_environment_reaches_the_model_stand_in() {
    let dir = TempDir::new();
    let server = stand_in(vec![]).await;
    let declared = vec![HarnessEnvVar::new(
        "ANTHROPIC_CUSTOM_HEADERS".to_owned(),
        "x-declared-setting: eu-1".to_owned(),
    )];
    let stack = agent_stack(
        &dir.0,
        models_at(&server.uri(), declared),
        hook_binary().clone(),
    )
    .await;
    let (run, _agent) = stack.start_run(Authority::DocumentOnly).await;
    let events = events_of(&stack, run).await;
    assert_eq!(final_message(&events).as_deref(), Some(FINAL_TEXT));
    stack.stop().await;

    let requests = server.received_requests().await.expect("recording is on");
    let messages: Vec<&Request> = requests
        .iter()
        .filter(|request| request.url.path().ends_with("/v1/messages"))
        .collect();
    assert!(!messages.is_empty(), "every request reached the stand-in");
    for request in messages {
        assert_eq!(request.headers.get("x-api-key").unwrap(), FAKE_KEY);
        assert!(
            request.headers.get("authorization").is_none(),
            "no sign-in used"
        );
        assert_eq!(request.headers.get("x-declared-setting").unwrap(), "eu-1");
    }

    let before = server.received_requests().await.expect("recording").len();
    let refused = ClaudeLocalAdapter::new(
        ClaudeAdapterConfig {
            claude_path: wrapper(&dir.0, None),
            hook_program: hook_binary().clone(),
            app_data_root: dir.0.join("other-root"),
            profile: ProgramProfile::AgentProfile,
            tested_versions: vec![PINNED.to_owned()],
            sign_in: SignInMode::OrganisationKey {
                models: models_at(
                    &server.uri(),
                    vec![HarnessEnvVar::new(
                        "CLAUDE_CONFIG_DIR".to_owned(),
                        "/elsewhere".to_owned(),
                    )],
                ),
                route: KeyRoute::Environment,
            },
            stop_deadline: Duration::from_secs(1),
        },
        stack.front.clone() as Arc<McpFront>,
        stack.relay.clone() as Arc<HookRelay>,
        stack.engine.clone(),
        Arc::new(support::NoSetup),
        Some(Arc::new(support::NoRecords) as Arc<dyn SessionRecordStore>),
    );
    assert_eq!(
        refused.expect_err("a reserved name").code(),
        "CLA_ENV_RESERVED"
    );
    assert_eq!(
        server.received_requests().await.expect("recording").len(),
        before
    );
}

/// Behavioural check (plan 01, Phase 7): a session started by the adapter in `--restricted` mode
/// makes one allowed and one denied tool call. Both reach the Rules Engine through the hook, and
/// the denied one never runs. The hook sits on a path with a space, as it does on macOS.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn behavioural_check_an_allowed_and_a_denied_call_reach_the_rules_engine() {
    let dir = TempDir::new();
    let inside = dir.0.join("agent").join("notes.md");
    let outside = dir.0.join("outside.md");
    std::fs::create_dir_all(dir.0.join("agent")).expect("the agent folder");
    std::fs::write(&inside, "allowed-content-1234").expect("the allowed file");
    std::fs::write(&outside, "denied-content-5678").expect("the denied file");
    let server = stand_in(vec![
        ("toolu_allowed", inside.display().to_string()),
        ("toolu_denied", outside.display().to_string()),
    ])
    .await;
    let hook = hook_with_a_space(&dir.0);
    let stack = agent_stack(&dir.0, models_at(&server.uri(), vec![]), hook).await;
    let (run, _agent) = stack.start_run(Authority::DocumentOnly).await;
    let events = events_of(&stack, run).await;
    assert_eq!(final_message(&events).as_deref(), Some(FINAL_TEXT));
    assert_eq!(stack.wait_end(run).await.0, RunState::Completed);

    // The session ran in `--restricted` mode.
    let args = std::fs::read_to_string(dir.0.join("args")).expect("the arguments");
    assert!(args.lines().any(|arg| arg == "--restricted"));

    // Both calls reached the Rules Engine through the hook.
    let rows = stack.requests(run).await;
    assert_eq!(rows.len(), 2, "two journalled decisions: {rows:?}");
    assert_eq!(rows[0].0, "Read");
    assert_eq!(
        (rows[0].1.as_str(), rows[0].2.as_str()),
        ("allow", "executed")
    );
    assert_eq!(rows[1].0, "Read");
    assert_eq!(rows[1].1, "deny");
    assert_ne!(rows[1].2, "executed");

    // The denied call never ran: the model saw the first file and an error for the second.
    let requests = server.received_requests().await.expect("recording is on");
    let bodies: Vec<String> = requests
        .iter()
        .map(|request| String::from_utf8_lossy(&request.body).to_string())
        .collect();
    assert!(bodies
        .iter()
        .any(|body| body.contains("allowed-content-1234")));
    assert!(!bodies
        .iter()
        .any(|body| body.contains("denied-content-5678")));
    let progress = events
        .iter()
        .filter(|event| matches!(event, RunEvent::Progress { .. }))
        .count();
    assert_eq!(progress, 1, "one tool call ran: {events:?}");
    let lines = stream_lines(&dir.0);
    let denied = lines
        .iter()
        .filter(|line| line["type"] == "user")
        .flat_map(|line| {
            line["message"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .find(|block| block["tool_use_id"] == "toolu_denied")
        .expect("the denied call's result");
    assert_eq!(denied["is_error"], true);
    stack.stop().await;
}

/// FU-075, run on the real CLI: a package server started from `mcp.json` holds what it declared and
/// not the session's secret or key.
#[tokio::test]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn a_package_server_receives_only_what_it_declares() {
    let dir = TempDir::new();
    let seen = dir.0.join("server-env");
    let server_script = dir.0.join("package-server");
    write_script(
        &server_script,
        &format!(
            "#!/bin/sh\nfor a in \"$@\"; do echo \"ARG=$a\"; done > '{s}'\nenv >> '{s}'\ncat > /dev/null\n",
            s = seen.display()
        ),
    );
    let server = stand_in(vec![]).await;
    let agent_folder = dir.0.join("agent");
    std::fs::create_dir_all(&agent_folder).expect("the agent folder");
    let environment = vec![HarnessEnvVar::new(
        "GATEWAY_TOKEN".to_owned(),
        "gateway-secret".to_owned(),
    )];
    let stack = Stack::start_edited(
        &dir.0,
        Options {
            claude: wrapper(&dir.0, None),
            hook: hook_binary().clone(),
            profile: ProgramProfile::AgentProfile,
            sign_in: SignInMode::OrganisationKey {
                models: models_at(&server.uri(), environment),
                route: KeyRoute::Environment,
            },
            tested_versions: vec![PINNED.to_owned()],
            agent_folder,
        },
        |setup| {
            setup.package_servers = vec![PackageMcpServer {
                name: "tools".to_owned(),
                command: server_script.clone(),
                args: vec![
                    "${ORDIN_HOOK_SECRET}".to_owned(),
                    "${ANTHROPIC_API_KEY}".to_owned(),
                ],
                env: [
                    ("DECLARED".to_owned(), "by-the-package".to_owned()),
                    ("EXPANDED".to_owned(), "${ANTHROPIC_API_KEY}".to_owned()),
                ]
                .into(),
            }];
        },
    )
    .await;
    let (run, _agent) = stack.start_run(Authority::DocumentOnly).await;
    events_of(&stack, run).await;
    stack.wait_end(run).await;
    stack.stop().await;
    let env = std::fs::read_to_string(&seen).expect("the server started");
    assert!(env.contains("DECLARED=by-the-package"), "{env}");
    // On 2.1.316, written into `mcp.json` these three arrive expanded: 43 characters of hook secret
    // and the model key. Out of it they stay as the package wrote them.
    assert!(env.contains("ARG=${ORDIN_HOOK_SECRET}"), "{env}");
    assert!(env.contains("ARG=${ANTHROPIC_API_KEY}"), "{env}");
    assert!(env.contains("EXPANDED=${ANTHROPIC_API_KEY}"), "{env}");
    let without_literals = env
        .replace("${ORDIN_HOOK_SECRET}", "")
        .replace("${ANTHROPIC_API_KEY}", "");
    let env = without_literals;
    for held in [
        "ORDIN_HOOK_SECRET",
        "ORDIN_HOOK_ENDPOINT",
        "ORDIN_SESSION_ID",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        FAKE_KEY,
        "GATEWAY_TOKEN",
        "gateway-secret",
        "CLAUDE_CODE_SESSION_ID",
    ] {
        assert!(!env.contains(held), "{held} reached the server: {env}");
    }
}

/// A plugin with the command `hello` and the skill `sk`, each declaring `hooks` that mark a file
/// under `marks`.
fn plugin_with_hooks(agent: &Path, marks: &Path) {
    let plugin = agent.join("plugin");
    std::fs::create_dir_all(plugin.join(".claude-plugin")).expect("the plugin");
    std::fs::create_dir_all(plugin.join("commands")).expect("the commands");
    std::fs::create_dir_all(plugin.join("skills").join("sk")).expect("the skill");
    std::fs::create_dir_all(marks).expect("the marks");
    std::fs::write(
        plugin.join(".claude-plugin").join("plugin.json"),
        r#"{"name":"analyst","version":"1.0.0","description":"x"}"#,
    )
    .expect("the manifest");
    let mut hooks = String::from("hooks:\n");
    for event in ["PreToolUse", "PostToolUse", "Stop"] {
        let matcher = if event == "Stop" {
            "    - hooks:\n"
        } else {
            "    - matcher: \"*\"\n      hooks:\n"
        };
        hooks.push_str(&format!(
            "  {event}:\n{matcher}        - type: command\n          command: touch {}/{event}\n",
            marks.display()
        ));
    }
    std::fs::write(
        plugin.join("commands").join("hello.md"),
        format!("---\ndescription: x\n{hooks}---\nCOMMAND-BODY-4711\n"),
    )
    .expect("the command");
    std::fs::write(
        plugin.join("skills").join("sk").join("SKILL.md"),
        format!("---\nname: sk\ndescription: x\n{hooks}---\nSKILL-BODY-4712\n"),
    )
    .expect("the skill file");
}

/// What the model is sent, and which marks appeared, for `input` with [`plugin_with_hooks`].
async fn sent_for(input: StartInput) -> (String, Vec<String>) {
    let dir = TempDir::new();
    let agent = dir.0.join("agent");
    let marks = dir.0.join("marks");
    plugin_with_hooks(&agent, &marks);
    let notes = agent.join("notes.md");
    std::fs::write(&notes, "x").expect("the file");
    let server = stand_in(vec![("toolu_a", notes.display().to_string())]).await;
    let stack = Stack::start(
        &dir.0,
        Options {
            claude: wrapper(&dir.0, None),
            hook: hook_binary().clone(),
            profile: ProgramProfile::AgentProfile,
            sign_in: SignInMode::OrganisationKey {
                models: models_at(&server.uri(), vec![]),
                route: KeyRoute::Environment,
            },
            tested_versions: vec![PINNED.to_owned()],
            agent_folder: agent,
        },
    )
    .await;
    let (run, _agent) = stack
        .start_input_run(Authority::DocumentOnly, None, input)
        .await;
    events_of(&stack, run).await;
    stack.wait_end(run).await;
    stack.stop().await;
    let mut ran: Vec<String> = std::fs::read_dir(&marks)
        .expect("the marks")
        .flatten()
        .map(|mark| mark.file_name().to_string_lossy().into_owned())
        .collect();
    ran.sort();
    (bodies_of(&server).await.join("\n"), ran)
}

/// FU-076, run on the real CLI: `/` as free text is text; `Command` is how a command runs.
#[tokio::test]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn free_text_starting_with_a_slash_runs_no_command() {
    let (sent, _) = sent_for(StartInput::FreeText {
        text: "/analyst:hello".to_owned(),
    })
    .await;
    assert!(!sent.contains("COMMAND-BODY-4711"), "{sent}");
    assert!(sent.contains("/analyst:hello"), "{sent}");
    let (sent, _) = sent_for(StartInput::Command {
        name: "hello".to_owned(),
        arguments: String::new(),
    })
    .await;
    assert!(sent.contains("COMMAND-BODY-4711"), "the command ran");
}

/// What makes `hooks` in front matter a refusal at install (Installers.md SA-RECON-06), observed on
/// 2.1.316 under `--restricted`: a skill's run as shell commands, a command's do not.
#[tokio::test]
#[ignore = "harness job: needs Claude Code 2.1.316"]
async fn a_skill_s_front_matter_hooks_run_as_shell_commands() {
    let (sent, ran) = sent_for(StartInput::Command {
        name: "sk".to_owned(),
        arguments: String::new(),
    })
    .await;
    assert!(sent.contains("SKILL-BODY-4712"), "the skill ran");
    assert_eq!(ran, ["PostToolUse", "PreToolUse", "Stop"]);
    let (sent, ran) = sent_for(StartInput::Command {
        name: "hello".to_owned(),
        arguments: String::new(),
    })
    .await;
    assert!(sent.contains("COMMAND-BODY-4711"), "the command ran");
    assert!(ran.is_empty(), "{ran:?}");
}
