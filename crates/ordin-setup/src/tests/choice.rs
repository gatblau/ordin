//! The person chooses the assistant at first start (SetupAssistant.md SET-CHO-1 to SET-CHO-7):
//! one test per Gherkin scenario of the amendment, over the real guided sequence and the
//! crate's SQLite store, with stand-in probes and a test `HarnessChoice`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_organisation_profile::{HarnessName, ProfileState};
use ordin_types::domain::identity::{HarnessAttachmentState, HarnessKind};
use ordin_types::CodedError;

use super::*;
use crate::store;
use crate::types::{
    CheckKind, CheckOutcome, CheckTrigger, ChoiceError, HarnessChoice, HarnessLocation,
    HarnessSignIn, StepState,
};

#[cfg(unix)]
const COPILOT_AT: &str = "/home/p/.local/bin/copilot";
#[cfg(windows)]
const COPILOT_AT: &str = "C:/Users/p/AppData/Roaming/npm/copilot.cmd";

/// The program's side of the choice: what it holds, what it was given, and whether keeping fails.
struct TestChoice {
    held: Mutex<Option<HarnessKind>>,
    given: Mutex<Vec<HarnessKind>>,
    fail: Mutex<bool>,
    calls: AtomicU32,
}

impl TestChoice {
    fn new(held: Option<HarnessKind>) -> Arc<TestChoice> {
        Arc::new(TestChoice {
            held: Mutex::new(held),
            given: Mutex::new(Vec::new()),
            fail: Mutex::new(false),
            calls: AtomicU32::new(0),
        })
    }
}

#[async_trait::async_trait]
impl HarnessChoice for TestChoice {
    fn chosen(&self) -> Option<HarnessKind> {
        *lock(&self.held)
    }
    async fn choose(&self, harness: HarnessKind) -> Result<(), ChoiceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        lock(&self.given).push(harness);
        if *lock(&self.fail) {
            return Err(ChoiceError);
        }
        *lock(&self.held) = Some(harness);
        Ok(())
    }
}

fn signed_in(method: &str) -> HarnessSignIn {
    HarnessSignIn::SignedIn {
        method: Some(method.to_owned()),
    }
}

fn copilot(env: &Env, status: HarnessSignIn) -> Arc<FakeProbe> {
    let probe = FakeProbe::new(HarnessKind::CopilotCli, "1.0.115", status);
    *lock(&probe.location) = Ok(Some(HarnessLocation {
        path: COPILOT_AT.into(),
        version: "1.0.115".to_owned(),
    }));
    env.setup.register_probe(probe.clone());
    probe
}

/// The desktop with no company profile, both assistants found and signed in, and a choice
/// registered with no answer.
async fn both_found() -> (Env, Arc<FakeProbe>, Arc<TestChoice>) {
    let env = Env::new().await;
    *lock(&env.profile.status) = ProfileState::NotUsed;
    let copilot = copilot(&env, signed_in("github"));
    let choice = TestChoice::new(None);
    env.setup.set_harness_choice(choice.clone());
    (env, copilot, choice)
}

/// Waits until the guided run shows the question; panics after five seconds.
async fn question_shown(env: &Env) {
    let mut progress = env.setup.progress();
    let shown = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let asked = progress
                .borrow_and_update()
                .steps
                .iter()
                .any(|s| s.check == CheckKind::HarnessChoice && s.state == StepState::NeedsYou);
            if asked {
                return;
            }
            if progress.changed().await.is_err() {
                return;
            }
        }
    })
    .await;
    assert!(shown.is_ok(), "the question was never shown");
}

fn asked(report: &crate::SetupReport) -> bool {
    report
        .results
        .iter()
        .any(|r| r.check == CheckKind::HarnessChoice)
}

async fn attachment(env: &Env, kind: HarnessKind) -> crate::HarnessAttachment {
    store::attachment(&env.pool, kind)
        .await
        .expect("store")
        .unwrap_or_else(|| panic!("no {kind:?} attachment"))
}

// Scenario: Happy path — both found, no profile, no choice: the run asks (SET-CHO-3, SET-CHO-4)
// spec: ordin-desktop/components/SetupAssistant#SET-CHO-3
// spec: ordin-desktop/components/SetupAssistant#SET-CHO-4
#[tokio::test]
async fn set_cho_both_found_the_run_asks_and_goes_on_with_the_choice() {
    let (env, copilot, choice) = both_found().await;
    // `progress` keeps a value only while someone subscribes, as the desktop's events do.
    let _subscribed = env.setup.progress();
    let answer = async {
        question_shown(&env).await;
        assert_eq!(
            attachment(&env, HarnessKind::ClaudeCode).await.state,
            HarnessAttachmentState::Ready
        );
        assert_eq!(
            attachment(&env, HarnessKind::CopilotCli).await.state,
            HarnessAttachmentState::Ready
        );
        env.setup
            .choose_harness(HarnessKind::CopilotCli)
            .await
            .expect("chosen");
    };
    let (report, ()) = tokio::join!(env.setup.run_guided(), answer);
    let report = report.expect("guided");
    assert!(report.ready);
    assert_eq!(*lock(&choice.given), vec![HarnessKind::CopilotCli]);
    let r = report
        .results
        .iter()
        .find(|r| r.check == CheckKind::HarnessChoice)
        .expect("the choice's result");
    assert_eq!(r.outcome, CheckOutcome::Passed);
    let step = env
        .setup
        .progress()
        .borrow()
        .steps
        .iter()
        .find(|s| s.check == CheckKind::HarnessChoice)
        .cloned()
        .expect("the step");
    assert_eq!(step.state, StepState::Done);
    assert_eq!(step.message_id, "setup-step-harness-choice");
    assert_eq!(copilot.sign_ins.load(Ordering::SeqCst), 0);
    assert_eq!(env.claude.sign_ins.load(Ordering::SeqCst), 0);
    let ready = env.setup.ready_harness().await.expect("ready");
    assert_eq!(ready.harness, HarnessKind::CopilotCli);
}

// Scenario: Happy path — a chosen harness that is signed out is signed in next (SET-CHO-4, SA-CHO-01)
#[tokio::test]
async fn set_cho_a_chosen_signed_out_harness_is_signed_in_next() {
    let env = Env::new().await;
    *lock(&env.profile.status) = ProfileState::NotUsed;
    copilot(&env, HarnessSignIn::SignedOut);
    env.setup.set_harness_choice(TestChoice::new(None));
    let answer = async {
        question_shown(&env).await;
        // SA-CHO-01: offered, marked as needing sign-in.
        assert_eq!(
            attachment(&env, HarnessKind::CopilotCli).await.state,
            HarnessAttachmentState::NeedsSignIn
        );
        env.setup
            .choose_harness(HarnessKind::CopilotCli)
            .await
            .expect("chosen");
        let mut progress = env.setup.progress();
        let waiting = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let needs =
                    progress.borrow_and_update().steps.iter().any(|s| {
                        s.check == CheckKind::HarnessSignIn && s.state == StepState::NeedsYou
                    });
                if needs || progress.changed().await.is_err() {
                    return needs;
                }
            }
        })
        .await;
        assert_eq!(waiting, Ok(true), "the chosen harness's sign-in waits");
    };
    let (report, ()) = tokio::join!(env.setup.run_guided(), answer);
    // Nobody signs in, so the short sign-in wait of the test tuning ends the run.
    let report = report.expect("guided");
    assert!(!report.ready);
}

// Scenario: Edge case — a choice already exists: no question, chosen first (SET-CHO-2, SET-CHO-5)
// spec: ordin-desktop/components/SetupAssistant#SET-CHO-2
// spec: ordin-desktop/components/SetupAssistant#SET-CHO-5
#[tokio::test]
async fn set_cho_a_choice_already_made_is_walked_first_and_nothing_is_asked() {
    let env = Env::new().await;
    *lock(&env.profile.status) = ProfileState::NotUsed;
    copilot(&env, signed_in("github"));
    env.setup
        .set_harness_choice(TestChoice::new(Some(HarnessKind::CopilotCli)));
    let report = env.setup.run_guided().await.expect("guided");
    assert!(report.ready);
    assert!(!asked(&report));
    let ready = env.setup.ready_harness().await.expect("ready");
    assert_eq!(ready.harness, HarnessKind::CopilotCli);
}

// Scenario: Edge case — only one found: no question (SET-CHO-3)
#[tokio::test]
async fn set_cho_only_one_found_asks_nothing() {
    let env = Env::new().await;
    *lock(&env.profile.status) = ProfileState::NotUsed;
    let probe = copilot(&env, signed_in("github"));
    *lock(&probe.location) = Ok(None);
    env.setup.set_harness_choice(TestChoice::new(None));
    let report = env.setup.run_guided().await.expect("guided");
    assert!(report.ready);
    assert!(!asked(&report));
    assert_eq!(
        env.setup.ready_harness().await.expect("ready").harness,
        HarnessKind::ClaudeCode
    );
}

// Scenario: Edge case — a company profile decides (SET-CHO-2, SET-CHO-3)
#[tokio::test]
async fn set_cho_a_company_profile_decides_and_nothing_is_asked() {
    let env = Env::new().await;
    env.profile
        .edit(|d| d.harnesses = vec![harness(HarnessName::ClaudeCode, &[])]);
    copilot(&env, signed_in("github"));
    env.setup.set_harness_choice(TestChoice::new(None));
    let report = env.setup.run_guided().await.expect("guided");
    assert!(!asked(&report));
    let attachments = env.setup.harness_attachments().await.expect("attachments");
    assert!(!attachments
        .iter()
        .any(|a| a.harness == HarnessKind::CopilotCli));
}

// Scenario: Edge case — a harness with no access is not offered (SET-CHO-3, SA-CHO-02)
#[tokio::test]
async fn set_cho_a_harness_with_no_access_is_not_offered() {
    let env = Env::new().await;
    *lock(&env.profile.status) = ProfileState::NotUsed;
    *lock(&env.claude.status) = Ok(HarnessSignIn::NoAccess);
    copilot(&env, signed_in("github"));
    env.setup.set_harness_choice(TestChoice::new(None));
    let report = env.setup.run_guided().await.expect("guided");
    assert!(!asked(&report));
    // The run goes on as before: Claude Code first, no access, then Copilot.
    assert!(report.ready);
    assert_eq!(
        env.setup.ready_harness().await.expect("ready").harness,
        HarnessKind::CopilotCli
    );
}

// Scenario: Edge case — background checks never ask (SET-CHO-6)
// spec: ordin-desktop/components/SetupAssistant#SET-CHO-6
#[tokio::test]
async fn set_cho_background_checks_never_ask() {
    let (env, _copilot, choice) = both_found().await;
    let report = tokio::time::timeout(
        Duration::from_secs(5),
        env.setup.run_checks(CheckTrigger::Background),
    )
    .await
    .expect("checks do not wait")
    .expect("checks");
    assert!(!asked(&report));
    assert_eq!(choice.calls.load(Ordering::SeqCst), 0);
}

// Scenario: Error path — choosing with no question waiting (SET-CHO-4)
#[tokio::test]
async fn set_cho_choosing_with_no_question_waiting() {
    let (env, _copilot, choice) = both_found().await;
    let error = env
        .setup
        .choose_harness(HarnessKind::ClaudeCode)
        .await
        .expect_err("no question");
    assert_eq!(error.code(), "SETUP_NO_CHOICE_PENDING");
    assert!(!error.retryable());
    assert_eq!(choice.calls.load(Ordering::SeqCst), 0);
}

// Scenario: Error path — choosing a harness that was not offered (SET-CHO-4)
#[tokio::test]
async fn set_cho_choosing_a_harness_that_is_not_offered_any_more() {
    let (env, _copilot, choice) = both_found().await;
    let answer = async {
        question_shown(&env).await;
        sqlx::query("DELETE FROM harness_attachments WHERE harness = ?1")
            .bind(store::harness_text(HarnessKind::CopilotCli))
            .execute(&env.pool)
            .await
            .expect("removed");
        let error = env
            .setup
            .choose_harness(HarnessKind::CopilotCli)
            .await
            .expect_err("not offered");
        assert_eq!(error.code(), "SETUP_HARNESS_NOT_OFFERED");
        assert_eq!(choice.calls.load(Ordering::SeqCst), 0);
        // The question still waits, and Claude Code can still be chosen.
        env.setup
            .choose_harness(HarnessKind::ClaudeCode)
            .await
            .expect("chosen");
    };
    let (report, ()) = tokio::join!(env.setup.run_guided(), answer);
    assert!(report.expect("guided").ready);
}

// Scenario: Error path — the choice cannot be kept (SET-CHO-4)
#[tokio::test]
async fn set_cho_the_choice_cannot_be_kept() {
    let (env, _copilot, choice) = both_found().await;
    *lock(&choice.fail) = true;
    let answer = async {
        question_shown(&env).await;
        let error = env
            .setup
            .choose_harness(HarnessKind::CopilotCli)
            .await
            .expect_err("not kept");
        assert_eq!(error.code(), "SETUP_CHOICE_NOT_SAVED");
        assert!(error.retryable());
        let still = env
            .setup
            .progress()
            .borrow()
            .steps
            .iter()
            .any(|s| s.check == CheckKind::HarnessChoice && s.state == StepState::NeedsYou);
        assert!(still, "the step stays needs_you");
        // Choosing again once the program can keep it goes on.
        *lock(&choice.fail) = false;
        env.setup
            .choose_harness(HarnessKind::CopilotCli)
            .await
            .expect("chosen");
    };
    let (report, ()) = tokio::join!(env.setup.run_guided(), answer);
    assert!(report.expect("guided").ready);
}
