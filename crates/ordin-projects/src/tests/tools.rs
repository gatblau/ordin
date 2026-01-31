//! The MCP servers `code`, `github`, `gitlab` and `checks`, called as the MCP Front calls them.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ordin_approval::{ApprovalRecord, PressOrigin};
use ordin_mcp_front::{
    CallVia, ProgressSink, ServerName, SessionGrant, ToolCallContext, ToolFailure, ToolKey,
    ToolOutput,
};
use ordin_rules::{AgentToolPolicy, InstanceRuleSet, RunStartFacts};
use ordin_types::domain::{Authority, HarnessKind};
use ordin_types::session::ToolCaller;
use ordin_types::wire::approval::{ApprovalPress, PressDecision};
use ordin_types::{AgentInstanceId, CodingTaskId, RunId, ToolRequestId, WorkspaceId};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

// The stand-ins are shell scripts, so the tests that use them run on Unix only.
#[cfg(unix)]
use super::standin::{hosts, hosts_with, point_tools};
use super::support::{fixture, sh, worktree, write, Fixture};
#[cfg(unix)]
use super::support::{GITHUB_TOKEN, GITLAB_TOKEN, NOBODY};
use crate::lsp::InstanceKey;
use crate::service::lock;

fn key(server: &str, tool: &str) -> ToolKey {
    ToolKey {
        server: ServerName::new(server).unwrap(),
        tool: tool.to_owned(),
    }
}

fn context(workspace: WorkspaceId, task: Option<CodingTaskId>) -> ToolCallContext {
    ToolCallContext {
        run_id: RunId::new(),
        workspace_id: workspace,
        agent_instance_id: AgentInstanceId::new(),
        caller: ToolCaller::Agent,
        scope_id: None,
        coding_task_id: task,
        call_id: ToolRequestId::new(),
        correlation_id: "test".to_owned(),
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: CancellationToken::new(),
        progress: ProgressSink::detached(),
        only_needed_parts: false,
        via: CallVia::Harness,
    }
}

/// A rule set for `agent` that declares `servers`.
fn declare(f: &Fixture, agent: AgentInstanceId, servers: &[&str]) {
    declare_with(f, agent, servers, Authority::SystemChanging);
}

fn declare_with(f: &Fixture, agent: AgentInstanceId, servers: &[&str], authority: Authority) {
    f.rules
        .load_rule_set(InstanceRuleSet {
            agent_instance_id: agent,
            package_name: "eng".to_owned(),
            authority,
            agent_folder: f.dir.0.join("agent"),
            agent_folder_read_only: false,
            declaration: None,
            configured: None,
            tool_policy: Some(AgentToolPolicy {
                prohibited_verbs: Vec::new(),
                forbidden_flags: Vec::new(),
                requires_approval: Vec::new(),
                mcp_servers: servers.iter().map(|s| (*s).to_owned()).collect(),
            }),
            approval_list: Vec::new(),
            package_read_only_tools: Vec::new(),
            tools: Vec::new(),
            unmapped_operations: Vec::new(),
            external: Vec::new(),
            commands: Vec::new(),
            reviewer: None,
        })
        .expect("the rule set loads");
}

/// A grant of an agent whose package declares both repository hosts.
fn grant(f: &Fixture, task: Option<CodingTaskId>) -> SessionGrant {
    let agent = AgentInstanceId::new();
    declare(f, agent, &["github", "gitlab"]);
    SessionGrant {
        run_id: RunId::new(),
        workspace_id: f.workspace,
        agent_instance_id: agent,
        harness: HarnessKind::ClaudeCode,
        profile: ordin_mcp_front::ProgramProfile::Desktop,
        caller: ToolCaller::Agent,
        scope_id: None,
        coding_task_id: task,
        only_servers: None,
    }
}

async fn call(
    f: &Fixture,
    task: Option<CodingTaskId>,
    server: &str,
    tool: &str,
    arguments: Value,
) -> Result<ToolOutput, ToolFailure> {
    f.service
        .tool_provider()
        .call(context(f.workspace, task), &key(server, tool), arguments)
        .await
}

/// A code-tool call by an agent of `authority`.
async fn code_as(
    f: &Fixture,
    authority: Authority,
    task: CodingTaskId,
    tool: &str,
    arguments: Value,
) -> Result<ToolOutput, ToolFailure> {
    let agent = AgentInstanceId::new();
    declare_with(f, agent, &[], authority);
    let mut ctx = context(f.workspace, Some(task));
    ctx.agent_instance_id = agent;
    f.service
        .tool_provider()
        .call(ctx, &key("code", tool), arguments)
        .await
}

#[tokio::test]
async fn the_servers_list_their_tools_with_valid_specs() {
    let f = fixture().await;
    let provider = f.service.tool_provider();
    assert_eq!(provider.provider_id(), "projects");
    let servers = provider.servers();
    let names: Vec<_> = servers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["code", "github", "gitlab", "checks"]);
    assert_eq!(servers[0].tools.len(), 5);
    for tool in servers.iter().flat_map(|s| &s.tools) {
        assert_eq!(tool.input_schema["type"], "object");
        assert!((1..=120).contains(&tool.description.len()));
        assert!((1..=300).contains(&tool.argument_form.len()));
        assert!((1..=200).contains(&tool.plain.len()));
    }
    provider.session_closed(RunId::new()).await;
}

#[tokio::test]
async fn a_coding_session_is_offered_code_and_checks_and_a_linked_host_its_tool() {
    let f = fixture().await;
    let (_, folder) = f.project().await;
    let provider = f.service.tool_provider();
    let plain = provider.offer(&grant(&f, None)).await.unwrap();
    assert!(plain.is_empty(), "no origin yet, and no coding task");
    git2::Repository::open(&folder)
        .unwrap()
        .remote("origin", "https://github.com/acme/portal.git")
        .unwrap();
    let linked = provider.offer(&grant(&f, None)).await.unwrap();
    assert_eq!(linked, [key("github", "gh")]);
    let coding = provider
        .offer(&grant(&f, Some(CodingTaskId::new())))
        .await
        .unwrap();
    assert!(coding.contains(&key("code", "definition")));
    assert!(coding.contains(&key("checks", "run_check")));
    assert_eq!(coding.len(), 7);

    let other = f.dir.0.join("gitlab-repo");
    let repo = super::support::repository(&other, &[("a.txt", "a\n")]);
    repo.remote("origin", "https://gitlab.acme.co.uk/ops/automation.git")
        .unwrap();
    let place = f.place(&other).await;
    f.service.open_project(place, f.workspace).await.unwrap();
    let both = provider.offer(&grant(&f, None)).await.unwrap();
    assert_eq!(both, [key("github", "gh"), key("gitlab", "glab")]);

    let mut undeclared = grant(&f, None);
    undeclared.agent_instance_id = AgentInstanceId::new();
    let nothing = provider.offer(&undeclared).await.unwrap();
    assert!(nothing.is_empty(), "no rule set, nothing declared");
    declare(&f, undeclared.agent_instance_id, &["gitlab"]);
    let one = provider.offer(&undeclared).await.unwrap();
    assert_eq!(one, [key("gitlab", "glab")]);
}

// Unix only: the stand-in tool is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn a_form_off_the_list_is_refused_and_no_program_starts() {
    let h = hosts().await;
    let f = &h.fixture;
    let (_, folder) = f.project().await;
    git2::Repository::open(&folder)
        .unwrap()
        .remote("origin", "https://github.com/acme/portal.git")
        .unwrap();
    let provider = f.service.tool_provider();
    for args in [
        json!(["api", "/user"]),
        json!(["alias", "set", "x", "--shell", "sh"]),
        json!(["extension", "install", "a/b"]),
        json!(["pr", "list", "--repo", "other/repo"]),
        json!(["pr", "view", "1", "--jq", "env.GH_TOKEN"]),
    ] {
        let arguments = json!({ "args": args });
        let refused = provider.check_arguments(&key("github", "gh"), &arguments);
        assert_eq!(refused.unwrap_err().at, "/args", "{arguments}");
        let reply = call(f, None, "github", "gh", arguments).await;
        assert_eq!(reply.unwrap_err().code, "PROJ_CHECK_DENIED");
    }
}

// Unix only: the stand-in tool is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn a_write_form_waits_for_the_person_and_runs_only_once_approved() {
    let h = hosts().await;
    let f = &h.fixture;
    let (project, folder) = f.project().await;
    git2::Repository::open(&folder)
        .unwrap()
        .remote("origin", "https://github.com/acme/portal.git")
        .unwrap();
    let started = f.start_task(&project, "gh").await;
    let ctx = session(f, started.task.id, Authority::SystemChanging, &[]).await;
    let provider = f.service.tool_provider();
    let gh = key("github", "gh");
    let arguments = json!({"args": ["pr", "merge", "12", "--squash"]});
    let (reply, ()) = tokio::join!(provider.call(ctx, &gh, arguments), async {
        press(f, card(f).await, PressDecision::Reject).await;
    });
    assert_eq!(reply.unwrap_err().code, "PROJ_CHECK_DENIED");
    let ctx = session(f, started.task.id, Authority::SystemChanging, &[]).await;
    let arguments = json!({"args": ["pr", "merge", "12", "--squash"]});
    let (reply, ()) = tokio::join!(provider.call(ctx, &gh, arguments), async {
        press(f, card(f).await, PressDecision::Approve).await;
    });
    assert!(reply.unwrap().text.contains("args:pr merge 12 --squash"));
}

// Unix only: the stand-in tool is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn a_write_form_honours_a_prohibited_verb() {
    let h = hosts().await;
    let f = &h.fixture;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "gh").await;
    let ctx = session(f, started.task.id, Authority::SystemChanging, &["gh.pr"]).await;
    let reply = f
        .service
        .tool_provider()
        .call(
            ctx,
            &key("github", "gh"),
            json!({"args": ["pr", "merge", "12"]}),
        )
        .await;
    assert_eq!(reply.unwrap_err().code, "PROJ_CHECK_DENIED");
}

#[tokio::test]
async fn arguments_are_checked_in_fixed_code() {
    let f = fixture().await;
    let provider = f.service.tool_provider();
    let check = |server: &str, tool: &str, arguments: Value| {
        provider.check_arguments(&key(server, tool), &arguments)
    };
    assert!(check(
        "code",
        "hover",
        json!({"path": "a.rs", "line": 1, "character": 2})
    )
    .is_ok());
    assert!(check("code", "hover", json!({"path": "a.rs", "line": 1})).is_err());
    assert!(check(
        "code",
        "hover",
        json!({"path": "a.rs", "line": 1, "character": 2, "x": 1})
    )
    .is_err());
    let climbing = check(
        "code",
        "definition",
        json!({"path": "../a.rs", "line": 1, "character": 2}),
    );
    assert_eq!(climbing.unwrap_err().at, "/path");
    assert!(check("code", "symbols", json!({})).is_ok());
    assert!(check("code", "diagnostics", json!({"path": "/etc/passwd"})).is_err());
    assert!(check("github", "gh", json!({"args": ["pr", "list"]})).is_ok());
    assert!(check("gitlab", "glab", json!({"args": []})).is_err());
    let many: Vec<String> = (0..65).map(|n| n.to_string()).collect();
    assert!(check("github", "gh", json!({ "args": many })).is_err());
    assert!(check("github", "gh", json!({"args": ["x".repeat(4001)]})).is_err());
    assert!(check("checks", "run_check", json!({"command": "make"})).is_ok());
    assert!(check("checks", "run_check", json!({"command": ""})).is_err());
    assert!(check("work", "report", json!({})).is_err());
}

#[tokio::test]
async fn until_c_33_is_confirmed_the_code_tools_are_unavailable() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    let failure = call(
        &f,
        Some(started.task.id),
        "code",
        "hover",
        json!({"path": "src/rates.rs", "line": 0, "character": 0}),
    )
    .await
    .unwrap_err();
    assert_eq!(failure.code, "PROJ_LSP_UNAVAILABLE");
    assert_eq!(
        failure.message,
        "Code intelligence isn't available for this language."
    );
    assert!(failure.retryable);
    let no_task = call(&f, None, "code", "symbols", json!({}))
        .await
        .unwrap_err();
    assert_eq!(no_task.code, "PROJ_LSP_UNAVAILABLE");
    let listing = call(&f, Some(started.task.id), "code", "symbols", json!({})).await;
    assert_eq!(listing.unwrap_err().code, "PROJ_LSP_UNAVAILABLE");
    let run = RunId::new();
    write(&worktree(&started.task), "src/rates.rs", "// ERROR x\n");
    f.service
        .end_coding_run(started.task.id, run)
        .await
        .unwrap();
    let row = f.service.task_row(started.task.id).await.unwrap();
    assert_eq!(row.task.new_problems, 0, "no line for this language");
}

#[tokio::test]
async fn the_code_tools_read_through_the_server_once_a_language_is_confirmed() {
    let f = fixture().await;
    lock(&f.service.tunables)
        .worktree_confirmed
        .insert("rust".to_owned());
    f.service.reconcile_settings().await.unwrap();
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    let root = worktree(&started.task);
    write(&root, "src/rates.rs", "fn fetch() {\n// ERROR x\n}\n");
    let task = Some(started.task.id);
    let at = json!({"path": "src/rates.rs", "line": 1, "character": 2});

    let id = started.task.id;
    let found = code_as(&f, Authority::SystemChanging, id, "definition", at.clone())
        .await
        .unwrap();
    let body = found.structured.unwrap();
    assert_eq!(
        body["locations"],
        json!([{"path": "src/rates.rs", "line": 1, "character": 0}]),
        "the location outside the worktree is dropped"
    );
    let used = code_as(&f, Authority::SystemChanging, id, "references", at.clone())
        .await
        .unwrap();
    assert_eq!(
        used.structured.unwrap()["locations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // An agent that may not run commands gets no server on a worktree changed since its base: the
    // server would run what was written there.
    let refused = code_as(&f, Authority::DocumentOnly, id, "hover", at.clone())
        .await
        .unwrap_err();
    assert_eq!(refused.code, "PROJ_LSP_UNAVAILABLE");
    assert!(!refused.retryable);
    let refused = call(&f, task, "code", "hover", at.clone())
        .await
        .unwrap_err();
    assert_eq!(
        refused.code, "PROJ_LSP_UNAVAILABLE",
        "no rule set: no commands"
    );
    let hover = code_as(&f, Authority::SystemChanging, id, "hover", at)
        .await
        .unwrap();
    assert_eq!(hover.structured.unwrap()["contents"], "The rates client.");
    let symbols = code_as(
        &f,
        Authority::SystemChanging,
        id,
        "symbols",
        json!({"path": "src/rates.rs"}),
    )
    .await
    .unwrap();
    assert_eq!(symbols.structured.unwrap()["symbols"][0]["name"], "fetch");
    let everywhere = code_as(&f, Authority::SystemChanging, id, "symbols", json!({}))
        .await
        .unwrap();
    assert_eq!(
        everywhere.structured.unwrap()["symbols"][0]["location"]["path"],
        "src/rates.rs"
    );
    let problems = code_as(
        &f,
        Authority::SystemChanging,
        id,
        "diagnostics",
        json!({"path": "src/rates.rs"}),
    )
    .await
    .unwrap();
    assert_eq!(problems.structured.unwrap()["diagnostics"][0]["code"], "E1");
    let mut all = Value::Null;
    for _ in 0..100 {
        let reply = code_as(&f, Authority::SystemChanging, id, "diagnostics", json!({}))
            .await
            .unwrap();
        all = reply.structured.unwrap();
        if all["diagnostics"].get("src/rates.rs").is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(all["diagnostics"]["src/rates.rs"].is_array(), "{all}");
    let unknown = call(&f, task, "code", "rename", json!({})).await;
    assert_eq!(unknown.unwrap_err().code, "PROJ_LSP_UNAVAILABLE");
    let absent = call(&f, task, "code", "symbols", json!({"path": "src/none.rs"})).await;
    assert_eq!(absent.unwrap_err().code, "PROJ_LSP_UNAVAILABLE");
}

/// A running agent's call of a tool on the task, as the MCP Front makes it: the run is known to
/// the Rules Engine, with the agent's authority and the verbs its policy prohibits.
async fn session(
    f: &Fixture,
    task: CodingTaskId,
    authority: Authority,
    prohibited: &[&str],
) -> ToolCallContext {
    let (run, agent) = (RunId::new(), AgentInstanceId::new());
    f.rules
        .load_rule_set(InstanceRuleSet {
            agent_instance_id: agent,
            package_name: "eng".to_owned(),
            authority,
            agent_folder: f.dir.0.join("agent"),
            agent_folder_read_only: false,
            declaration: None,
            configured: None,
            tool_policy: Some(AgentToolPolicy {
                prohibited_verbs: prohibited.iter().map(|v| (*v).to_owned()).collect(),
                forbidden_flags: Vec::new(),
                requires_approval: Vec::new(),
                mcp_servers: Vec::new(),
            }),
            approval_list: Vec::new(),
            package_read_only_tools: Vec::new(),
            tools: Vec::new(),
            unmapped_operations: Vec::new(),
            external: Vec::new(),
            commands: Vec::new(),
            reviewer: None,
        })
        .expect("the rule set loads");
    f.rules
        .begin_run(RunStartFacts {
            run_id: run,
            agent_instance_id: agent,
            workspace_id: f.workspace,
            crew_charter_id: None,
            command: None,
            text: String::new(),
            scope: None,
            asker: None,
            started_by: "person".to_owned(),
            started_by_id: "person:asha".to_owned(),
        })
        .await
        .expect("the run begins");
    let mut ctx = context(f.workspace, Some(task));
    ctx.run_id = run;
    ctx.agent_instance_id = agent;
    ctx
}

async fn run_check(
    f: &Fixture,
    ctx: ToolCallContext,
    command: &str,
) -> Result<ToolOutput, ToolFailure> {
    f.service
        .tool_provider()
        .call(
            ctx,
            &key("checks", "run_check"),
            json!({ "command": command }),
        )
        .await
}

/// The one pending approval, once its card has appeared.
async fn card(f: &Fixture) -> ApprovalRecord {
    for _ in 0..500 {
        let pending = f.approvals.list_pending(None, None).await.unwrap().items;
        if let Some(first) = pending.into_iter().next() {
            return first;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no approval was asked for");
}

/// Answers a card as the person would.
async fn press(f: &Fixture, record: ApprovalRecord, decision: PressDecision) {
    let card_ref = match decision {
        PressDecision::Approve => record.card.buttons.approve.clone(),
        PressDecision::Reject => record.card.buttons.reject.clone(),
    };
    f.approvals
        .decide(
            ApprovalPress { card_ref, decision },
            PressOrigin::LocalScreen,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn run_check_runs_a_declared_check_in_the_worktree_without_a_card() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec![sh::PWD.to_owned()])
        .await
        .unwrap();
    let started = f.start_task(&project, "tools").await;
    let ctx = session(&f, started.task.id, Authority::SystemChanging, &[]).await;
    let reply = run_check(&f, ctx, sh::PWD).await.unwrap();
    let body = reply.structured.unwrap();
    assert_eq!(body["exit_code"], 0);
    let printed = PathBuf::from(body["output"].as_str().unwrap().trim());
    assert_eq!(
        std::fs::canonicalize(printed).unwrap(),
        std::fs::canonicalize(worktree(&started.task)).unwrap()
    );
    assert_eq!(f.entries_of("check_run").await.len(), 1);
    let no_task = call(&f, None, "checks", "run_check", json!({"command": sh::PWD})).await;
    assert_eq!(no_task.unwrap_err().code, "PROJ_LSP_UNAVAILABLE");
}

#[tokio::test]
async fn run_check_stops_a_declared_check_that_runs_too_long() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec![sh::SLOW.to_owned()])
        .await
        .unwrap();
    let started = f.start_task(&project, "tools").await;
    let ctx = session(&f, started.task.id, Authority::SystemChanging, &[]).await;
    lock(&f.service.tunables).check_timeout = Duration::from_millis(100);
    let slow = run_check(&f, ctx, sh::SLOW).await;
    assert_eq!(slow.unwrap_err().code, "PROJ_CHECK_TIMEOUT");
}

#[tokio::test]
async fn run_check_waits_for_the_person_on_an_undeclared_command_and_runs_it_once_approved() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    let root = worktree(&started.task);
    let ctx = session(&f, started.task.id, Authority::SystemChanging, &[]).await;
    let (reply, ()) = tokio::join!(run_check(&f, ctx, sh::MAKE_RAN), async {
        let record = card(&f).await;
        assert!(!root.join("ran").exists(), "nothing runs before the answer");
        press(&f, record, PressDecision::Approve).await;
    });
    assert_eq!(reply.unwrap().structured.unwrap()["exit_code"], 0);
    assert!(root.join("ran").exists());
}

#[tokio::test]
async fn run_check_runs_nothing_when_the_person_rejects_the_command() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    let root = worktree(&started.task);
    let ctx = session(&f, started.task.id, Authority::SystemChanging, &[]).await;
    let (reply, ()) = tokio::join!(run_check(&f, ctx, "touch ran"), async {
        press(&f, card(&f).await, PressDecision::Reject).await;
    });
    assert_eq!(reply.unwrap_err().code, "PROJ_CHECK_DENIED");
    assert!(!root.join("ran").exists());
}

#[tokio::test]
async fn run_check_times_out_when_nobody_answers_and_runs_nothing() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    let ctx = session(&f, started.task.id, Authority::SystemChanging, &[]).await;
    lock(&f.service.tunables).check_timeout = Duration::from_millis(100);
    let reply = run_check(&f, ctx, "touch ran").await;
    assert_eq!(reply.unwrap_err().code, "PROJ_CHECK_TIMEOUT");
    assert!(!worktree(&started.task).join("ran").exists());
    assert!(f.entries_of("check_run").await.is_empty());
}

#[tokio::test]
async fn run_check_refuses_an_agent_without_the_authority_to_run_commands() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec!["touch ran".to_owned()])
        .await
        .unwrap();
    let started = f.start_task(&project, "tools").await;
    let ctx = session(&f, started.task.id, Authority::DocumentOnly, &[]).await;
    let reply = run_check(&f, ctx, "touch ran").await;
    assert_eq!(reply.unwrap_err().code, "PROJ_CHECK_DENIED");
    assert!(!worktree(&started.task).join("ran").exists());
}

#[tokio::test]
async fn run_check_refuses_a_verb_the_agent_s_policy_prohibits_even_as_a_declared_check() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec!["touch ran".to_owned()])
        .await
        .unwrap();
    let started = f.start_task(&project, "tools").await;
    let ctx = session(&f, started.task.id, Authority::SystemChanging, &["touch"]).await;
    let reply = run_check(&f, ctx, "touch ran").await;
    assert_eq!(reply.unwrap_err().code, "PROJ_CHECK_DENIED");
    assert!(!worktree(&started.task).join("ran").exists());
}

#[tokio::test]
async fn run_check_refuses_a_run_the_rules_engine_does_not_know() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec!["touch ran".to_owned()])
        .await
        .unwrap();
    let started = f.start_task(&project, "tools").await;
    let ctx = context(f.workspace, Some(started.task.id));
    let reply = run_check(&f, ctx, "touch ran").await;
    assert_eq!(reply.unwrap_err().code, "PROJ_CHECK_DENIED");
    assert!(!worktree(&started.task).join("ran").exists());
}

#[tokio::test]
async fn run_check_refuses_a_task_of_a_closed_project() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    f.service
        .set_check_commands(project.id, vec!["touch ran".to_owned()])
        .await
        .unwrap();
    let started = f.start_task(&project, "tools").await;
    let ctx = session(&f, started.task.id, Authority::SystemChanging, &[]).await;
    f.service.close_project(project.id).await.unwrap();
    let reply = run_check(&f, ctx, "touch ran").await;
    assert_eq!(reply.unwrap_err().code, "PROJ_NOT_FOUND");
    assert!(!worktree(&started.task).join("ran").exists());
}

// Unix only: the stand-in tool is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn gh_runs_with_the_token_in_its_environment_and_its_output_is_the_result() {
    let h = hosts().await;
    let f = &h.fixture;
    let (_, folder) = f.project().await;
    git2::Repository::open(&folder)
        .unwrap()
        .remote("origin", "https://github.com/acme/portal.git")
        .unwrap();
    let reply = call(f, None, "github", "gh", json!({"args": ["pr", "list"]}))
        .await
        .unwrap();
    assert!(reply.text.contains("args:pr list"), "{}", reply.text);
    assert!(
        reply.text.contains("token:***"),
        "the token leaves no trace"
    );
    assert!(!reply.text.contains(GITHUB_TOKEN));
    assert!(reply.text.contains("prompt:1 colour:1"));
    assert!(reply.text.contains(folder.to_str().unwrap()));
    assert_eq!(reply.structured.unwrap()["exit_code"], 0);
    let failed = call(
        f,
        None,
        "github",
        "gh",
        json!({"args": ["pr", "view", "fail"]}),
    )
    .await
    .unwrap();
    assert_eq!(failed.structured.unwrap()["exit_code"], 3);
}

// Unix only: the stand-in tool is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn glab_gets_the_company_host_and_its_own_token() {
    let h = hosts().await;
    let f = &h.fixture;
    let other = f.dir.0.join("gitlab-repo");
    let repo = super::support::repository(&other, &[("a.txt", "a\n")]);
    repo.remote("origin", "https://gitlab.acme.co.uk/ops/automation.git")
        .unwrap();
    let place = f.place(&other).await;
    f.service.open_project(place, f.workspace).await.unwrap();
    let reply = call(f, None, "gitlab", "glab", json!({"args": ["mr", "list"]}))
        .await
        .unwrap();
    assert!(
        reply.text.contains("host:gitlab.acme.co.uk"),
        "{}",
        reply.text
    );
    assert!(!reply.text.contains(GITLAB_TOKEN));
}

// Unix only: the stand-in tool is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn gh_runs_in_the_coding_task_s_worktree() {
    let h = hosts().await;
    let f = &h.fixture;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "gh").await;
    let reply = call(
        f,
        Some(started.task.id),
        "github",
        "gh",
        json!({"args": ["pr", "status"]}),
    )
    .await
    .unwrap();
    assert!(
        reply.text.contains(&started.task.worktree_path),
        "{}",
        reply.text
    );
}

// Unix only: the stand-in tool is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn a_tool_with_no_sign_in_or_no_project_fails_with_a_code() {
    let h = hosts_with(NOBODY).await;
    let (_, folder) = h.fixture.project().await;
    git2::Repository::open(&folder)
        .unwrap()
        .remote("origin", "https://github.com/acme/portal.git")
        .unwrap();
    let failure = call(
        &h.fixture,
        None,
        "github",
        "gh",
        json!({"args": ["pr", "list"]}),
    )
    .await
    .unwrap_err();
    assert_eq!(failure.code, "GH_SIGN_IN_NEEDED");
    let signed_in = hosts().await;
    let none = call(
        &signed_in.fixture,
        None,
        "gitlab",
        "glab",
        json!({"args": ["mr", "list"]}),
    )
    .await
    .unwrap_err();
    assert_eq!(none.code, "PROJ_NOT_FOUND");
    let stray = call(&signed_in.fixture, None, "work", "report", json!({})).await;
    assert_eq!(stray.unwrap_err().code, "PROJ_LSP_UNAVAILABLE");
}

// Unix only: the stand-in tool is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn a_cancelled_call_ends_at_once() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "cancel").await;
    let ctx = context(f.workspace, Some(started.task.id));
    ctx.cancel.cancel();
    let failure = f
        .service
        .tool_provider()
        .call(
            ctx,
            &key("checks", "run_check"),
            json!({"command": "sleep 30"}),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "PROJ_CHECK_TIMEOUT");
    let mut late = context(f.workspace, None);
    late.deadline = Instant::now();
    point_tools(&f, "http://127.0.0.1:9");
    let failure = f
        .service
        .tool_provider()
        .call(
            late,
            &key("gitlab", "glab"),
            json!({"args": ["mr", "list"]}),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        failure.code.as_str(),
        "GH_UNREACHABLE" | "PROJ_NOT_FOUND"
    ));
}

#[tokio::test]
async fn a_check_receives_only_the_listed_environment() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    // `cmd.exe` has no `env`; its `set` prints `NAME=value`, cut to the name below.
    let command = if cfg!(windows) {
        "set"
    } else {
        "env | sed 's/=.*//'"
    };
    f.service
        .set_check_commands(project.id, vec![command.to_owned()])
        .await
        .unwrap();
    let ctx = session(&f, started.task.id, Authority::SystemChanging, &[]).await;
    let reply = run_check(&f, ctx, command).await.unwrap();
    let body = reply.structured.unwrap();
    let output = body["output"].as_str().unwrap();
    let names: Vec<&str> = output
        .lines()
        .map(|line| line.split_once('=').map_or(line, |(name, _)| name))
        .collect();
    assert!(names.contains(&"PATH"), "{output}");
    // `cargo test` sets these in this process, and `/bin/sh` adds the last three itself. On Windows
    // `cmd.exe` adds PROMPT, and the check sets NoDefaultCurrentDirectoryInExePath for it.
    assert!(std::env::var_os("CARGO_PKG_NAME").is_some());
    for name in names {
        assert!(
            crate::checks::CHECK_ENVIRONMENT.contains(&name)
                || matches!(name, "PWD" | "SHLVL" | "_" | "OLDPWD")
                || (cfg!(windows)
                    && matches!(name, "PROMPT" | "NoDefaultCurrentDirectoryInExePath")),
            "{name} reached a check"
        );
    }
}

#[tokio::test]
async fn no_language_server_starts_before_the_person_s_answer() {
    let f = fixture().await;
    lock(&f.service.tunables)
        .worktree_confirmed
        .insert("rust".to_owned());
    f.service.reconcile_settings().await.unwrap();
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    let task = Some(started.task.id);
    let at = json!({"path": "src/rates.rs", "line": 1, "character": 2});
    f.service
        .answer_language_servers(project.id, false)
        .await
        .unwrap();
    let refused = call(&f, task, "code", "definition", at.clone()).await;
    assert_eq!(refused.unwrap_err().code, "PROJ_LSP_UNAVAILABLE");
    assert!(lock(&f.service.lsp.instances).is_empty());
    assert_eq!(
        f.service.language_servers_answer(project.id).await.unwrap(),
        Some(false)
    );
    f.service
        .answer_language_servers(project.id, true)
        .await
        .unwrap();
    call(&f, task, "code", "definition", at).await.unwrap();
    assert_eq!(lock(&f.service.lsp.instances).len(), 1);
    f.service
        .answer_language_servers(project.id, false)
        .await
        .unwrap();
    assert!(
        lock(&f.service.lsp.instances).is_empty(),
        "a no stops what runs on the repository"
    );
}

#[tokio::test]
async fn an_editor_asks_once_and_starts_nothing_until_answered() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let folder = f.dir.0.join("hostile");
    super::support::repository(
        &folder,
        &[("src/lib.rs", "fn a() {}\n"), ("README.md", "x\n")],
    );
    let place = f.place(&folder).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();

    let first = f
        .service
        .editor_open(project.id, "src/lib.rs")
        .await
        .unwrap();
    assert!(first.asks_to_start_servers);
    assert_eq!(
        first.server_line,
        "highlighted · language server waiting for your answer"
    );
    assert!(lock(&f.service.lsp.instances).is_empty());
    let message = json!({"jsonrpc": "2.0", "method": "textDocument/didOpen",
        "params": {"textDocument": {"uri": crate::lsp::uri_of(&folder.join("src/lib.rs"))}}});
    let relay = f.service.lsp_send(first.stream, message).await;
    assert_eq!(relay.unwrap_err().code(), "PROJ_LSP_UNAVAILABLE");
    assert!(lock(&f.service.lsp.instances).is_empty());
    assert_eq!(
        f.service.language_servers_answer(project.id).await.unwrap(),
        None
    );

    let no_code = f
        .service
        .editor_open(project.id, "README.md")
        .await
        .unwrap();
    assert!(
        !no_code.asks_to_start_servers,
        "a file with no server asks nothing"
    );

    f.service
        .answer_language_servers(project.id, true)
        .await
        .unwrap();
    let after = f
        .service
        .editor_open(project.id, "src/lib.rs")
        .await
        .unwrap();
    assert!(!after.asks_to_start_servers, "the answer is remembered");
    assert_eq!(lock(&f.service.lsp.instances).len(), 1);
}

// Unix only: the stand-in language server is a shell script.
#[cfg(unix)]
fn server_script(f: &Fixture, name: &str, body: &str) -> PathBuf {
    super::standin::linked_script(&f.dir.0, name, body)
}

// Unix only: the stand-in language server is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn a_language_server_receives_only_the_listed_environment() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let (project, folder) = f.project().await;
    let seen = f.dir.0.join("server-env");
    let real = f.installer.program.lock().unwrap().clone().unwrap();
    let body = format!("env > '{}'\nexec '{}'", seen.display(), real.display());
    *f.installer.program.lock().unwrap() = Some(server_script(&f, "env-server", &body));
    let key = InstanceKey::Editor(project.id, "rust".to_owned());
    f.service.instance(key, &folder, "rust").await.unwrap();
    assert!(std::env::var_os("CARGO_PKG_NAME").is_some());
    let text = std::fs::read_to_string(&seen).unwrap();
    for name in text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(n, _)| n)
    {
        assert!(
            crate::checks::CHECK_ENVIRONMENT.contains(&name)
                || matches!(name, "PWD" | "SHLVL" | "_" | "OLDPWD" | "STAND_IN"),
            "{name} reached a language server"
        );
    }
}

/// The repository at `folder` moved aside and another put in its place, with its own root commit.
fn replaced(folder: &Path, origin: Option<&str>, text: &str) {
    let aside = format!("moved-{}", uuid::Uuid::now_v7());
    std::fs::rename(folder, folder.with_file_name(aside)).unwrap();
    let repo = super::support::repository(folder, &[("src/lib.rs", text)]);
    if let Some(origin) = origin {
        repo.remote("origin", origin).unwrap();
    }
}

#[tokio::test]
async fn a_yes_does_not_pass_to_another_repository_at_the_same_path() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let folder = f.dir.0.join("trusted");
    let repo = super::support::repository(&folder, &[("src/lib.rs", "fn a() {}\n")]);
    repo.remote("origin", "https://github.com/acme/trusted.git")
        .unwrap();
    let place = f.place(&folder).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();
    f.service
        .answer_language_servers(project.id, true)
        .await
        .unwrap();
    assert_eq!(
        f.service.language_servers_answer(project.id).await.unwrap(),
        Some(true)
    );
    // The same history under another address is asked about again.
    repo.remote_set_url("origin", "https://github.com/evil/hostile.git")
        .unwrap();
    assert_eq!(
        f.service.language_servers_answer(project.id).await.unwrap(),
        None
    );
    repo.remote_set_url("origin", "https://github.com/acme/trusted.git")
        .unwrap();
    replaced(
        &folder,
        Some("https://github.com/evil/hostile.git"),
        "// hostile\n",
    );
    assert_eq!(
        f.service.language_servers_answer(project.id).await.unwrap(),
        None,
        "another repository at the path is asked about again"
    );
    let session = f
        .service
        .editor_open(project.id, "src/lib.rs")
        .await
        .unwrap();
    assert!(session.asks_to_start_servers);
    assert!(lock(&f.service.lsp.instances).is_empty());

    // The same address on a different history is not the same repository either.
    f.service
        .answer_language_servers(project.id, true)
        .await
        .unwrap();
    replaced(
        &folder,
        Some("https://github.com/evil/hostile.git"),
        "// another history
",
    );
    assert_eq!(
        f.service.language_servers_answer(project.id).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn a_yes_stands_when_the_repository_gains_a_commit() {
    let f = fixture().await;
    let folder = f.dir.0.join("grows");
    let repo = super::support::repository(&folder, &[("a.txt", "a\n")]);
    repo.remote("origin", "https://github.com/acme/grows.git")
        .unwrap();
    let place = f.place(&folder).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();
    f.service
        .answer_language_servers(project.id, true)
        .await
        .unwrap();
    std::fs::write(folder.join("b.txt"), "b\n").unwrap();
    crate::git::commit_all(&folder, "more").unwrap();
    assert_eq!(
        f.service.language_servers_answer(project.id).await.unwrap(),
        Some(true)
    );
}

#[tokio::test]
async fn a_repository_with_nothing_to_bind_to_leaves_no_stored_answer() {
    let f = fixture().await;
    let (project, folder) = f.project().await;
    assert_eq!(
        f.service.language_servers_answer(project.id).await.unwrap(),
        Some(true),
        "the answer holds while ordin runs"
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM language_server_answers")
        .fetch_one(&f.service.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "no origin, so nothing is stored");
    lock(&f.service.lsp.unbound_answers).clear();
    assert_eq!(
        f.service.language_servers_answer(project.id).await.unwrap(),
        None,
        "a new run of ordin asks again"
    );
    f.service
        .answer_language_servers(project.id, true)
        .await
        .unwrap();
    replaced(&folder, None, "// hostile\n");
    let answer = f.service.language_servers_answer(project.id).await.unwrap();
    assert_eq!(answer, None, "another history at the path asks again");
}

// Unix only: the stand-in language server is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn a_no_stops_a_server_still_in_initialize() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let (project, folder) = f.project().await;
    let real = f.installer.program.lock().unwrap().clone().unwrap();
    let body = format!("sleep 1\nexec '{}'", real.display());
    *f.installer.program.lock().unwrap() = Some(server_script(&f, "slow-server", &body));
    let key = InstanceKey::Editor(project.id, "rust".to_owned());
    let (started, answered) = tokio::join!(f.service.instance(key, &folder, "rust"), async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        f.service.answer_language_servers(project.id, false).await
    });
    started.unwrap();
    answered.unwrap();
    assert!(
        lock(&f.service.lsp.instances).is_empty(),
        "the server the no raced is stopped"
    );
}

#[tokio::test]
async fn a_no_finishes_past_a_server_whose_task_row_is_gone() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let (project, folder) = f.project().await;
    for _ in 0..3 {
        let key = InstanceKey::Task(CodingTaskId::new(), "rust".to_owned());
        f.service
            .start_instance(key, &folder, "rust")
            .await
            .unwrap();
    }
    f.service
        .answer_language_servers(project.id, false)
        .await
        .unwrap();
    assert!(lock(&f.service.lsp.instances).is_empty());
}

#[tokio::test]
async fn code_no_server_may_read_is_marked_unchecked_not_clean() {
    let f = fixture().await;
    lock(&f.service.tunables)
        .worktree_confirmed
        .insert("rust".to_owned());
    f.service.reconcile_settings().await.unwrap();
    let (project, _) = f.project().await;
    f.service
        .answer_language_servers(project.id, false)
        .await
        .unwrap();
    let started = f.start_task(&project, "tools").await;
    write(
        &worktree(&started.task),
        "src/rates.rs",
        "fn fetch() {\n// ERROR new\n}\n",
    );
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let row = f.service.task_row(started.task.id).await.unwrap();
    assert_eq!(row.task.new_problems, 0);
    assert!(row.task.problems_unchecked, "0 here is not a clean result");
}

#[tokio::test]
async fn code_of_a_language_not_confirmed_for_worktrees_is_marked_unchecked() {
    let f = fixture().await;
    f.service.reconcile_settings().await.unwrap();
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    write(&worktree(&started.task), "src/rates.rs", "fn fetch() {}\n");
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let row = f.service.task_row(started.task.id).await.unwrap();
    assert!(row.task.problems_unchecked);
}

#[tokio::test]
async fn a_change_with_no_code_is_not_marked_unchecked() {
    let f = fixture().await;
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    write(&worktree(&started.task), "README.md", "changed\n");
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let row = f.service.task_row(started.task.id).await.unwrap();
    assert!(!row.task.problems_unchecked);
}

#[tokio::test]
async fn code_a_server_read_is_not_marked_unchecked() {
    let f = fixture().await;
    lock(&f.service.tunables)
        .worktree_confirmed
        .insert("rust".to_owned());
    f.service.reconcile_settings().await.unwrap();
    let (project, _) = f.project().await;
    let started = f.start_task(&project, "tools").await;
    write(&worktree(&started.task), "src/rates.rs", "fn fetch() {}\n");
    f.service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    let row = f.service.task_row(started.task.id).await.unwrap();
    assert!(!row.task.problems_unchecked);
}
