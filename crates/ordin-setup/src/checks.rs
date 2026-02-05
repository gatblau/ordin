//! The check runner: the guided sequence (A), `sign_in_harness` (B) and the checks (D), with the
//! ADR-114 step SET-UP-23 and the ADR-072 step SET-UP-27.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ordin_catalogue::RepositorySyncOutcome;
use ordin_craft_toolkit::ProgramCheckState;
use ordin_organisation_profile::ProfileState;
use ordin_organisation_profile::RefreshOutcome;
use ordin_types::domain::identity::{HarnessAttachment, HarnessAttachmentState, HarnessKind};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::HarnessAttachmentId;
use sha2::{Digest, Sha256};

use crate::assistant::{hex, lock, read, Inner};
use crate::error::SetupError;
use crate::process::{self, Ran, COMMAND_TIMEOUT};
use crate::store;
use crate::types::{
    BundledProgram, CheckKind, CheckOutcome, CheckResult, CheckTrigger, HarnessProbe,
    HarnessSignIn, PersonAction, ProbeError, ProbeRepair, ProgressStep, RefusedProgram,
    SetupProgram, SetupProgress, SetupReport, StepProblem, StepState,
};
use crate::TARGET;

/// The longest `sign_in_method` kept (2C rule); longer text is cut (B.2).
const SIGN_IN_METHOD_MAX: usize = 120;

/// The message id of a code: the code in kebab case (the registry's `sentence_id` form).
fn message_id(code: &str) -> String {
    code.to_lowercase().replace('_', "-")
}

pub(crate) fn passed(check: CheckKind) -> CheckResult {
    CheckResult {
        check,
        outcome: CheckOutcome::Passed,
        code: None,
        message_id: None,
        message_args: BTreeMap::new(),
        action: None,
        refused_program: None,
        checked_at: Timestamp::now(),
    }
}

fn with_outcome(check: CheckKind, outcome: CheckOutcome) -> CheckResult {
    CheckResult {
        outcome,
        ..passed(check)
    }
}

pub(crate) fn failed(check: CheckKind, code: &str, action: Option<PersonAction>) -> CheckResult {
    CheckResult {
        check,
        outcome: CheckOutcome::Failed,
        code: Some(code.to_owned()),
        message_id: Some(message_id(code)),
        message_args: BTreeMap::new(),
        action,
        refused_program: None,
        checked_at: Timestamp::now(),
    }
}

fn arg(mut result: CheckResult, key: &str, value: &str) -> CheckResult {
    result.message_args.insert(key.to_owned(), value.to_owned());
    result
}

/// `SETUP_HARNESS_MISSING` and `SETUP_HARNESS_UNTESTED` for the `copilot` harness carry Copilot's own
/// sentence ids; the codes do not change (SET-CPX-4). Claude Code keeps the code's own id.
// spec: ordin-desktop/components/SetupAssistant#SET-CPX-4
fn harness_failed(kind: HarnessKind, code: &str) -> CheckResult {
    let mut result = failed(CheckKind::Harness, code, None);
    if kind == HarnessKind::CopilotCli {
        result.message_id = Some(format!("{}-copilot", message_id(code)));
    }
    arg(result, "harness", harness_words(kind))
}

fn harness_words(kind: HarnessKind) -> &'static str {
    match kind {
        HarnessKind::ClaudeCode => "Claude Code",
        HarnessKind::CopilotCli => "Copilot",
    }
}

/// The steps the person sees and their states.
struct Book {
    steps: Vec<ProgressStep>,
}

impl Book {
    fn new(kinds: &[CheckKind]) -> Book {
        Book {
            steps: kinds
                .iter()
                .map(|k| ProgressStep {
                    check: *k,
                    state: StepState::Waiting,
                    // The progress line of each step (11 §5.1); the screens' message catalogue.
                    message_id: format!("setup-step-{}", k.as_str().replace('_', "-")),
                    problem: None,
                })
                .collect(),
        }
    }

    fn insert_after(&mut self, after: CheckKind, kind: CheckKind) {
        if self.steps.iter().any(|s| s.check == kind) {
            return;
        }
        let at = self
            .steps
            .iter()
            .position(|s| s.check == after)
            .map_or(self.steps.len(), |i| i + 1);
        self.steps.insert(
            at,
            ProgressStep {
                check: kind,
                state: StepState::Waiting,
                message_id: format!("setup-step-{}", kind.as_str().replace('_', "-")),
                problem: None,
            },
        );
    }

    fn set(&mut self, kind: CheckKind, state: StepState) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.check == kind) {
            step.state = state;
        }
    }

    fn end(&mut self, result: &CheckResult) {
        let failed = result.outcome == CheckOutcome::Failed;
        self.set(
            result.check,
            if failed {
                StepState::Failed
            } else {
                StepState::Done
            },
        );
        // spec: ordin-desktop/components/SetupAssistant#SET-R1-3
        let problem = match (&result.message_id, failed) {
            (Some(message_id), true) => Some(StepProblem {
                message_id: message_id.clone(),
                message_args: result.message_args.clone(),
            }),
            _ => None,
        };
        if let Some(step) = self.steps.iter_mut().find(|s| s.check == result.check) {
            step.problem = problem;
        }
    }

    fn progress(&self, running: bool) -> SetupProgress {
        let done = self
            .steps
            .iter()
            .filter(|s| matches!(s.state, StepState::Done | StepState::Failed))
            .count();
        SetupProgress {
            running,
            steps: self.steps.clone(),
            done: u32::try_from(done).unwrap_or(u32::MAX),
            total: u32::try_from(self.steps.len()).unwrap_or(u32::MAX),
        }
    }
}

/// What one harness's check found.
struct HarnessCheck {
    harness: CheckResult,
    sign_in: Option<CheckResult>,
    ready: bool,
}

/// How one bundled program's start check ended (D.3).
enum ProgramRun {
    Passed { restored: bool },
    Damaged,
    Refused(RefusedProgram),
    Failed,
}

fn file_sha256(path: &Path) -> Option<Sha256Hex> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Some(Sha256Hex(hex(&hasher.finalize())))
}

fn intact(program: &BundledProgram) -> bool {
    if !program.path.is_file() {
        return false;
    }
    match &program.expected_sha256 {
        Some(expected) => file_sha256(&program.path).is_some_and(|h| h.0 == expected.0),
        None => true,
    }
}

impl Inner {
    fn publish(&self, book: &Book, running: bool) {
        let _ = self.progress.send(book.progress(running));
    }

    // Each value is one column of `harness_attachments`.
    #[allow(clippy::too_many_arguments)]
    async fn write_attachment(
        &self,
        kind: HarnessKind,
        location: &Path,
        version: Option<String>,
        tested: bool,
        signed_in: bool,
        method: Option<String>,
        state: HarnessAttachmentState,
    ) -> Result<HarnessAttachment, SetupError> {
        let id = store::attachment(&self.pool, kind)
            .await?
            .map_or_else(HarnessAttachmentId::new, |a| a.id);
        let attachment = HarnessAttachment {
            id,
            harness: kind,
            location: location.to_string_lossy().into_owned(),
            version,
            tested,
            signed_in,
            sign_in_method: method.map(|m| m.chars().take(SIGN_IN_METHOD_MAX).collect()),
            state,
            last_checked_at: Timestamp::now(),
        };
        store::put_attachment(&self.pool, &attachment).await?;
        tracing::info!(
            target: TARGET,
            harness = store::harness_text(kind),
            version = attachment.version.as_deref().unwrap_or(""),
            tested,
            signed_in,
            state = store::state_text(state),
            "setup.harness"
        );
        Ok(attachment)
    }

    /// A.2 (guided) and D.4's status-only read.
    async fn company_settings(&self, guided: bool) -> CheckResult {
        match self.ports.profile.status().state {
            // SET-R1-6: an install with no company profile address has no company profile: the step is
            // skipped, never a failure with Send to IT (nothing waits on IT, plan 04 FU-014).
            // spec: ordin-desktop/components/SetupAssistant#SET-R1-6
            ProfileState::NotUsed | ProfileState::NoAddress => {
                with_outcome(CheckKind::CompanySettings, CheckOutcome::Skipped)
            }
            ProfileState::Active => passed(CheckKind::CompanySettings),
            ProfileState::NeverFetched | ProfileState::Stale if guided => {
                match self.ports.profile.refresh().await {
                    Ok(RefreshOutcome::Activated { .. } | RefreshOutcome::Unchanged) => {
                        passed(CheckKind::CompanySettings)
                    }
                    Ok(RefreshOutcome::Refused { .. }) | Err(()) => failed(
                        CheckKind::CompanySettings,
                        "SETUP_PROFILE_UNREACHABLE",
                        None,
                    ),
                }
            }
            ProfileState::NeverFetched | ProfileState::Stale => failed(
                CheckKind::CompanySettings,
                "SETUP_PROFILE_UNREACHABLE",
                None,
            ),
        }
    }

    /// Locates one harness and reads its sign-in state; never starts a sign-in. In the guided run
    /// a signed-out harness on the desktop waits for `sign_in_harness` (A.4).
    async fn check_harness(
        &self,
        kind: HarnessKind,
        tested_versions: &[String],
        probe: &Arc<dyn HarnessProbe>,
        book: Option<&mut Book>,
    ) -> Result<HarnessCheck, SetupError> {
        let words = harness_words(kind);
        let location = match probe.locate().await {
            Ok(Some(found)) => found,
            Ok(None) | Err(ProbeError::NotFound) => {
                return Ok(HarnessCheck {
                    harness: harness_failed(kind, "SETUP_HARNESS_MISSING"),
                    sign_in: None,
                    ready: false,
                })
            }
            Err(ProbeError::Refused) => {
                return Ok(HarnessCheck {
                    harness: arg(
                        failed(CheckKind::Harness, "SETUP_PROGRAM_REFUSED", None),
                        "program",
                        words,
                    ),
                    sign_in: None,
                    ready: false,
                })
            }
            Err(error) => {
                // spec: ordin-desktop/components/SetupAssistant#SET-R1-5
                probe_failed(kind, "locate", &error);
                return Ok(HarnessCheck {
                    harness: arg(
                        failed(
                            CheckKind::Harness,
                            "SETUP_HARNESS_CHECK_FAILED",
                            Some(PersonAction::TryAgain),
                        ),
                        "harness",
                        words,
                    ),
                    sign_in: None,
                    ready: false,
                });
            }
        };
        // SA-SETUP-02: tested when the list is empty or holds the version exactly.
        let tested = tested_versions.is_empty() || tested_versions.contains(&location.version);
        let version = Some(location.version.clone());
        self.write_attachment(
            kind,
            &location.path,
            version.clone(),
            tested,
            false,
            None,
            HarnessAttachmentState::Found,
        )
        .await?;
        self.write_attachment(
            kind,
            &location.path,
            version.clone(),
            tested,
            false,
            None,
            HarnessAttachmentState::Checked,
        )
        .await?;
        if !tested {
            self.write_attachment(
                kind,
                &location.path,
                version,
                false,
                false,
                None,
                HarnessAttachmentState::NeedsUpdate,
            )
            .await?;
            return Ok(HarnessCheck {
                harness: harness_failed(kind, "SETUP_HARNESS_UNTESTED"),
                sign_in: None,
                ready: false,
            });
        }
        let status = probe.sign_in_status().await;
        let (sign_in, ready) = match status {
            Ok(HarnessSignIn::SignedIn { method }) => {
                self.write_attachment(
                    kind,
                    &location.path,
                    version,
                    true,
                    true,
                    method,
                    HarnessAttachmentState::Ready,
                )
                .await?;
                (passed(CheckKind::HarnessSignIn), true)
            }
            Ok(HarnessSignIn::NoAccess) => (
                arg(
                    failed(CheckKind::HarnessSignIn, "SETUP_NO_HARNESS_ACCESS", None),
                    "harness",
                    words,
                ),
                false,
            ),
            Ok(HarnessSignIn::SignedOut) => {
                self.write_attachment(
                    kind,
                    &location.path,
                    version,
                    true,
                    false,
                    None,
                    HarnessAttachmentState::NeedsSignIn,
                )
                .await?;
                if self.program == SetupProgram::PersonalServer {
                    (
                        failed(CheckKind::HarnessSignIn, "SETUP_SIGN_IN_ON_SERVER", None),
                        false,
                    )
                } else if let Some(book) = book {
                    self.wait_for_sign_in(kind, book).await
                } else {
                    (
                        arg(
                            failed(
                                CheckKind::HarnessSignIn,
                                "SETUP_NO_READY_HARNESS",
                                Some(PersonAction::SignInAgain),
                            ),
                            "harness",
                            words,
                        ),
                        false,
                    )
                }
            }
            Err(ProbeError::Refused) => (
                arg(
                    failed(CheckKind::HarnessSignIn, "SETUP_PROGRAM_REFUSED", None),
                    "program",
                    words,
                ),
                false,
            ),
            Err(error) => {
                // spec: ordin-desktop/components/SetupAssistant#SET-R1-5
                probe_failed(kind, "sign_in_status", &error);
                (
                    failed(
                        CheckKind::HarnessSignIn,
                        "SETUP_HARNESS_CHECK_FAILED",
                        Some(PersonAction::TryAgain),
                    ),
                    false,
                )
            }
        };
        Ok(HarnessCheck {
            harness: passed(CheckKind::Harness),
            sign_in: Some(sign_in),
            ready,
        })
    }

    /// A.4: the step needs the person; the run waits until `sign_in_harness` makes the
    /// attachment ready, or the sign-in wait ends.
    async fn wait_for_sign_in(&self, kind: HarnessKind, book: &mut Book) -> (CheckResult, bool) {
        book.insert_after(CheckKind::Harness, CheckKind::HarnessSignIn);
        book.set(CheckKind::HarnessSignIn, StepState::NeedsYou);
        self.publish(book, true);
        let mut changes = self.signed_in.subscribe();
        let wait = read(&self.tuning).sign_in_wait;
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Ok(Some(a)) = store::attachment(&self.pool, kind).await {
                if a.state == HarnessAttachmentState::Ready {
                    return (passed(CheckKind::HarnessSignIn), true);
                }
            }
            match tokio::time::timeout_at(deadline, changes.changed()).await {
                Ok(Ok(())) => continue,
                _ => {
                    return (
                        failed(
                            CheckKind::HarnessSignIn,
                            "SETUP_SIGN_IN_TIMEOUT",
                            Some(PersonAction::TryAgain),
                        ),
                        false,
                    )
                }
            }
        }
    }

    /// SET-CHO-3: the desktop, no company profile, a registered choice with no answer yet, and a
    /// probe for each of Claude Code and Copilot.
    // spec: ordin-desktop/components/SetupAssistant#SET-CHO-3
    fn question_applies(&self) -> bool {
        self.program == SetupProgram::Desktop
            && matches!(
                self.ports.profile.status().state,
                ProfileState::NotUsed | ProfileState::NoAddress
            )
            && read(&self.choice).is_some()
            && self.chosen_harness().is_none()
            && self.probe(HarnessKind::ClaudeCode).is_some()
            && self.probe(HarnessKind::CopilotCli).is_some()
    }

    /// SET-CHO-3: locates one harness and reads its sign-in once, starting nothing. True when the
    /// harness is offered: found with a readable version, and `ready` or `needs_sign_in`.
    // spec: ordin-desktop/components/SetupAssistant#SET-CHO-3
    async fn survey(
        &self,
        kind: HarnessKind,
        probe: &Arc<dyn HarnessProbe>,
    ) -> Result<bool, SetupError> {
        let location = match probe.locate().await {
            Ok(Some(found)) => found,
            Ok(None) => return Ok(false),
            Err(error) => {
                probe_failed(kind, "locate", &error);
                return Ok(false);
            }
        };
        let version = Some(location.version.clone());
        // SA-CHO-03: with no company profile `tested_versions` is empty, so every version is tested.
        for state in [
            HarnessAttachmentState::Found,
            HarnessAttachmentState::Checked,
        ] {
            self.write_attachment(
                kind,
                &location.path,
                version.clone(),
                true,
                false,
                None,
                state,
            )
            .await?;
        }
        match probe.sign_in_status().await {
            Ok(HarnessSignIn::SignedIn { method }) => {
                self.write_attachment(
                    kind,
                    &location.path,
                    version,
                    true,
                    true,
                    method,
                    HarnessAttachmentState::Ready,
                )
                .await?;
                Ok(true)
            }
            // SA-CHO-02: no access leaves the attachment `checked` and the harness is not offered.
            Ok(HarnessSignIn::NoAccess) => Ok(false),
            Ok(HarnessSignIn::SignedOut) | Err(_) => {
                self.write_attachment(
                    kind,
                    &location.path,
                    version,
                    true,
                    false,
                    None,
                    HarnessAttachmentState::NeedsSignIn,
                )
                .await?;
                Ok(true)
            }
        }
    }

    /// SET-CHO-3: the step needs the person; the run waits for `choose_harness`. `None` when the
    /// assistant closes first.
    // spec: ordin-desktop/components/SetupAssistant#SET-CHO-3
    async fn wait_for_choice(
        &self,
        offered: Vec<HarnessKind>,
        book: &mut Book,
    ) -> Option<HarnessKind> {
        book.insert_after(CheckKind::Harness, CheckKind::HarnessChoice);
        book.set(CheckKind::HarnessChoice, StepState::NeedsYou);
        self.chose.send_replace(None);
        *lock(&self.pending_choice) = Some(offered);
        self.publish(book, true);
        let mut answers = self.chose.subscribe();
        let mut closed = self.closed.subscribe();
        let chosen = loop {
            if let Some(kind) = *answers.borrow_and_update() {
                break Some(kind);
            }
            if *closed.borrow_and_update() {
                break None;
            }
            tokio::select! {
                changed = answers.changed() => if changed.is_err() { break None },
                changed = closed.changed() => if changed.is_err() { break None },
            }
        };
        *lock(&self.pending_choice) = None;
        chosen
    }

    /// SET-CHO-4.
    // spec: ordin-desktop/components/SetupAssistant#SET-CHO-4
    pub(crate) async fn choose_harness(&self, harness: HarnessKind) -> Result<(), SetupError> {
        let offered = lock(&self.pending_choice).clone();
        let Some(offered) = offered else {
            return Err(SetupError::permanent("SETUP_NO_CHOICE_PENDING"));
        };
        let still_offered = offered.contains(&harness)
            && matches!(
                store::attachment(&self.pool, harness).await?,
                Some(a) if matches!(
                    a.state,
                    HarnessAttachmentState::Ready | HarnessAttachmentState::NeedsSignIn
                )
            );
        if !still_offered {
            return Err(SetupError::permanent("SETUP_HARNESS_NOT_OFFERED"));
        }
        let choice = read(&self.choice).clone();
        let Some(choice) = choice else {
            return Err(SetupError::permanent("SETUP_NO_CHOICE_PENDING"));
        };
        if choice.choose(harness).await.is_err() {
            tracing::warn!(
                target: TARGET,
                harness = store::harness_text(harness),
                "setup.choice_not_saved"
            );
            return Err(SetupError::of("SETUP_CHOICE_NOT_SAVED"));
        }
        if lock(&self.pending_choice).take().is_none() {
            return Err(SetupError::permanent("SETUP_NO_CHOICE_PENDING"));
        }
        tracing::info!(
            target: TARGET,
            harness = store::harness_text(harness),
            "setup.harness_chosen"
        );
        self.chose.send_replace(Some(harness));
        Ok(())
    }

    /// A.3–A.4 over the listed harnesses: the first one found, tested and checked decides. When
    /// SET-CHO-3 holds, the person chooses first and only the chosen harness is checked; the
    /// second value is the `HarnessChoice` result.
    async fn guided_harness(
        &self,
        book: &mut Book,
    ) -> Result<(HarnessCheck, Option<CheckResult>), SetupError> {
        if self.question_applies() {
            let mut offered = Vec::new();
            for kind in [HarnessKind::ClaudeCode, HarnessKind::CopilotCli] {
                if let Some(probe) = self.probe(kind) {
                    if self.survey(kind, &probe).await? {
                        offered.push(kind);
                    }
                }
            }
            if offered.len() >= 2 {
                book.end(&passed(CheckKind::Harness));
                let Some(kind) = self.wait_for_choice(offered, book).await else {
                    return Ok((
                        HarnessCheck {
                            harness: passed(CheckKind::Harness),
                            sign_in: None,
                            ready: false,
                        },
                        None,
                    ));
                };
                let choice = passed(CheckKind::HarnessChoice);
                book.end(&choice);
                self.publish(book, true);
                if let Some(probe) = self.probe(kind) {
                    let check = self.check_harness(kind, &[], &probe, Some(book)).await?;
                    return Ok((check, Some(choice)));
                }
            } else if let [kind] = offered[..] {
                // Fewer than two offered: no question, and the run goes on with the offered one.
                if let Some(probe) = self.probe(kind) {
                    let check = self.check_harness(kind, &[], &probe, Some(book)).await?;
                    return Ok((check, None));
                }
            }
        }
        let mut first_failure: Option<CheckResult> = None;
        for (kind, tested) in self.listed_harnesses() {
            let Some(probe) = self.probe(kind) else {
                continue;
            };
            let check = self
                .check_harness(kind, &tested, &probe, Some(book))
                .await?;
            if check.harness.outcome == CheckOutcome::Failed {
                first_failure.get_or_insert(check.harness);
                continue;
            }
            return Ok((check, None));
        }
        Ok((
            HarnessCheck {
                harness: first_failure.unwrap_or_else(|| {
                    arg(
                        failed(CheckKind::Harness, "SETUP_HARNESS_MISSING", None),
                        "harness",
                        "Claude Code",
                    )
                }),
                sign_in: None,
                ready: false,
            },
            None,
        ))
    }

    /// A.
    pub(crate) async fn guided(self: Arc<Self>) -> Result<SetupReport, SetupError> {
        let _run = self.run_lock.lock().await;
        let started = Timestamp::now();
        tracing::info!(target: TARGET, "setup.guided.started");
        let snapshot = self.ports.profile.current();
        let mut kinds = vec![CheckKind::CompanySettings, CheckKind::Harness];
        if !snapshot.document.agent_catalogue.repositories.is_empty() {
            kinds.push(CheckKind::AgentRepositories);
        }
        if !self.bundled.is_empty() {
            kinds.push(CheckKind::Programs);
        }
        if !snapshot.document.craft.apps.is_empty() {
            kinds.push(CheckKind::CraftApps);
        }
        let mut book = Book::new(&kinds);
        self.publish(&book, true);
        let mut results = Vec::new();

        book.set(CheckKind::CompanySettings, StepState::Running);
        self.publish(&book, true);
        let company = self.company_settings(true).await;
        book.end(&company);
        results.push(company);
        self.publish(&book, true);

        book.set(CheckKind::Harness, StepState::Running);
        self.publish(&book, true);
        let (harness, choice) = self.guided_harness(&mut book).await?;
        book.end(&harness.harness);
        results.push(harness.harness);
        if let Some(choice) = choice {
            results.push(choice);
        }
        if let Some(sign_in) = harness.sign_in {
            book.end(&sign_in);
            results.push(sign_in);
        }
        self.publish(&book, true);
        let ready = harness.ready;
        let missing = results
            .iter()
            .any(|r| r.code.as_deref() == Some("SETUP_HARNESS_MISSING"));

        // A.3: with no harness found the sequence ends here.
        if !missing {
            let snapshot = self.ports.profile.current();
            if !snapshot.document.agent_catalogue.repositories.is_empty() {
                book.set(CheckKind::AgentRepositories, StepState::Running);
                self.publish(&book, true);
                let r = self.agent_repositories().await;
                book.end(&r);
                results.push(r);
                self.publish(&book, true);
            }
            if !self.bundled.is_empty() {
                book.set(CheckKind::Programs, StepState::Running);
                self.publish(&book, true);
                let r = self.programs().await;
                book.end(&r);
                results.push(r);
                self.publish(&book, true);
            }
            if !snapshot.document.craft.apps.is_empty() {
                book.set(CheckKind::CraftApps, StepState::Running);
                self.publish(&book, true);
                let r = self.craft_apps().await;
                book.end(&r);
                results.push(r);
                self.publish(&book, true);
            }
        }

        // A.7.
        let ended = Timestamp::now();
        if ready {
            store::set_completed(&self.pool, ended, &self.requirements_digest()).await?;
        }
        for result in &results {
            store::put_check(&self.pool, result).await?;
        }
        self.publish(&book, false);
        tracing::info!(
            target: TARGET,
            total = book.steps.len(),
            ready,
            duration_ms = ended.unix_millis() - started.unix_millis(),
            "setup.guided.completed"
        );
        Ok(SetupReport {
            trigger: None,
            results,
            ready,
            started_at: started,
            ended_at: ended,
        })
    }

    /// B.
    pub(crate) async fn sign_in_harness(
        &self,
        harness: HarnessKind,
    ) -> Result<HarnessAttachment, SetupError> {
        let Some(probe) = self.probe(harness) else {
            return Err(SetupError::permanent("SETUP_HARNESS_UNKNOWN"));
        };
        if self.program == SetupProgram::PersonalServer {
            return Err(SetupError::permanent("SETUP_SIGN_IN_ON_SERVER"));
        }
        let tested_versions = self
            .listed_harnesses()
            .into_iter()
            .find(|(k, _)| *k == harness)
            .map(|(_, t)| t)
            .unwrap_or_default();
        let current = match store::attachment(&self.pool, harness).await? {
            Some(a) => a,
            None => {
                let found = match probe.locate().await {
                    Ok(Some(found)) => found,
                    Ok(None) | Err(ProbeError::NotFound) => {
                        return Err(SetupError::permanent("SETUP_HARNESS_MISSING"))
                    }
                    Err(ProbeError::Refused) => {
                        return Err(SetupError::permanent("SETUP_PROGRAM_REFUSED"))
                    }
                    Err(error) => {
                        // spec: ordin-desktop/components/SetupAssistant#SET-R1-5
                        probe_failed(harness, "locate", &error);
                        return Err(SetupError::of("SETUP_HARNESS_CHECK_FAILED"));
                    }
                };
                let tested = tested_versions.is_empty() || tested_versions.contains(&found.version);
                self.write_attachment(
                    harness,
                    &found.path,
                    Some(found.version),
                    tested,
                    false,
                    None,
                    HarnessAttachmentState::NeedsSignIn,
                )
                .await?
            }
        };
        let (poll, wait) = {
            let tuning = read(&self.tuning);
            (tuning.sign_in_poll, tuning.sign_in_wait)
        };
        let starter = probe.clone();
        let mut flow = tokio::spawn(async move { starter.start_sign_in().await });
        let mut flow_done = false;
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if !flow_done {
                if let Ok(joined) = tokio::time::timeout(Duration::ZERO, &mut flow).await {
                    flow_done = true;
                    // C-25: `NotOffered` keeps polling, so a sign-in made in the CLI is picked up.
                    if let Ok(Err(ProbeError::Refused)) = joined {
                        return Err(SetupError::permanent("SETUP_PROGRAM_REFUSED"));
                    }
                }
            }
            match probe.sign_in_status().await {
                Ok(HarnessSignIn::SignedIn { method }) => {
                    let state = if current.tested {
                        HarnessAttachmentState::Ready
                    } else {
                        HarnessAttachmentState::NeedsUpdate
                    };
                    let done = self
                        .write_attachment(
                            harness,
                            Path::new(&current.location),
                            current.version.clone(),
                            current.tested,
                            true,
                            method,
                            state,
                        )
                        .await?;
                    self.signed_in.send_modify(|n| *n += 1);
                    return Ok(done);
                }
                Ok(HarnessSignIn::NoAccess) => {
                    return Err(SetupError::permanent("SETUP_NO_HARNESS_ACCESS"))
                }
                Ok(HarnessSignIn::SignedOut) | Err(_) => {}
            }
            if tokio::time::Instant::now() + poll > deadline {
                if !flow_done {
                    flow.abort();
                }
                return Err(SetupError::of("SETUP_SIGN_IN_TIMEOUT"));
            }
            tokio::time::sleep(poll).await;
        }
    }

    async fn tokens(&self) -> CheckResult {
        match self.ports.identity.refresh_due().await {
            Ok(report) if !report.needs_sign_in.is_empty() => failed(
                CheckKind::Tokens,
                "CRED_SIGN_IN_REQUIRED",
                Some(PersonAction::SignInAgain),
            ),
            Ok(report) if !report.unreachable.is_empty() => {
                // Retried at the next interval.
                failed(
                    CheckKind::Tokens,
                    "CRED_PROVIDER_UNREACHABLE",
                    Some(PersonAction::TryAgain),
                )
            }
            Ok(report) if report.refreshed > 0 => {
                with_outcome(CheckKind::Tokens, CheckOutcome::Repaired)
            }
            Ok(_) => passed(CheckKind::Tokens),
            Err(e) => failed(
                CheckKind::Tokens,
                ordin_types::CodedError::code(&e),
                Some(PersonAction::TryAgain),
            ),
        }
    }

    async fn agent_repositories(&self) -> CheckResult {
        match self.ports.catalogue.sync_profile_repositories().await {
            Ok(report) => {
                if report
                    .results
                    .iter()
                    .any(|r| matches!(r.outcome, RepositorySyncOutcome::Failed { .. }))
                {
                    failed(
                        CheckKind::AgentRepositories,
                        "SETUP_CATALOGUE_FAILED",
                        Some(PersonAction::TryAgain),
                    )
                } else if report
                    .results
                    .iter()
                    .any(|r| r.outcome == RepositorySyncOutcome::Repaired)
                {
                    with_outcome(CheckKind::AgentRepositories, CheckOutcome::Repaired)
                } else {
                    passed(CheckKind::AgentRepositories)
                }
            }
            Err(()) => failed(
                CheckKind::AgentRepositories,
                "SETUP_CATALOGUE_FAILED",
                Some(PersonAction::TryAgain),
            ),
        }
    }

    async fn agent_folders(&self) -> CheckResult {
        match self.ports.packages.verify_agent_folders(true).await {
            Ok(report) if !report.failed.is_empty() => {
                failed(CheckKind::AgentFolders, "SETUP_AGENT_FOLDER_FAILED", None)
            }
            Ok(report) if report.repaired > 0 => {
                with_outcome(CheckKind::AgentFolders, CheckOutcome::Repaired)
            }
            Ok(_) => passed(CheckKind::AgentFolders),
            Err(()) => failed(CheckKind::AgentFolders, "SETUP_AGENT_FOLDER_FAILED", None),
        }
    }

    /// D.3 for one program.
    async fn program_run(&self, program: &BundledProgram) -> ProgramRun {
        let mut restored = false;
        if !intact(program) {
            // D.3.1: restore once from the pristine copy.
            let Some(source) = &program.restore_from else {
                return ProgramRun::Damaged;
            };
            if std::fs::copy(source, &program.path).is_err() || !intact(program) {
                return ProgramRun::Damaged;
            }
            restored = true;
            tracing::info!(target: TARGET, check = "programs", outcome = "repaired", "setup.repair");
        }
        let ran = process::run(&program.path, &program.version_args, None, COMMAND_TIMEOUT).await;
        match ran {
            Ran::Exited {
                code: Some(0),
                stdout,
                ..
            } => {
                let version = stdout
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .chars()
                    .take(64)
                    .collect::<String>();
                lock(&self.program_versions).insert(program.name.clone(), version);
                ProgramRun::Passed { restored }
            }
            Ran::Refused => {
                let sha256 = file_sha256(&program.path).unwrap_or(Sha256Hex(String::new()));
                tracing::warn!(
                    target: TARGET,
                    program = program.name.as_str(),
                    publisher = program.publisher.as_str(),
                    sha256 = sha256.0.as_str(),
                    "setup.program_refused"
                );
                ProgramRun::Refused(RefusedProgram {
                    name: program.name.clone(),
                    publisher: program.publisher.clone(),
                    sha256,
                })
            }
            Ran::NotFound => ProgramRun::Damaged,
            _ => ProgramRun::Failed,
        }
    }

    /// D.3 over every bundled program: one result; checks continue past a refusal (UX-A46).
    async fn programs(&self) -> CheckResult {
        let mut refused: Option<(RefusedProgram, String)> = None;
        let mut damaged: Option<String> = None;
        let mut failures: Option<String> = None;
        let mut restored = false;
        for program in &self.bundled {
            match self.program_run(program).await {
                ProgramRun::Passed { restored: r } => restored |= r,
                ProgramRun::Refused(p) => {
                    refused.get_or_insert((p, program.needed_for.clone()));
                }
                ProgramRun::Damaged => {
                    damaged.get_or_insert(program.name.clone());
                }
                ProgramRun::Failed => {
                    failures.get_or_insert(program.name.clone());
                }
            }
        }
        if let Some((program, off)) = refused {
            let name = program.name.clone();
            let mut r = failed(CheckKind::Programs, "SETUP_PROGRAM_REFUSED", None);
            r = arg(arg(r, "program", &name), "off", &off);
            r.refused_program = Some(program);
            return r;
        }
        if let Some(name) = damaged {
            return arg(
                failed(CheckKind::Programs, "SETUP_PROGRAM_DAMAGED", None),
                "program",
                &name,
            );
        }
        if let Some(name) = failures {
            return arg(
                failed(
                    CheckKind::Programs,
                    "SETUP_PROGRAM_FAILED",
                    Some(PersonAction::TryAgain),
                ),
                "program",
                &name,
            );
        }
        if restored {
            with_outcome(CheckKind::Programs, CheckOutcome::Repaired)
        } else {
            passed(CheckKind::Programs)
        }
    }

    async fn craft_apps(&self) -> CheckResult {
        let checks = match self.ports.craft.check_apps(true).await {
            Ok(c) => c,
            Err(()) => {
                return failed(
                    CheckKind::CraftApps,
                    "SETUP_CRAFT_APP_FAILED",
                    Some(PersonAction::TryAgain),
                )
            }
        };
        if let Some((app, publisher, sha256)) = checks.iter().find_map(|c| match &c.state {
            ProgramCheckState::Refused { publisher, sha256 } => {
                Some((c.app_name.clone(), publisher.clone(), sha256.clone()))
            }
            _ => None,
        }) {
            let mut r = arg(
                failed(CheckKind::CraftApps, "SETUP_PROGRAM_REFUSED", None),
                "program",
                &app,
            );
            r.refused_program = Some(RefusedProgram {
                name: app,
                publisher,
                sha256,
            });
            return r;
        }
        if let Some(app) = checks
            .iter()
            .find(|c| matches!(c.state, ProgramCheckState::Missing))
        {
            return arg(
                failed(
                    CheckKind::CraftApps,
                    "SETUP_CRAFT_APP_FAILED",
                    Some(PersonAction::TryAgain),
                ),
                "program",
                &app.app_name,
            );
        }
        if checks
            .iter()
            .any(|c| matches!(c.state, ProgramCheckState::Repaired))
        {
            with_outcome(CheckKind::CraftApps, CheckOutcome::Repaired)
        } else {
            passed(CheckKind::CraftApps)
        }
    }

    /// D.4 harness part: every listed harness with a probe; the first ready one decides, else the
    /// first checked. `only` limits it to one harness (D.2).
    async fn harness_checks(
        &self,
        only: Option<HarnessKind>,
    ) -> Result<(CheckResult, CheckResult, bool), SetupError> {
        let mut first: Option<HarnessCheck> = None;
        for (kind, tested) in self.listed_harnesses() {
            if only.is_some_and(|o| o != kind) {
                continue;
            }
            let Some(probe) = self.probe(kind) else {
                continue;
            };
            let mut check = self.check_harness(kind, &tested, &probe, None).await?;
            // D.4: the probe's repair (a stopped CLI server), once per run.
            if check.harness.outcome != CheckOutcome::Failed {
                match probe.repair().await {
                    Ok(ProbeRepair::Repaired) => {
                        check.harness.outcome = CheckOutcome::Repaired;
                        tracing::info!(target: TARGET, check = "harness", outcome = "repaired", "setup.repair");
                    }
                    Ok(ProbeRepair::Failed) | Err(_) => {
                        check.harness = failed(
                            CheckKind::Harness,
                            "SETUP_HARNESS_CHECK_FAILED",
                            Some(PersonAction::TryAgain),
                        );
                    }
                    Ok(ProbeRepair::NothingToDo) => {}
                }
            }
            if check.ready {
                let sign_in = check
                    .sign_in
                    .unwrap_or_else(|| passed(CheckKind::HarnessSignIn));
                return Ok((check.harness, sign_in, true));
            }
            if first.is_none() {
                first = Some(check);
            }
        }
        match first {
            Some(check) => {
                let sign_in = check.sign_in.unwrap_or_else(|| {
                    with_outcome(CheckKind::HarnessSignIn, CheckOutcome::Skipped)
                });
                Ok((check.harness, sign_in, false))
            }
            None => Ok((
                arg(
                    failed(CheckKind::Harness, "SETUP_HARNESS_MISSING", None),
                    "harness",
                    "Claude Code",
                ),
                with_outcome(CheckKind::HarnessSignIn, CheckOutcome::Skipped),
                false,
            )),
        }
    }

    /// D.2 and D.4.
    pub(crate) async fn checks(
        self: Arc<Self>,
        trigger: CheckTrigger,
    ) -> Result<SetupReport, SetupError> {
        let _run = self.run_lock.lock().await;
        let started = Timestamp::now();
        let mut results = Vec::new();
        let ready;
        if trigger == CheckTrigger::BeforeSession {
            let mut only = None;
            for (kind, _) in self.listed_harnesses() {
                if store::attachment(&self.pool, kind)
                    .await?
                    .is_some_and(|a| a.state == HarnessAttachmentState::Ready)
                {
                    only = Some(kind);
                    break;
                }
            }
            let (harness, sign_in, is_ready) = self.harness_checks(only).await?;
            results.push(harness);
            results.push(sign_in);
            ready = is_ready;
        } else {
            results.push(self.company_settings(false).await);
            let (harness, sign_in, is_ready) = self.harness_checks(None).await?;
            results.push(harness);
            results.push(sign_in);
            ready = is_ready;
            results.push(self.tokens().await);
            let snapshot = self.ports.profile.current();
            results.push(
                if snapshot.document.agent_catalogue.repositories.is_empty() {
                    with_outcome(CheckKind::AgentRepositories, CheckOutcome::Skipped)
                } else {
                    self.agent_repositories().await
                },
            );
            results.push(self.agent_folders().await);
            results.push(if self.bundled.is_empty() {
                with_outcome(CheckKind::Programs, CheckOutcome::Skipped)
            } else {
                self.programs().await
            });
            results.push(if snapshot.document.craft.apps.is_empty() {
                with_outcome(CheckKind::CraftApps, CheckOutcome::Skipped)
            } else {
                self.craft_apps().await
            });
            if trigger == CheckTrigger::Start {
                match self.program {
                    SetupProgram::PersonalServer => self.after_server_restart().await?,
                    SetupProgram::Desktop => {
                        if let Some(second) = self.second_install() {
                            results.push(second);
                        }
                    }
                }
                for missing in crate::tools::missing_path_tools(std::env::var_os("PATH").as_deref())
                {
                    tracing::info!(target: TARGET, tool = missing.command.as_str(), "setup.path_tool_missing");
                }
            }
        }
        for result in &results {
            store::put_check(&self.pool, result).await?;
            tracing::debug!(
                target: TARGET,
                check = result.check.as_str(),
                outcome = result.outcome.as_str(),
                code = result.code.as_deref().unwrap_or(""),
                trigger = ordin_types::log::wire_word(&trigger).as_str(),
                "setup.check"
            );
        }
        // A failed background check is a quiet status, not a pop-up (11 §6).
        let kinds: Vec<CheckKind> = results.iter().map(|r| r.check).collect();
        let mut book = Book::new(&kinds);
        for result in &results {
            book.end(result);
        }
        self.publish(&book, false);
        Ok(SetupReport {
            trigger: Some(trigger),
            results,
            ready,
            started_at: started,
            ended_at: Timestamp::now(),
        })
    }

    /// SET-UP-27: the other install on this account, named; nothing is changed.
    // spec: ordin-desktop/components/SetupAssistant#SET-UP-27
    fn second_install(&self) -> Option<CheckResult> {
        let updater = read(&self.updater).clone()?;
        Some(match updater.other_install() {
            Some(path) => {
                let folder = path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                arg(
                    failed(CheckKind::SecondInstall, "SETUP_SECOND_INSTALL", None),
                    "folder",
                    &folder,
                )
            }
            None => passed(CheckKind::SecondInstall),
        })
    }
}

/// SET-R1-5: why a harness check could not be done, so Diagnostics can say it. The person still sees
/// `SETUP_HARNESS_CHECK_FAILED`; the line carries the probe step and the kind of failure, never output.
fn probe_failed(kind: HarnessKind, step: &'static str, error: &ProbeError) {
    tracing::warn!(
        target: TARGET,
        harness = store::harness_text(kind),
        step,
        reason = format!("{error:?}").as_str(),
        code = "SETUP_HARNESS_CHECK_FAILED",
        "setup.harness_check_failed"
    );
}

#[cfg(test)]
mod log_tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Lines(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lines").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The text ordin's own log format writes while `f` runs.
    fn logged(f: impl FnOnce()) -> String {
        let lines = Lines::default();
        let writer = lines.clone();
        let subscriber = ordin_types::log::subscriber(move || writer.clone(), tracing::Level::INFO);
        tracing::subscriber::with_default(subscriber, f);
        let text = String::from_utf8(lines.0.lock().expect("lines").clone()).expect("utf-8");
        text
    }
    #[test]
    fn a_failed_probe_logs_the_kind_of_failure_as_text() {
        let text = logged(|| probe_failed(HarnessKind::ClaudeCode, "locate", &ProbeError::Timeout));
        assert!(text.contains(r#""reason":"Timeout""#), "{text}");
        assert!(!text.contains("<debug>"), "{text}");
    }
}
