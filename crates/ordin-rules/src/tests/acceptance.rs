//! X2: one test per Gherkin scenario of RulesEngine.md, with the scenario's name. The seven
//! content-check scenarios of the ADR-107 amendment run against an HTTP stand-in in
//! `tests/content_check.rs`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ordin_approval::PressOrigin;
use ordin_types::domain::agents::Authority;
use ordin_types::domain::email::EmailAttachment;
use ordin_types::domain::places::{FolderAccess, Grantee};
use ordin_types::domain::runs::{ApprovalState, ToolRequestDecision, ToolRequestState};
use ordin_types::wire::approval::{ApprovalPress, PressDecision};
use ordin_types::wire::rules::{RuleSource, ToolOrigin, Verdict};
use ordin_types::{AgentInstanceId, ApprovalId, PlaceId, RunId, WorkspaceId};
use serde_json::json;

use super::support::*;
use crate::store;
use crate::types::{
    ApprovalNeed, ApprovalWait, CallContext, CommandOrigin, CommandRequest, EmailCheck,
    EmailVerdict, EmailViolation, EndDecision, GitHostSetting, PathAccessRequest, PlaceRead,
    PlaceReader, ProgramKind, RulesOutcome, StartOutcome, ToolClass,
};
use crate::vocab::{
    AddressCondition, AgentToolPolicy, ArgumentForm, ArgumentOutcome, ArgumentRule, Condition,
    ConfiguredControls, DecidingOutcome, Declaration, DutyGate, GateOutcome, Gates, GrantedPlace,
    HeartbeatForm, JudgedLoopGate, MatchMode, MessageRule, Point, StepGate,
};
use crate::{duty_lock_key, RulesEngine};

pub(crate) fn verdict(outcome: &RulesOutcome) -> &'static str {
    match outcome.decision.verdict {
        Verdict::Allow => "allow",
        Verdict::Deny { .. } => "deny",
        Verdict::Ask { .. } => "ask",
    }
}

pub(crate) fn rule(outcome: &RulesOutcome) -> &str {
    &outcome.decision.rule.rule_id
}

pub(crate) fn reason(outcome: &RulesOutcome) -> String {
    match &outcome.decision.verdict {
        Verdict::Deny { reason } => reason.clone(),
        other => panic!("expected a denial, got {other:?}"),
    }
}

fn external(kind: &str) -> CallContext {
    CallContext {
        subagent: None,
        class: Some(ToolClass::External {
            kind: kind.to_owned(),
            approval: ApprovalNeed::Always,
            destination: "acme/supplier-portal".to_owned(),
            leaves_company: true,
        }),
    }
}

fn mail_profile(f: &Fixture, extra: serde_json::Value) {
    let mut email = json!({"system": "company_server"});
    if let (Some(target), Some(source)) = (email.as_object_mut(), extra.as_object()) {
        for (k, v) in source {
            target.insert(k.clone(), v.clone());
        }
    }
    f.set_profile(json!({ "email": email }));
}

pub(crate) async fn approve(f: &Fixture, approval: ApprovalId) {
    let record = f.approvals.get(approval).await.expect("the approval");
    f.approvals
        .decide(
            ApprovalPress {
                card_ref: record.card.buttons.approve.clone(),
                decision: PressDecision::Approve,
            },
            PressOrigin::LocalScreen,
        )
        .await
        .expect("approve");
}

async fn pending(f: &Fixture) -> usize {
    f.approvals
        .list_pending(None, None)
        .await
        .expect("pending approvals")
        .items
        .len()
}

fn grant(agent: AgentInstanceId, root: &str, access: FolderAccess) -> (GrantedPlace, PlaceId) {
    let place = PlaceId::new();
    (
        GrantedPlace {
            grantee: Grantee::AgentInstance {
                agent_instance_id: agent,
            },
            place_id: place,
            root: PathBuf::from(root),
            access,
        },
        place,
    )
}

fn analyst(f: &Fixture) -> crate::vocab::InstanceRuleSet {
    rule_set(
        AgentInstanceId::new(),
        Authority::DocumentOnly,
        &f.folder("analyst-1"),
    )
}

fn engineer(f: &Fixture) -> crate::vocab::InstanceRuleSet {
    let mut set = rule_set(
        AgentInstanceId::new(),
        Authority::SystemChanging,
        &f.folder("eng-1"),
    );
    set.package_name = "eng".to_owned();
    set
}

fn fetch_only_docs() -> ArgumentRule {
    ArgumentRule {
        name: "fetch-only-docs".to_owned(),
        tool: "mcp__supplier-db__fetch".to_owned(),
        argument: "url".to_owned(),
        form: ArgumentForm::RefuseUnless,
        outcome: ArgumentOutcome::Refuse,
        present: None,
        equals_any: Vec::new(),
        patterns: Vec::new(),
        max_length: None,
        address: Some(AddressCondition {
            schemes: vec!["https".to_owned()],
            hosts: vec!["example.org".to_owned()],
            with_subdomains: true,
            path_prefixes: vec!["/api/public/".to_owned()],
            ..AddressCondition::default()
        }),
    }
}

fn with_gates(gates: Gates) -> Option<Declaration> {
    Some(Declaration {
        format_version: 1,
        gates,
        message_rules: Vec::new(),
        approval_required: Vec::new(),
    })
}

#[tokio::test]
async fn happy_path_a_read_inside_the_agent_folder_is_allowed() {
    let f = fixture().await;
    let set = analyst(&f);
    let folder = set.agent_folder.clone();
    let r = running(&f, set, None).await;
    let notes = folder.join("notes.md").display().to_string();
    let arguments = json!({ "file_path": notes });
    let outcome = f
        .engine
        .decide_tool_call(
            &r.builtin("Read", arguments.clone()),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "allow");
    assert_eq!(rule(&outcome), "app.default_allow");
    let request = f
        .journal
        .tool_request(outcome.tool_request_id)
        .await
        .unwrap()
        .expect("the ToolRequest is journalled");
    assert_eq!(request.state, ToolRequestState::Decided);
    assert_eq!(request.decision, Some(ToolRequestDecision::Allow));
    assert_eq!(
        request.arguments_digest,
        crate::decide::digest_of(&arguments).0
    );
    assert_eq!(
        request.deciding_rule.as_deref(),
        Some("app_folder_policy:app.default_allow")
    );
}

#[tokio::test]
async fn folder_grant_a_write_to_a_place_granted_for_reading_is_denied() {
    let f = fixture().await;
    let set = analyst(&f);
    let r = running(&f, set, None).await;
    let (quotes, _) = grant(
        r.agent,
        abs!("/Users/asha/Documents/Quotes"),
        FolderAccess::Read,
    );
    f.engine.replace_grants(r.workspace, vec![quotes]);
    let read = f
        .engine
        .decide_tool_call(
            &r.builtin(
                "Read",
                json!({"file_path": abs!("/Users/asha/Documents/Quotes/q3.xlsx")}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&read), "allow");
    let outcome = f
        .engine
        .decide_tool_call(
            &r.builtin(
                "Write",
                json!({"file_path": abs!("/Users/asha/Documents/Quotes/q3.xlsx")}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(outcome.decision.rule.source, RuleSource::AppFolderPolicy);
    assert_eq!(rule(&outcome), "app.folder_grant");
}

#[tokio::test]
async fn protected_location_is_never_granted() {
    let f = fixture().await;
    let r = running(&f, analyst(&f), None).await;
    let (home, _) = grant(r.agent, HOME, FolderAccess::ReadAndChange);
    f.engine.replace_grants(r.workspace, vec![home]);
    let outcome = f
        .engine
        .decide_tool_call(
            &r.builtin(
                "Read",
                json!({"file_path": abs!("/Users/asha/.ssh/id_ed25519")}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(rule(&outcome), "app.folder_grant");
}

#[tokio::test]
async fn approval_list_from_the_declaration_asks_every_time() {
    let f = fixture().await;
    let mut set = analyst(&f);
    set.declaration = Some(Declaration {
        format_version: 1,
        approval_required: vec!["email_send".to_owned()],
        ..Declaration::default()
    });
    set.approval_list = vec!["email_send".to_owned()];
    set.tools = vec![tool("email_send", &["to"])];
    let r = running(&f, set, None).await;
    for to in ["a@acme.co.uk", "b@acme.co.uk"] {
        let outcome = f
            .engine
            .decide_tool_call(
                &r.call(
                    "email_send",
                    ToolOrigin::PackageMcpServer {
                        server: "mailer".to_owned(),
                    },
                    json!({ "to": to }),
                ),
                &CallContext::default(),
            )
            .await;
        assert_eq!(verdict(&outcome), "ask");
        assert_eq!(rule(&outcome), "approval-list");
        assert_eq!(
            outcome.decision.rule.source,
            RuleSource::DeclarationToolRule
        );
        assert!(outcome.approval_id.is_some());
    }
}

#[tokio::test]
async fn the_sub_agent_tool_asks_under_either_of_its_names() {
    let f = fixture().await;
    for name in ["Task", "Agent"] {
        let mut set = analyst(&f);
        set.declaration = Some(Declaration {
            format_version: 1,
            approval_required: vec!["task".to_owned()],
            ..Declaration::default()
        });
        set.approval_list = vec!["task".to_owned()];
        let r = running(&f, set, None).await;
        let outcome = f
            .engine
            .decide_tool_call(
                &r.call(
                    name,
                    ToolOrigin::BuiltIn,
                    json!({ "subagent_type": "general-purpose" }),
                ),
                &CallContext::default(),
            )
            .await;
        assert_eq!(verdict(&outcome), "ask", "{name}");
        assert_eq!(rule(&outcome), "approval-list", "{name}");
    }
}

#[tokio::test]
async fn a_control_on_the_default_sub_agent_covers_a_call_naming_none() {
    let f = fixture().await;
    for name in ["Task", "Agent"] {
        let mut set = analyst(&f);
        set.declaration = Some(Declaration {
            format_version: 1,
            approval_required: vec!["task:general-purpose".to_owned()],
            ..Declaration::default()
        });
        set.approval_list = vec!["task:general-purpose".to_owned()];
        let r = running(&f, set, None).await;
        let outcome = f
            .engine
            .decide_tool_call(
                &r.call(name, ToolOrigin::BuiltIn, json!({ "prompt": "x" })),
                &CallContext::default(),
            )
            .await;
        assert_eq!(verdict(&outcome), "ask", "{name}");
    }
}

#[tokio::test]
async fn an_agent_cannot_write_what_its_next_session_loads() {
    let f = fixture().await;
    let set = analyst(&f);
    let folder = set.agent_folder.clone();
    let r = running(&f, set, None).await;
    let decide = |tool: &'static str, path: std::path::PathBuf| {
        let call = r.builtin(tool, json!({ "file_path": path.display().to_string() }));
        let engine = f.engine.clone();
        async move {
            engine
                .decide_tool_call(&call, &CallContext::default())
                .await
        }
    };
    for file in [
        "plugin/hooks/hooks.json",
        "plugin/commands/x.md",
        "instructions.md",
        "CLAUDE.md",
        ".claude/settings.json",
        ".github/hooks/x.json",
        "work/../plugin/hooks/hooks.json",
    ] {
        let outcome = decide("Write", folder.join(file)).await;
        assert_eq!(verdict(&outcome), "deny", "{file}");
        assert_eq!(rule(&outcome), "app.folder_grant", "{file}");
    }
    assert_eq!(
        verdict(&decide("Write", folder.join("notes.md")).await),
        "allow"
    );
    let read = decide("Read", folder.join("instructions.md")).await;
    assert_eq!(verdict(&read), "allow");
}

#[tokio::test]
async fn a_second_instance_reads_what_the_run_started_with() {
    let f = fixture().await;
    let set = analyst(&f);
    let r = running(&f, set, None).await;
    let record = f.engine.adopted_run(r.run).await.unwrap().unwrap();
    assert_eq!(record.agent_instance_id, r.agent);
    assert_eq!(record.workspace_id, r.workspace);
    assert_eq!(record.tool_subset, None);
    assert_eq!(f.engine.adopted_run(RunId::new()).await.unwrap(), None);
    f.engine.end_run(r.run).await;
    assert_eq!(f.engine.adopted_run(r.run).await.unwrap(), None);
}

#[tokio::test]
async fn a_second_instance_reads_the_narrowed_tools_of_the_run() {
    let f = fixture().await;
    let mut set = analyst(&f);
    set.declaration = Some(Declaration {
        format_version: 1,
        message_rules: vec![MessageRule {
            name: "read-only".to_owned(),
            priority: 0,
            command: None,
            conditions: vec![],
            decides: Some(DecidingOutcome::Tools {
                tools: vec!["Read".to_owned()],
            }),
            adds: vec![],
        }],
        ..Declaration::default()
    });
    let r = running(&f, set, None).await;
    let record = f.engine.adopted_run(r.run).await.unwrap().unwrap();
    assert_eq!(record.tool_subset, Some(vec!["Read".to_owned()]));
}

// SA-RECON-12 (amended 2026-01-24): the run's own time limit reaches a second instance.
#[tokio::test]
async fn a_second_instance_reads_the_time_limit_the_run_started_with() {
    let f = fixture().await;
    let mut set = analyst(&f);
    set.commands = vec!["triage".to_owned()];
    set.declaration = Some(Declaration {
        format_version: 1,
        gates: Gates {
            bounds: vec![crate::vocab::CommandBound {
                command: "triage".to_owned(),
                wall_clock_minutes: Some(5),
                max_rounds: None,
                max_repeated_failures: None,
            }],
            ..Gates::default()
        },
        message_rules: vec![MessageRule {
            name: "route".to_owned(),
            priority: 0,
            command: None,
            conditions: vec![],
            decides: Some(DecidingOutcome::Command {
                command: "triage".to_owned(),
            }),
            adds: vec![],
        }],
        ..Declaration::default()
    });
    let agent = set.agent_instance_id;
    f.engine.load_rule_set(set).expect("the set loads");
    let run = RunId::new();
    let decision = f
        .engine
        .begin_run(start(run, agent, WorkspaceId::new()))
        .await
        .unwrap();
    assert_eq!(decision.limit_seconds, Some(300));
    let record = f.engine.adopted_run(run).await.unwrap().unwrap();
    assert_eq!(record.limit_seconds, Some(300));

    // No bound lowered it: none is recorded.
    let plain = analyst(&f);
    let r = running(&f, plain, None).await;
    let record = f.engine.adopted_run(r.run).await.unwrap().unwrap();
    assert_eq!(record.limit_seconds, None);
}

#[tokio::test]
async fn a_write_into_a_git_folder_is_denied_though_the_place_is_granted() {
    let f = fixture().await;
    let set = analyst(&f);
    let r = running(&f, set, None).await;
    let (project, _) = grant(
        r.agent,
        abs!("/Users/asha/Documents/Project"),
        FolderAccess::ReadAndChange,
    );
    f.engine.replace_grants(r.workspace, vec![project]);
    let call = |tool: &str, path: &str| r.builtin(tool, json!({ "file_path": path }));
    let config = abs!("/Users/asha/Documents/Project/.git/config");
    let outcome = f
        .engine
        .decide_tool_call(&call("Write", config), &CallContext::default())
        .await;
    assert_eq!(verdict(&outcome), "deny");
    let read = f
        .engine
        .decide_tool_call(&call("Read", config), &CallContext::default())
        .await;
    assert_eq!(verdict(&read), "allow");
    let outside = f
        .engine
        .decide_tool_call(
            &call("Write", abs!("/Users/asha/Documents/Project/.gitignore")),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outside), "allow");
}

#[tokio::test]
async fn a_policy_verb_on_task_covers_a_named_sub_agent() {
    let f = fixture().await;
    for name in ["Task", "Agent"] {
        let mut set = analyst(&f);
        set.tool_policy = Some(AgentToolPolicy {
            prohibited_verbs: vec!["task".to_owned()],
            ..AgentToolPolicy::default()
        });
        let r = running(&f, set, None).await;
        let outcome = f
            .engine
            .decide_tool_call(
                &r.call(
                    name,
                    ToolOrigin::BuiltIn,
                    json!({ "subagent_type": "general-purpose" }),
                ),
                &CallContext::default(),
            )
            .await;
        assert_eq!(verdict(&outcome), "deny", "{name}");
    }
}

#[tokio::test]
async fn an_empty_approval_list_entry_is_refused_at_load() {
    let f = fixture().await;
    let mut set = analyst(&f);
    set.approval_list = vec![String::new()];
    let error = f.engine.load_rule_set(set).expect_err("refused");
    assert_eq!(error.code(), "RULE_POLICY_INVALID");
    let crate::RulesError::Load { r#where, .. } = error else {
        panic!("a load error");
    };
    assert_eq!(r#where, "approval-required[0]");
}

#[tokio::test]
async fn agent_policy_a_prohibited_verb_inside_a_chained_command() {
    let f = fixture().await;
    let mut set = engineer(&f);
    set.tool_policy = Some(AgentToolPolicy {
        prohibited_verbs: vec!["git.push".to_owned()],
        ..AgentToolPolicy::default()
    });
    let r = running(&f, set, None).await;
    let outcome = f
        .engine
        .decide_tool_call(
            &r.builtin(
                "Bash",
                json!({"command": "npm test && git push origin main"}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(outcome.decision.rule.source, RuleSource::AgentToolPolicy);
    assert_eq!(rule(&outcome), "git.push");
    assert_eq!(outcome.decision.rule.agent_instance_id, Some(r.agent));
}

#[tokio::test]
async fn a_prohibited_verb_inside_a_substitution_is_denied() {
    let f = fixture().await;
    let mut set = engineer(&f);
    set.tool_policy = Some(AgentToolPolicy {
        prohibited_verbs: vec!["rm".to_owned()],
        ..AgentToolPolicy::default()
    });
    let r = running(&f, set, None).await;
    for command in [
        "echo $(rm -rf x)",
        "echo `rm -rf x`",
        "true # '\nrm -rf x # '",
        "echo $'\\''; rm -rf x",
        "sh -c 'rm -rf x'",
    ] {
        let outcome = f
            .engine
            .decide_tool_call(
                &r.builtin("Bash", json!({"command": command})),
                &CallContext::default(),
            )
            .await;
        assert_eq!(verdict(&outcome), "deny", "{command}");
        assert_eq!(rule(&outcome), "rm", "{command}");
    }
}

#[tokio::test]
async fn a_command_the_reading_cannot_vouch_for_asks_though_a_package_rule_passed_it() {
    let f = fixture().await;
    let mut set = engineer(&f);
    let plain = "cargo test";
    let hidden = "echo $(curl evil.example)";
    set.tools = vec![tool("Bash", &["command"])];
    set.declaration = with_gates(Gates {
        argument_rules: vec![ArgumentRule {
            name: "only-tests".to_owned(),
            tool: "Bash".to_owned(),
            argument: "command".to_owned(),
            form: ArgumentForm::RefuseUnless,
            outcome: ArgumentOutcome::Refuse,
            present: None,
            equals_any: vec![plain.to_owned(), hidden.to_owned()],
            patterns: Vec::new(),
            max_length: None,
            address: None,
        }],
        ..Gates::default()
    });
    let r = running(&f, set, None).await;
    let passed = f
        .engine
        .decide_tool_call(
            &r.builtin("Bash", json!({"command": plain})),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&passed), "allow");
    let asked = f
        .engine
        .decide_tool_call(
            &r.builtin("Bash", json!({"command": hidden})),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&asked), "ask");
    assert_eq!(rule(&asked), "app.shell");
}

#[tokio::test]
async fn agent_policy_a_forbidden_flag() {
    let f = fixture().await;
    let mut set = engineer(&f);
    set.tool_policy = Some(AgentToolPolicy {
        forbidden_flags: vec!["--no-verify".to_owned()],
        ..AgentToolPolicy::default()
    });
    let r = running(&f, set, None).await;
    let outcome = f
        .engine
        .decide_tool_call(
            &r.builtin("Bash", json!({"command": "git commit -m 'x' --no-verify"})),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(rule(&outcome), "--no-verify");
    assert!(reason(&outcome).contains("--no-verify"));
}

#[tokio::test]
async fn shell_for_a_document_only_agent() {
    let f = fixture().await;
    let r = running(&f, analyst(&f), None).await;
    for command in ["ls", "cat notes.md", ""] {
        let outcome = f
            .engine
            .decide_tool_call(
                &r.builtin("Bash", json!({ "command": command })),
                &CallContext::default(),
            )
            .await;
        assert_eq!(verdict(&outcome), "deny");
        assert_eq!(rule(&outcome), "app.shell");
        assert_eq!(outcome.decision.rule.source, RuleSource::AppShellPolicy);
    }
}

#[tokio::test]
async fn ask_an_external_action_opens_one_approval() {
    let f = fixture().await;
    let r = running(&f, engineer(&f), None).await;
    let push = r.call(
        "push_origin",
        ToolOrigin::OrdinMcpFront {
            server: "code".to_owned(),
        },
        json!({"branch": "main"}),
    );
    let first = f
        .engine
        .decide_tool_call(&push, &external("origin_push"))
        .await;
    assert_eq!(verdict(&first), "ask");
    assert_eq!(rule(&first), "app.external_action");
    assert_eq!(pending(&f).await, 1);
    let record = f.approvals.get(first.approval_id.unwrap()).await.unwrap();
    assert!(matches!(
        record.card.asked_by,
        ordin_types::wire::approval::ApprovalAsker::Run { run_id } if run_id == r.run
    ));
    let again = f
        .engine
        .decide_tool_call(&push, &external("origin_push"))
        .await;
    assert_eq!(verdict(&again), "ask");
    assert_eq!(again.approval_id, first.approval_id);
    assert_eq!(pending(&f).await, 1, "no second approval exists");
}

#[tokio::test]
async fn approved_once_on_resume_c_80() {
    let f = fixture().await;
    mail_profile(&f, json!({}));
    let r = running(&f, analyst(&f), None).await;
    let send = r.call(
        "mcp__mail__email_ask_to_send",
        ToolOrigin::OrdinMcpFront {
            server: "mail".to_owned(),
        },
        json!({"to": [{"address": "dan@example.org"}], "subject": "Rates"}),
    );
    let first = f
        .engine
        .decide_tool_call(&send, &external("email_send"))
        .await;
    assert_eq!(verdict(&first), "ask");
    approve(&f, first.approval_id.unwrap()).await;
    let resumed = f
        .engine
        .decide_tool_call(&send, &external("email_send"))
        .await;
    assert_eq!(verdict(&resumed), "allow");
    assert_eq!(rule(&resumed), "app.approved_once");
    let third = f
        .engine
        .decide_tool_call(&send, &external("email_send"))
        .await;
    assert_eq!(verdict(&third), "ask");
    assert_ne!(third.approval_id, first.approval_id);
}

#[tokio::test]
async fn argument_rule_refuses_a_look_alike_host() {
    let f = fixture().await;
    let mut set = analyst(&f);
    set.tools = vec![tool("mcp__supplier-db__fetch", &["url"])];
    set.declaration = with_gates(Gates {
        argument_rules: vec![fetch_only_docs()],
        ..Gates::default()
    });
    let r = running(&f, set, None).await;
    let origin = ToolOrigin::PackageMcpServer {
        server: "supplier-db".to_owned(),
    };
    let outcome = f
        .engine
        .decide_tool_call(
            &r.call(
                "mcp__supplier-db__fetch",
                origin.clone(),
                json!({"url": "https://example.org.evil.net/api/public/x"}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(
        reason(&outcome),
        "Refused by the control fetch-only-docs: the argument url is not permitted."
    );
    assert_eq!(
        outcome.decision.rule.source,
        RuleSource::DeclarationArgumentRule
    );
    // The example's second call passes the rule and asks, as a package tool.
    let passes = f
        .engine
        .decide_tool_call(
            &r.call(
                "mcp__supplier-db__fetch",
                origin,
                json!({"url": "https://api.example.org/api/public/rates"}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&passes), "ask");
    assert_eq!(rule(&passes), "app.package_tool");
}

#[tokio::test]
async fn argument_rule_that_cannot_be_evaluated_holds_against_the_call() {
    let f = fixture().await;
    let mut set = analyst(&f);
    set.tools = vec![tool("bulk_fetch", &["items[].url"])];
    set.declaration = with_gates(Gates {
        argument_rules: vec![ArgumentRule {
            name: "no-plain-http".to_owned(),
            tool: "bulk_fetch".to_owned(),
            argument: "items[].url".to_owned(),
            form: ArgumentForm::RefuseWhen,
            outcome: ArgumentOutcome::Refuse,
            present: None,
            equals_any: Vec::new(),
            patterns: vec!["^http:".to_owned()],
            max_length: None,
            address: None,
        }],
        ..Gates::default()
    });
    let r = running(&f, set, None).await;
    let outcome = f
        .engine
        .decide_tool_call(
            &r.call(
                "bulk_fetch",
                ToolOrigin::OrdinMcpFront {
                    server: "work".to_owned(),
                },
                json!({"items": "not a list"}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(rule(&outcome), "no-plain-http");
}

#[tokio::test]
async fn pause_rule_asks_instead_of_refusing() {
    let f = fixture().await;
    let mut set = analyst(&f);
    set.tools = vec![tool("post_message", &["channel", "text"])];
    set.declaration = with_gates(Gates {
        argument_rules: vec![ArgumentRule {
            name: "team-channel-only".to_owned(),
            tool: "post_message".to_owned(),
            argument: "channel".to_owned(),
            form: ArgumentForm::RefuseUnless,
            outcome: ArgumentOutcome::Pause,
            present: None,
            equals_any: vec!["#team".to_owned()],
            patterns: Vec::new(),
            max_length: None,
            address: None,
        }],
        ..Gates::default()
    });
    let r = running(&f, set, None).await;
    let metrics = f.metrics();
    let outcome = f
        .engine
        .decide_tool_call(
            &r.call(
                "post_message",
                ToolOrigin::OrdinMcpFront {
                    server: "work".to_owned(),
                },
                json!({"channel": "#general", "text": "hello"}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "ask");
    assert_eq!(
        outcome.decision.rule.source,
        RuleSource::DeclarationArgumentRule
    );
    assert_eq!(rule(&outcome), "team-channel-only");
    let seen = metrics.0.lock().unwrap().clone();
    assert!(seen
        .iter()
        .any(|(name, _)| name == "wrightrun_tool_calls_paused_by_rule_total"));
}

#[tokio::test]
async fn message_rule_ends_a_run_with_a_stated_reply() {
    let f = fixture().await;
    let mut set = analyst(&f);
    set.configured = Some(ConfiguredControls {
        message_rules: vec![MessageRule {
            name: "holiday-reply".to_owned(),
            priority: 0,
            command: None,
            conditions: vec![Condition::Keywords {
                words: vec!["holiday".to_owned(), "policy".to_owned()],
                r#match: MatchMode::All,
            }],
            decides: Some(DecidingOutcome::Reply {
                text: "The holiday policy is in the handbook, section 4.".to_owned(),
            }),
            adds: Vec::new(),
        }],
        ..ConfiguredControls::default()
    });
    let agent = set.agent_instance_id;
    f.engine.load_rule_set(set).unwrap();
    let mut facts = start(RunId::new(), agent, WorkspaceId::new());
    facts.text = "Where is the holiday policy?".to_owned();
    let decision = f.engine.begin_run(facts).await.unwrap();
    match decision.outcome {
        StartOutcome::Replied { control, text } => {
            assert_eq!(control, "holiday-reply");
            assert_eq!(text, "The holiday policy is in the handbook, section 4.");
        }
        other => panic!("expected Replied, got {other:?}"),
    }
}

pub(crate) fn triage_duty(max_cycles: Option<u32>) -> DutyGate {
    DutyGate {
        duty: "triage-inbox".to_owned(),
        command: "triage".to_owned(),
        folder: "/.agent-run/triage-inbox/".to_owned(),
        work_check: None,
        done_check: None,
        max_cycles,
        period_seconds: max_cycles.map(|_| 86_400),
        max_failed_cycles: None,
        heartbeat: Some(HeartbeatForm {
            entry: "{time} {ending} cycle {cycle}".to_owned(),
            endings: vec!["cycle-limit".to_owned(), "overlap".to_owned()],
        }),
    }
}

fn triage_set(f: &Fixture, max_cycles: Option<u32>) -> crate::vocab::InstanceRuleSet {
    let mut set = analyst(f);
    set.commands = vec!["triage".to_owned()];
    set.declaration = with_gates(Gates {
        duties: vec![triage_duty(max_cycles)],
        ..Gates::default()
    });
    set
}

#[tokio::test]
async fn duty_lock_two_cycles_start_at_the_same_instant() {
    let f = fixture().await;
    let set = triage_set(&f, None);
    let agent = set.agent_instance_id;
    f.engine.load_rule_set(set.clone()).unwrap();
    let second = RulesEngine::build(
        f.store.clone(),
        f.journal.clone(),
        f.profile.clone(),
        f.approvals.clone(),
        f.host.clone(),
        config(ProgramKind::Desktop),
    );
    second.load_rule_set(set).unwrap();
    let mut a = start(RunId::new(), agent, WorkspaceId::new());
    a.command = Some("triage".to_owned());
    let mut b = start(RunId::new(), agent, WorkspaceId::new());
    b.command = Some("triage".to_owned());
    let (one, two) = tokio::join!(f.engine.begin_run(a), second.begin_run(b));
    let outcomes = [one.unwrap().outcome, two.unwrap().outcome];
    let proceeded = outcomes
        .iter()
        .filter(|o| matches!(o, StartOutcome::Proceed))
        .count();
    assert_eq!(proceeded, 1, "exactly one proceeds");
    assert!(outcomes.iter().any(|o| matches!(
        o,
        StartOutcome::Ended { control, ending } if control == "duty:triage-inbox:lock" && ending == "overlap"
    )));
}

#[tokio::test]
async fn cycle_limit_counts_within_a_fixed_window() {
    let f = fixture().await;
    let set = triage_set(&f, Some(2));
    let agent = set.agent_instance_id;
    f.engine.load_rule_set(set).unwrap();
    let mut outcomes = Vec::new();
    for _ in 0..3 {
        let run = RunId::new();
        let mut facts = start(run, agent, WorkspaceId::new());
        facts.command = Some("triage".to_owned());
        outcomes.push(f.engine.begin_run(facts).await.unwrap().outcome);
        f.engine.end_run(run).await;
    }
    assert!(matches!(outcomes[0], StartOutcome::Proceed));
    assert!(matches!(outcomes[1], StartOutcome::Proceed));
    assert!(matches!(
        &outcomes[2],
        StartOutcome::Ended { ending, .. } if ending == "cycle-limit"
    ));
    let lines = f.host.appended.lock().unwrap().clone();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].0, "/.agent-run/triage-inbox/heartbeat.md");
    assert!(lines[0].1.contains("cycle-limit"));
    assert!(lines[0].1.ends_with("cycle 1"));
}

#[tokio::test]
async fn judged_loop_refuses_the_ending_then_asks_after_three_refusals() {
    let f = fixture().await;
    let mut set = analyst(&f);
    set.commands = vec!["draft".to_owned()];
    set.reviewer = Some("checker".to_owned());
    set.declaration = with_gates(Gates {
        judged_loops: vec![JudgedLoopGate {
            commands: vec!["draft".to_owned()],
            reviewer: "checker".to_owned(),
            pause_after: 3,
        }],
        ..Gates::default()
    });
    let folder = set.agent_folder.clone();
    let r = running(&f, set, Some("draft")).await;
    let write = f
        .engine
        .decide_tool_call(
            &r.builtin(
                "Write",
                json!({"file_path": folder.join("draft.md").display().to_string()}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&write), "allow");
    for _ in 0..3 {
        match f.engine.decide_run_end(r.run).await {
            EndDecision::Continue { control, message } => {
                assert_eq!(control, "judged-loop:draft");
                assert!(message.contains("checker"));
            }
            other => panic!("expected Continue, got {other:?}"),
        }
    }
    let EndDecision::Ask { gates, approval_id } = f.engine.decide_run_end(r.run).await else {
        panic!("expected Ask");
    };
    assert_eq!(gates, vec!["judged-loop:draft".to_owned()]);
    approve(&f, approval_id).await;
    assert!(matches!(
        f.engine.settle_end_gates(r.run, true).await,
        EndDecision::End
    ));
}

#[tokio::test]
async fn configuration_cannot_override_a_package_gate() {
    let f = fixture().await;
    let gate = StepGate {
        name: "deploy-needs-plan".to_owned(),
        point: Point::BeforeTool,
        command: None,
        tool: None,
        conditions: vec![Condition::PathAbsent {
            path: "/plan.md".to_owned(),
        }],
        outcome: GateOutcome::Refuse,
    };
    let mut set = analyst(&f);
    set.declaration = with_gates(Gates {
        steps: vec![gate.clone()],
        ..Gates::default()
    });
    let agent = set.agent_instance_id;
    f.engine.load_rule_set(set.clone()).unwrap();
    set.configured = Some(ConfiguredControls {
        steps: vec![gate],
        ..ConfiguredControls::default()
    });
    let error = f.engine.load_rule_set(set).expect_err("refused");
    assert_eq!(error.code(), "RULE_GATE_OVERRIDE");
    let items = f.engine.startup_items(agent);
    assert!(
        items.iter().any(|item| matches!(item,
            ordin_types::wire::startup::StartUpReportItem::EnforcedRule { rule, .. } if rule.rule_id == "deploy-needs-plan")),
        "the previous set stays loaded"
    );
}

#[tokio::test]
async fn fail_closed_no_rule_set_loaded() {
    let f = fixture().await;
    let ghost = AgentInstanceId::new();
    let run = RunId::new();
    let workspace = WorkspaceId::new();
    assert!(f
        .engine
        .begin_run(start(run, ghost, workspace))
        .await
        .is_err());
    let outcome = f
        .engine
        .decide_tool_call(
            &call(
                run,
                ghost,
                workspace,
                "Read",
                ToolOrigin::BuiltIn,
                json!({"file_path": abs!("/tmp/x")}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(outcome.decision.rule.source, RuleSource::FailClosed);
}

#[tokio::test]
async fn fail_closed_the_approval_service_is_unavailable() {
    let f = fixture().await;
    let r = running(&f, engineer(&f), None).await;
    f.drop_table("approval_buttons").await;
    f.drop_table("approvals").await;
    let (outcome, code) = f
        .engine
        .decide_coded(
            &r.builtin("Bash", json!({"command": "make deploy"})),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(outcome.decision.rule.source, RuleSource::FailClosed);
    assert_eq!(code, Some("RULE_APPROVAL_UNAVAILABLE"));
}

#[tokio::test]
async fn server_switch_external_kind_switched_off() {
    let f = fixture_with(ProgramKind::PersonalServer).await;
    f.set_profile(json!({"server": {"external_actions_off": ["telegram_post"]}}));
    let r = running(&f, analyst(&f), None).await;
    let outcome = f
        .engine
        .decide_tool_call(
            &r.call(
                "mcp__telegram__telegram_post",
                ToolOrigin::OrdinMcpFront {
                    server: "telegram".to_owned(),
                },
                json!({"text": "hi"}),
            ),
            &external("telegram_post"),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(rule(&outcome), "app.server.external_off");
}

#[tokio::test]
async fn email_rules_from_the_profile() {
    let f = fixture().await;
    mail_profile(
        &f,
        json!({"internal_only": true, "company_domains": ["acme.co.uk"]}),
    );
    let r = running(&f, analyst(&f), None).await;
    let outcome = f
        .engine
        .decide_tool_call(
            &r.call(
                "mcp__mail__email_ask_to_send",
                ToolOrigin::OrdinMcpFront {
                    server: "mail".to_owned(),
                },
                json!({"to": [{"address": "dan.moss@kestrel.co.uk"}]}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(rule(&outcome), "app.email.internal_only");
}

#[tokio::test]
async fn reviewer_kept_to_reading_agent_profile_c_78_confirmed() {
    let f = fixture_with(ProgramKind::AgentSingle).await;
    let mut set = analyst(&f);
    set.reviewer = Some("checker".to_owned());
    let folder = set.agent_folder.clone();
    let r = running(&f, set, None).await;
    let ctx = CallContext {
        subagent: Some("checker".to_owned()),
        class: None,
    };
    let path = folder.join("x.md").display().to_string();
    let outcome = f
        .engine
        .decide_tool_call(&r.builtin("Write", json!({ "file_path": path })), &ctx)
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(rule(&outcome), "app.reviewer_read_only");
    let read = f
        .engine
        .decide_tool_call(&r.builtin("Read", json!({ "file_path": path })), &ctx)
        .await;
    assert_eq!(verdict(&read), "allow");
}

fn command(r: &Running, command: &str, declared: bool) -> CommandRequest {
    CommandRequest {
        workspace_id: r.workspace,
        origin: CommandOrigin::Run {
            run_id: r.run,
            agent_instance_id: r.agent,
        },
        command: command.to_owned(),
        working_dir: PathBuf::from(abs!("/Users/asha/code/supplier-portal")),
        declared_check: declared,
    }
}

#[tokio::test]
async fn a_declared_check_from_a_run_is_allowed_without_a_card() {
    let f = fixture().await;
    let r = running(&f, engineer(&f), None).await;
    let outcome = f
        .engine
        .decide_command(command(&r, "cargo test", true))
        .await;
    assert_eq!(verdict(&outcome), "allow");
    assert_eq!(rule(&outcome), "app.shell.declared_check");
    assert_eq!(pending(&f).await, 0);
}

#[tokio::test]
async fn an_undeclared_command_from_a_run_asks_and_the_caller_waits() {
    let f = fixture().await;
    let r = running(&f, engineer(&f), None).await;
    let outcome = f
        .engine
        .decide_command(command(&r, "make deploy", false))
        .await;
    assert_eq!(verdict(&outcome), "ask");
    assert_eq!(rule(&outcome), "app.shell");
    let approval = outcome.approval_id.expect("an approval id");
    let waiting = {
        let engine = f.engine.clone();
        tokio::spawn(async move { engine.wait_for_approval(approval, None).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    approve(&f, approval).await;
    assert_eq!(waiting.await.unwrap().unwrap(), ApprovalWait::Approved);
    let unused = store::approved_call_unused(f.pool(), &approval.to_string())
        .await
        .unwrap();
    assert_eq!(unused, Some(false), "the approved-once record is spent");
}

#[tokio::test]
async fn a_persons_undeclared_command_is_refused() {
    let f = fixture().await;
    let outcome = f
        .engine
        .decide_command(CommandRequest {
            workspace_id: WorkspaceId::new(),
            origin: CommandOrigin::Person,
            command: "rm -rf build".to_owned(),
            working_dir: PathBuf::from(abs!("/Users/asha/code/supplier-portal")),
            declared_check: false,
        })
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(rule(&outcome), "app.shell.person_undeclared");
}

#[tokio::test]
async fn waiting_past_the_deadline_leaves_the_approval_open() {
    let f = fixture().await;
    let r = running(&f, engineer(&f), None).await;
    let outcome = f
        .engine
        .decide_tool_call(
            &r.builtin("Bash", json!({"command": "make deploy"})),
            &CallContext::default(),
        )
        .await;
    let approval = outcome.approval_id.expect("an approval");
    let until = tokio::time::Instant::now() + Duration::from_millis(50);
    let waited = f
        .engine
        .wait_for_approval(approval, Some(until))
        .await
        .unwrap();
    assert_eq!(waited, ApprovalWait::Pending);
    assert_eq!(
        store::approved_call_unused(f.pool(), &approval.to_string())
            .await
            .unwrap(),
        Some(true)
    );
    assert_eq!(
        f.approvals.get(approval).await.unwrap().state,
        ApprovalState::Pending
    );
}

#[tokio::test]
async fn github_setting_lets_a_pull_request_comment_through() {
    let f = fixture().await;
    let r = running(&f, engineer(&f), None).await;
    let (portal, place) = grant(
        r.agent,
        abs!("/Users/asha/code/supplier-portal"),
        FolderAccess::ReadAndChange,
    );
    f.engine.replace_grants(r.workspace, vec![portal]);
    f.engine.replace_git_host_settings(
        place,
        vec![GitHostSetting {
            kind: "comment".to_owned(),
            approval_required: false,
        }],
    );
    // `cmd.exe` gives a quote no meaning, so on Windows a quoted segment gets no kind and asks.
    let command = if cfg!(windows) {
        "gh pr comment 12 --body done"
    } else {
        "gh pr comment 12 --body 'done'"
    };
    let comment = f
        .engine
        .decide_tool_call(
            &r.builtin("Bash", json!({"command": command})),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&comment), "allow");
    assert_eq!(rule(&comment), "app.git_host_setting");
    let merge = f
        .engine
        .decide_tool_call(
            &r.builtin("Bash", json!({"command": "gh pr merge 12"})),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&merge), "ask");
    assert_eq!(rule(&merge), "app.shell");
}

#[tokio::test]
async fn a_protected_location_cannot_become_a_place() {
    let f = fixture().await;
    assert!(f
        .engine
        .is_protected_location(Path::new(abs!("/Users/asha/.ssh"))));
    assert!(!f
        .engine
        .is_protected_location(Path::new(abs!("/Users/asha/Documents/Quotes"))));
}

#[tokio::test]
async fn reading_a_granted_place() {
    let f = fixture().await;
    let agent = AgentInstanceId::new();
    let workspace = WorkspaceId::new();
    let (quotes, place) = grant(
        agent,
        abs!("/Users/asha/Documents/Quotes"),
        FolderAccess::Read,
    );
    f.engine.replace_grants(workspace, vec![quotes]);
    let mut check = PlaceRead {
        workspace_id: workspace,
        reader: PlaceReader::Agent {
            agent_instance_id: agent,
        },
        place_id: place,
        path: "2026/q3.pdf".to_owned(),
    };
    assert!(f.engine.may_read(&check));
    check.path = "../other/secret.txt".to_owned();
    assert!(!f.engine.may_read(&check));
    check.path = "2026/q3.pdf".to_owned();
    check.reader = PlaceReader::HostedAgent;
    assert!(!f.engine.may_read(&check));
}

#[tokio::test]
async fn path_check_for_ordins_own_conversion_files() {
    let f = fixture().await;
    let mut req = PathAccessRequest {
        workspace_id: None,
        agent: None,
        crew_charter_id: None,
        path: PathBuf::from(APP_ROOT).join("conversions/deck.pptx"),
        access: FolderAccess::ReadAndChange,
    };
    assert!(matches!(
        f.engine.check_path_access(&req).verdict,
        Verdict::Allow
    ));
    req.path = PathBuf::from(abs!("/etc/hosts"));
    let decision = f.engine.check_path_access(&req);
    assert!(matches!(decision.verdict, Verdict::Deny { .. }));
    assert_eq!(decision.rule.rule_id, "app.folder_grant");
}

#[tokio::test]
async fn email_rules_collect_every_violation() {
    let f = fixture().await;
    mail_profile(
        &f,
        json!({"internal_only": true, "company_domains": ["acme.co.uk"], "max_recipients": 2, "foot_line_required": true}),
    );
    let to = vec![
        "asha@acme.co.uk".to_owned(),
        "dan.moss@kestrel.co.uk".to_owned(),
    ];
    let cc = vec!["li@acme.co.uk".to_owned()];
    let verdict = f.engine.check_email(&EmailCheck {
        to: &to,
        cc: &cc,
        bcc: &[],
        attachments: &[],
        drafted_by_agent: true,
        body: "Here are the rates.",
        foot_line: "Drafted with an agent in ordin",
    });
    assert_eq!(
        verdict,
        EmailVerdict::Refused {
            reasons: vec![
                EmailViolation::TooManyRecipients { count: 3, max: 2 },
                EmailViolation::RecipientOutsideCompany {
                    address: "dan.moss@kestrel.co.uk".to_owned()
                },
                EmailViolation::FootLineMissing,
            ]
        }
    );
}

#[tokio::test]
async fn duty_lock_state_for_the_operator() {
    let f = fixture().await;
    let key = duty_lock_key("triage-inbox");
    let until = crate::engine::now_ms() + 20_000;
    sqlx::query("INSERT INTO rules_control_locks (key, holder, expires_at) VALUES (?1, ?2, ?3)")
        .bind(&key)
        .bind(RunId::new().to_string())
        .bind(until)
        .execute(f.pool())
        .await
        .unwrap();
    assert!(f.engine.lock_held(&key).await.unwrap());
    assert!(!f.engine.lock_held("duty:other").await.unwrap());
}

#[tokio::test]
async fn address_parsing_resolves_dot_segments() {
    let address =
        RulesEngine::parse_address("HTTPS://API.Example.org:443/api/public/%2e%2e/admin?x=1")
            .unwrap();
    assert_eq!(address.scheme, "https");
    assert_eq!(address.host, "api.example.org");
    assert_eq!(address.port, Some(443));
    assert_eq!(address.path, "/api/admin");
    assert_eq!(address.query.as_deref(), Some("x=1"));
}

#[tokio::test]
async fn a_craft_server_added_after_start_is_still_ordins() {
    let f = fixture().await;
    let r = running(&f, analyst(&f), None).await;
    let craft = f
        .engine
        .decide_tool_call(
            &r.call(
                "mcp__craft_slides__render",
                ToolOrigin::PackageMcpServer {
                    server: "craft_slides".to_owned(),
                },
                json!({}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(
        verdict(&craft),
        "allow",
        "treated as OrdinMcpFront, not a package tool"
    );
    let package = f
        .engine
        .decide_tool_call(
            &r.call(
                "mcp__supplier-db__fetch",
                ToolOrigin::PackageMcpServer {
                    server: "supplier-db".to_owned(),
                },
                json!({}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(rule(&package), "app.package_tool");
}

fn slack(r: &Running, tool: &str) -> ordin_types::wire::rules::ToolCall {
    r.call(
        tool,
        ToolOrigin::OrdinMcpFront {
            server: "slack".to_owned(),
        },
        json!({"channel": "C123"}),
    )
}

#[tokio::test]
async fn slack_switched_off_on_the_server_denies_drafting_and_asking() {
    let f = fixture_with(ProgramKind::PersonalServer).await;
    f.set_profile(json!({"server": {"external_actions_off": ["slack_post"]}}));
    let r = running(&f, analyst(&f), None).await;
    for tool in [
        "mcp__slack__slack_ask_to_post",
        "mcp__slack__slack_draft_post",
    ] {
        let outcome = f
            .engine
            .decide_tool_call(&slack(&r, tool), &CallContext::default())
            .await;
        assert_eq!(verdict(&outcome), "deny");
        assert_eq!(rule(&outcome), "app.server.slack_off");
        assert_eq!(
            reason(&outcome),
            "Slack posts are switched off on this server."
        );
    }
}

#[tokio::test]
async fn reading_slack_is_not_switched_off() {
    let f = fixture_with(ProgramKind::PersonalServer).await;
    f.set_profile(json!({"server": {"external_actions_off": ["slack_post"]}}));
    let r = running(&f, analyst(&f), None).await;
    let outcome = f
        .engine
        .decide_tool_call(
            &slack(&r, "mcp__slack__slack_read"),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "allow");
    assert_eq!(rule(&outcome), "app.default_allow");
}

#[tokio::test]
async fn the_slack_tools_ask_no_card_of_their_own_on_the_desktop() {
    let f = fixture().await;
    let r = running(&f, analyst(&f), None).await;
    let outcome = f
        .engine
        .decide_tool_call(
            &slack(&r, "mcp__slack__slack_ask_to_post"),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "allow");
    assert_eq!(outcome.approval_id, None);
    assert_eq!(pending(&f).await, 0);
}

#[tokio::test]
async fn the_switch_is_server_only() {
    let f = fixture().await;
    f.set_profile(json!({"server": {"external_actions_off": ["slack_post"]}}));
    let r = running(&f, analyst(&f), None).await;
    let outcome = f
        .engine
        .decide_tool_call(
            &slack(&r, "mcp__slack__slack_draft_post"),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "allow");
}

/// Attachments from a closed place are refused (K.7), so the email scenarios' helper is used here.
#[tokio::test]
async fn attachments_from_a_closed_place_are_refused() {
    let f = fixture().await;
    let place = PlaceId::new();
    mail_profile(&f, json!({"no_attach_places": [place.to_string()]}));
    let attachments = vec![EmailAttachment {
        place_id: place,
        path: "q3.xlsx".to_owned(),
    }];
    let verdict = f.engine.check_email(&EmailCheck {
        to: &["a@acme.co.uk".to_owned()],
        cc: &[],
        bcc: &[],
        attachments: &attachments,
        drafted_by_agent: false,
        body: "",
        foot_line: "",
    });
    assert!(matches!(
        verdict,
        EmailVerdict::Refused { ref reasons } if matches!(reasons[0], EmailViolation::AttachmentFromClosedPlace { .. })
    ));
    let _ = Arc::clone(&f.engine);
}

// spec: ordin-desktop/components/RulesEngine#RE-PATH-1
#[cfg(windows)]
#[tokio::test]
async fn error_path_a_write_resolved_into_the_application_data_root_is_refused_on_windows() {
    let Fixture {
        _dir,
        store,
        journal,
        approvals,
        host,
        profile,
        ..
    } = fixture().await;
    // A real application data root on disk, in its plain form as the program writes it.
    let temp = std::fs::canonicalize(&_dir.0).expect("canonical temporary folder");
    let data_root = ordin_types::paths::plain_form(temp).join("ordin");
    let folder = data_root.join("agents").join("analyst-1");
    let journal_folder = data_root.join("journal");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&journal_folder).unwrap();
    // A directory junction needs no privilege, unlike a symbolic link; it is resolved by the
    // operating system as a link is.
    let link = folder.join("shortcut");
    let made = std::process::Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&link)
        .arg(&journal_folder)
        .status()
        .expect("run mklink");
    assert!(made.success(), "mklink /J failed");
    // The operating system resolves the link to the verbatim form; the engine reads it plainly.
    let resolved = crate::policy::real_path(&link.join("journal.db")).expect("resolved");
    assert!(
        !resolved.to_string_lossy().starts_with(r"\\?\"),
        "{}",
        resolved.display()
    );
    assert_eq!(resolved, journal_folder.join("journal.db"));

    let mut settings = config(ProgramKind::Desktop);
    settings.app_data_root = data_root.clone();
    let engine = RulesEngine::build(
        store.clone(),
        journal.clone(),
        profile.clone(),
        approvals.clone(),
        host.clone(),
        settings,
    );
    let f = Fixture {
        _dir,
        store,
        journal,
        approvals,
        host,
        profile,
        engine,
        data_root,
    };
    let set = rule_set(AgentInstanceId::new(), Authority::DocumentOnly, &folder);
    let r = running(&f, set, None).await;
    let notes = folder.join("notes.md").display().to_string();
    let own = f
        .engine
        .decide_tool_call(
            &r.builtin("Write", json!({ "file_path": notes })),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&own), "allow");
    let through = link.join("journal.db").display().to_string();
    let through_link = f
        .engine
        .decide_tool_call(
            &r.builtin("Write", json!({ "file_path": through })),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&through_link), "deny");
    assert_eq!(
        through_link.decision.rule.source,
        RuleSource::AppFolderPolicy
    );
}

// An agent's own folder lies inside the application data root, which is also a protected root. Inside its own
// folder an agent reads, and writes unless the folder is read-only; nothing else of the data root is its to
// use, not another agent's folder and not ordin's own files (RulesEngine.md H.2). Found when the document agent
// could not save a summary into its own folder.
// spec: ordin-desktop/components/RulesEngine#SA-RECON-14
#[tokio::test]
async fn an_agent_uses_its_own_folder_inside_the_protected_data_root() {
    let f = fixture().await;
    let id = AgentInstanceId::new();
    let folder = PathBuf::from(APP_ROOT)
        .join("agents")
        .join(id.to_string())
        .join("folder");
    let r = running(&f, rule_set(id, Authority::DocumentOnly, &folder), None).await;
    let check = |path: PathBuf, access: FolderAccess| {
        f.engine
            .check_path_access(&PathAccessRequest {
                workspace_id: Some(r.workspace),
                agent: Some(r.agent),
                crew_charter_id: None,
                path,
                access,
            })
            .verdict
    };
    let summary = folder.join("Quarterly plan — summary.md");
    assert!(
        matches!(
            check(summary.clone(), FolderAccess::ReadAndChange),
            Verdict::Allow
        ),
        "a write in its own folder"
    );
    assert!(
        matches!(check(summary, FolderAccess::Read), Verdict::Allow),
        "a read in its own folder"
    );
    let other = PathBuf::from(APP_ROOT)
        .join("agents")
        .join(AgentInstanceId::new().to_string())
        .join("folder")
        .join("notes.md");
    assert!(
        matches!(check(other, FolderAccess::Read), Verdict::Deny { .. }),
        "another agent's folder stays closed"
    );
    let journal = PathBuf::from(APP_ROOT).join("journal.db");
    assert!(
        matches!(check(journal, FolderAccess::Read), Verdict::Deny { .. }),
        "ordin's own files stay closed"
    );
    assert!(
        matches!(
            check(
                PathBuf::from(HOME).join(".ssh/id_ed25519"),
                FolderAccess::Read
            ),
            Verdict::Deny { .. }
        ),
        "a protected root stays closed"
    );
}

#[tokio::test]
async fn a_read_only_agent_folder_inside_the_data_root_refuses_writes() {
    let f = fixture().await;
    let id = AgentInstanceId::new();
    let folder = PathBuf::from(APP_ROOT)
        .join("agents")
        .join(id.to_string())
        .join("folder");
    let mut set = rule_set(id, Authority::DocumentOnly, &folder);
    set.agent_folder_read_only = true;
    let r = running(&f, set, None).await;
    let mut req = PathAccessRequest {
        workspace_id: Some(r.workspace),
        agent: Some(r.agent),
        crew_charter_id: None,
        path: folder.join("notes.md"),
        access: FolderAccess::Read,
    };
    assert!(
        matches!(f.engine.check_path_access(&req).verdict, Verdict::Allow),
        "a read is allowed"
    );
    req.access = FolderAccess::ReadAndChange;
    assert!(
        matches!(
            f.engine.check_path_access(&req).verdict,
            Verdict::Deny { .. }
        ),
        "a write is refused"
    );
}
