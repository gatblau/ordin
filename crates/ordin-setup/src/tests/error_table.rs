//! X1: one test per row of SetupAssistant.md's *Error Table* and of the ADR-114 amendment's added
//! and changed rows. The first test checks every row's status (retryable or permanent) and its
//! response-body action; the rest bring about each row's condition. Rows whose condition an
//! acceptance scenario already brings about name that scenario instead of repeating it.

use std::sync::atomic::Ordering;

use ordin_craft_toolkit::{CraftProgramCheck, ProgramCheckState};
use ordin_organisation_profile::{CraftApp, ProfilePlatform, ProfileState, ServerLanguage};
use ordin_package_catalogue::{AgentFolderFailure, AgentFolderReport};
use ordin_types::domain::identity::{HarnessAttachment, HarnessAttachmentState};
use ordin_types::{CodedError, HarnessAttachmentId, RepairAction};
use sha2::{Digest, Sha256};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::acceptance::language_entry;
use super::updates::{archive, Hook, Server};
use super::*;
use crate::assistant::hex;
use crate::error::repair_of;
use crate::store;
use crate::types::{CheckKind, CheckOutcome, CheckTrigger, HarnessSignIn, ProbeError};

fn result(report: &crate::SetupReport, kind: CheckKind) -> crate::CheckResult {
    report
        .results
        .iter()
        .find(|r| r.check == kind)
        .cloned()
        .unwrap_or_else(|| panic!("no {kind:?} result"))
}

async fn on_demand(env: &Env) -> crate::SetupReport {
    env.setup
        .run_checks(CheckTrigger::OnDemand)
        .await
        .expect("checks")
}

/// (code, Err(retryable), the response body's `action`), one entry per code of both tables.
const ROWS: &[(&str, bool, Option<RepairAction>)] = &[
    ("SETUP_NO_PROFILE_ADDRESS", false, None),
    ("SETUP_STORE_UNSUPPORTED", false, None),
    (
        "SETUP_UPDATE_UNREACHABLE",
        true,
        Some(RepairAction::TryAgain),
    ),
    ("SETUP_UPDATE_SIGNATURE_INVALID", false, None),
    ("SETUP_UPDATE_FEED_INVALID", false, None),
    ("SETUP_UPDATE_NONE", false, None),
    (
        "SETUP_UPDATE_INSTALL_FAILED",
        true,
        Some(RepairAction::TryAgain),
    ),
    ("SETUP_LANGUAGE_SERVER_NOT_APPROVED", false, None),
    (
        "SETUP_LANGUAGE_SERVER_INSTALL_FAILED",
        true,
        Some(RepairAction::TryAgain),
    ),
    ("SETUP_LANGUAGE_SERVER_DIGEST_MISMATCH", false, None),
    ("SETUP_LANGUAGE_SERVER_PLATFORM_UNSUPPORTED", false, None),
    ("SETUP_PROFILE_UNREACHABLE", true, None),
    ("SETUP_HARNESS_MISSING", false, None),
    ("SETUP_HARNESS_UNTESTED", false, None),
    ("SETUP_NO_HARNESS_ACCESS", false, None),
    ("SETUP_SIGN_IN_TIMEOUT", true, Some(RepairAction::TryAgain)),
    ("SETUP_SIGN_IN_ON_SERVER", false, None),
    ("SETUP_HARNESS_UNKNOWN", false, None),
    (
        "SETUP_NO_READY_HARNESS",
        false,
        Some(RepairAction::SignInAgain),
    ),
    (
        "SETUP_HARNESS_CHECK_FAILED",
        true,
        Some(RepairAction::TryAgain),
    ),
    ("SETUP_CATALOGUE_FAILED", true, Some(RepairAction::TryAgain)),
    ("SETUP_AGENT_FOLDER_FAILED", false, None),
    ("SETUP_PROGRAM_DAMAGED", false, None),
    ("SETUP_PROGRAM_REFUSED", false, None),
    ("SETUP_PROGRAM_FAILED", true, Some(RepairAction::TryAgain)),
    ("SETUP_CRAFT_APP_FAILED", true, Some(RepairAction::TryAgain)),
    ("SETUP_STORE_FAILED", true, Some(RepairAction::TryAgain)),
    ("HARN_DUPLICATE", false, None),
    ("HARN_PATH_INVALID", false, None),
    ("HARN_VERSION_INVALID", false, None),
    ("HARN_SIGN_IN_METHOD_INVALID", false, None),
    ("HARN_STATE_INCONSISTENT", false, None),
    // ADR-114 amendment rows.
    ("SETUP_SELF_UPDATE_UNAVAILABLE", false, None),
    ("SETUP_UPDATE_NO_ARCHIVE", false, None),
    (
        "SETUP_UPDATE_REPLACE_FAILED",
        true,
        Some(RepairAction::TryAgain),
    ),
    ("SETUP_UPDATE_IN_PROGRESS", false, None),
    ("SETUP_UPDATE_VERSION_CHANGED", false, None),
    // Carried by a failed check result, never returned as an error.
    ("SETUP_SECOND_INSTALL", false, None),
];

#[test]
fn every_row_has_its_status_and_action() {
    let mut codes: Vec<&str> = ROWS.iter().map(|(c, _, _)| *c).collect();
    codes.sort_unstable();
    assert_eq!(
        codes,
        crate::ALL_CODES,
        "the rows name exactly the exported codes"
    );
    for (code, retryable, action) in ROWS {
        let error = SetupError::of(code);
        assert_eq!(error.code(), *code);
        assert_eq!(error.retryable(), *retryable, "{code}");
        assert_eq!(error.repair(), *action, "{code}");
        assert_eq!(repair_of(code), *action, "{code}");
        let entry = ordin_types::error_registry::lookup(code).expect("registered");
        assert_eq!(
            entry.retryable, *retryable,
            "{code}: the registry agrees with the table"
        );
    }
    // The permanent form of the language-server row sends to IT.
    let permanent = SetupError::permanent("SETUP_LANGUAGE_SERVER_INSTALL_FAILED");
    assert_eq!(permanent.repair(), None);
    // Identity's row keeps Identity's code and flag.
    assert_eq!(
        repair_of("CRED_SIGN_IN_REQUIRED"),
        Some(RepairAction::SignInAgain)
    );
}

// Row: Installer carried no profile address — SET-R1-6: the step is skipped, with no code and no IT action.
// spec: ordin-desktop/components/SetupAssistant#SET-R1-6
#[tokio::test]
async fn row_installer_carried_no_profile_address() {
    let env = Env::new().await;
    *lock(&env.profile.status) = ProfileState::NoAddress;
    let report = on_demand(&env).await;
    let r = result(&report, CheckKind::CompanySettings);
    assert_eq!(r.outcome, CheckOutcome::Skipped);
    assert_eq!(r.code, None);
    assert_eq!(r.action, None);
    let stored = store::check(&env.pool, CheckKind::CompanySettings)
        .await
        .expect("read")
        .expect("stored");
    assert_eq!(stored.code, None);
}

// Row: `new` given a PostgreSQL datastore
#[tokio::test]
async fn row_new_given_a_postgresql_datastore() {
    let env = Env::new().await;
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://ordin@127.0.0.1:1/ordin")
        .expect("lazy pool");
    let ports = Ports {
        profile: env.profile.clone(),
        identity: env.identity.clone(),
        catalogue: env.catalogue.clone(),
        packages: env.packages.clone(),
        craft: env.craft.clone(),
    };
    let refused = SetupAssistant::with_ports(
        ports,
        SetupConfig {
            program: SetupProgram::Desktop,
            bundled_programs: Vec::new(),
            application_version: "0.1.0".to_owned(),
            http: reqwest::Client::new(),
        },
        Datastore::Postgres(pool),
        short_tuning(),
    );
    let error = refused.err().expect("refused");
    assert_eq!(error.code(), "SETUP_STORE_UNSUPPORTED");
    assert!(!error.retryable());
}

// Rows: Update feed unreachable, update signature refused, `install_update` with no offer,
// installing the update failed — brought about by `error_the_persons_check_cannot_reach_the_feed`,
// `error_a_download_with_a_bad_signature_is_dropped`,
// `error_restart_to_update_before_the_download_is_ready` and `error_the_install_fails` in
// `tests::updates`.

// Row (amendment): Signed archive lacks a program
#[tokio::test]
async fn row_signed_archive_lacks_a_program() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    s.offer("0.5.0", archive("0.5.0", Hook::Missing), None, None)
        .await;
    let error = s.env.setup.run_server_update().await.expect_err("refused");
    assert_eq!(error.code(), "SETUP_UPDATE_FEED_INVALID");
    assert!(!error.retryable());
    assert_eq!(s.file("ordin-server"), "old server");
    assert_eq!(s.file("ordin-hook"), "old hook");
}

// Row: Language server install failed (download)
#[tokio::test]
async fn row_language_server_download_failed_is_retryable() {
    let env = Env::new().await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    env.tuning().platform = Some("linux_x86_64");
    env.profile.edit(|d| {
        d.language_servers = vec![language_entry(
            ServerLanguage::Go,
            "gopls",
            "0.20.0",
            ProfilePlatform::LinuxX86_64,
            &format!("{}/gopls", server.uri()),
            &"0".repeat(64),
            "gopls",
            "gopls",
        )]
    });
    let error = env
        .setup
        .ensure_language_server("go", "gopls", "0.20.0")
        .await
        .expect_err("failed");
    assert_eq!(error.code(), "SETUP_LANGUAGE_SERVER_INSTALL_FAILED");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
    assert!(!env
        .dir
        .path()
        .join("language-servers")
        .join("gopls")
        .join("0.20.0")
        .exists());
}

// Row: Language server install failed (archive lacking `binary_path`)
#[tokio::test]
async fn row_language_server_archive_lacking_binary_is_permanent() {
    let env = Env::new().await;
    let server = MockServer::start().await;
    let download = {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(b"#!/bin/sh\necho gopls\n").expect("gzip");
        gz.finish().expect("gzip")
    };
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(download.clone()))
        .mount(&server)
        .await;
    env.tuning().platform = Some("linux_x86_64");
    env.profile.edit(|d| {
        d.language_servers = vec![language_entry(
            ServerLanguage::Go,
            "gopls",
            "0.20.0",
            ProfilePlatform::LinuxX86_64,
            &format!("{}/gopls.gz", server.uri()),
            &hex(&Sha256::digest(&download)),
            "gopls.gz",
            "bin/not-gopls",
        )]
    });
    let error = env
        .setup
        .ensure_language_server("go", "gopls", "0.20.0")
        .await
        .expect_err("failed");
    assert_eq!(error.code(), "SETUP_LANGUAGE_SERVER_INSTALL_FAILED");
    assert!(!error.retryable());
    assert_eq!(error.repair(), None);
}

// Rows: not approved, digest differs, no download for this platform — brought about by the
// acceptance scenarios of the same names. Profile cannot be fetched or verified: by
// `error_profile_unreachable_on_first_start` in `acceptance`.

// Row: No listed harness found
#[tokio::test]
async fn row_no_listed_harness_found() {
    let env = Env::new().await;
    *lock(&env.claude.location) = Ok(None);
    let report = on_demand(&env).await;
    let r = result(&report, CheckKind::Harness);
    assert_eq!(r.code.as_deref(), Some("SETUP_HARNESS_MISSING"));
    assert_eq!(r.action, None);
    assert!(!report.ready);
}

// Row: Harness version not in `tested_versions`
#[tokio::test]
async fn row_harness_version_not_tested() {
    let env = Env::new().await;
    env.profile
        .edit(|d| d.harnesses = vec![harness(HarnessName::ClaudeCode, &["2.0.0"])]);
    let report = on_demand(&env).await;
    let r = result(&report, CheckKind::Harness);
    assert_eq!(r.code.as_deref(), Some("SETUP_HARNESS_UNTESTED"));
    let attachments = env.setup.harness_attachments().await.expect("attachments");
    assert_eq!(attachments[0].state, HarnessAttachmentState::NeedsUpdate);
}

// Row: Harness reports no access / no seat
#[tokio::test]
async fn row_harness_reports_no_access() {
    let env = Env::new().await;
    *lock(&env.claude.status) = Ok(HarnessSignIn::NoAccess);
    let report = on_demand(&env).await;
    let r = result(&report, CheckKind::HarnessSignIn);
    assert_eq!(r.code.as_deref(), Some("SETUP_NO_HARNESS_ACCESS"));
    assert_eq!(
        r.message_args.get("harness").map(String::as_str),
        Some("Claude Code")
    );
    let error = env
        .setup
        .sign_in_harness(HarnessKind::ClaudeCode)
        .await
        .expect_err("no access");
    assert_eq!(error.code(), "SETUP_NO_HARNESS_ACCESS");
}

// Row: Harness sign-in not finished in 10 min (the test's wait is 800 ms)
#[tokio::test]
async fn row_harness_sign_in_not_finished() {
    let env = Env::new().await;
    *lock(&env.claude.status) = Ok(HarnessSignIn::SignedOut);
    let error = env
        .setup
        .sign_in_harness(HarnessKind::ClaudeCode)
        .await
        .expect_err("timed out");
    assert_eq!(error.code(), "SETUP_SIGN_IN_TIMEOUT");
    assert!(error.retryable());
    assert_eq!(env.claude.sign_ins.load(Ordering::SeqCst), 1);
}

// Row: Harness sign-in asked on the server — `edge_server_harness_sign_in` in `acceptance`.

// Row: No probe registered for the harness
#[tokio::test]
async fn row_no_probe_registered_for_the_harness() {
    let env = Env::new().await;
    let error = env
        .setup
        .sign_in_harness(HarnessKind::CopilotCli)
        .await
        .expect_err("unknown");
    assert_eq!(error.code(), "SETUP_HARNESS_UNKNOWN");
    assert!(!error.retryable());
}

// Row: No ready harness for a session
#[tokio::test]
async fn row_no_ready_harness_for_a_session() {
    let env = Env::new().await;
    *lock(&env.claude.status) = Ok(HarnessSignIn::SignedOut);
    let error = env.setup.ready_harness().await.expect_err("none ready");
    assert_eq!(error.code(), "SETUP_NO_READY_HARNESS");
    assert_eq!(error.repair(), Some(RepairAction::SignInAgain));
    let report = on_demand(&env).await;
    assert_eq!(
        result(&report, CheckKind::HarnessSignIn).code.as_deref(),
        Some("SETUP_NO_READY_HARNESS")
    );
}

// Row: Harness output unreadable / command timed out
#[tokio::test]
async fn row_harness_output_unreadable_or_timed_out() {
    let env = Env::new().await;
    for failure in [ProbeError::Timeout, ProbeError::Unreadable] {
        *lock(&env.claude.location) = Err(failure);
        let report = on_demand(&env).await;
        let r = result(&report, CheckKind::Harness);
        assert_eq!(r.code.as_deref(), Some("SETUP_HARNESS_CHECK_FAILED"));
        assert_eq!(r.action, Some(crate::PersonAction::TryAgain));
    }
    *lock(&env.claude.location) = Err(ProbeError::Timeout);
    let error = env
        .setup
        .sign_in_harness(HarnessKind::ClaudeCode)
        .await
        .expect_err("failed");
    assert_eq!(error.code(), "SETUP_HARNESS_CHECK_FAILED");
    assert!(error.retryable());
}

// Row: Agent repository clone or fetch failed
#[tokio::test]
async fn row_agent_repository_clone_or_fetch_failed() {
    let env = Env::new().await;
    env.profile
        .edit(|d| d.agent_catalogue.repositories = vec![repository("Contracts")]);
    *lock(&env.catalogue.0) = Ok(synced(&[RepositorySyncOutcome::Failed {
        code: "CAT_REPO_UNREACHABLE".to_owned(),
    }]));
    let report = on_demand(&env).await;
    assert_eq!(
        result(&report, CheckKind::AgentRepositories)
            .code
            .as_deref(),
        Some("SETUP_CATALOGUE_FAILED")
    );
    *lock(&env.catalogue.0) = Err(());
    let report = on_demand(&env).await;
    let r = result(&report, CheckKind::AgentRepositories);
    assert_eq!(r.code.as_deref(), Some("SETUP_CATALOGUE_FAILED"));
    assert_eq!(r.action, Some(crate::PersonAction::TryAgain));
}

// Row: Agent folder could not be re-installed
#[tokio::test]
async fn row_agent_folder_could_not_be_reinstalled() {
    let env = Env::new().await;
    *lock(&env.packages.0) = Ok(AgentFolderReport {
        checked: 1,
        repaired: 0,
        failed: vec![AgentFolderFailure {
            agent_name: "Reviewer".to_owned(),
            code: "PKG_FOLDER_UNUSABLE".to_owned(),
        }],
    });
    let report = on_demand(&env).await;
    let r = result(&report, CheckKind::AgentFolders);
    assert_eq!(r.code.as_deref(), Some("SETUP_AGENT_FOLDER_FAILED"));
    assert_eq!(r.action, None);
    *lock(&env.packages.0) = Err(());
    let report = on_demand(&env).await;
    assert_eq!(
        result(&report, CheckKind::AgentFolders).code.as_deref(),
        Some("SETUP_AGENT_FOLDER_FAILED")
    );
}

// Row: Bundled program missing or damaged, restore failed
#[tokio::test]
async fn row_bundled_program_missing_restore_failed() {
    let scripts = TempDir::new();
    let env = Env::with(Options {
        bundled: vec![bundled(scripts.path().join("ordin-hook"), "the ordin hook")],
        ..Options::default()
    })
    .await;
    let report = on_demand(&env).await;
    let r = result(&report, CheckKind::Programs);
    assert_eq!(r.code.as_deref(), Some("SETUP_PROGRAM_DAMAGED"));
    assert_eq!(
        r.message_args.get("program").map(String::as_str),
        Some("the ordin hook")
    );
}

// Row: Program refused by the operating system — `error_program_stopped_by_company_allow_listing`
// in `acceptance`.

// Row: Program exited non-zero or timed out
#[tokio::test]
async fn row_program_exited_non_zero() {
    let first = Env::new().await;
    let program = first.script("ordin-hook", "exit 3");
    let env = Env::on(
        first.dir,
        first.store,
        Options {
            bundled: vec![bundled(program, "the ordin hook")],
            ..Options::default()
        },
    )
    .await;
    let report = on_demand(&env).await;
    let r = result(&report, CheckKind::Programs);
    assert_eq!(r.code.as_deref(), Some("SETUP_PROGRAM_FAILED"));
    assert_eq!(r.action, Some(crate::PersonAction::TryAgain));
}

// Row: ArtCraft app missing after repair
#[tokio::test]
async fn row_craft_app_missing_after_repair() {
    let env = Env::new().await;
    env.profile.edit(|d| {
        d.craft.apps = vec![CraftApp {
            app_id: "inkscape".to_owned(),
            version: "1.4.0".to_owned(),
            allowed_tools: None,
            platforms: std::collections::BTreeMap::new(),
        }]
    });
    *lock(&env.craft.0) = Ok(vec![CraftProgramCheck {
        app_name: "Inkscape".to_owned(),
        version: "1.4.0".to_owned(),
        state: ProgramCheckState::Missing,
    }]);
    let report = on_demand(&env).await;
    let r = result(&report, CheckKind::CraftApps);
    assert_eq!(r.code.as_deref(), Some("SETUP_CRAFT_APP_FAILED"));
    assert_eq!(
        r.message_args.get("program").map(String::as_str),
        Some("Inkscape")
    );
    *lock(&env.craft.0) = Err(());
    let report = on_demand(&env).await;
    assert_eq!(
        result(&report, CheckKind::CraftApps).code.as_deref(),
        Some("SETUP_CRAFT_APP_FAILED")
    );
}

// Row: A token needs a new sign-in — `error_refresh_refused_needs_sign_in` in `acceptance`.

// Row: Database write failed
#[tokio::test]
async fn row_database_write_failed() {
    let env = Env::new().await;
    env.pool.close().await;
    let error = on_demand_error(&env).await;
    assert_eq!(error.code(), "SETUP_STORE_FAILED");
    assert!(error.retryable());
}

async fn on_demand_error(env: &Env) -> SetupError {
    env.setup
        .run_checks(CheckTrigger::OnDemand)
        .await
        .expect_err("store closed")
}

fn attachment() -> HarnessAttachment {
    HarnessAttachment {
        id: HarnessAttachmentId::new(),
        harness: HarnessKind::ClaudeCode,
        location: super::CLAUDE_AT.to_owned(),
        version: Some("2.1.302".to_owned()),
        tested: true,
        signed_in: true,
        sign_in_method: Some("claude.ai".to_owned()),
        state: HarnessAttachmentState::Ready,
        last_checked_at: Timestamp::now(),
    }
}

type Change = Box<dyn Fn(&mut HarnessAttachment)>;

// Row: `harness_attachments` rule broken (2C)
#[test]
fn row_harness_attachment_rule_broken() {
    assert!(store::check_attachment(&attachment()).is_ok());
    let cases: [(&str, Change); 6] = [
        (
            "HARN_PATH_INVALID",
            Box::new(|a| a.location = "claude".to_owned()),
        ),
        (
            "HARN_PATH_INVALID",
            Box::new(|a| a.location = String::new()),
        ),
        (
            "HARN_VERSION_INVALID",
            Box::new(|a| a.version = Some("9".repeat(65))),
        ),
        (
            "HARN_SIGN_IN_METHOD_INVALID",
            Box::new(|a| a.sign_in_method = Some("m".repeat(121))),
        ),
        ("HARN_STATE_INCONSISTENT", Box::new(|a| a.signed_in = false)),
        ("HARN_STATE_INCONSISTENT", Box::new(|a| a.tested = false)),
    ];
    for (expected, change) in cases {
        let mut a = attachment();
        change(&mut a);
        let error = store::check_attachment(&a).expect_err(expected);
        assert_eq!(error.code(), expected);
        assert!(!error.retryable());
    }
}

// Row: `harness_attachments` rule broken — `HARN_DUPLICATE` is kept away: a second write for the
// same harness replaces the first.
#[tokio::test]
async fn row_harness_duplicate_is_kept_away() {
    let env = Env::new().await;
    store::put_attachment(&env.pool, &attachment())
        .await
        .expect("first");
    let mut second = attachment();
    second.version = Some("2.1.303".to_owned());
    store::put_attachment(&env.pool, &second)
        .await
        .expect("second");
    let all = store::attachments(&env.pool).await.expect("read");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].version.as_deref(), Some("2.1.303"));
}

// Amendment row: Server archive download failed
#[tokio::test]
async fn row_server_archive_download_failed() {
    let s = Server::new("0.4.0", ServerRunMode::UserUnit).await;
    let gone = format!("{}/gone.tar.gz", s.mock.uri());
    s.offer("0.5.0", archive("0.5.0", Hook::File), None, Some(gone))
        .await;
    let error = s.env.setup.run_server_update().await.expect_err("failed");
    assert_eq!(error.code(), "SETUP_UPDATE_UNREACHABLE");
    assert!(error.retryable());
    assert_eq!(s.file("ordin-server"), "old server");
}

// Amendment row: `update_server` when the feed offers no newer version
#[tokio::test]
async fn row_update_server_with_no_newer_version() {
    let s = Server::new("0.5.0", ServerRunMode::UserUnit).await;
    s.offer("0.5.0", archive("0.5.0", Hook::File), None, None)
        .await;
    assert_eq!(
        code(s.env.setup.update_server("0.5.0").await),
        "SETUP_UPDATE_NONE"
    );
}

// Amendment rows SETUP_SELF_UPDATE_UNAVAILABLE, SETUP_UPDATE_NO_ARCHIVE,
// SETUP_UPDATE_SIGNATURE_INVALID (server archive), SETUP_UPDATE_FEED_INVALID (version differs),
// SETUP_UPDATE_REPLACE_FAILED, SETUP_UPDATE_IN_PROGRESS and SETUP_UPDATE_VERSION_CHANGED are
// brought about in `tests::updates` by `error_the_server_commands_on_the_desktop`,
// `error_a_server_in_a_container_cannot_replace_itself`, `error_no_archive_for_this_architecture`,
// `error_a_server_archive_with_a_bad_signature`,
// `error_the_signed_archive_is_not_the_version_offered`, `error_replacing_a_program_fails`,
// `error_a_second_update_while_one_runs` and `error_a_newer_version_came_out_meanwhile`.
