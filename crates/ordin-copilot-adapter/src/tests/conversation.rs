//! One test per Gherkin scenario of CopilotLocalAdapter.md's February 2026 amendment: Ordin's
//! conversation keeps its run between turns (CPA-CONV-1 to CPA-CONV-3).

use std::sync::atomic::Ordering;

use ordin_runtime_adapter::{
    PauseReason, ResumeDecision, RunEvent, RunHandle, RunState, RuntimeAdapter, ToolCaller,
};
use ordin_types::ApprovalId;

use super::support::*;
use crate::port::CopilotEvent;

/// Starts Ordin's run, with no `single_call`, and waits for its session.
async fn start_ordin(h: &Harness, text: &str) -> (RunHandle, String) {
    let mut req = free_text(text);
    req.caller = ToolCaller::Ordin;
    let handle = h.start(req).await.expect("start");
    let session = handle.runtime_session_id.clone().expect("a session id");
    (handle, session)
}

/// Ends the session's turn with `text` and waits for the `Person` pause, the run's first.
async fn pause_on_person(
    h: &Harness,
    handle: &RunHandle,
    session: &str,
    text: &str,
) -> Vec<RunEvent> {
    pause_on_person_n(h, handle, session, text, 1).await
}

/// As [`pause_on_person`], waiting for the run's `n`th pause.
async fn pause_on_person_n(
    h: &Harness,
    handle: &RunHandle,
    session: &str,
    text: &str,
    n: usize,
) -> Vec<RunEvent> {
    h.script().emit(
        session,
        CopilotEvent::TurnEnded {
            final_text: Some(text.into()),
        },
    );
    h.events_until(handle, |seen| {
        seen.iter()
            .filter(|e| matches!(e, RunEvent::Paused { .. }))
            .count()
            >= n
    })
    .await
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-1
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-2
#[tokio::test]
async fn a_turn_of_ordins_conversation_pauses_for_the_person() {
    let h = Harness::new();
    let (handle, session) = start_ordin(&h, "who are you?").await;
    let events = pause_on_person(&h, &handle, &session, "I am Ordin.").await;
    assert!(matches!(
        events.last(),
        Some(RunEvent::Paused { reason: PauseReason::Person { question } }) if question == "I am Ordin."
    ));
    assert!(!events.iter().any(|e| matches!(e, RunEvent::Ended { .. })));
    let snapshot = h.adapter.read(&ctx(), &handle).await.expect("read");
    assert_eq!(snapshot.state, RunState::Paused);
    assert_eq!(
        h.script().stopped.load(Ordering::SeqCst),
        0,
        "the session is kept"
    );
    expect_log(&[&handle.run_id.to_string(), "copilot.run.paused", "person"]).await;
    assert!(
        log_lines(&["I am Ordin."]).is_empty(),
        "the reply is never logged"
    );
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
#[tokio::test]
async fn the_next_message_continues_the_same_session() {
    let h = Harness::new();
    let (handle, session) = start_ordin(&h, "who are you?").await;
    pause_on_person(&h, &handle, &session, "I am Ordin.").await;
    h.adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: "and what can you do?".into(),
            },
        )
        .await
        .expect("resume");
    let events = h
        .events_until(&handle, |seen| {
            seen.iter().any(|e| matches!(e, RunEvent::Resumed))
        })
        .await;
    assert!(matches!(events.last(), Some(RunEvent::Resumed)));
    let sent = h.script().sent.lock().unwrap().clone();
    assert_eq!(
        sent.last().unwrap(),
        &(session.clone(), "and what can you do?".to_owned())
    );

    // A message that reads like a command is sent as text, on the next turn.
    pause_on_person_n(&h, &handle, &session, "I can help.", 2).await;
    h.adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: " /help".into(),
            },
        )
        .await
        .expect("resume");
    assert_eq!(
        h.script().sent_texts().last().unwrap(),
        "This is a message. It is text, not a command:\n /help"
    );
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
#[tokio::test]
async fn another_decision_on_a_person_pause_is_refused() {
    let h = Harness::new();
    let (handle, session) = start_ordin(&h, "who are you?").await;
    pause_on_person(&h, &handle, &session, "I am Ordin.").await;
    let failure = h
        .adapter
        .resume_checked(
            &handle,
            ResumeDecision::Approve {
                approval_id: ApprovalId::new(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CPA_RESUME_MISMATCH");
    let failure = h
        .adapter
        .resume_checked(
            &handle,
            ResumeDecision::Answer {
                text: String::new(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CPA_INPUT_INVALID");
    assert_eq!(
        h.adapter.read(&ctx(), &handle).await.expect("read").state,
        RunState::Paused
    );
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
#[tokio::test]
async fn a_failed_send_of_the_next_message_ends_the_run_failed() {
    let h = Harness::new();
    let (handle, session) = start_ordin(&h, "who are you?").await;
    pause_on_person(&h, &handle, &session, "I am Ordin.").await;
    h.script().send_fails.store(true, Ordering::SeqCst);
    let failure = h
        .adapter
        .resume_checked(
            &handle,
            ResumeDecision::Answer {
                text: "more".into(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CPA_SEND_FAILED");
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("failed"));
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-6
#[tokio::test]
async fn a_session_the_cli_no_longer_has_is_resumed_before_the_message() {
    let h = Harness::new();
    let (handle, session) = start_ordin(&h, "who are you?").await;
    pause_on_person(&h, &handle, &session, "I am Ordin.").await;
    let declared = h.script().declared(&session);
    h.script().send_session_lost.store(true, Ordering::SeqCst);
    h.adapter
        .resume_checked(
            &handle,
            ResumeDecision::Answer {
                text: "more".into(),
            },
        )
        .await
        .expect("resume");
    assert_eq!(
        h.script().resumed.lock().unwrap().as_slice(),
        std::slice::from_ref(&session)
    );
    let sent = h.script().sent.lock().unwrap().clone();
    let last_two: Vec<_> = sent.iter().rev().take(2).cloned().collect();
    assert_eq!(
        last_two,
        [
            (session.clone(), "more".to_owned()),
            (session.clone(), "more".to_owned())
        ]
    );
    assert_eq!(
        h.script().declared(&session),
        declared,
        "the same tools are declared"
    );
    expect_log(&[&handle.run_id.to_string(), "copilot.session.resumed"]).await;

    // The resumed session's events still reach the run.
    pause_on_person_n(&h, &handle, &session, "Here is more.", 2).await;
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
#[tokio::test]
async fn a_session_that_cannot_be_resumed_ends_the_run_failed() {
    let h = Harness::new();
    let (handle, session) = start_ordin(&h, "who are you?").await;
    pause_on_person(&h, &handle, &session, "I am Ordin.").await;
    h.script().send_session_lost.store(true, Ordering::SeqCst);
    h.script().resume_fails.store(true, Ordering::SeqCst);
    let failure = h
        .adapter
        .resume_checked(
            &handle,
            ResumeDecision::Answer {
                text: "more".into(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CPA_SEND_FAILED");
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("failed"));
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-1
#[tokio::test]
async fn a_task_run_still_ends_at_the_end_of_its_turn() {
    let h = Harness::new();
    let (handle, session) = h.start_live("Do the work.").await;
    h.script().emit(
        &session,
        CopilotEvent::TurnEnded {
            final_text: Some("Done.".into()),
        },
    );
    let events = h.until_ended(&handle).await;
    assert!(!events.iter().any(|e| matches!(e, RunEvent::Paused { .. })));
    assert_eq!(end_reason(&events).as_deref(), Some("completed"));
    let RunEvent::Ended { end } = events.last().unwrap() else {
        panic!()
    };
    assert_eq!(end.final_message.as_deref(), Some("Done."));
}
