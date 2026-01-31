//! X1: one test per row of RulesEngine.md's Error Table, and X4: the codes this crate owns. The
//! ADR-107 row (`RULE_CHECKER_UNREACHABLE`) is also run against the HTTP stand-in in
//! `tests/content_check.rs`.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use ordin_types::domain::agent_profile::{CheckPoint, ContentCheck, OnBlock, OnUnreachable};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::places::FolderAccess;
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::wire::rules::{RuleSource, ToolOrigin, Verdict};
use ordin_types::{AgentInstanceId, ApprovalId, CodedError, RunId, WorkspaceId};
use serde_json::json;

use super::acceptance::{reason, rule, verdict};
use super::support::*;
use crate::types::{
    CallContext, CommandOrigin, CommandRequest, ContentCheckOutcome, PathAccessRequest,
};
use crate::vocab::{
    AgentToolPolicy, ArgumentForm, ArgumentOutcome, ArgumentRule, CallTarget, Condition,
    ConfiguredControls, DecidingOutcome, Declaration, GateOutcome, Gates, JudgedLoopGate,
    MatchMode, MessageRule, Point, StepGate,
};
use crate::{RulesEngine, RulesError, ALL_CODES};

fn set(f: &Fixture) -> crate::vocab::InstanceRuleSet {
    rule_set(
        AgentInstanceId::new(),
        Authority::SystemChanging,
        &f.folder("eng-1"),
    )
}

fn assert_load(result: Result<Vec<crate::ControlLine>, RulesError>, code: &str) -> String {
    match result {
        Err(error @ RulesError::Load { .. }) => {
            assert_eq!(error.code(), code);
            assert!(!error.retryable(), "{code} is permanent");
            let RulesError::Load {
                r#where, message, ..
            } = error
            else {
                unreachable!()
            };
            assert!(!message.is_empty());
            r#where
        }
        other => panic!("expected {code}, got {other:?}"),
    }
}

fn arg_rule(name: &str, tool: &str, argument: &str) -> ArgumentRule {
    ArgumentRule {
        name: name.to_owned(),
        tool: tool.to_owned(),
        argument: argument.to_owned(),
        form: ArgumentForm::RefuseWhen,
        outcome: ArgumentOutcome::Refuse,
        present: None,
        equals_any: vec!["x".to_owned()],
        patterns: Vec::new(),
        max_length: None,
        address: None,
    }
}

fn declaring(gates: Gates) -> Option<Declaration> {
    Some(Declaration {
        format_version: 1,
        gates,
        ..Declaration::default()
    })
}

#[tokio::test]
async fn a_control_names_a_tool_that_is_not_bound() {
    let f = fixture().await;
    let mut s = set(&f);
    s.declaration = declaring(Gates {
        argument_rules: vec![arg_rule("fetch-only-docs", "http_get", "url")],
        ..Gates::default()
    });
    let place = assert_load(f.engine.load_rule_set(s.clone()), "RULE_UNBOUND_TOOL");
    assert_eq!(place, "wrightrun-declaration.yaml: gates.argument-rules[0]");
    // A step gate, a called condition, a tools outcome and a reviewer that is not a sub-agent.
    s.declaration = declaring(Gates {
        steps: vec![StepGate {
            name: "g".to_owned(),
            point: Point::BeforeTool,
            command: None,
            tool: Some("missing".to_owned()),
            conditions: Vec::new(),
            outcome: GateOutcome::Refuse,
        }],
        ..Gates::default()
    });
    assert_load(f.engine.load_rule_set(s.clone()), "RULE_UNBOUND_TOOL");
    s.declaration = declaring(Gates {
        steps: vec![StepGate {
            name: "g".to_owned(),
            point: Point::End,
            command: None,
            tool: None,
            conditions: vec![Condition::Called {
                target: CallTarget::Subagent,
                name: "nobody".to_owned(),
                count: 1,
            }],
            outcome: GateOutcome::Refuse,
        }],
        ..Gates::default()
    });
    assert_load(f.engine.load_rule_set(s.clone()), "RULE_UNBOUND_TOOL");
    s.declaration = declaring(Gates {
        judged_loops: vec![JudgedLoopGate {
            commands: vec!["draft".to_owned()],
            reviewer: "nobody".to_owned(),
            pause_after: 3,
        }],
        ..Gates::default()
    });
    assert_load(f.engine.load_rule_set(s.clone()), "RULE_UNBOUND_TOOL");
    s.declaration = Some(Declaration {
        format_version: 1,
        message_rules: vec![MessageRule {
            name: "only".to_owned(),
            priority: 0,
            command: None,
            conditions: vec![Condition::RunInProgress],
            decides: Some(DecidingOutcome::Tools {
                tools: vec!["missing".to_owned()],
            }),
            adds: Vec::new(),
        }],
        ..Declaration::default()
    });
    assert_load(f.engine.load_rule_set(s), "RULE_UNBOUND_TOOL");
}

#[tokio::test]
async fn an_argument_rule_names_a_place_the_tool_does_not_describe() {
    let f = fixture().await;
    let mut s = set(&f);
    s.tools = vec![tool("fetch", &["url", "body.*"])];
    s.declaration = declaring(Gates {
        argument_rules: vec![arg_rule("r", "fetch", "headers")],
        ..Gates::default()
    });
    assert_load(f.engine.load_rule_set(s.clone()), "RULE_UNKNOWN_ARGUMENT");
    s.declaration = declaring(Gates {
        argument_rules: vec![arg_rule("r", "fetch", "body.target.href")],
        ..Gates::default()
    });
    assert!(
        f.engine.load_rule_set(s).is_ok(),
        "a place under `.*` is described"
    );
}

#[tokio::test]
async fn a_pattern_is_empty_over_200_characters_or_refused() {
    let f = fixture().await;
    for pattern in [String::new(), "a".repeat(201), "(".to_owned()] {
        let mut s = set(&f);
        s.configured = Some(ConfiguredControls {
            message_rules: vec![MessageRule {
                name: "m".to_owned(),
                priority: 0,
                command: None,
                conditions: vec![Condition::Pattern {
                    patterns: vec![pattern],
                    r#match: MatchMode::Any,
                }],
                decides: None,
                adds: Vec::new(),
            }],
            ..ConfiguredControls::default()
        });
        assert_eq!(
            assert_load(f.engine.load_rule_set(s), "RULE_BAD_PATTERN"),
            "controls.message-rules[0]"
        );
    }
}

#[tokio::test]
async fn more_than_200_argument_rules() {
    let f = fixture().await;
    let mut s = set(&f);
    s.tools = vec![tool("fetch", &["url"])];
    s.declaration = declaring(Gates {
        argument_rules: (0..201)
            .map(|i| arg_rule(&format!("r{i}"), "fetch", "url"))
            .collect(),
        ..Gates::default()
    });
    assert_load(f.engine.load_rule_set(s), "RULE_TOO_MANY_RULES");
}

#[tokio::test]
async fn a_configured_control_takes_a_package_controls_name() {
    let f = fixture().await;
    let mut s = set(&f);
    s.declaration = declaring(Gates {
        duties: vec![super::acceptance::triage_duty(None)],
        ..Gates::default()
    });
    s.configured = Some(ConfiguredControls {
        steps: vec![StepGate {
            name: "duty:triage-inbox:lock".to_owned(),
            point: Point::End,
            command: None,
            tool: None,
            conditions: Vec::new(),
            outcome: GateOutcome::Refuse,
        }],
        ..ConfiguredControls::default()
    });
    assert_load(f.engine.load_rule_set(s.clone()), "RULE_GATE_OVERRIDE");
    // ADR-107: a content check may not take any other control's name.
    s.configured = Some(ConfiguredControls {
        content_checks: vec![ContentCheck {
            name: "duty:triage-inbox:stop".to_owned(),
            connection: "screen".to_owned(),
            operation: "check".to_owned(),
            points: vec![CheckPoint::Input],
            on_block: OnBlock::End,
            on_unreachable: OnUnreachable::Block,
            categories: Vec::new(),
            timeout_seconds: 5,
            max_chars: 100,
        }],
        ..ConfiguredControls::default()
    });
    assert_load(f.engine.load_rule_set(s), "RULE_GATE_OVERRIDE");
}

#[tokio::test]
async fn an_agent_policy_entry_is_malformed() {
    let f = fixture().await;
    let cases = [
        AgentToolPolicy {
            prohibited_verbs: vec!["Git Push".to_owned()],
            ..AgentToolPolicy::default()
        },
        AgentToolPolicy {
            forbidden_flags: vec!["no-verify".to_owned()],
            ..AgentToolPolicy::default()
        },
        AgentToolPolicy {
            requires_approval: vec!["a.b.c".to_owned()],
            ..AgentToolPolicy::default()
        },
    ];
    for policy in cases {
        let mut s = set(&f);
        s.tool_policy = Some(policy);
        assert_load(f.engine.load_rule_set(s), "RULE_POLICY_INVALID");
    }
    let mut s = set(&f);
    s.approval_list = vec!["x".repeat(201)];
    assert_load(f.engine.load_rule_set(s), "RULE_POLICY_INVALID");
}

#[tokio::test]
async fn a_decision_for_an_instance_with_no_loaded_set() {
    let f = fixture().await;
    let ghost = AgentInstanceId::new();
    let error = f
        .engine
        .begin_run(start(RunId::new(), ghost, WorkspaceId::new()))
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "RULE_RULESET_NOT_LOADED");
    assert!(!error.retryable());
    // Before a tool: a run whose set was unloaded afterwards.
    let r = running(&f, set(&f), None).await;
    f.engine.unload_rule_set(r.agent);
    let (outcome, code) = f
        .engine
        .decide_coded(
            &r.builtin("Read", json!({"file_path": "/x"})),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(outcome.decision.rule.source, RuleSource::FailClosed);
    assert_eq!(reason(&outcome), "This agent's rules are not loaded.");
    assert_eq!(code, Some("RULE_RULESET_NOT_LOADED"));
}

#[tokio::test]
async fn a_fact_cannot_be_read_at_the_start() {
    let f = fixture().await;
    let mut s = set(&f);
    s.commands = vec!["triage".to_owned()];
    s.declaration = declaring(Gates {
        duties: vec![super::acceptance::triage_duty(None)],
        ..Gates::default()
    });
    let agent = s.agent_instance_id;
    f.engine.load_rule_set(s).unwrap();
    f.host.fail.store(true, Ordering::SeqCst);
    let mut facts = start(RunId::new(), agent, WorkspaceId::new());
    facts.command = Some("triage".to_owned());
    let error = f.engine.begin_run(facts).await.expect_err("the run fails");
    assert_eq!(error.code(), "RULE_FACT_UNAVAILABLE");
    assert!(error.retryable());
    let RulesError::FactUnavailable { control } = error else {
        panic!("FactUnavailable");
    };
    assert_eq!(control, "duty:triage-inbox:stop");
    assert_eq!(f.count("SELECT COUNT(*) FROM rules_control_locks").await, 0);
}

#[tokio::test]
async fn a_fact_cannot_be_read_before_a_tool_call() {
    let f = fixture().await;
    let mut s = set(&f);
    s.declaration = declaring(Gates {
        steps: vec![StepGate {
            name: "needs-plan".to_owned(),
            point: Point::BeforeTool,
            command: None,
            tool: Some("Write".to_owned()),
            conditions: vec![Condition::PathAbsent {
                path: "/plan.md".to_owned(),
            }],
            outcome: GateOutcome::Refuse,
        }],
        ..Gates::default()
    });
    let folder = s.agent_folder.clone();
    let r = running(&f, s, None).await;
    f.host.fail.store(true, Ordering::SeqCst);
    let (outcome, code) = f
        .engine
        .decide_coded(
            &r.builtin(
                "Write",
                json!({"file_path": folder.join("a.md").display().to_string()}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(
        reason(&outcome),
        "Refused by the control needs-plan: a declared control could not be evaluated."
    );
    assert_eq!(code, Some("RULE_FACT_UNAVAILABLE"));
}

#[tokio::test]
async fn tool_name_too_long_arguments_over_1_mib_malformed_origin() {
    let f = fixture().await;
    let r = running(&f, set(&f), None).await;
    let cases = [
        (
            r.builtin(&"t".repeat(129), json!({})),
            "RULE_TOOL_NAME_INVALID",
            "app.input.tool_name",
        ),
        (
            r.builtin("bad name!", json!({})),
            "RULE_TOOL_NAME_INVALID",
            "app.input.tool_name",
        ),
        (
            r.builtin("Read", json!({"file_path": "x".repeat(1_048_577)})),
            "RULE_ARGUMENTS_TOO_LARGE",
            "app.input.arguments_size",
        ),
        (
            r.call(
                "t",
                ToolOrigin::PackageMcpServer {
                    server: "bad server".to_owned(),
                },
                json!({}),
            ),
            "RULE_ORIGIN_INVALID",
            "app.input.origin",
        ),
        (
            r.call(
                "t",
                ToolOrigin::OrdinMcpFront {
                    server: "s".repeat(65),
                },
                json!({}),
            ),
            "RULE_ORIGIN_INVALID",
            "app.input.origin",
        ),
    ];
    for (call, code, rule_id) in cases {
        let (outcome, got) = f.engine.decide_coded(&call, &CallContext::default()).await;
        assert_eq!(verdict(&outcome), "deny");
        assert_eq!(outcome.decision.rule.source, RuleSource::FailClosed);
        assert_eq!(rule(&outcome), rule_id);
        assert_eq!(got, Some(code));
    }
}

#[tokio::test]
async fn the_calls_run_is_unknown_or_belongs_to_another_instance() {
    let f = fixture().await;
    let r = running(&f, set(&f), None).await;
    let unknown = call(
        RunId::new(),
        r.agent,
        r.workspace,
        "Read",
        ToolOrigin::BuiltIn,
        json!({}),
    );
    let (outcome, code) = f
        .engine
        .decide_coded(&unknown, &CallContext::default())
        .await;
    assert_eq!(outcome.decision.rule.source, RuleSource::FailClosed);
    assert_eq!(code, Some("RULE_RUN_UNKNOWN"));
    let other = call(
        r.run,
        AgentInstanceId::new(),
        r.workspace,
        "Read",
        ToolOrigin::BuiltIn,
        json!({}),
    );
    let (outcome, code) = f.engine.decide_coded(&other, &CallContext::default()).await;
    assert_eq!(outcome.decision.rule.source, RuleSource::FailClosed);
    assert_eq!(code, Some("RULE_SESSION_MISMATCH"));
}

#[tokio::test]
async fn the_tool_request_cannot_be_journalled() {
    let f = fixture().await;
    let r = running(&f, set(&f), None).await;
    f.drop_table("tool_requests").await;
    let (outcome, code) = f
        .engine
        .decide_coded(&r.builtin("Read", json!({})), &CallContext::default())
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(outcome.decision.rule.source, RuleSource::FailClosed);
    assert_eq!(code, Some("RULE_JOURNAL_UNAVAILABLE"));
}

#[tokio::test]
async fn the_approval_service_refuses_or_fails_for_an_ask() {
    let f = fixture().await;
    let r = running(&f, set(&f), None).await;
    f.drop_table("approval_buttons").await;
    f.drop_table("approvals").await;
    let (outcome, code) = f
        .engine
        .decide_coded(
            &r.builtin("Bash", json!({"command": "make"})),
            &CallContext::default(),
        )
        .await;
    assert_eq!(outcome.decision.rule.source, RuleSource::FailClosed);
    assert_eq!(
        reason(&outcome),
        "waiting for the person's approval could not start"
    );
    assert_eq!(code, Some("RULE_APPROVAL_UNAVAILABLE"));
}

#[tokio::test]
async fn a_control_state_statement_fails() {
    let f = fixture().await;
    let r = running(&f, set(&f), None).await;
    f.drop_table("rules_run_facts").await;
    let (outcome, code) = f
        .engine
        .decide_coded(
            &r.builtin(
                "Read",
                json!({"file_path": f.folder("eng-1").join("a").display().to_string()}),
            ),
            &CallContext::default(),
        )
        .await;
    assert_eq!(verdict(&outcome), "deny");
    assert_eq!(rule(&outcome), "app.internal");
    assert_eq!(code, Some("RULE_STATE_UNAVAILABLE"));
    let error = f
        .engine
        .begin_run(start(RunId::new(), r.agent, r.workspace))
        .await
        .expect_err("the start fails");
    assert_eq!(error.code(), "RULE_STATE_UNAVAILABLE");
    assert!(error.retryable());
}

#[tokio::test]
async fn a_text_is_not_an_address() {
    let error = RulesEngine::parse_address("https://user@example.org/").expect_err("refused");
    assert_eq!(error.code(), "RULE_NOT_AN_ADDRESS");
    assert!(!error.retryable());
    assert_eq!(
        error.to_string(),
        "not an address: the address carries user information"
    );
}

#[tokio::test]
async fn a_rule_kept_as_a_setting_is_not_allowed() {
    let f = fixture().await;
    let mut s = set(&f);
    s.commands = vec!["answer".to_owned()];
    let agent = s.agent_instance_id;
    f.engine.load_rule_set(s).unwrap();
    let allowed = vec!["reply".to_owned(), "command".to_owned()];
    let good = MessageRule {
        name: "r-1917b157949f".to_owned(),
        priority: 0,
        command: None,
        conditions: vec![Condition::Keywords {
            words: vec!["rates".to_owned()],
            r#match: MatchMode::Any,
        }],
        decides: Some(DecidingOutcome::Reply {
            text: "See the rates page.".to_owned(),
        }),
        adds: Vec::new(),
    };
    assert!(f
        .engine
        .check_setting_rule(agent, "kept-answers", &allowed, &good)
        .is_ok());
    let mut bad = good.clone();
    bad.decides = Some(DecidingOutcome::End {
        ending: "x".to_owned(),
    });
    let error = f
        .engine
        .check_setting_rule(agent, "kept-answers", &allowed, &bad)
        .expect_err("refused");
    assert_eq!(error.code(), "RULE_SETTING_RULE_NOT_ALLOWED");
    assert_eq!(
        error.to_string(),
        "setting rule not allowed: kept-answers/r-1917b157949f"
    );
    let refusals = [
        MessageRule {
            name: "Bad Name".to_owned(),
            ..good.clone()
        },
        MessageRule {
            conditions: vec![Condition::RunInProgress],
            ..good.clone()
        },
        MessageRule {
            priority: 2_000,
            ..good.clone()
        },
        MessageRule {
            command: Some("missing".to_owned()),
            ..good.clone()
        },
        MessageRule {
            decides: Some(DecidingOutcome::Command {
                command: "missing".to_owned(),
            }),
            ..good.clone()
        },
        MessageRule {
            decides: Some(DecidingOutcome::SingleCall {
                command: "answer".to_owned(),
            }),
            ..good.clone()
        },
        MessageRule {
            decides: Some(DecidingOutcome::Reply {
                text: String::new(),
            }),
            ..good.clone()
        },
        MessageRule {
            conditions: vec![Condition::Pattern {
                patterns: vec!["(".to_owned()],
                r#match: MatchMode::Any,
            }],
            ..good.clone()
        },
        MessageRule {
            conditions: vec![Condition::Keywords {
                words: Vec::new(),
                r#match: MatchMode::Any,
            }],
            ..good.clone()
        },
        MessageRule {
            conditions: vec![Condition::KeywordShare {
                words: vec!["a".to_owned()],
                share: crate::vocab::DecimalShare("0.2".to_owned()),
            }],
            ..good.clone()
        },
        MessageRule {
            conditions: vec![Condition::Setting {
                name: "s".to_owned(),
                equals: json!([1]),
            }],
            ..good.clone()
        },
        MessageRule {
            decides: None,
            ..good.clone()
        },
        MessageRule {
            adds: vec![crate::vocab::AdditiveOutcome::AttachAction {
                action: "a".to_owned(),
            }],
            ..good.clone()
        },
    ];
    for rule in refusals {
        assert!(
            f.engine
                .check_setting_rule(agent, "kept-answers", &allowed, &rule)
                .is_err(),
            "{rule:?}"
        );
    }
    assert_eq!(
        f.engine
            .check_setting_rule(AgentInstanceId::new(), "s", &allowed, &good)
            .unwrap_err()
            .code(),
        "RULE_RULESET_NOT_LOADED"
    );
}

#[tokio::test]
async fn resume_run_for_an_unknown_run() {
    let f = fixture().await;
    let error = f
        .engine
        .resume_run(RunId::new())
        .await
        .expect_err("unknown");
    assert_eq!(error.code(), "RULE_RUN_UNKNOWN");
    assert!(!error.retryable());
}

#[tokio::test]
async fn a_command_from_a_person_that_is_not_a_declared_check() {
    let f = fixture().await;
    let (outcome, code) = f
        .engine
        .decide_command_coded(CommandRequest {
            workspace_id: WorkspaceId::new(),
            origin: CommandOrigin::Person,
            command: "make deploy".to_owned(),
            working_dir: PathBuf::from(abs!("/Users/asha/code")),
            declared_check: false,
        })
        .await;
    assert_eq!(rule(&outcome), "app.shell.person_undeclared");
    assert_eq!(
        reason(&outcome),
        "Only the project's declared checks run from here."
    );
    assert_eq!(code, Some("RULE_COMMAND_NOT_DECLARED"));
}

#[tokio::test]
async fn a_command_request_with_empty_or_over_long_text_or_a_relative_working_folder() {
    let f = fixture().await;
    let r = running(&f, set(&f), None).await;
    for (command, dir) in [
        (String::new(), abs!("/Users/asha/code")),
        ("x".repeat(8_001), abs!("/Users/asha/code")),
        ("make".to_owned(), "relative/dir"),
    ] {
        for origin in [
            CommandOrigin::Person,
            CommandOrigin::Run {
                run_id: r.run,
                agent_instance_id: r.agent,
            },
        ] {
            let (outcome, code) = f
                .engine
                .decide_command_coded(CommandRequest {
                    workspace_id: r.workspace,
                    origin,
                    command: command.clone(),
                    working_dir: PathBuf::from(dir),
                    declared_check: true,
                })
                .await;
            assert_eq!(rule(&outcome), "app.input.command");
            assert_eq!(outcome.decision.rule.source, RuleSource::FailClosed);
            assert_eq!(code, Some("RULE_COMMAND_INVALID"));
        }
    }
}

#[tokio::test]
async fn the_approval_service_fails_while_a_caller_waits() {
    let f = fixture().await;
    let error = f
        .engine
        .wait_for_approval(ApprovalId::new(), None)
        .await
        .expect_err("unavailable");
    assert!(matches!(error, RulesError::ApprovalUnavailable));
    assert_eq!(error.code(), "RULE_APPROVAL_UNAVAILABLE");
    assert!(error.retryable());
}

#[tokio::test]
async fn a_path_checked_outside_a_call_is_protected_or_outside_the_grants() {
    let f = fixture().await;
    let s = set(&f);
    let agent = s.agent_instance_id;
    f.engine.load_rule_set(s).unwrap();
    let mut req = PathAccessRequest {
        workspace_id: Some(WorkspaceId::new()),
        agent: Some(agent),
        crew_charter_id: None,
        path: PathBuf::from(abs!("/Users/asha/Documents/elsewhere.txt")),
        access: FolderAccess::Read,
    };
    let decision = f.engine.check_path_access(&req);
    assert_eq!(decision.rule.rule_id, "app.folder_grant");
    assert!(matches!(
        decision.verdict,
        Verdict::Deny { ref reason } if reason == "the path is outside the places this agent may use"
    ));
    req.path = PathBuf::from("relative.txt");
    assert!(matches!(
        f.engine.check_path_access(&req).verdict,
        Verdict::Deny { .. }
    ));
    req.path = f.folder("eng-1").join("out.pdf");
    req.access = FolderAccess::ReadAndChange;
    assert!(matches!(
        f.engine.check_path_access(&req).verdict,
        Verdict::Allow
    ));
    req.agent = Some(AgentInstanceId::new());
    let decision = f.engine.check_path_access(&req);
    assert_eq!(decision.rule.source, RuleSource::FailClosed);
}

#[tokio::test]
async fn a_lock_or_retained_content_statement_fails() {
    let f = fixture().await;
    f.drop_table("rules_control_locks").await;
    f.drop_table("rules_control_starts").await;
    let error = f.engine.lock_held("duty:x").await.expect_err("state");
    assert_eq!(error.code(), "RULE_STATE_UNAVAILABLE");
    assert!(error.retryable());
    assert!(f.engine.forget_asker("slack", "U1").await.is_err());
    assert!(f.engine.count_for_asker("slack", "U1").await.is_err());
    assert!(f.engine.count_scope("slack:C1").await.is_err());
    assert!(f.engine.forget_scope("slack:C1").await.is_err());
    assert!(f
        .engine
        .prune_before(ordin_types::time::Timestamp::now())
        .await
        .is_err());
}

#[tokio::test]
async fn the_checker_times_out_refuses_or_answers_badly_or_no_checker_is_attached() {
    let f = fixture_with(crate::types::ProgramKind::AgentSingle).await;
    let mut s = set(&f);
    s.configured = Some(ConfiguredControls {
        content_checks: vec![ContentCheck {
            name: "screen".to_owned(),
            connection: "moderation".to_owned(),
            operation: "check".to_owned(),
            points: vec![CheckPoint::Input],
            on_block: OnBlock::RetryWithoutContext,
            on_unreachable: OnUnreachable::Block,
            categories: Vec::new(),
            timeout_seconds: 1,
            max_chars: 100,
        }],
        ..ConfiguredControls::default()
    });
    let agent = s.agent_instance_id;
    f.engine.load_rule_set(s).unwrap();
    let outcome = f
        .engine
        .check_content(agent, RunId::new(), CheckPoint::Input, "a question")
        .await;
    assert_eq!(
        outcome,
        ContentCheckOutcome::Blocked {
            control: "screen".to_owned(),
            category: None,
            on_block: OnBlock::End,
            unreachable: true,
        }
    );
    // No set loaded: blocked, fail closed.
    let outcome = f
        .engine
        .check_content(AgentInstanceId::new(), RunId::new(), CheckPoint::Input, "q")
        .await;
    assert!(
        matches!(outcome, ContentCheckOutcome::Blocked { ref control, .. } if control == "app.ruleset")
    );
}

#[test]
fn x4_every_code_is_registered_to_this_crate() {
    assert!(!ALL_CODES.is_empty());
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-rules", "{code}");
        let prefix = code.split('_').next().unwrap();
        assert!(owns_prefix("ordin-rules", prefix), "{prefix}");
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted, ALL_CODES,
        "ALL_CODES is sorted and has no duplicates"
    );
    let raised = [
        RulesError::load("RULE_UNBOUND_TOOL", String::new(), ""),
        RulesError::FactUnavailable {
            control: String::new(),
        },
        RulesError::State(sqlx::Error::PoolClosed),
        RulesError::RuleSetNotLoaded,
        RulesError::RunUnknown,
        RulesError::NotAnAddress("x"),
        RulesError::SettingRuleNotAllowed(String::new()),
        RulesError::ApprovalUnavailable,
    ];
    for error in raised {
        assert!(ALL_CODES.contains(&error.code()), "{}", error.code());
        assert_eq!(CodedError::code(&error), error.code());
        assert_eq!(CodedError::retryable(&error), error.retryable());
        assert_eq!(CodedError::repair(&error).is_some(), error.retryable());
    }
}
