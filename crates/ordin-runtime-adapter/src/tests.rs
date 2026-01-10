//! X2: one test per Gherkin scenario of RuntimeAdapterInterface.md, each named after its scenario;
//! the exact title is the comment above each test. Unit tests, because several scenarios use the
//! `ScriptedAdapter`, which outside this crate's own tests needs feature `testkit`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use futures::executor::block_on;
use futures::stream::{self, StreamExt};
use ordin_types::time::Timestamp;
use ordin_types::WorkspaceId;
use ordin_types::{AgentInstanceId, ApprovalId, CodedError, CrewRunId, RemoteAgentId, RunId};

use crate::*;

fn run_id() -> RunId {
    RunId::parse("0192a1c2-3344-7556-8677-8899aabbccdd").unwrap()
}

fn ctx() -> AdapterContext {
    AdapterContext {
        correlation_id: "0192a1c2-3344-7556-8677-8899aabbccdd:1".to_string(),
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(10),
        cancel: tokio_util::sync::CancellationToken::new(),
    }
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

/// The *Public Interface* example's start request.
fn start_run(target: RunTarget, arguments: &str) -> StartRun {
    StartRun {
        run_id: run_id(),
        workspace_id: WorkspaceId::parse("0192a0f1-1111-7222-8333-444455556666").unwrap(),
        target,
        input: StartInput::Command {
            name: "compare-quotes".into(),
            arguments: arguments.into(),
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

fn started() -> RunEvent {
    RunEvent::Started {
        runtime_session_id: Some("session-1".into()),
        remote_thread_id: None,
    }
}

fn progress(line: &str, done: u64, total: Option<u64>) -> RunEvent {
    RunEvent::Progress {
        progress: RunProgress {
            line: line.into(),
            count: Some(ProgressCount { done, total }),
        },
    }
}

fn ended(reason: RunEndReason) -> RunEvent {
    RunEvent::Ended {
        end: RunEnd {
            reason,
            final_message: None,
            artefacts: vec![],
        },
    }
}

fn scripted(kind: AdapterKind, unsupported: &'static [AdapterVerb]) -> ScriptedAdapter {
    ScriptedAdapter {
        kind,
        unsupported,
        script: vec![],
        snapshot: None,
        start_error: None,
    }
}

const SLACK_UNSUPPORTED: &[AdapterVerb] = &[
    AdapterVerb::Pause,
    AdapterVerb::Resume,
    AdapterVerb::Cancel,
    AdapterVerb::Progress,
];

/// Guards `events` as an adapter of `kind` and collects every item.
fn guard(kind: AdapterKind, events: Vec<RunEvent>) -> Vec<Guarded> {
    block_on(guard_events(kind, run_id(), stream::iter(events).boxed()).collect())
}

fn violation_codes(items: &[Guarded]) -> Vec<&'static str> {
    items
        .iter()
        .filter_map(|item| match item {
            Guarded::Violation(e) => Some(e.code),
            Guarded::Event(_) => None,
        })
        .collect()
}

fn kinds(items: &[Guarded]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            Guarded::Violation(e) => format!("violation:{}", e.code),
            Guarded::Event(event) => serde_json::to_value(event).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string(),
        })
        .collect()
}

/// An adapter that counts its `cancel` calls, for "the adapter's cancel method is not called".
struct CountingAdapter {
    kind: AdapterKind,
    unsupported: &'static [AdapterVerb],
    cancels: AtomicUsize,
}

#[async_trait::async_trait]
impl RuntimeAdapter for CountingAdapter {
    fn kind(&self) -> AdapterKind {
        self.kind
    }
    fn unsupported_verbs(&self) -> &'static [AdapterVerb] {
        self.unsupported
    }
    async fn start(&self, _: &AdapterContext, _: StartRun) -> Result<RunHandle, AdapterError> {
        Err(AdapterError::Unreachable)
    }
    fn events(
        &self,
        _: &AdapterContext,
        _: &RunHandle,
    ) -> Result<futures::stream::BoxStream<'static, RunEvent>, AdapterError> {
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
        self.cancels.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

// Scenario: Happy path — a guarded stream passes through unchanged
#[test]
fn happy_path_a_guarded_stream_passes_through_unchanged() {
    let adapter = ScriptedAdapter {
        script: vec![
            started(),
            progress("Read supplier quotes", 12, Some(40)),
            ended(RunEndReason::Completed),
        ],
        ..scripted(AdapterKind::ClaudeLocal, &[])
    };
    let handle = block_on(adapter.start(&ctx(), start_run(local_target(), ""))).unwrap();
    let inner = adapter.events(&ctx(), &handle).unwrap();
    let items: Vec<Guarded> =
        block_on(guard_events(AdapterKind::ClaudeLocal, handle.run_id, inner).collect());
    assert_eq!(kinds(&items), ["started", "progress", "ended"]);
    assert!(violation_codes(&items).is_empty());
    let Guarded::Event(RunEvent::Progress { progress }) = &items[1] else {
        panic!("the second item is the progress event");
    };
    assert_eq!(progress.line, "Read supplier quotes");
    assert_eq!(
        progress.count.map(|c| (c.done, c.total)),
        Some((12, Some(40)))
    );
}

// Scenario: Error path — an event before started
#[test]
fn error_path_an_event_before_started() {
    let items = guard(
        AdapterKind::ClaudeLocal,
        vec![
            progress("Read supplier quotes", 1, None),
            started(),
            ended(RunEndReason::Completed),
        ],
    );
    assert_eq!(
        kinds(&items),
        ["violation:RA_EVENT_ORDER", "started", "ended"]
    );
    let Guarded::Violation(first) = &items[0] else {
        panic!("a violation comes first");
    };
    assert_eq!(first.field, "events");
}

// Scenario: Error path — a stream that ends without ended
#[test]
fn error_path_a_stream_that_ends_without_ended() {
    let items = guard(AdapterKind::ClaudeLocal, vec![started()]);
    assert_eq!(
        kinds(&items),
        ["started", "violation:RA_EVENT_ORDER", "ended"]
    );
    let Guarded::Event(RunEvent::Ended { end }) = &items[2] else {
        panic!("a synthetic end follows");
    };
    assert_eq!(end.reason, RunEndReason::Interrupted);
    assert!(end.final_message.is_none());
    assert!(end.artefacts.is_empty());
}

// Scenario: Edge case — a harness plan of two steps is dropped, the run continues
#[test]
fn edge_case_a_harness_plan_of_two_steps_is_dropped_the_run_continues() {
    let items = guard(
        AdapterKind::ClaudeLocal,
        vec![
            started(),
            RunEvent::HarnessPlan {
                steps: vec!["Read".into(), "Write".into()],
            },
            progress("Read supplier quotes", 1, None),
            ended(RunEndReason::Completed),
        ],
    );
    assert_eq!(
        kinds(&items),
        ["started", "violation:RA_PLAN_INVALID", "progress", "ended"]
    );
}

// Scenario: Error path — no_reply from a local adapter
#[test]
fn error_path_no_reply_from_a_local_adapter() {
    let items = guard(
        AdapterKind::CopilotLocal,
        vec![started(), ended(RunEndReason::NoReply)],
    );
    assert_eq!(
        kinds(&items),
        ["started", "violation:RA_EVENT_INVALID", "ended"]
    );
    let Guarded::Event(RunEvent::Ended { end }) = &items[2] else {
        panic!("the corrected end follows");
    };
    assert_eq!(end.reason, RunEndReason::Failed);
}

// Scenario: Error path — a remote agent given to a local adapter
#[test]
fn error_path_a_remote_agent_given_to_a_local_adapter() {
    let req = start_run(remote_target(), "");
    let err = check_start(AdapterKind::ClaudeLocal, &req).unwrap_err();
    assert_eq!(err.code, "RA_TARGET_MISMATCH");
}

// Scenario: Edge case — the longest allowed arguments
#[test]
fn edge_case_the_longest_allowed_arguments() {
    let req = start_run(local_target(), &"a".repeat(16_384));
    assert_eq!(check_start(AdapterKind::ClaudeLocal, &req), Ok(()));
    let req = start_run(local_target(), &"a".repeat(16_385));
    assert_eq!(
        check_start(AdapterKind::ClaudeLocal, &req)
            .unwrap_err()
            .code,
        "RA_INPUT_TOO_LARGE"
    );
}

// Scenario: Error path — cancel on the Slack route
#[test]
fn error_path_cancel_on_the_slack_route() {
    let adapter = CountingAdapter {
        kind: AdapterKind::SlackThread,
        unsupported: SLACK_UNSUPPORTED,
        cancels: AtomicUsize::new(0),
    };
    let handle = RunHandle {
        run_id: run_id(),
        adapter: AdapterKind::SlackThread,
        runtime_session_id: None,
        remote_thread_id: Some("1712345678.000100".into()),
    };
    // The Session Supervisor's order (Internal Logic 12): check, then call only on `Ok`.
    let outcome = check_verb(&adapter, AdapterVerb::Cancel)
        .and_then(|()| block_on(adapter.cancel(&ctx(), &handle)));
    let err = outcome.unwrap_err();
    assert!(matches!(
        err,
        AdapterError::VerbNotOffered {
            verb: AdapterVerb::Cancel
        }
    ));
    assert_eq!(err.code(), "RA_VERB_NOT_OFFERED");
    assert_eq!(adapter.cancels.load(Ordering::SeqCst), 0);
}

// Scenario: Error path — approving a different card
#[test]
fn error_path_approving_a_different_card() {
    let paused_on = PauseReason::Approval {
        approval_id: ApprovalId::parse("0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f").unwrap(),
    };
    let decision = ResumeDecision::Approve {
        approval_id: ApprovalId::parse("0192a1c3-0000-7000-8000-000000000001").unwrap(),
    };
    assert_eq!(
        check_resume(&decision, &paused_on).unwrap_err().code,
        "RA_RESUME_MISMATCH"
    );
}

// Scenario: Error path — an inconsistent snapshot
#[test]
fn error_path_an_inconsistent_snapshot() {
    let snapshot = RunSnapshot {
        run_id: run_id(),
        state: RunState::Running,
        end: Some(RunEnd {
            reason: RunEndReason::Completed,
            final_message: None,
            artefacts: vec![],
        }),
        checked_at: Timestamp::now(),
    };
    assert_eq!(
        check_snapshot(&snapshot).unwrap_err().code,
        "RA_SNAPSHOT_INCONSISTENT"
    );
}

// Scenario: Error path — two adapters of one kind
#[test]
fn error_path_two_adapters_of_one_kind() {
    let mut registry = AdapterRegistry::new();
    registry
        .register(Arc::new(scripted(AdapterKind::ClaudeLocal, &[])))
        .unwrap();
    let err = registry
        .register(Arc::new(scripted(AdapterKind::ClaudeLocal, &[])))
        .unwrap_err();
    assert_eq!(
        err,
        RegistryError::Duplicate {
            kind: AdapterKind::ClaudeLocal
        }
    );
    assert_eq!(err.code(), "RA_ADAPTER_DUPLICATE");
}

// Scenario: Error path — answering a run that waits for the hosted agent's approvers
#[test]
fn error_path_answering_a_run_that_waits_for_the_hosted_agents_approvers() {
    let decision = ResumeDecision::Answer {
        text: "Go ahead".into(),
    };
    assert_eq!(
        check_resume(&decision, &PauseReason::RemoteApprovers)
            .unwrap_err()
            .code,
        "RA_RESUME_MISMATCH"
    );
}

// Scenario: Error path — remote_approvers from a local adapter
#[test]
fn error_path_remote_approvers_from_a_local_adapter() {
    let items = guard(
        AdapterKind::ClaudeLocal,
        vec![
            started(),
            RunEvent::Paused {
                reason: PauseReason::RemoteApprovers,
            },
            ended(RunEndReason::Completed),
        ],
    );
    assert_eq!(
        kinds(&items),
        ["started", "violation:RA_EVENT_INVALID", "ended"]
    );
    let Guarded::Violation(violation) = &items[1] else {
        panic!("the paused event is replaced by a violation");
    };
    assert_eq!(violation.field, "reason");
}

// Scenario: Error path — a model given to a remote adapter
#[test]
fn error_path_a_model_given_to_a_remote_adapter() {
    let model: ModelChoice =
        serde_json::from_str(r#"{"level":"standard","model":"sonnet"}"#).unwrap();
    let req = StartRun {
        model: Some(model),
        ..start_run(remote_target(), "")
    };
    let err = check_start(AdapterKind::HostedAgent, &req).unwrap_err();
    assert_eq!(err.code, "RA_MODEL_UNKNOWN");
    assert_eq!(err.field, "model");
}

// Scenario: Happy path — an adapter's own check becomes InvalidRequest
#[test]
fn happy_path_an_adapters_own_check_becomes_invalidrequest() {
    let check = CheckError {
        code: "RA_HANDLE_INVALID",
        field: "remote_thread_id",
    };
    let err = AdapterError::from(check);
    assert!(matches!(&err, AdapterError::InvalidRequest { code } if code == "RA_HANDLE_INVALID"));
    assert_eq!(err.code(), "RA_HANDLE_INVALID");
}

// Scenario: Happy path — a crew step's grant context reaches the adapter
#[test]
fn happy_path_a_crew_steps_grant_context_reaches_the_adapter() {
    let req = StartRun {
        caller: ToolCaller::CrewMember {
            crew_run_id: CrewRunId::parse("0192a1c5-0000-7000-8000-000000000001").unwrap(),
        },
        ..start_run(local_target(), "")
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains(
        r#""caller":{"kind":"crew_member","crew_run_id":"0192a1c5-0000-7000-8000-000000000001"}"#
    ));
    assert!(!json.contains("\"scope_id\""));
    assert!(!json.contains("\"coding_task_id\""));
    assert_eq!(check_start(AdapterKind::ClaudeLocal, &req), Ok(()));
}

// Scenario: Happy path — verbs not offered for the start-up report
#[test]
fn happy_path_verbs_not_offered_for_the_start_up_report() {
    let mut registry = AdapterRegistry::new();
    // Registered Slack first: the report's order is the kind order, not the registration order.
    registry
        .register(Arc::new(scripted(
            AdapterKind::SlackThread,
            SLACK_UNSUPPORTED,
        )))
        .unwrap();
    registry
        .register(Arc::new(scripted(AdapterKind::ClaudeLocal, &[])))
        .unwrap();
    assert_eq!(
        registry.verbs_not_offered(),
        [
            (AdapterKind::SlackThread, AdapterVerb::Pause),
            (AdapterKind::SlackThread, AdapterVerb::Resume),
            (AdapterKind::SlackThread, AdapterVerb::Cancel),
            (AdapterKind::SlackThread, AdapterVerb::Progress),
        ]
    );
    assert_eq!(
        registry.kinds(),
        [AdapterKind::ClaudeLocal, AdapterKind::SlackThread]
    );
}

// --- The scripted adapter (feature `testkit`) ------------------------------------------------

#[test]
fn scripted_adapter_answers_each_verb_as_the_spec_says() {
    let snapshot = RunSnapshot {
        run_id: run_id(),
        state: RunState::Completed,
        end: Some(RunEnd {
            reason: RunEndReason::Completed,
            final_message: Some("Done.".into()),
            artefacts: vec![],
        }),
        checked_at: Timestamp::now(),
    };
    let adapter = ScriptedAdapter {
        script: vec![
            RunEvent::Started {
                runtime_session_id: None,
                remote_thread_id: Some("1712345678.000100".into()),
            },
            ended(RunEndReason::NoReply),
        ],
        snapshot: Some(snapshot),
        ..scripted(AdapterKind::SlackThread, SLACK_UNSUPPORTED)
    };
    assert_eq!(adapter.kind(), AdapterKind::SlackThread);
    let handle = block_on(adapter.start(&ctx(), start_run(remote_target(), ""))).unwrap();
    assert_eq!(handle.run_id, run_id());
    assert_eq!(
        handle.remote_thread_id.as_deref(),
        Some("1712345678.000100")
    );
    assert_eq!(check_handle(&handle), Ok(()));
    let events: Vec<RunEvent> = block_on(adapter.events(&ctx(), &handle).unwrap().collect());
    assert_eq!(events.len(), 2);
    let read = block_on(adapter.read(&ctx(), &handle)).unwrap();
    assert_eq!(read.state, RunState::Completed);
    assert_eq!(check_snapshot(&read), Ok(()));
    let resume = block_on(adapter.resume(
        &ctx(),
        &handle,
        ResumeDecision::Answer { text: "Yes".into() },
    ));
    assert!(matches!(
        resume,
        Err(AdapterError::VerbNotOffered {
            verb: AdapterVerb::Resume
        })
    ));
    assert!(matches!(
        block_on(adapter.cancel(&ctx(), &handle)),
        Err(AdapterError::VerbNotOffered {
            verb: AdapterVerb::Cancel
        })
    ));
}

#[test]
fn scripted_adapter_returns_its_start_error_and_unknown_run_without_a_snapshot() {
    let adapter = ScriptedAdapter {
        start_error: Some(AdapterError::SignedOut),
        ..scripted(AdapterKind::ClaudeLocal, &[])
    };
    let err = block_on(adapter.start(&ctx(), start_run(local_target(), ""))).unwrap_err();
    assert_eq!(err.code(), "RA_SIGNED_OUT");
    let handle = RunHandle {
        run_id: run_id(),
        adapter: AdapterKind::ClaudeLocal,
        runtime_session_id: None,
        remote_thread_id: None,
    };
    assert!(block_on(adapter.resume(
        &ctx(),
        &handle,
        ResumeDecision::Answer { text: "Yes".into() }
    ))
    .is_ok());
    assert!(block_on(adapter.cancel(&ctx(), &handle)).is_ok());
    assert!(matches!(
        block_on(adapter.read(&ctx(), &handle)),
        Err(AdapterError::UnknownRun)
    ));
    // A script without `started` gives a handle with no identifiers.
    let handle =
        block_on(adapter_without_error().start(&ctx(), start_run(local_target(), ""))).unwrap();
    assert!(handle.runtime_session_id.is_none() && handle.remote_thread_id.is_none());
}

fn adapter_without_error() -> ScriptedAdapter {
    scripted(AdapterKind::ClaudeLocal, &[])
}

#[test]
fn registry_get_returns_the_registered_adapter_and_refuses_an_unknown_kind() {
    let mut registry = AdapterRegistry::new();
    registry
        .register(Arc::new(scripted(AdapterKind::HostedAgent, &[])))
        .unwrap();
    let copy = registry.clone();
    assert_eq!(
        copy.get(AdapterKind::HostedAgent).unwrap().kind(),
        AdapterKind::HostedAgent
    );
    let err = copy.get(AdapterKind::CopilotLocal).err().unwrap();
    assert_eq!(
        err,
        RegistryError::Unknown {
            kind: AdapterKind::CopilotLocal
        }
    );
    assert_eq!(err.code(), "RA_ADAPTER_UNKNOWN");
    assert!(!err.retryable());
}

// --- Guard rules beyond the scenarios (Internal Logic 7) -------------------------------------

#[test]
fn guard_drops_a_second_started_and_events_after_ended() {
    let items = guard(
        AdapterKind::HostedAgent,
        vec![
            started(),
            started(),
            RunEvent::Paused {
                reason: PauseReason::RemoteApprovers,
            },
            RunEvent::Resumed,
            RunEvent::Usage {
                usage: UsageDelta::default(),
            },
            ended(RunEndReason::Abandoned),
            progress("late", 1, None),
        ],
    );
    assert_eq!(
        kinds(&items),
        [
            "started",
            "violation:RA_EVENT_ORDER",
            "paused",
            "resumed",
            "usage",
            "ended",
            "violation:RA_EVENT_ORDER"
        ]
    );
}

#[test]
fn guard_checks_progress_plans_questions_and_routes() {
    let link = url::Url::parse("https://example.slack.com/archives/C1/p1712345678000100").unwrap();
    let valid_plan: Vec<String> = ["Read", "Compare", "Write"].map(String::from).to_vec();
    let items = guard(
        AdapterKind::ClaudeLocal,
        vec![
            started(),
            progress("", 1, None),
            progress(&"x".repeat(161), 1, None),
            progress("Counting", 41, Some(40)),
            progress(&"x".repeat(160), 40, Some(40)),
            RunEvent::HarnessPlan {
                steps: valid_plan.clone(),
            },
            RunEvent::HarnessPlan {
                steps: vec!["a".into(), "".into(), "c".into()],
            },
            RunEvent::HarnessPlan {
                steps: vec!["x".repeat(81), "b".into(), "c".into()],
            },
            RunEvent::HarnessPlan {
                steps: vec!["s".to_string(); 8],
            },
            RunEvent::Paused {
                reason: PauseReason::Person {
                    question: String::new(),
                },
            },
            RunEvent::Paused {
                reason: PauseReason::Person {
                    question: "Which quote?".into(),
                },
            },
            RunEvent::Paused {
                reason: PauseReason::DecisionElsewhere { link: link.clone() },
            },
            RunEvent::Paused {
                reason: PauseReason::SignIn,
            },
            ended(RunEndReason::Completed),
        ],
    );
    assert_eq!(
        kinds(&items),
        [
            "started",
            "violation:RA_EVENT_INVALID",
            "violation:RA_EVENT_INVALID",
            "violation:RA_EVENT_INVALID",
            "progress",
            "harness_plan",
            "violation:RA_PLAN_INVALID",
            "violation:RA_PLAN_INVALID",
            "violation:RA_PLAN_INVALID",
            "violation:RA_EVENT_INVALID",
            "paused",
            "violation:RA_EVENT_INVALID",
            "paused",
            "ended"
        ]
    );
    // The same Slack decision is accepted from the Slack route.
    let slack = guard(
        AdapterKind::SlackThread,
        vec![
            started(),
            RunEvent::Paused {
                reason: PauseReason::DecisionElsewhere { link },
            },
            ended(RunEndReason::NoReply),
        ],
    );
    assert_eq!(kinds(&slack), ["started", "paused", "ended"]);
}

#[test]
fn guard_corrects_abandoned_from_a_local_adapter_and_cuts_a_long_final_message() {
    let items = guard(
        AdapterKind::ClaudeLocal,
        vec![
            started(),
            RunEvent::Ended {
                end: RunEnd {
                    reason: RunEndReason::Abandoned,
                    final_message: Some("é".repeat(32_769)),
                    artefacts: vec![],
                },
            },
        ],
    );
    assert_eq!(
        kinds(&items),
        [
            "started",
            "violation:RA_EVENT_INVALID",
            "violation:RA_EVENT_INVALID",
            "ended"
        ]
    );
    let Guarded::Event(RunEvent::Ended { end }) = &items[3] else {
        panic!("the corrected end comes last");
    };
    assert_eq!(end.reason, RunEndReason::Failed);
    assert_eq!(
        end.final_message.as_deref().map(|m| m.chars().count()),
        Some(32_768)
    );
    // A message at the limit passes unchanged, and Abandoned is the hosted agent's to report.
    let items = guard(
        AdapterKind::HostedAgent,
        vec![
            started(),
            RunEvent::Ended {
                end: RunEnd {
                    reason: RunEndReason::Abandoned,
                    final_message: Some("é".repeat(32_768)),
                    artefacts: vec![],
                },
            },
        ],
    );
    assert_eq!(kinds(&items), ["started", "ended"]);
}

#[test]
fn guard_of_an_empty_stream_yields_a_violation_and_a_synthetic_end() {
    let items = guard(AdapterKind::SlackThread, vec![]);
    assert_eq!(kinds(&items), ["violation:RA_EVENT_ORDER", "ended"]);
}

#[test]
fn guard_waits_while_the_inner_stream_is_pending() {
    use futures::task::noop_waker_ref;
    use std::task::{Context, Poll};

    let mut guarded = guard_events(
        AdapterKind::ClaudeLocal,
        run_id(),
        stream::pending::<RunEvent>().boxed(),
    );
    let mut cx = Context::from_waker(noop_waker_ref());
    assert!(matches!(guarded.poll_next_unpin(&mut cx), Poll::Pending));
}
