//! X1: one test per row of RuntimeAdapterInterface.md's *Error Table*, in table order. Each provokes
//! the condition where this crate raises it and asserts the code, the retryability and the repair
//! action; the HTTP status is the registry's (ErrorHandling.md: 200 on the Service API, none for
//! the internal codes), asserted for each row.

use std::sync::Arc;

use futures::executor::block_on;
use futures::stream::{self, BoxStream, StreamExt};
use ordin_runtime_adapter::*;
use ordin_types::error_registry::lookup;
use ordin_types::time::Timestamp;
use ordin_types::{
    AgentInstanceId, ApprovalId, ArtefactId, CodedError, RemoteAgentId, RepairAction, RunId,
    WorkspaceId,
};

/// Asserts the code, the retryability, the repair action and the registry row of `err`.
fn assert_coded(
    err: &dyn CodedError,
    code: &str,
    retryable: bool,
    repair: Option<RepairAction>,
    http_status: Option<u16>,
) {
    assert_eq!(err.code(), code);
    assert_eq!(err.retryable(), retryable, "{code} retryability");
    assert_eq!(err.repair(), repair, "{code} repair");
    let entry = lookup(code).unwrap_or_else(|| panic!("{code} is registered"));
    assert_eq!(entry.retryable, retryable, "{code} registry retryability");
    assert_eq!(entry.http_status, http_status, "{code} status");
}

fn assert_check(err: CheckError, code: &str, field: &str) {
    assert_eq!((err.code, err.field), (code, field));
    assert_coded(&err, code, false, None, Some(200));
}

fn run_id() -> RunId {
    RunId::parse("0192a1c2-3344-7556-8677-8899aabbccdd").unwrap()
}

fn local_target() -> RunTarget {
    RunTarget::AgentInstance {
        agent_instance_id: AgentInstanceId::parse("0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff").unwrap(),
    }
}

fn remote_target() -> RunTarget {
    RunTarget::RemoteAgent {
        remote_agent_id: RemoteAgentId::parse("0192a0f9-bbbb-7bbb-8ccc-ddddeeeeffff").unwrap(),
    }
}

fn start_run(target: RunTarget) -> StartRun {
    StartRun {
        run_id: run_id(),
        workspace_id: WorkspaceId::parse("0192a0f1-1111-7222-8333-444455556666").unwrap(),
        target,
        input: StartInput::Command {
            name: "compare-quotes".into(),
            arguments: "Kestrel, Acme and Northwind Q3 quotes".into(),
        },
        attachments: vec![],
        limits: RunLimits {
            wall_clock_secs: 3600,
            request_budget: None,
            idle_timeout_secs: 600,
        },
        thread: None,
        model: None,
        thinking: None,
        caller: ToolCaller::Agent,
        scope_id: None,
        coding_task_id: None,
    }
}

fn guard(kind: AdapterKind, events: Vec<RunEvent>) -> Vec<CheckError> {
    let items: Vec<Guarded> =
        block_on(guard_events(kind, run_id(), stream::iter(events).boxed()).collect());
    items
        .into_iter()
        .filter_map(|item| match item {
            Guarded::Violation(e) => Some(e),
            Guarded::Event(_) => None,
        })
        .collect()
}

fn started() -> RunEvent {
    RunEvent::Started {
        runtime_session_id: None,
        remote_thread_id: None,
    }
}

fn completed() -> RunEvent {
    RunEvent::Ended {
        end: RunEnd {
            reason: RunEndReason::Completed,
            final_message: None,
            artefacts: vec![],
        },
    }
}

/// A route that offers every verb but `cancel`; each call fails with `Unreachable`.
struct NoCancel(AdapterKind);

#[async_trait::async_trait]
impl RuntimeAdapter for NoCancel {
    fn kind(&self) -> AdapterKind {
        self.0
    }
    fn unsupported_verbs(&self) -> &'static [AdapterVerb] {
        &[AdapterVerb::Cancel]
    }
    async fn start(&self, _: &AdapterContext, _: StartRun) -> Result<RunHandle, AdapterError> {
        Err(AdapterError::Unreachable)
    }
    fn events(
        &self,
        _: &AdapterContext,
        _: &RunHandle,
    ) -> Result<BoxStream<'static, RunEvent>, AdapterError> {
        Err(AdapterError::Unreachable)
    }
    async fn resume(
        &self,
        _: &AdapterContext,
        _: &RunHandle,
        _: ResumeDecision,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::Unreachable)
    }
    async fn read(&self, _: &AdapterContext, _: &RunHandle) -> Result<RunSnapshot, AdapterError> {
        Err(AdapterError::Unreachable)
    }
    async fn cancel(&self, _: &AdapterContext, _: &RunHandle) -> Result<(), AdapterError> {
        Err(AdapterError::Unreachable)
    }
}

fn approval(raw: &str) -> ApprovalId {
    ApprovalId::parse(raw).unwrap()
}

#[test]
fn verb_in_the_adapters_unsupported_list() {
    let adapter = NoCancel(AdapterKind::SlackThread);
    let err = check_verb(&adapter, AdapterVerb::Cancel).unwrap_err();
    assert!(matches!(
        err,
        AdapterError::VerbNotOffered {
            verb: AdapterVerb::Cancel
        }
    ));
    assert_coded(&err, "RA_VERB_NOT_OFFERED", false, None, Some(200));
    assert!(check_verb(&adapter, AdapterVerb::Resume).is_ok());
}

#[test]
fn harness_signed_out() {
    assert_coded(
        &AdapterError::SignedOut,
        "RA_SIGNED_OUT",
        false,
        Some(RepairAction::SignInAgain),
        Some(200),
    );
}

#[test]
fn harness_version_not_in_the_tested_list() {
    let err = AdapterError::HarnessUntested {
        found: "2.0.1".into(),
    };
    assert_coded(
        &err,
        "RA_HARNESS_UNTESTED",
        false,
        Some(RepairAction::SendToIt),
        Some(200),
    );
}

#[test]
fn route_unreachable() {
    assert_coded(
        &AdapterError::Unreachable,
        "RA_UNREACHABLE",
        true,
        Some(RepairAction::TryAgain),
        Some(200),
    );
}

#[test]
fn plan_allowance_exhausted() {
    let err = AdapterError::AllowanceExhausted {
        service_message: "You have reached your limit.".into(),
    };
    assert_coded(&err, "RA_ALLOWANCE_EXHAUSTED", false, None, Some(200));
    // The harness's own sentence never reaches the error's text (*Security*).
    assert!(!err.to_string().contains("limit."));
}

#[test]
fn remote_thread_busy() {
    assert_coded(
        &AdapterError::ThreadBusy,
        "RA_THREAD_BUSY",
        true,
        None,
        Some(200),
    );
}

#[test]
fn adapter_does_not_know_the_run() {
    assert_coded(
        &AdapterError::UnknownRun,
        "RA_UNKNOWN_RUN",
        false,
        None,
        Some(200),
    );
}

#[test]
fn an_adapters_own_input_check_fails() {
    for code in [
        "RA_TARGET_MISMATCH",
        "RA_COMMAND_UNKNOWN",
        "RA_INPUT_TOO_LARGE",
        "RA_HANDLE_INVALID",
        "RA_RESUME_MISMATCH",
    ] {
        let err = AdapterError::InvalidRequest { code: code.into() };
        assert_coded(&err, code, false, None, Some(200));
    }
    // Through `From<CheckError>`, as an adapter raises it.
    let check = check_start(AdapterKind::HostedAgent, &start_run(local_target())).unwrap_err();
    let err = AdapterError::from(check);
    assert!(matches!(&err, AdapterError::InvalidRequest { code } if code == "RA_TARGET_MISMATCH"));
}

#[test]
fn the_remote_side_refused_the_request() {
    // A Hosted Agent Adapter code (HostedAgentAdapter.md, `decide_approver_card`).
    let err = AdapterError::Refused {
        code: "HAA_CARD_DECIDED".into(),
    };
    assert_eq!(err.code(), "HAA_CARD_DECIDED");
    assert!(!err.retryable());
    assert_eq!(err.repair(), None);
    // SPEC-GAP-1: a code the registry does not hold is answered as the Service API answers it.
    let unregistered = AdapterError::Refused {
        code: "NOT_A_CODE".into(),
    };
    assert_eq!(unregistered.code(), "SVC_INTERNAL");
}

#[test]
fn a_pause_reason_from_the_wrong_route() {
    let link = url::Url::parse("https://example.slack.com/archives/C1/p1").unwrap();
    for (kind, reason) in [
        (AdapterKind::CopilotLocal, PauseReason::RemoteApprovers),
        (
            AdapterKind::HostedAgent,
            PauseReason::DecisionElsewhere { link },
        ),
    ] {
        let violations = guard(
            kind,
            vec![started(), RunEvent::Paused { reason }, completed()],
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].field, "reason");
        assert_coded(&violations[0], "RA_EVENT_INVALID", false, None, None);
    }
}

#[test]
fn call_deadline_passed() {
    assert_coded(
        &AdapterError::Deadline,
        "RA_DEADLINE",
        true,
        Some(RepairAction::TryAgain),
        Some(200),
    );
}

#[test]
fn target_kind_does_not_fit_the_adapter() {
    for (kind, target) in [
        (AdapterKind::CopilotLocal, remote_target()),
        (AdapterKind::SlackThread, local_target()),
    ] {
        let err = check_start(kind, &start_run(target)).unwrap_err();
        assert_check(err, "RA_TARGET_MISMATCH", "target");
    }
}

#[test]
fn command_name_malformed() {
    for name in ["", "Compare", "-compare", "compare quotes", &"a".repeat(65)] {
        let req = StartRun {
            input: StartInput::Command {
                name: name.into(),
                arguments: String::new(),
            },
            ..start_run(local_target())
        };
        let err = check_start(AdapterKind::ClaudeLocal, &req).unwrap_err();
        assert_check(err, "RA_COMMAND_UNKNOWN", "input.name");
    }
    // 64 characters, digits, `_` and `-` pass.
    let req = StartRun {
        input: StartInput::Command {
            name: format!("0a_b-{}", "c".repeat(59)),
            arguments: String::new(),
        },
        ..start_run(local_target())
    };
    assert_eq!(check_start(AdapterKind::ClaudeLocal, &req), Ok(()));
}

#[test]
fn arguments_text_attachment_count_reason_or_answer_too_long() {
    let free = |text: String| StartRun {
        input: StartInput::FreeText { text },
        ..start_run(local_target())
    };
    for (req, field) in [
        (free(String::new()), "input.text"),
        (free("a".repeat(16_385)), "input.text"),
        (
            StartRun {
                attachments: vec![ArtefactId::new(); 51],
                ..start_run(local_target())
            },
            "attachments",
        ),
    ] {
        let err = check_start(AdapterKind::CopilotLocal, &req).unwrap_err();
        assert_check(err, "RA_INPUT_TOO_LARGE", field);
    }
    assert_eq!(
        check_start(AdapterKind::CopilotLocal, &free("a".repeat(16_384))),
        Ok(())
    );
    let fifty = StartRun {
        attachments: vec![ArtefactId::new(); 50],
        ..start_run(local_target())
    };
    assert_eq!(check_start(AdapterKind::ClaudeLocal, &fifty), Ok(()));

    let card = approval("0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f");
    let paused = PauseReason::Approval { approval_id: card };
    let reject = |reason: String| ResumeDecision::Reject {
        approval_id: card,
        reason: Some(reason),
    };
    assert_check(
        check_resume(&reject("r".repeat(501)), &paused).unwrap_err(),
        "RA_INPUT_TOO_LARGE",
        "decision.reason",
    );
    assert_eq!(check_resume(&reject("r".repeat(500)), &paused), Ok(()));

    let person = PauseReason::Person {
        question: "Which quote?".into(),
    };
    for text in [String::new(), "a".repeat(4_001)] {
        let answer = ResumeDecision::Answer { text };
        assert_check(
            check_resume(&answer, &person).unwrap_err(),
            "RA_INPUT_TOO_LARGE",
            "decision.text",
        );
    }
    let answer = ResumeDecision::Answer {
        text: "a".repeat(4_000),
    };
    assert_eq!(check_resume(&answer, &person), Ok(()));
}

#[test]
fn a_control_character_in_the_input_is_refused_with_the_shared_text_code() {
    let req = StartRun {
        input: StartInput::FreeText {
            text: "line\u{0007}bell".into(),
        },
        ..start_run(local_target())
    };
    let err = check_start(AdapterKind::ClaudeLocal, &req).unwrap_err();
    assert_eq!((err.code, err.field), ("TEXT_CONTROL_CHAR", "input.text"));
}

#[test]
fn wall_clock_idle_timeout_or_request_budget_out_of_range() {
    let with = |wall_clock_secs, request_budget, idle_timeout_secs| StartRun {
        limits: RunLimits {
            wall_clock_secs,
            request_budget,
            idle_timeout_secs,
        },
        ..start_run(local_target())
    };
    for req in [
        with(0, None, 600),
        with(28_801, None, 600),
        with(3600, None, 29),
        with(3600, None, 3_601),
        with(3600, Some(0), 600),
    ] {
        let err = check_start(AdapterKind::ClaudeLocal, &req).unwrap_err();
        assert_check(err, "RA_LIMIT_OUT_OF_RANGE", "limits");
    }
    for req in [with(1, Some(1), 30), with(28_800, None, 3_600)] {
        assert_eq!(check_start(AdapterKind::ClaudeLocal, &req), Ok(()));
    }
}

#[test]
fn thread_session_or_remote_identifier_malformed_or_missing() {
    // `thread` on a start request: only for the hosted agent, 1–256 characters.
    let threaded = |target, thread: &str| StartRun {
        thread: Some(thread.into()),
        ..start_run(target)
    };
    for (kind, req) in [
        (AdapterKind::SlackThread, threaded(remote_target(), "t-1")),
        (AdapterKind::HostedAgent, threaded(remote_target(), "")),
        (
            AdapterKind::HostedAgent,
            threaded(remote_target(), &"t".repeat(257)),
        ),
    ] {
        assert_check(
            check_start(kind, &req).unwrap_err(),
            "RA_HANDLE_INVALID",
            "thread",
        );
    }
    assert_eq!(
        check_start(
            AdapterKind::HostedAgent,
            &threaded(remote_target(), &"t".repeat(256))
        ),
        Ok(())
    );

    // A run handle once `started` has been seen.
    let handle =
        |adapter, runtime_session_id: Option<&str>, remote_thread_id: Option<&str>| RunHandle {
            run_id: run_id(),
            adapter,
            runtime_session_id: runtime_session_id.map(String::from),
            remote_thread_id: remote_thread_id.map(String::from),
        };
    let long = "s".repeat(257);
    for (h, field) in [
        (
            handle(AdapterKind::ClaudeLocal, Some(""), None),
            "runtime_session_id",
        ),
        (
            handle(AdapterKind::ClaudeLocal, Some(&long), None),
            "runtime_session_id",
        ),
        (
            handle(AdapterKind::HostedAgent, None, None),
            "remote_thread_id",
        ),
        (
            handle(AdapterKind::SlackThread, None, Some(&long)),
            "remote_thread_id",
        ),
    ] {
        assert_check(check_handle(&h).unwrap_err(), "RA_HANDLE_INVALID", field);
    }
    assert_eq!(
        check_handle(&handle(AdapterKind::CopilotLocal, Some("s-1"), None)),
        Ok(())
    );
    assert_eq!(
        check_handle(&handle(AdapterKind::HostedAgent, None, Some("r-1"))),
        Ok(())
    );
}

#[test]
fn model_name_malformed() {
    let named = |model: &str| StartRun {
        model: Some(ModelChoice {
            level: ModelLevel::Standard,
            model: Some(model.into()),
        }),
        ..start_run(local_target())
    };
    for name in ["", "son net", "sonnet/4", &"m".repeat(129)] {
        let err = check_start(AdapterKind::ClaudeLocal, &named(name)).unwrap_err();
        assert_check(err, "RA_MODEL_INVALID", "model.model");
    }
    for name in [
        "sonnet",
        "claude-sonnet-4.5",
        "gpt-5:mini_x",
        &"m".repeat(128),
    ] {
        assert_eq!(check_start(AdapterKind::CopilotLocal, &named(name)), Ok(()));
    }
    // A level with the harness's automatic selection.
    let automatic = StartRun {
        model: Some(ModelChoice {
            level: ModelLevel::Automatic,
            model: None,
        }),
        thinking: Some(ThinkingLevel::High),
        ..start_run(local_target())
    };
    assert_eq!(check_start(AdapterKind::CopilotLocal, &automatic), Ok(()));
}

#[test]
fn model_given_to_a_remote_adapter() {
    let req = StartRun {
        model: Some(ModelChoice {
            level: ModelLevel::Automatic,
            model: None,
        }),
        ..start_run(remote_target())
    };
    for kind in [AdapterKind::HostedAgent, AdapterKind::SlackThread] {
        assert_check(
            check_start(kind, &req).unwrap_err(),
            "RA_MODEL_UNKNOWN",
            "model",
        );
    }
}

/// A harness that accepts no model name, as an untested harness version would.
struct NoModels;

impl HarnessModels for NoModels {
    fn harness(&self) -> HarnessKind {
        HarnessKind::CopilotCli
    }
    fn default_level_models(&self) -> LevelModels {
        LevelModels::from([(ModelLevel::Standard, "gpt-5".to_string())])
    }
    fn per_session_model(&self) -> bool {
        false
    }
    fn accepts_model(&self, _: &str) -> bool {
        false
    }
}

#[test]
fn model_name_not_accepted_by_the_harness() {
    // Journal only: the Usage Planner falls back to automatic selection and journals the code.
    // This crate provides the contract and the registered code.
    let harness: Arc<dyn HarnessModels> = Arc::new(NoModels);
    assert_eq!(harness.harness(), HarnessKind::CopilotCli);
    assert!(!harness.per_session_model());
    let defaults = harness.default_level_models();
    let name = defaults.get(&ModelLevel::Standard).expect("a default");
    assert!(!harness.accepts_model(name));
    assert!(!defaults.contains_key(&ModelLevel::Automatic));
    let entry = lookup("RA_MODEL_UNKNOWN").expect("registered");
    assert!(!entry.retryable);
}

#[test]
fn resume_decision_does_not_answer_the_pause() {
    let card = approval("0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f");
    let other = approval("0192a1c3-0000-7000-8000-000000000001");
    let link = url::Url::parse("https://example.slack.com/archives/C1/p1").unwrap();
    let on_card = PauseReason::Approval { approval_id: card };
    let person = PauseReason::Person {
        question: "Which quote?".into(),
    };
    let approve = |id| ResumeDecision::Approve { approval_id: id };
    let reject = |id| ResumeDecision::Reject {
        approval_id: id,
        reason: None,
    };
    let answer = ResumeDecision::Answer { text: "Go".into() };
    for (decision, paused) in [
        (approve(other), on_card.clone()),
        (reject(other), on_card.clone()),
        (approve(card), person.clone()),
        (answer.clone(), on_card.clone()),
        (approve(card), PauseReason::SignIn),
        (answer.clone(), PauseReason::SignIn),
        (answer.clone(), PauseReason::DecisionElsewhere { link }),
        (reject(card), PauseReason::RemoteApprovers),
    ] {
        assert_check(
            check_resume(&decision, &paused).unwrap_err(),
            "RA_RESUME_MISMATCH",
            "decision",
        );
    }
    assert_eq!(check_resume(&approve(card), &on_card), Ok(()));
    assert_eq!(check_resume(&reject(card), &on_card), Ok(()));
    assert_eq!(check_resume(&answer, &person), Ok(()));
}

#[test]
fn snapshot_end_and_state_disagree() {
    let end = |reason| RunEnd {
        reason,
        final_message: None,
        artefacts: vec![],
    };
    let snapshot = |state, end| RunSnapshot {
        run_id: run_id(),
        state,
        end,
        checked_at: Timestamp::now(),
    };
    for s in [
        snapshot(RunState::Completed, None),
        snapshot(RunState::Paused, Some(end(RunEndReason::Cancelled))),
        snapshot(RunState::Failed, Some(end(RunEndReason::Completed))),
    ] {
        assert_check(
            check_snapshot(&s).unwrap_err(),
            "RA_SNAPSHOT_INCONSISTENT",
            "end",
        );
    }
    for s in [
        snapshot(RunState::Queued, None),
        snapshot(RunState::NoReply, Some(end(RunEndReason::NoReply))),
    ] {
        assert_eq!(check_snapshot(&s), Ok(()));
    }
}

#[test]
fn every_end_reason_maps_to_the_run_state_of_the_same_name() {
    for (reason, state) in [
        (RunEndReason::Completed, RunState::Completed),
        (RunEndReason::Failed, RunState::Failed),
        (RunEndReason::StoppedAtLimit, RunState::StoppedAtLimit),
        (RunEndReason::Cancelled, RunState::Cancelled),
        (RunEndReason::Interrupted, RunState::Interrupted),
        (RunEndReason::Abandoned, RunState::Abandoned),
        (RunEndReason::NoReply, RunState::NoReply),
    ] {
        assert_eq!(end_state(reason), state);
    }
}

#[test]
fn event_before_started_after_ended_a_second_started_or_no_ended() {
    let progress = || RunEvent::Progress {
        progress: RunProgress {
            line: "Reading".into(),
            count: None,
        },
    };
    for events in [
        vec![progress(), started(), completed()],
        vec![started(), started(), completed()],
        vec![started(), completed(), progress()],
        vec![started(), progress()],
    ] {
        let violations = guard(AdapterKind::ClaudeLocal, events);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].field, "events");
        assert_coded(&violations[0], "RA_EVENT_ORDER", false, None, None);
    }
}

#[test]
fn harness_plan_outside_3_7_steps_or_1_80_characters() {
    for steps in [
        vec!["Read".to_string(), "Write".to_string()],
        vec!["s".to_string(); 8],
        vec!["Read".to_string(), String::new(), "Write".to_string()],
        vec!["x".repeat(81), "b".into(), "c".into()],
    ] {
        let violations = guard(
            AdapterKind::ClaudeLocal,
            vec![started(), RunEvent::HarnessPlan { steps }, completed()],
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].field, "steps");
        assert_coded(&violations[0], "RA_PLAN_INVALID", false, None, None);
    }
}

#[test]
fn progress_line_question_final_message_or_end_reason_invalid() {
    let progress = |line: String, count| RunEvent::Progress {
        progress: RunProgress { line, count },
    };
    let ended = |reason, final_message| RunEvent::Ended {
        end: RunEnd {
            reason,
            final_message,
            artefacts: vec![],
        },
    };
    let cases = [
        (
            vec![progress(String::new(), None), completed()],
            "progress.line",
        ),
        (
            vec![progress("x".repeat(161), None), completed()],
            "progress.line",
        ),
        (
            vec![
                progress(
                    "Counting".into(),
                    Some(ProgressCount {
                        done: 2,
                        total: Some(1),
                    }),
                ),
                completed(),
            ],
            "progress.count",
        ),
        (
            vec![
                RunEvent::Paused {
                    reason: PauseReason::Person {
                        question: "q".repeat(501),
                    },
                },
                completed(),
            ],
            "question",
        ),
        (
            vec![ended(RunEndReason::Completed, Some("m".repeat(32_769)))],
            "final_message",
        ),
        (vec![ended(RunEndReason::NoReply, None)], "reason"),
        (vec![ended(RunEndReason::Abandoned, None)], "reason"),
    ];
    for (mut events, field) in cases {
        events.insert(0, started());
        let violations = guard(AdapterKind::ClaudeLocal, events);
        assert_eq!(violations.len(), 1, "{field}");
        assert_eq!(violations[0].field, field);
        assert_coded(&violations[0], "RA_EVENT_INVALID", false, None, None);
    }
}

#[test]
fn second_adapter_of_one_kind_registered() {
    let mut registry = AdapterRegistry::new();
    registry
        .register(Arc::new(NoCancel(AdapterKind::HostedAgent)))
        .unwrap();
    let err = registry
        .register(Arc::new(NoCancel(AdapterKind::HostedAgent)))
        .unwrap_err();
    assert_eq!(
        err,
        RegistryError::Duplicate {
            kind: AdapterKind::HostedAgent
        }
    );
    assert_coded(&err, "RA_ADAPTER_DUPLICATE", false, None, Some(200));
}

#[test]
fn no_adapter_of_the_kind_in_this_program_unknown_kind_on_read() {
    let registry = AdapterRegistry::new();
    let err = registry.get(AdapterKind::ClaudeLocal).err().unwrap();
    assert_eq!(
        err,
        RegistryError::Unknown {
            kind: AdapterKind::ClaudeLocal
        }
    );
    assert_coded(&err, "RA_ADAPTER_UNKNOWN", false, None, Some(200));
    assert!(registry.kinds().is_empty());
    // An adapter kind outside the closed set does not parse.
    assert!(serde_json::from_str::<AdapterKind>("\"gemini_local\"").is_err());
}

#[test]
fn an_attachment_not_granted_found_by_the_session_supervisor() {
    // Raised by the Session Supervisor with this crate's code; `check_start` does not check grants.
    let entry = lookup("RA_ATTACHMENT_NOT_GRANTED").expect("registered");
    assert_eq!(entry.owner_crate, "ordin-runtime-adapter");
    assert!(!entry.retryable);
    assert_eq!(entry.http_status, Some(200));
    let req = StartRun {
        attachments: vec![ArtefactId::new()],
        ..start_run(local_target())
    };
    assert_eq!(check_start(AdapterKind::ClaudeLocal, &req), Ok(()));
}
