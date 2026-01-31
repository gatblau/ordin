//! X2: one test per Gherkin scenario of SetupAssistant.md's main *Acceptance Criteria*, named after
//! it. The ADR-114 and ADR-072 scenarios are in `updates.rs`.

use std::sync::atomic::Ordering;
use std::time::Duration;

use ordin_organisation_profile::{
    HarnessName, LanguageServerAsset, LanguageServerEntry, ProfilePlatform, ProfileState,
    ServerLanguage,
};
use ordin_types::domain::identity::{HarnessAttachmentState, HarnessKind};
use ordin_types::num::Sha256Hex;
use ordin_types::{CodedError, CredentialId};
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::assistant::hex;
use crate::store;
use crate::types::{CheckKind, CheckOutcome, CheckTrigger, HarnessSignIn, PersonAction, StepState};

fn result(report: &crate::SetupReport, kind: CheckKind) -> &crate::CheckResult {
    report
        .results
        .iter()
        .find(|r| r.check == kind)
        .unwrap_or_else(|| panic!("no {kind:?} result"))
}

// Scenario: Happy path — first start, Claude Code already signed in
#[tokio::test]
async fn happy_path_first_start_claude_code_already_signed_in() {
    let script_dir = TempDir::new();
    let hook = script_dir.path().join("ordin-hook");
    write_script(&hook, "echo 'ordin-hook 0.1.0'");
    let env = Env::with(Options {
        bundled: vec![bundled(hook, "the ordin hook")],
        ..Options::default()
    })
    .await;
    env.profile
        .edit(|d| d.agent_catalogue.repositories = vec![repository("Contracts")]);
    let mut progress = env.setup.progress();
    let report = env.setup.run_guided().await.expect("guided");
    let seen = progress.borrow_and_update().clone();
    assert_eq!(seen.total, 4);
    assert!(!seen.steps.iter().any(|s| s.state == StepState::NeedsYou));
    assert_eq!(env.claude.sign_ins.load(Ordering::SeqCst), 0);
    let attachments = env.setup.harness_attachments().await.expect("attachments");
    assert_eq!(attachments[0].state, HarnessAttachmentState::Ready);
    assert!(report.ready);
    let (completed, digest) = store::setup_state(&env.pool)
        .await
        .expect("state")
        .expect("row");
    assert!(completed.is_some());
    assert_eq!(
        digest.as_deref(),
        Some(env.setup.inner.requirements_digest().as_str())
    );
    assert!(!env.setup.needs_guided_setup().await.expect("needs"));
}

// Scenario: Happy path — signed out, one-button sign-in
#[tokio::test]
async fn happy_path_signed_out_one_button_sign_in() {
    let env = Env::new().await;
    *lock(&env.claude.status) = Ok(HarnessSignIn::SignedOut);
    *lock(&env.claude.after_sign_in) = Some(HarnessSignIn::SignedIn { method: None });
    let mut progress = env.setup.progress();
    let guided = {
        let setup = &env.setup;
        async move { setup.run_guided().await }
    };
    let watcher = async {
        loop {
            progress.changed().await.expect("progress");
            let p = progress.borrow_and_update().clone();
            if let Some(step) = p.steps.iter().find(|s| s.check == CheckKind::HarnessSignIn) {
                if step.state == StepState::NeedsYou {
                    break;
                }
            }
        }
        let attachment = env
            .setup
            .sign_in_harness(HarnessKind::ClaudeCode)
            .await
            .expect("sign in");
        assert_eq!(attachment.state, HarnessAttachmentState::Ready);
    };
    let (report, ()) = tokio::join!(guided, watcher);
    let report = report.expect("guided");
    assert!(report.ready);
    assert_eq!(
        result(&report, CheckKind::HarnessSignIn).outcome,
        CheckOutcome::Passed
    );
}

// Scenario: Edge — profile changed what setup requires
#[tokio::test]
async fn edge_profile_changed_what_setup_requires() {
    let env = Env::new().await;
    env.setup.run_guided().await.expect("guided");
    assert!(!env.setup.needs_guided_setup().await.expect("needs"));
    env.profile.edit(|d| {
        d.harnesses = vec![
            harness(HarnessName::Copilot, &[]),
            harness(HarnessName::ClaudeCode, &[]),
        ]
    });
    assert!(env.setup.needs_guided_setup().await.expect("needs"));
}

// Scenario: Edge — untested harness version
#[tokio::test]
async fn edge_untested_harness_version() {
    let env = Env::new().await;
    env.profile
        .edit(|d| d.harnesses = vec![harness(HarnessName::ClaudeCode, &["2.1.302"])]);
    *lock(&env.claude.location) = Ok(Some(crate::types::HarnessLocation {
        path: super::CLAUDE_AT.into(),
        version: "2.1.310".to_owned(),
    }));
    let report = env.setup.run_guided().await.expect("guided");
    let attachment = &env.setup.harness_attachments().await.expect("a")[0];
    assert_eq!(attachment.state, HarnessAttachmentState::NeedsUpdate);
    let r = result(&report, CheckKind::Harness);
    assert_eq!(r.code.as_deref(), Some("SETUP_HARNESS_UNTESTED"));
    assert_eq!(r.action, None);
}

// Scenario: Error — harness missing
#[tokio::test]
async fn error_harness_missing() {
    let env = Env::new().await;
    *lock(&env.claude.location) = Ok(None);
    let report = env.setup.run_guided().await.expect("guided");
    let r = result(&report, CheckKind::Harness);
    assert_eq!(r.code.as_deref(), Some("SETUP_HARNESS_MISSING"));
    // The sentence "Claude Code needs to be installed by IT on this computer" is the message of
    // this id, with the harness named.
    assert_eq!(r.message_id.as_deref(), Some("setup-harness-missing"));
    assert_eq!(r.message_args, args(&[("harness", "Claude Code")]));
    assert!(!report.ready);
}

// Scenario: Error — program stopped by company allow-listing
#[cfg(unix)]
#[tokio::test]
async fn error_program_stopped_by_company_allow_listing() {
    let scripts = TempDir::new();
    let photo = scripts.path().join("photocraft");
    write_script(&photo, "kill -9 $$");
    let mut program = bundled(photo.clone(), "PhotoCraft");
    program.publisher = "ArtCraft".to_owned();
    program.needed_for = "images open in your usual app".to_owned();
    let env = Env::with(Options {
        bundled: vec![program],
        ..Options::default()
    })
    .await;
    let report = env
        .setup
        .run_checks(CheckTrigger::OnDemand)
        .await
        .expect("checks");
    let r = result(&report, CheckKind::Programs);
    assert_eq!(r.code.as_deref(), Some("SETUP_PROGRAM_REFUSED"));
    let refused = r.refused_program.as_ref().expect("refused program");
    assert_eq!(refused.name, "PhotoCraft");
    assert_eq!(refused.publisher, "ArtCraft");
    let expected = hex(&Sha256::digest(std::fs::read(&photo).expect("read")));
    assert_eq!(refused.sha256.0, expected);
    // The other checks still ran.
    assert_eq!(
        result(&report, CheckKind::Tokens).outcome,
        CheckOutcome::Passed
    );
    assert_eq!(
        result(&report, CheckKind::AgentFolders).outcome,
        CheckOutcome::Passed
    );
}

// Scenario: Edge — damaged bundled program restored once
// Unix only: the bundled program is a shell script, run to read its version.
#[cfg(unix)]
#[tokio::test]
async fn edge_damaged_bundled_program_restored_once() {
    let scripts = TempDir::new();
    let good = scripts.path().join("pristine-hook");
    write_script(&good, "echo 'ordin-hook 0.1.0'");
    let hook = scripts.path().join("ordin-hook");
    write_script(&hook, "echo damaged");
    let mut program = bundled(hook.clone(), "the ordin hook");
    program.expected_sha256 = Some(Sha256Hex(hex(&Sha256::digest(
        std::fs::read(&good).expect("read"),
    ))));
    program.restore_from = Some(good.clone());
    let env = Env::with(Options {
        bundled: vec![program],
        ..Options::default()
    })
    .await;
    let report = env
        .setup
        .run_checks(CheckTrigger::Start)
        .await
        .expect("checks");
    assert_eq!(
        result(&report, CheckKind::Programs).outcome,
        CheckOutcome::Repaired
    );
    assert_eq!(
        std::fs::read(&hook).expect("restored"),
        std::fs::read(&good).expect("good")
    );
}

// Scenario: Error — profile unreachable on first start
#[tokio::test]
async fn error_profile_unreachable_on_first_start() {
    let env = Env::new().await;
    *lock(&env.profile.status) = ProfileState::NeverFetched;
    *lock(&env.profile.refresh) = Err(());
    let report = env.setup.run_guided().await.expect("guided");
    let r = result(&report, CheckKind::CompanySettings);
    assert_eq!(r.code.as_deref(), Some("SETUP_PROFILE_UNREACHABLE"));
    assert_eq!(r.action, None);
    // The sequence continued with the defaults.
    assert_eq!(
        result(&report, CheckKind::Harness).outcome,
        CheckOutcome::Passed
    );
    assert!(report.ready);
}

// Scenario: Edge — background token refresh
#[tokio::test]
async fn edge_background_token_refresh() {
    let env = Env::new().await;
    *lock(&env.identity.report) = Ok(ordin_identity::RefreshReport {
        refreshed: 1,
        needs_sign_in: Vec::new(),
        unreachable: Vec::new(),
    });
    let report = env
        .setup
        .run_checks(CheckTrigger::Background)
        .await
        .expect("checks");
    assert_eq!(env.identity.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(
        result(&report, CheckKind::Tokens).outcome,
        CheckOutcome::Repaired
    );
}

// Scenario: Error — refresh refused needs sign-in
#[tokio::test]
async fn error_refresh_refused_needs_sign_in() {
    let env = Env::new().await;
    *lock(&env.identity.report) = Ok(ordin_identity::RefreshReport {
        refreshed: 0,
        needs_sign_in: vec![CredentialId::new()],
        unreachable: Vec::new(),
    });
    let report = env
        .setup
        .run_checks(CheckTrigger::Background)
        .await
        .expect("checks");
    let r = result(&report, CheckKind::Tokens);
    assert_eq!(r.code.as_deref(), Some("CRED_SIGN_IN_REQUIRED"));
    assert_eq!(r.action, Some(PersonAction::SignInAgain));
}

// Scenario: Edge — server harness sign-in
#[tokio::test]
async fn edge_server_harness_sign_in() {
    let env = Env::with(Options {
        program: SetupProgram::PersonalServer,
        ..Options::default()
    })
    .await;
    *lock(&env.claude.status) = Ok(HarnessSignIn::SignedOut);
    let report = env
        .setup
        .run_checks(CheckTrigger::Background)
        .await
        .expect("checks");
    let attachment = &env.setup.harness_attachments().await.expect("a")[0];
    assert_eq!(attachment.state, HarnessAttachmentState::NeedsSignIn);
    assert_eq!(
        result(&report, CheckKind::HarnessSignIn).code.as_deref(),
        Some("SETUP_SIGN_IN_ON_SERVER")
    );
    assert_eq!(env.claude.sign_ins.load(Ordering::SeqCst), 0);
}

// Scenario: Happy path — the desktop finds and installs an update
// (replaced by the ADR-114 amendment's scenarios; kept as the amendment states: the offer is found
// on the timer and `install_update` leaves the restart to the program).
#[tokio::test]
async fn happy_path_the_desktop_finds_and_installs_an_update() {
    let env = Env::with(Options {
        version: "1.3.2",
        ..Options::default()
    })
    .await;
    env.profile
        .edit(|d| d.updates = Some(feed("https://it.example.com/ordin/updates/latest.json")));
    let updater = FakeUpdater::offering("1.4.0");
    env.setup.register_updater(updater.clone());
    env.setup.start_background_checks().await;
    let mut offer = None;
    for _ in 0..200 {
        offer = env
            .setup
            .update_offer()
            .filter(|o| o.state == crate::UpdateState::Ready);
        if offer.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(offer.expect("an offer").version, "1.4.0");
    env.setup.install_update().await.expect("install");
    assert_eq!(updater.restarts.load(Ordering::SeqCst), 0, "not restarted");
    env.setup.close().await;
}

// Scenario: Error — an update with a bad signature is not offered
#[tokio::test]
async fn error_an_update_with_a_bad_signature_is_not_offered() {
    let env = Env::new().await;
    env.profile
        .edit(|d| d.updates = Some(feed("https://it.example.com/latest.json")));
    let updater = FakeUpdater::offering("0.2.0");
    *lock(&updater.offer) = Err(crate::UpdaterError::SignatureInvalid);
    env.setup.register_updater(updater);
    env.setup.inner.clone().timer_update_check().await;
    assert_eq!(env.setup.update_offer(), None);
}

// Scenario: Happy path — the server reads the feed once
#[tokio::test]
async fn happy_path_the_server_reads_the_feed_once() {
    let env = Env::with(Options {
        program: SetupProgram::PersonalServer,
        version: "1.3.2",
        ..Options::default()
    })
    .await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "version": "1.4.0", "notes": "Faster search", "platforms": {}
        })))
        .expect(1)
        .mount(&server)
        .await;
    env.profile
        .edit(|d| d.updates = Some(feed(&format!("{}/latest.json", server.uri()))));
    assert_eq!(
        env.setup
            .check_update_feed()
            .await
            .expect("feed")
            .as_deref(),
        Some("1.4.0")
    );
    assert!(std::fs::read_dir(env.dir.path()).expect("dir").all(|e| !e
        .expect("entry")
        .file_name()
        .to_string_lossy()
        .starts_with(".update")));
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn language_entry(
    language: ServerLanguage,
    name: &str,
    version: &str,
    platform: ProfilePlatform,
    url: &str,
    sha: &str,
    file_name: &str,
    binary: &str,
) -> LanguageServerEntry {
    LanguageServerEntry {
        language,
        name: name.to_owned(),
        version: version.to_owned(),
        platforms: [(
            platform,
            LanguageServerAsset {
                asset_url: url.to_owned(),
                sha256: Sha256Hex(sha.to_owned()),
                file_name: file_name.to_owned(),
                binary_path: binary.to_owned(),
            },
        )]
        .into_iter()
        .collect(),
    }
}

// Scenario: Error — a language server the profile did not approve
#[tokio::test]
async fn error_a_language_server_the_profile_did_not_approve() {
    let env = Env::new().await;
    env.profile.edit(|d| {
        d.language_servers = vec![language_entry(
            ServerLanguage::Go,
            "gopls",
            "0.20.0",
            ProfilePlatform::LinuxX86_64,
            "https://dl.example/gopls.tar.gz",
            &"0".repeat(64),
            "gopls.tar.gz",
            "gopls",
        )]
    });
    let error = env
        .setup
        .ensure_language_server("go", "gopls", "0.19.1")
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "SETUP_LANGUAGE_SERVER_NOT_APPROVED");
}

fn gz(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).expect("gzip");
    encoder.finish().expect("gzip")
}

// Scenario: Happy — a language server from the build's table is installed once
#[tokio::test]
async fn happy_a_language_server_from_the_builds_table_is_installed_once() {
    let env = Env::new().await;
    let server = MockServer::start().await;
    let download = gz(b"#!/bin/sh\necho rust-analyzer\n");
    Mock::given(method("GET"))
        .and(path("/rust-analyzer-x86_64-unknown-linux-gnu.gz"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(download.clone()))
        .expect(1)
        .mount(&server)
        .await;
    {
        let mut tuning = env.tuning();
        tuning.platform = Some("linux_x86_64");
        tuning.language_table = vec![language_entry(
            ServerLanguage::Rust,
            "rust-analyzer",
            "2026-01-10",
            ProfilePlatform::LinuxX86_64,
            &format!("{}/rust-analyzer-x86_64-unknown-linux-gnu.gz", server.uri()),
            &hex(&Sha256::digest(&download)),
            "rust-analyzer-x86_64-unknown-linux-gnu.gz",
            "rust-analyzer-x86_64-unknown-linux-gnu",
        )];
    }
    let launch = env
        .setup
        .ensure_language_server("rust", "rust-analyzer", "2026-01-10")
        .await
        .expect("installed");
    let folder = env
        .dir
        .path()
        .join("language-servers")
        .join("rust-analyzer")
        .join("2026-01-10");
    assert_eq!(
        launch.program,
        folder.join("rust-analyzer-x86_64-unknown-linux-gnu")
    );
    assert!(launch.args.is_empty());
    assert!(folder.join("ready").is_file());
    assert!(launch.program.is_file());
    let again = env
        .setup
        .ensure_language_server("rust", "rust-analyzer", "2026-01-10")
        .await
        .expect("again");
    assert_eq!(again, launch);
}

// Scenario: Error — a download whose digest differs
#[tokio::test]
async fn error_a_download_whose_digest_differs() {
    let env = Env::new().await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not gopls".to_vec()))
        .mount(&server)
        .await;
    env.tuning().platform = Some("macos_arm64");
    env.profile.edit(|d| {
        d.language_servers = vec![language_entry(
            ServerLanguage::Go,
            "gopls",
            "0.20.0",
            ProfilePlatform::MacosArm64,
            &format!("{}/gopls", server.uri()),
            &"a".repeat(64),
            "gopls",
            "gopls",
        )]
    });
    let error = env
        .setup
        .ensure_language_server("go", "gopls", "0.20.0")
        .await
        .expect_err("mismatch");
    assert_eq!(error.code(), "SETUP_LANGUAGE_SERVER_DIGEST_MISMATCH");
    let base = env.dir.path().join("language-servers").join("gopls");
    assert!(!base.join("0.20.0").exists());
    assert_eq!(std::fs::read_dir(&base).map(|d| d.count()).unwrap_or(0), 0);
}

// Scenario: Edge — no download for this platform
#[tokio::test]
async fn edge_no_download_for_this_platform() {
    let env = Env::new().await;
    env.tuning().platform = Some("windows_x86_64");
    env.profile.edit(|d| {
        d.language_servers = vec![language_entry(
            ServerLanguage::Go,
            "gopls",
            "0.20.0",
            ProfilePlatform::LinuxX86_64,
            "https://dl.example/gopls",
            &"0".repeat(64),
            "gopls",
            "gopls",
        )]
    });
    let error = env
        .setup
        .ensure_language_server("go", "gopls", "0.20.0")
        .await
        .expect_err("unsupported");
    assert_eq!(error.code(), "SETUP_LANGUAGE_SERVER_PLATFORM_UNSUPPORTED");
}

// Scenario: Edge — background checks start once
#[tokio::test]
async fn edge_background_checks_start_once() {
    let env = Env::new().await;
    env.setup.start_background_checks().await;
    env.setup.start_background_checks().await;
    assert_eq!(env.setup.inner.timers_started.load(Ordering::SeqCst), 1);
    env.setup.close().await;
}

// Scenario: Edge — background checks are not started after close (SET-SC-1)
#[tokio::test]
async fn edge_background_checks_are_not_started_after_close() {
    let env = Env::new().await;
    env.setup.close().await;
    env.setup.start_background_checks().await;
    assert_eq!(env.setup.inner.timers_started.load(Ordering::SeqCst), 0);
    assert!(env.setup.inner.background.lock().unwrap().is_none());
}

// Scenario: Happy path — the program reads the build's table (SET-REC-1)
#[test]
fn happy_path_the_program_reads_the_builds_table() {
    let entry = language_entry(
        ServerLanguage::Rust,
        "rust-analyzer",
        "2026-01-10",
        ProfilePlatform::LinuxX86_64,
        "https://example.invalid/rust-analyzer.gz",
        &"a".repeat(64),
        "rust-analyzer.gz",
        "rust-analyzer",
    );
    let json = serde_json::to_string(&vec![entry.clone()]).unwrap();
    assert_eq!(crate::assistant::parse_language_servers(&json), vec![entry]);
}

// Scenario: Edge case — the table in the repository is empty (SET-REC-1)
#[test]
fn edge_case_the_table_in_the_repository_is_empty() {
    assert_eq!(include_str!("../language-servers.json").trim(), "[]");
    assert!(crate::bundled_language_servers().is_empty());
}

// Scenario: Edge case — a failed step says why while the run goes on (SET-R1-3)
#[tokio::test]
async fn edge_case_a_failed_step_says_why() {
    let env = Env::new().await;
    // A profile address that cannot be reached (SET-R1-6 skips a missing address instead).
    *lock(&env.profile.status) = ProfileState::NeverFetched;
    *lock(&env.profile.refresh) = Err(());
    let progress = env.setup.progress();
    env.setup.run_guided().await.expect("guided");
    let seen = progress.borrow().clone();
    let company = seen
        .steps
        .iter()
        .find(|s| s.check == CheckKind::CompanySettings)
        .expect("company settings step");
    assert_eq!(company.state, StepState::Failed);
    let problem = company.problem.as_ref().expect("problem");
    assert_eq!(problem.message_id, "setup-profile-unreachable");
    assert!(seen
        .steps
        .iter()
        .filter(|s| s.state != StepState::Failed)
        .all(|s| s.problem.is_none()));
}
