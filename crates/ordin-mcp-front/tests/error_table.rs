//! One test per row of the Error Table of McpFront.md (X1). Tool-call failures are checked by the
//! text the harness receives and by the `code` of the `mcp_front.call` log line, because the MCP
//! answer carries no code; sandbox-callable tools are checked through `ToolDispatch`, which does.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ordin_approval::PressOrigin;
use ordin_mcp_front::{McpError, ServerName, ToolDispatch, ToolFailure, ToolRegistry};
use ordin_service_api::blocks::{
    BlockRejection, CheckedReport, PlanState, ReportBlock, ReportToolArgs,
};
use ordin_types::wire::approval::{ApprovalPress, PressDecision};
use ordin_types::RunId;
use serde_json::{json, Value};
use support::*;
use tokio_util::sync::CancellationToken;

fn q(text: &str) -> Value {
    json!({ "query": text })
}

async fn decide_when_pending(env: &Env, decision: PressDecision) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let pending = env.approvals.list_pending(None, None).await.unwrap().items;
        if let Some(record) = pending.first() {
            let card_ref = match decision {
                PressDecision::Approve => record.card.buttons.approve.clone(),
                PressDecision::Reject => record.card.buttons.reject.clone(),
            };
            env.approvals
                .decide(
                    ApprovalPress { card_ref, decision },
                    PressOrigin::LocalScreen,
                )
                .await
                .expect("decide");
            return;
        }
        assert!(std::time::Instant::now() < deadline, "no pending approval");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_pending(env: &Env) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while env
        .approvals
        .list_pending(None, None)
        .await
        .unwrap()
        .items
        .is_empty()
    {
        assert!(std::time::Instant::now() < deadline, "no pending approval");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn one_plain(name: &str) -> Arc<TestProvider> {
    TestProvider::new("one", vec![server("one", vec![spec(name, false)])])
}

// ---- registration ----

#[tokio::test]
async fn error_table_server_name_breaks_the_pattern() {
    let error = ServerName::new("Not Valid").unwrap_err();
    assert_eq!(error.code(), "MCP_INVALID_NAME");
    assert!(!error.retryable());
    assert!(matches!(error, McpError::InvalidName { value } if value == "Not Valid"));
}

#[tokio::test]
async fn error_table_register_after_start() {
    let (env, _s, _m) = started(|_| {}).await;
    let error = env.front.register(one_plain("t")).unwrap_err();
    assert_eq!(error.code(), "MCP_REGISTRY_FROZEN");
    assert!(!error.retryable());
}

#[tokio::test]
async fn error_table_same_provider_registered_twice() {
    let env = env(|_| {}).await;
    env.front.register(one_plain("t")).unwrap();
    let error = env
        .front
        .register(TestProvider::new(
            "one",
            vec![server("two", vec![spec("t", false)])],
        ))
        .unwrap_err();
    assert_eq!(error.code(), "MCP_PROVIDER_DUPLICATE");
    assert!(matches!(
        error,
        McpError::ProviderDuplicate { provider_id: "one" }
    ));
    // The built-in provider's id is taken too.
    let builtin = TestProvider::new(
        "mcp-front-work",
        vec![server("other", vec![spec("t", false)])],
    );
    assert_eq!(
        env.front.register(builtin).unwrap_err().code(),
        "MCP_PROVIDER_DUPLICATE"
    );
}

#[tokio::test]
async fn error_table_server_claimed_by_another_provider() {
    let env = env(|_| {}).await;
    env.front.register(one_plain("t")).unwrap();
    // The same name, the built-in `work` name, and names and prefixes that overlap a prefix.
    let same = TestProvider::new("same", vec![server("one", vec![spec("t", false)])]);
    assert!(matches!(
        env.front.register(same),
        Err(McpError::ServerClaimed { by: "one", .. })
    ));
    let work = TestProvider::new("w", vec![server("work", vec![spec("t", false)])]);
    assert!(matches!(
        env.front.register(work),
        Err(McpError::ServerClaimed {
            by: "mcp-front-work",
            ..
        })
    ));
    env.front
        .register(TestProvider::with_prefix("craft", "craft_"))
        .unwrap();
    let under = TestProvider::new("under", vec![server("craft_x", vec![spec("t", false)])]);
    assert!(matches!(
        env.front.register(under),
        Err(McpError::ServerClaimed { by: "craft", .. })
    ));
    let overlap = TestProvider::with_prefix("craftier", "craft_");
    assert!(matches!(
        env.front.register(overlap),
        Err(McpError::ServerClaimed { by: "craft", .. })
    ));
    let narrower = TestProvider::with_prefix("narrow", "cr_");
    env.front.register(narrower).unwrap();
    let shorter = TestProvider::with_prefix("shorter", "c_");
    env.front.register(shorter).unwrap();
    let takes_one = TestProvider::new("takes", vec![server("one_more", vec![spec("t", false)])]);
    // "one_more" overlaps nobody's prefix, so it registers.
    env.front.register(takes_one).unwrap();
    let prefix_over_name = TestProvider::with_prefix("overname", "one_");
    assert!(matches!(
        env.front.register(prefix_over_name),
        Err(McpError::ServerClaimed { .. })
    ));
}

#[tokio::test]
async fn error_table_tool_spec_breaks_a_rule() {
    let env = env(|_| {}).await;
    type Change = Box<dyn Fn(&mut ordin_mcp_front::ToolSpec)>;
    let cases: Vec<(&str, Change)> = vec![
        ("name", Box::new(|s| s.name = "9bad".to_owned())),
        ("description", Box::new(|s| s.description = String::new())),
        (
            "description",
            Box::new(|s| s.description = "two\nlines".to_owned()),
        ),
        ("description", Box::new(|s| s.description = "d".repeat(121))),
        (
            "argument_form",
            Box::new(|s| s.argument_form = String::new()),
        ),
        (
            "argument_form",
            Box::new(|s| s.argument_form = "a".repeat(301)),
        ),
        ("plain", Box::new(|s| s.plain = String::new())),
        ("plain", Box::new(|s| s.plain = "p".repeat(201))),
        (
            "input_schema",
            Box::new(|s| s.input_schema = json!({"type": "string"})),
        ),
        ("input_schema", Box::new(|s| s.input_schema = json!({}))),
    ];
    for (i, (rule, change)) in cases.iter().enumerate() {
        let mut bad = spec("tool", false);
        change(&mut bad);
        let id: &'static str = Box::leak(format!("p{i}").into_boxed_str());
        let provider = TestProvider::new(id, vec![server(&format!("s{i}"), vec![bad])]);
        match env.front.register(provider) {
            Err(McpError::ToolSpecInvalid {
                rule: got,
                provider_id,
                ..
            }) => {
                assert_eq!(&got, rule, "case {i}");
                assert_eq!(provider_id, id);
            }
            other => panic!("case {i}: {other:?}"),
        }
    }
    // A repeated tool name, a bad prefix, and a repeated server name.
    let twice = TestProvider::new(
        "twice",
        vec![server("tw", vec![spec("t", false), spec("t", false)])],
    );
    assert!(matches!(
        env.front.register(twice),
        Err(McpError::ToolSpecInvalid {
            rule: "name_unique",
            ..
        })
    ));
    let prefix = TestProvider::with_prefix("badprefix", "Craft");
    assert!(matches!(
        env.front.register(prefix),
        Err(McpError::ToolSpecInvalid {
            rule: "server_prefix",
            ..
        })
    ));
    let doubled = TestProvider::new("dbl", vec![server("a", vec![]), server("a", vec![])]);
    assert!(matches!(
        env.front.register(doubled),
        Err(McpError::ToolSpecInvalid {
            rule: "server_name",
            ..
        })
    ));
    // The description limit is 1,024 in the agent profile.
    let agent = self::env(|c| c.profile = ordin_mcp_front::ProgramProfile::AgentProfile).await;
    let mut long = spec("tool", false);
    long.description = "d".repeat(1025);
    let provider = TestProvider::new("long", vec![server("agent", vec![long])]);
    assert!(matches!(
        agent.front.register(provider),
        Err(McpError::ToolSpecInvalid {
            rule: "description",
            ..
        })
    ));
}

#[tokio::test]
async fn error_table_server_under_a_prefix_appears_with_a_broken_spec() {
    let craft = TestProvider::with_prefix("craft-toolkit", "craft_");
    let (env, _s, _m) = started_with(|_| {}, vec![craft.clone()]).await;
    let mut broken = spec("convert", false);
    broken.description = String::new();
    craft
        .specs
        .lock()
        .unwrap()
        .push(server("craft_bad", vec![broken, spec("fine", false)]));
    craft.set_offer(|_| Ok(vec![key("craft_bad", "convert"), key("craft_bad", "fine")]));
    let session = env.open(&[], &[]).await;
    let tools = env.front.offered_tools(session.run).unwrap();
    assert!(tools
        .iter()
        .any(|t| t.server.as_str() == "craft_bad" && t.tool == "fine"));
    assert!(
        !tools.iter().any(|t| t.tool == "convert"),
        "the broken spec is not offered"
    );
    assert!(!log_lines(&["mcp_front.offer_ignored", "convert"]).is_empty());
}

#[tokio::test]
async fn error_table_tools_changed_offer_fails_or_times_out() {
    let mail = TestProvider::new(
        "mail-service",
        vec![server("mail", vec![spec("email_search", true)])],
    );
    let (env, _s, _m) = started_with(|_| {}, vec![mail.clone()]).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("mail").await;
    assert_eq!(bridge.list().await, ["email_search"]);
    // The offer takes longer than its 2 s.
    *mail.offer_delay.lock().unwrap() = Duration::from_secs(3);
    env.front.tools_changed(&ServerName::new("mail").unwrap());
    assert_eq!(
        bridge.recv().await["method"],
        "notifications/tools/list_changed"
    );
    assert!(bridge.list().await.is_empty());
    assert!(!log_lines(&["mcp_front.tools_changed_failed", "MCP_PROVIDER_FAILED"]).is_empty());
}

#[tokio::test]
async fn error_table_tools_changed_for_an_unserved_server_or_before_start() {
    let env = env(|_| {}).await;
    env.front.register(one_plain("t")).unwrap();
    // Before start: nothing happens.
    env.front.tools_changed(&ServerName::new("one").unwrap());
    env.front.bind_ref_resolver(env.resolver.clone()).unwrap();
    env.front.start(CancellationToken::new()).await.unwrap();
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("one").await;
    env.front.tools_changed(&ServerName::new("nobody").unwrap());
    // No notification arrives; the connection still answers.
    assert!(bridge.quiet(Duration::from_millis(500)).await);
    assert_eq!(bridge.list().await, ["t"]);
    assert!(!log_lines(&["mcp_front.tools_changed_ignored", "nobody"]).is_empty());
}

#[tokio::test]
async fn error_table_resolver_bound_a_second_time() {
    let env = env(|_| {}).await;
    env.front.bind_ref_resolver(env.resolver.clone()).unwrap();
    let error = env
        .front
        .bind_ref_resolver(env.resolver.clone())
        .unwrap_err();
    assert!(matches!(error, McpError::RefResolverAlreadyBound));
    assert_eq!(error.code(), "MCP_REF_RESOLVER_ALREADY_BOUND");
}

#[tokio::test]
async fn error_table_report_or_plan_before_the_resolver_is_bound() {
    let env = env(|_| {}).await;
    env.front.start(CancellationToken::new()).await.unwrap();
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let report = bridge.call(1, "report", table_block("T", 4)).await;
    assert_eq!(text_of(&report), "Reports can't be shown yet.");
    let plan = bridge
        .call(
            2,
            "plan",
            json!({"action": "state", "steps": ["a", "b", "c"]}),
        )
        .await;
    assert_eq!(text_of(&plan), "The plan can't be recorded yet.");
    assert_eq!(
        call_codes(session.run),
        ["MCP_REF_RESOLVER_NOT_BOUND", "MCP_REF_RESOLVER_NOT_BOUND"]
    );
}

// ---- start and sessions ----

#[tokio::test]
async fn error_table_start_called_twice() {
    let (env, _s, _m) = started(|_| {}).await;
    let error = env.front.start(CancellationToken::new()).await.unwrap_err();
    assert!(matches!(error, McpError::AlreadyStarted));
    assert_eq!(error.code(), "MCP_ALREADY_STARTED");
}

// Unix: the socket's folder cannot be made. Windows has no socket folder; see the test below.
#[cfg(unix)]
#[tokio::test]
async fn error_table_socket_cannot_be_bound() {
    let dir = TempDir::new();
    // The data root is a file, so the folder for the socket cannot be made.
    let blocker = dir.0.join("file");
    std::fs::write(&blocker, b"x").unwrap();
    let env = env(|c| c.app_data_root = blocker.clone()).await;
    let error = env.front.start(CancellationToken::new()).await.unwrap_err();
    assert_eq!(error.code(), "MCP_SOCKET_UNAVAILABLE");
    assert!(error.retryable());
    match &error {
        McpError::SocketUnavailable { detail } => {
            assert!(!detail.contains(blocker.to_str().unwrap()))
        }
        other => panic!("{other:?}"),
    }
    // A failed start leaves the hub unstarted and registrable.
    env.front.register(one_plain("t")).unwrap();
}

/// Windows: the pipe name is already held by another process, so the first instance cannot be made.
#[cfg(windows)]
#[tokio::test]
async fn error_table_socket_cannot_be_bound() {
    let installation = uuid::Uuid::now_v7();
    let name = format!(r"\\.\pipe\ordin-mcp-{installation}");
    let _held = tokio::net::windows::named_pipe::ServerOptions::new()
        .first_pipe_instance(true)
        .create(&name)
        .unwrap();
    let env = env(|c| c.installation_id = installation).await;
    let error = env.front.start(CancellationToken::new()).await.unwrap_err();
    assert_eq!(error.code(), "MCP_SOCKET_UNAVAILABLE");
    assert!(error.retryable());
    match &error {
        McpError::SocketUnavailable { detail } => assert!(!detail.contains(&name)),
        other => panic!("{other:?}"),
    }
    // A failed start leaves the hub unstarted and registrable.
    env.front.register(one_plain("t")).unwrap();
}

#[tokio::test]
async fn error_table_open_session_before_start() {
    let env = env(|_| {}).await;
    let agent = env.agent(&[], &[]);
    let (run, workspace) = env.begin_run(agent).await;
    let error = env
        .front
        .open_session(env.grant(agent, run, workspace))
        .await
        .unwrap_err();
    assert!(matches!(error, McpError::NotStarted));
    assert_eq!(error.code(), "MCP_NOT_STARTED");
    assert!(error.retryable());
}

#[tokio::test]
async fn error_table_open_session_for_a_run_with_an_open_session() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let error = env
        .front
        .open_session(env.grant(session.agent, session.run, session.workspace))
        .await
        .unwrap_err();
    assert!(matches!(error, McpError::SessionExists { run_id } if run_id == session.run));
    assert_eq!(error.code(), "MCP_SESSION_EXISTS");
}

#[tokio::test]
async fn error_table_a_provider_offer_fails_or_exceeds_two_seconds() {
    let (env, _s, memory) = started(|_| {}).await;
    let agent = env.agent(&[], &[]);
    let (run, workspace) = env.begin_run(agent).await;
    // A provider failure that says it is retryable.
    memory.set_offer(|_| {
        Err(ToolFailure {
            code: "MEM_BUSY".to_owned(),
            message: "Busy.".to_owned(),
            retryable: true,
        })
    });
    let error = env
        .front
        .open_session(env.grant(agent, run, workspace))
        .await
        .unwrap_err();
    match &error {
        McpError::ProviderFailed {
            provider_id,
            code,
            retryable,
        } => {
            assert_eq!(
                (*provider_id, code.as_str(), *retryable),
                ("memory-service", "MEM_BUSY", true)
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(error.code(), "MCP_PROVIDER_FAILED");
    assert!(error.retryable());
    // A failure that does not.
    memory.set_offer(|_| {
        Err(ToolFailure {
            code: "MEM_OFF".to_owned(),
            message: "Off.".to_owned(),
            retryable: false,
        })
    });
    let error = env
        .front
        .open_session(env.grant(agent, run, workspace))
        .await
        .unwrap_err();
    assert!(!error.retryable());
    // An offer slower than 2 s is a retryable MCP_PROVIDER_FAILED, and no session is kept.
    memory.set_offer(|_| Ok(vec![key("memory", "recall")]));
    *memory.offer_delay.lock().unwrap() = Duration::from_secs(3);
    let error = env
        .front
        .open_session(env.grant(agent, run, workspace))
        .await
        .unwrap_err();
    match &error {
        McpError::ProviderFailed {
            code, retryable, ..
        } => {
            assert_eq!((code.as_str(), *retryable), ("MCP_PROVIDER_FAILED", true));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        env.front.offered_tools(run).unwrap_err().code(),
        "MCP_SESSION_UNKNOWN"
    );
}

#[tokio::test]
async fn error_table_unknown_run() {
    let (env, _s, _m) = started(|_| {}).await;
    let unknown = RunId::new();
    assert!(
        matches!(env.front.offered_tools(unknown), Err(McpError::SessionUnknown { run_id }) if run_id == unknown)
    );
    assert_eq!(
        env.front.session_tools(unknown).unwrap_err().code(),
        "MCP_SESSION_UNKNOWN"
    );
    let dispatched = env
        .front
        .dispatch(
            unknown,
            key("memory", "recall"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(dispatched.code(), "MCP_SESSION_UNKNOWN");
}

// ---- bridge handshake ----

#[tokio::test]
async fn error_table_handshake_malformed() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let refused = json!({"kind": "refused", "code": "MCP_BRIDGE_MALFORMED"});
    // Not JSON.
    let mut bridge = Bridge::dial(&session.endpoint()).await;
    bridge.send_raw("this is not json").await;
    assert_eq!(bridge.recv().await, refused);
    assert!(bridge.recv_opt(Duration::from_secs(5)).await.is_none());
    // Missing fields.
    let mut bridge = Bridge::dial(&session.endpoint()).await;
    bridge.send(json!({"protocol": 1})).await;
    assert_eq!(bridge.recv().await, refused);
    // Over 1 KiB.
    let mut bridge = Bridge::dial(&session.endpoint()).await;
    bridge.send_raw(&"x".repeat(2000)).await;
    assert_eq!(bridge.recv().await, refused);
    // Nothing within 5 s.
    let mut bridge = Bridge::dial(&session.endpoint()).await;
    let waited = std::time::Instant::now();
    assert_eq!(
        bridge.recv_opt(Duration::from_secs(10)).await.unwrap(),
        refused
    );
    assert!(waited.elapsed() >= Duration::from_secs(4));
}

#[tokio::test]
async fn error_table_handshake_protocol_is_not_one() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = Bridge::dial(&session.endpoint()).await;
    bridge
        .send(json!({"protocol": 2, "session": session.run.to_string(), "server": "work", "token": session.token()}))
        .await;
    assert_eq!(
        bridge.recv().await,
        json!({"kind": "refused", "code": "MCP_BRIDGE_PROTOCOL_UNSUPPORTED"})
    );
}

#[tokio::test]
async fn error_table_handshake_session_is_not_open() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let refused = json!({"kind": "refused", "code": "MCP_SESSION_UNKNOWN"});
    let mut bridge = Bridge::dial(&session.endpoint()).await;
    assert_eq!(
        bridge
            .handshake(&RunId::new().to_string(), "work", &session.token())
            .await,
        refused
    );
    let mut bridge = Bridge::dial(&session.endpoint()).await;
    assert_eq!(
        bridge
            .handshake("not-a-run-id", "work", &session.token())
            .await,
        refused
    );
}

#[tokio::test]
async fn error_table_handshake_token_does_not_match() {
    let (env, _s, _m) = started(|_| {}).await;
    let first = env.open(&[], &[]).await;
    let second = env.open(&[], &[]).await;
    // Another session's token is not this session's.
    let mut bridge = Bridge::dial(&first.endpoint()).await;
    let reply = bridge
        .handshake(&first.run.to_string(), "work", &second.token())
        .await;
    assert_eq!(
        reply,
        json!({"kind": "refused", "code": "MCP_BRIDGE_UNAUTHENTICATED"})
    );
}

#[tokio::test]
async fn error_table_handshake_server_not_launched() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    for server in ["mail", "Not Valid"] {
        let mut bridge = Bridge::dial(&session.endpoint()).await;
        let reply = bridge
            .handshake(&session.run.to_string(), server, &session.token())
            .await;
        assert_eq!(
            reply,
            json!({"kind": "refused", "code": "MCP_SERVER_NOT_OFFERED"}),
            "{server}"
        );
    }
}

// ---- the pipeline ----

#[tokio::test]
async fn error_table_more_than_the_permitted_calls_in_flight() {
    let (env, _s, memory) = started(|c| c.max_concurrent_calls_per_session = 1).await;
    memory.set_behaviour(Behaviour::Sleep(
        Duration::from_secs(120),
        "never".to_owned(),
    ));
    let session = env.open(&[], &[]).await;
    let first = {
        let front = env.front.clone();
        let run = session.run;
        tokio::spawn(async move {
            front
                .dispatch(
                    run,
                    key("memory", "recall"),
                    q("a"),
                    CancellationToken::new(),
                )
                .await
        })
    };
    while memory.call_count() == 0 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let started_at = std::time::Instant::now();
    let second = env
        .front
        .dispatch(
            session.run,
            key("memory", "recall"),
            q("b"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(started_at.elapsed() >= Duration::from_secs(29));
    assert!(second.is_error);
    assert_eq!(second.code.as_deref(), Some("MCP_TOO_MANY_CALLS"));
    assert_eq!(
        second.text,
        "Too many calls at once; wait for one to finish."
    );
    env.front.close_session(session.run).await.unwrap();
    let _ = first.await;
}

#[tokio::test]
async fn error_table_tool_not_offered_to_the_session() {
    let (env, _s, memory) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("memory").await;
    let answer = bridge.call(1, "nope", q("x")).await;
    assert_eq!(
        text_of(&answer),
        "This tool is not offered to this session."
    );
    assert_eq!(call_codes(session.run), ["MCP_TOOL_NOT_OFFERED"]);
    assert_eq!(memory.call_count(), 0);
    assert_eq!(env.tool_requests(session.run).await, 0);
}

#[tokio::test]
async fn error_table_arguments_above_one_mebibyte() {
    let (env, _s, memory) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let result = env
        .front
        .dispatch(
            session.run,
            key("memory", "recall"),
            q(&"x".repeat(1024 * 1024 + 10)),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.code.as_deref(), Some("MCP_ARGUMENTS_TOO_LARGE"));
    assert_eq!(
        result.text,
        "The arguments are larger than 1 MiB; pass a reference to the document instead."
    );
    assert_eq!(memory.call_count(), 0);
    assert_eq!(env.tool_requests(session.run).await, 0);
}

#[tokio::test]
async fn error_table_check_arguments_refuses() {
    let (env, _s, memory) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let result = env
        .front
        .dispatch(
            session.run,
            key("memory", "recall"),
            json!({"bogus": 1}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.code.as_deref(), Some("MCP_ARGUMENTS_INVALID"));
    assert!(
        result
            .text
            .starts_with("Wrong arguments for recall at /bogus: "),
        "{}",
        result.text
    );
    assert!(
        result.text.ends_with("Correct form: recall(query: string)"),
        "{}",
        result.text
    );
    assert_eq!(memory.call_count(), 0);
    assert_eq!(env.tool_requests(session.run).await, 0);
}

#[tokio::test]
async fn error_table_waiting_for_the_approval_fails() {
    let (env, _s, memory) = started(|c| c.approval_wait = Duration::from_secs(20)).await;
    env.sql("CREATE TRIGGER block_used BEFORE UPDATE ON rules_approved_calls BEGIN SELECT RAISE(ABORT, 'blocked'); END")
        .await;
    let session = env.open(&[], &["remember"]).await;
    let front = env.front.clone();
    let run = session.run;
    let call = tokio::spawn(async move {
        front
            .dispatch(
                run,
                key("memory", "remember"),
                q("x"),
                CancellationToken::new(),
            )
            .await
    });
    decide_when_pending(&env, PressDecision::Approve).await;
    let result = call.await.unwrap().unwrap();
    assert_eq!(result.code.as_deref(), Some("MCP_DECISION_FAILED"));
    assert_eq!(
        result.text,
        "This call could not be checked, so it was not run."
    );
    assert_eq!(memory.call_count(), 0);
    assert!(!log_lines(&["mcp_front.decision_failed", &run.to_string()]).is_empty());
}

#[tokio::test]
async fn error_table_the_approval_expired() {
    let (env, _s, memory) = started(|c| c.approval_wait = Duration::from_secs(20)).await;
    let session = env.open(&[], &["remember"]).await;
    let front = env.front.clone();
    let run = session.run;
    let call = tokio::spawn(async move {
        front
            .dispatch(
                run,
                key("memory", "remember"),
                q("x"),
                CancellationToken::new(),
            )
            .await
    });
    wait_for_pending(&env).await;
    env.approvals.expire_for_run(run).await.unwrap();
    let result = call.await.unwrap().unwrap();
    assert_eq!(result.code.as_deref(), Some("MCP_REJECTED"));
    assert_eq!(result.text, "The approval expired without a decision.");
    assert_eq!(memory.call_count(), 0);
}

#[tokio::test]
async fn error_table_report_cannot_be_saved() {
    let (env, _s, _m) = started(|_| {}).await;
    env.sql("CREATE TRIGGER no_reports BEFORE INSERT ON reports BEGIN SELECT RAISE(ABORT, 'blocked'); END")
        .await;
    let parsed: ReportBlock = serde_json::from_value({
        let mut v = report_block(2);
        v.as_object_mut().unwrap().remove("kind");
        v
    })
    .unwrap();
    *env.check.report.lock().unwrap() = Some(Ok(CheckedReport {
        block: ReportToolArgs::Report(parsed),
        dropped_parts: Vec::new(),
    }));
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge.call(1, "report", report_block(2)).await;
    assert!(is_error(&answer));
    assert_eq!(
        text_of(&answer),
        "The report could not be saved; try again."
    );
    assert_eq!(call_codes(session.run), ["MCP_JOURNAL_FAILED"]);
    assert!(env.events("block_shown").await.is_empty());
}

// A run has one multi-part report (`reports.run_id`). The second call is a refusal the agent can act
// on, not a failure it would retry for ever. FU-046.
#[tokio::test]
async fn error_table_a_second_report_in_one_run_is_refused_for_good() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let first = bridge.call(1, "report", report_block(2)).await;
    assert!(!is_error(&first), "{first}");
    let second = bridge.call(2, "report", report_block(2)).await;
    assert!(is_error(&second));
    assert_eq!(
        text_of(&second),
        "This run already has its report. Show anything more as a table, a chart or key figures."
    );
    assert_eq!(
        call_codes(session.run),
        ["", "REPORT_RUN_HAS_REPORT"],
        "the second call is the only failure"
    );
    let stored = env.journal.report_for_run(session.run).await.unwrap();
    assert!(stored.is_some());
    assert_eq!(env.events("block_shown").await.len(), 1);
}

#[tokio::test]
async fn error_table_the_rules_engine_denies() {
    let (env, sandbox, _m) = started(|_| {}).await;
    let session = env.open(&["python_sandbox"], &[]).await;
    let mut bridge = session.connect("sandbox").await;
    let answer = bridge.call(1, "python_sandbox", q("x")).await;
    assert!(text_of(&answer).starts_with("Refused by the agent's tool policy"));
    assert_eq!(call_codes(session.run), ["MCP_DENIED"]);
    assert_eq!(sandbox.call_count(), 0);
}

#[tokio::test]
async fn error_table_the_person_rejects() {
    let (env, _s, memory) = started(|c| c.approval_wait = Duration::from_secs(20)).await;
    let session = env.open(&[], &["remember"]).await;
    let mut bridge = session.connect("memory").await;
    let (answer, _) = tokio::join!(
        bridge.call(1, "remember", q("x")),
        decide_when_pending(&env, PressDecision::Reject)
    );
    assert!(is_error(&answer));
    assert_eq!(text_of(&answer), "The person rejected this.");
    assert_eq!(call_codes(session.run), ["MCP_REJECTED"]);
    assert_eq!(memory.call_count(), 0);
}

#[tokio::test]
async fn error_table_no_decision_within_the_approval_wait() {
    let (env, _s, memory) = started(|c| c.approval_wait = Duration::from_millis(200)).await;
    let session = env.open(&[], &["remember"]).await;
    let result = env
        .front
        .dispatch(
            session.run,
            key("memory", "remember"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.code.as_deref(), Some("MCP_AWAITING_APPROVAL"));
    assert_eq!(
        result.text,
        "Waiting for the person's approval. Call again with the same arguments once they have decided."
    );
    assert_eq!(memory.call_count(), 0);
}

#[tokio::test]
async fn error_table_provider_call_exceeds_the_call_timeout() {
    let (env, _s, memory) = started(|c| c.call_timeout = Duration::from_millis(300)).await;
    memory.set_behaviour(Behaviour::Sleep(Duration::from_secs(5), "late".to_owned()));
    let session = env.open(&[], &[]).await;
    let result = env
        .front
        .dispatch(
            session.run,
            key("memory", "recall"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.code.as_deref(), Some("MCP_CALL_TIMEOUT"));
    assert_eq!(result.text, "The tool took too long and was stopped.");
    assert!(memory.tokens.lock().unwrap()[0].is_cancelled());
}

#[tokio::test]
async fn error_table_call_cancelled_by_the_harness_or_session_close() {
    let (env, _s, memory) = started(|_| {}).await;
    memory.set_behaviour(Behaviour::Sleep(
        Duration::from_secs(60),
        "never".to_owned(),
    ));
    let session = env.open(&[], &[]).await;
    // Cancelled by the harness: no answer, and the call's token is cancelled.
    let mut bridge = session.connect("memory").await;
    bridge
        .send(json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "recall", "arguments": {"query": "x"}}}))
        .await;
    while memory.call_count() == 0 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    bridge
        .send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 7}}))
        .await;
    assert!(
        bridge.quiet(Duration::from_millis(800)).await,
        "a cancelled call gets no answer"
    );
    let waited = std::time::Instant::now();
    while !memory.tokens.lock().unwrap()[0].is_cancelled() {
        assert!(waited.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // The connection answers other requests, and no answer for call 7 arrives.
    assert_eq!(bridge.list().await, ["recall", "remember"]);
    assert!(call_codes(session.run).contains(&"MCP_CALL_CANCELLED".to_owned()));
    // Cancelled by session close.
    let front = env.front.clone();
    let run = session.run;
    let again = tokio::spawn(async move {
        front
            .dispatch(
                run,
                key("memory", "recall"),
                q("y"),
                CancellationToken::new(),
            )
            .await
    });
    while memory.call_count() < 2 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    env.front.close_session(session.run).await.unwrap();
    let result = again.await.unwrap().unwrap();
    assert_eq!(result.code.as_deref(), Some("MCP_SESSION_CLOSED"));
}

#[tokio::test]
async fn error_table_program_shutting_down() {
    let env = env(|_| {}).await;
    let memory = TestProvider::new(
        "memory-service",
        vec![server("memory", vec![spec("recall", true)])],
    );
    memory.set_behaviour(Behaviour::Sleep(
        Duration::from_secs(60),
        "never".to_owned(),
    ));
    env.front.register(memory.clone()).unwrap();
    env.front.bind_ref_resolver(env.resolver.clone()).unwrap();
    let shutdown = CancellationToken::new();
    env.front.start(shutdown.clone()).await.unwrap();
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("memory").await;
    bridge
        .send(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "recall", "arguments": {"query": "x"}}}))
        .await;
    while memory.call_count() == 0 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    shutdown.cancel();
    let answer = bridge.recv().await;
    assert_eq!(answer["id"], 3);
    assert_eq!(
        text_of(&answer),
        "ordin is stopping; try again after it restarts."
    );
    assert!(is_error(&answer));
    assert_eq!(call_codes(session.run), ["MCP_SHUTTING_DOWN"]);
}

#[tokio::test]
async fn error_table_provider_returns_a_tool_failure() {
    let (env, _s, memory) = started(|_| {}).await;
    memory.set_behaviour(Behaviour::Fail(ToolFailure {
        code: "MEM_NO_FACT".to_owned(),
        message: "There is no such fact.".to_owned(),
        retryable: false,
    }));
    let session = env.open(&[], &[]).await;
    let result = env
        .front
        .dispatch(
            session.run,
            key("memory", "recall"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(result.is_error);
    assert_eq!(result.code.as_deref(), Some("MEM_NO_FACT"));
    assert_eq!(result.text, "There is no such fact.");
}

#[tokio::test]
async fn error_table_provider_panics() {
    let (env, _s, memory) = started(|_| {}).await;
    memory.set_behaviour(Behaviour::Panic);
    let session = env.open(&[], &[]).await;
    let result = env
        .front
        .dispatch(
            session.run,
            key("memory", "recall"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.code.as_deref(), Some("MCP_PROVIDER_PANICKED"));
    assert_eq!(result.text, "The tool failed.");
    assert!(!log_lines(&["mcp_front.provider_panicked", "memory-service"]).is_empty());
    // The hub is unharmed: the next call runs.
    memory.set_behaviour(Behaviour::Text("fine".to_owned()));
    let next = env
        .front
        .dispatch(
            session.run,
            key("memory", "recall"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!next.is_error);
}

#[tokio::test]
async fn error_table_report_fails_the_block_check_entirely() {
    let (env, _s, _m) = started(|_| {}).await;
    *env.check.report.lock().unwrap() = Some(Err(vec![BlockRejection {
        code: "BLOCK_KIND_UNKNOWN".to_owned(),
        at: "/kind".to_owned(),
        reason: "Use one of report, table, chart or key_figures.".to_owned(),
    }]));
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge.call(1, "report", table_block("T", 4)).await;
    assert!(is_error(&answer));
    assert_eq!(
        text_of(&answer),
        "Use one of report, table, chart or key_figures."
    );
    assert_eq!(
        answer["result"]["structuredContent"]["rejections"][0]["at"],
        "/kind"
    );
    assert_eq!(call_codes(session.run), ["BLOCK_REPORT_REFUSED"]);
    assert!(env.events("block_shown").await.is_empty());
}

#[tokio::test]
async fn error_table_plan_state_breaks_the_step_rules() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge
        .call(
            1,
            "plan",
            json!({"action": "state", "steps": ["only", "two"]}),
        )
        .await;
    assert!(is_error(&answer));
    assert!(!text_of(&answer).is_empty());
    let long = bridge
        .call(
            2,
            "plan",
            json!({"action": "state", "steps": ["a", "b", "x".repeat(81)]}),
        )
        .await;
    assert!(is_error(&long));
    assert_eq!(
        call_codes(session.run),
        ["BLOCK_PLAN_INVALID", "BLOCK_PLAN_INVALID"]
    );
}

#[tokio::test]
async fn error_table_a_twenty_first_plan_version() {
    let (env, _s, _m) = started(|_| {}).await;
    *env.resolver.plan_override.lock().unwrap() = Some(PlanState {
        versions: 20,
        current_len: 3,
    });
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge
        .call(
            1,
            "plan",
            json!({"action": "state", "steps": ["a", "b", "c"]}),
        )
        .await;
    assert!(is_error(&answer));
    assert_eq!(call_codes(session.run), ["BLOCK_PLAN_TOO_MANY_VERSIONS"]);
}

#[tokio::test]
async fn error_table_plan_mark_with_no_plan() {
    let (env, _s, _m) = started(|_| {}).await;
    // The program's resolver says a plan exists, but this session has stated none.
    *env.resolver.plan_override.lock().unwrap() = Some(PlanState {
        versions: 1,
        current_len: 3,
    });
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge
        .call(
            1,
            "plan",
            json!({"action": "mark", "position": 1, "mark": "now"}),
        )
        .await;
    assert_eq!(
        text_of(&answer),
        "State your steps first with plan(action: \"state\")."
    );
    assert_eq!(call_codes(session.run), ["MCP_PLAN_NOT_STATED"]);
}

#[tokio::test]
async fn error_table_plan_mark_position_outside_the_plan() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    bridge
        .call(
            1,
            "plan",
            json!({"action": "state", "steps": ["a", "b", "c"]}),
        )
        .await;
    let answer = bridge
        .call(
            2,
            "plan",
            json!({"action": "mark", "position": 5, "mark": "done"}),
        )
        .await;
    assert!(is_error(&answer));
    assert_eq!(call_codes(session.run)[1], "BLOCK_PLAN_POSITION_INVALID");
}

#[tokio::test]
async fn error_table_plan_marks_a_second_step_now() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    bridge
        .call(
            1,
            "plan",
            json!({"action": "state", "steps": ["a", "b", "c"]}),
        )
        .await;
    bridge
        .call(
            2,
            "plan",
            json!({"action": "mark", "position": 1, "mark": "now"}),
        )
        .await;
    let second = bridge
        .call(
            3,
            "plan",
            json!({"action": "mark", "position": 2, "mark": "now"}),
        )
        .await;
    assert_eq!(text_of(&second), "Mark step 2 done or skipped first.");
    assert_eq!(call_codes(session.run)[2], "MCP_PLAN_ONE_NOW");
    // After the first is done, the second may be now.
    let done = bridge
        .call(
            4,
            "plan",
            json!({"action": "mark", "position": 1, "mark": "done"}),
        )
        .await;
    assert_eq!(text_of(&done), "Marked step 1 done.");
    let now = bridge
        .call(
            5,
            "plan",
            json!({"action": "mark", "position": 2, "mark": "now"}),
        )
        .await;
    assert_eq!(text_of(&now), "Marked step 2 now.");
}

#[tokio::test]
async fn error_table_read_rest_reference_unknown_or_evicted() {
    let (env, _s, memory) = started(|c| c.max_rest_bytes_per_session = 40_000).await;
    env.front.set_only_needed_parts(true);
    memory.set_behaviour(Behaviour::Text("z".repeat(50_000)));
    let session = env.open(&[], &[]).await;
    let mut work = session.connect("work").await;
    let never = work
        .call(
            1,
            "read_rest",
            json!({"reference": "rest:never-issued", "from": 0}),
        )
        .await;
    assert_eq!(
        text_of(&never),
        "That reference has expired; ask for the document again."
    );
    // Two trimmed outputs of 38,000 characters each exceed the 40,000-byte store: the first is evicted.
    let mut memory_bridge = session.connect("memory").await;
    let first = text_of(&memory_bridge.call(2, "recall", q("a")).await);
    let second = text_of(&memory_bridge.call(3, "recall", q("b")).await);
    let reference = |text: &str| text.split('"').nth(1).unwrap().to_owned();
    let evicted = work
        .call(
            4,
            "read_rest",
            json!({"reference": reference(&first), "from": 12000}),
        )
        .await;
    assert_eq!(
        text_of(&evicted),
        "That reference has expired; ask for the document again."
    );
    let kept = work
        .call(
            5,
            "read_rest",
            json!({"reference": reference(&second), "from": 12000}),
        )
        .await;
    assert!(!is_error(&kept), "{kept}");
    assert_eq!(
        call_codes(session.run)
            .iter()
            .filter(|c| *c == "MCP_REST_UNKNOWN")
            .count(),
        2
    );
}

#[tokio::test]
async fn error_table_read_rest_offset_beyond_the_end() {
    let (env, _s, memory) = started(|_| {}).await;
    env.front.set_only_needed_parts(true);
    memory.set_behaviour(Behaviour::Text("z".repeat(20_000)));
    let session = env.open(&[], &[]).await;
    let mut memory_bridge = session.connect("memory").await;
    let text = text_of(&memory_bridge.call(1, "recall", q("a")).await);
    let reference = text.split('"').nth(1).unwrap().to_owned();
    let mut work = session.connect("work").await;
    let past = work
        .call(
            2,
            "read_rest",
            json!({"reference": reference, "from": 99999}),
        )
        .await;
    assert!(is_error(&past));
    assert_eq!(
        text_of(&past),
        "That offset is past the end (20000 characters)."
    );
    let before = work
        .call(3, "read_rest", json!({"reference": reference, "from": 5}))
        .await;
    assert!(is_error(&before));
    assert_eq!(
        call_codes(session.run)
            .iter()
            .filter(|c| *c == "MCP_REST_OFFSET_INVALID")
            .count(),
        2
    );
    let exact = work
        .call(
            4,
            "read_rest",
            json!({"reference": reference, "from": 20000}),
        )
        .await;
    assert_eq!(text_of(&exact), "\n[End.]");
}

#[tokio::test]
async fn error_table_journal_write_fails_after_a_call() {
    let (env, sandbox, _m) = started(|_| {}).await;
    sandbox.set_behaviour(Behaviour::Progress(1, Some(2)));
    env.sql("CREATE TRIGGER no_progress BEFORE INSERT ON journal_events WHEN NEW.kind = 'run_progress' BEGIN SELECT RAISE(ABORT, 'blocked'); END")
        .await;
    let session = env.open(&[], &[]).await;
    let result = env
        .front
        .dispatch(
            session.run,
            key("sandbox", "python_sandbox"),
            q("x"),
            CancellationToken::new(),
        )
        .await;
    // `python_sandbox` is not sandbox-callable; use the harness route.
    assert!(result.unwrap().is_error);
    let mut bridge = session.connect("sandbox").await;
    let answer = bridge.call(1, "python_sandbox", q("x")).await;
    assert!(!is_error(&answer), "the answer is unchanged: {answer}");
    assert_eq!(text_of(&answer), "progressed");
    assert!(env.events("run_progress").await.is_empty());
    assert!(!log_lines(&["mcp_front.journal_failed", &session.run.to_string()]).is_empty());
}

#[tokio::test]
async fn error_table_output_above_the_byte_limit() {
    let (env, _s, memory) = started(|c| c.max_output_bytes = 1000).await;
    memory.set_behaviour(Behaviour::Text("é".repeat(2000)));
    let session = env.open(&[], &[]).await;
    let result = env
        .front
        .dispatch(
            session.run,
            key("memory", "recall"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!result.is_error);
    assert!(result.text.ends_with("\n[Cut at 4 MiB.]"));
    assert!(result.text.len() <= 1000 + "\n[Cut at 4 MiB.]".len());
    assert!(result
        .text
        .is_char_boundary(result.text.len() - "\n[Cut at 4 MiB.]".len()));
    assert!(!log_lines(&["mcp_front.output_truncated", "MCP_OUTPUT_TRUNCATED"]).is_empty());
}
