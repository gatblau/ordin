//! X1: one test per row of OrganisationProfile.md's *Error Table*, provoking the condition and asserting
//! the code, the retryability and the repair action the row's response body names. X4: `ALL_CODES` is
//! registered and owned.

use std::time::Duration;

use ordin_types::domain::agents::{RemoteAgent, RemoteAgentStatus, RemoteRoute};
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::{CodedError, RemoteAgentId, RepairAction};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::support::*;
use crate::*;

fn assert_code(error: &dyn CodedError, code: &str, retryable: bool, repair: Option<RepairAction>) {
    assert_eq!(error.code(), code);
    assert_eq!(error.retryable(), retryable, "{code} retryable");
    assert_eq!(error.repair(), repair, "{code} repair");
    let entry = lookup(code).expect("registered");
    assert_eq!(entry.owner_crate, "ordin-organisation-profile");
}

fn refused(code: &str) -> RefreshOutcome {
    RefreshOutcome::Refused { code: code.into() }
}

/// A service whose address answers `status` to the document.
async fn answering(status: u16) -> (TempRoot, Site, OrganisationProfileService) {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    let site = Site::new().await;
    Mock::given(method("GET"))
        .and(path("/profile.json"))
        .respond_with(ResponseTemplate::new(status))
        .mount(&site.server)
        .await;
    write_installer(root.path(), &site.url(), &[&Key::k1()]);
    let service = start(root.path(), &pool, TestClock::new()).await;
    settle(&service).await;
    (root, site, service)
}

/// X4.
#[test]
fn every_code_is_registered_under_a_prefix_this_crate_owns() {
    assert_eq!(ALL_CODES.len(), 24);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, "ordin-organisation-profile", "{code}");
        let (prefix, _) = code.split_once('_').unwrap();
        assert!(
            owns_prefix("ordin-organisation-profile", prefix),
            "{prefix}"
        );
    }
}

// Row: No profile address known
#[tokio::test]
async fn no_profile_address_known() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    let service = start(root.path(), &pool, TestClock::new()).await;
    let error = service.refresh().await.unwrap_err();
    assert_code(
        &error,
        "PROF_NO_ADDRESS",
        false,
        Some(RepairAction::SendToIt),
    );
    assert_eq!(
        service.status().last_failure.unwrap().code,
        "PROF_NO_ADDRESS"
    );
}

// Row: The address cannot be reached
#[tokio::test]
async fn the_address_cannot_be_reached() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    write_installer(
        root.path(),
        "http://127.0.0.1:1/profile.json",
        &[&Key::k1()],
    );
    let service = start(root.path(), &pool, TestClock::new()).await;
    let error = service.refresh().await.unwrap_err();
    assert_code(
        &error,
        "PROF_UNREACHABLE",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Row: The address answers a status other than 200 or 304
#[tokio::test]
async fn the_address_answers_another_status() {
    for (status, retryable, repair) in [
        (503, true, RepairAction::TryAgain),
        (429, true, RepairAction::TryAgain),
        (408, true, RepairAction::TryAgain),
        (403, false, RepairAction::SendToIt),
    ] {
        let (_root, _site, service) = answering(status).await;
        let error = service.refresh().await.unwrap_err();
        assert_eq!(error, ProfileError::HttpStatus { status });
        assert_code(&error, "PROF_HTTP_STATUS", retryable, Some(repair));
    }
}

// Row: Document over 1 MiB or signature over 8 KiB
#[tokio::test]
async fn document_or_signature_too_large() {
    let r = running_with("1.0.0", json!({})).await;
    let big = document(
        "1.1.0",
        json!({"voice": {"terms_use": ["x".repeat(1_048_576)]}}),
    );
    r.site.serve(&big, &Key::k1()).await;
    assert_eq!(
        r.service.refresh().await.unwrap(),
        refused("PROF_TOO_LARGE")
    );
    let doc = document("1.1.0", json!({}));
    r.site.serve_raw(&doc, &"a".repeat(8_193)).await;
    assert_eq!(
        r.service.refresh().await.unwrap(),
        refused("PROF_TOO_LARGE")
    );
    assert_code(&ProfileError::TooLarge, "PROF_TOO_LARGE", false, None);
}

// Row: Signature file absent
#[tokio::test]
async fn signature_file_absent() {
    let r = running_with("1.0.0", json!({})).await;
    r.site.server.reset().await;
    Mock::given(method("GET"))
        .and(path("/profile.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(document("1.1.0", json!({}))))
        .mount(&r.site.server)
        .await;
    assert_eq!(
        r.service.refresh().await.unwrap(),
        refused("PROF_SIGNATURE_MISSING")
    );
    assert_code(
        &ProfileError::SignatureMissing,
        "PROF_SIGNATURE_MISSING",
        false,
        None,
    );
}

// Row: JWS malformed, wrong algorithm, or signature fails
#[tokio::test]
async fn jws_malformed_wrong_algorithm_or_signature_fails() {
    let r = running_with("1.0.0", json!({})).await;
    let doc = document("1.1.0", json!({}));
    let rs256 = ordin_types::encoding::b64url_encode(br#"{"alg":"RS256"}"#);
    let b64 = ordin_types::encoding::b64url_encode(br#"{"alg":"EdDSA","b64":false}"#);
    for (i, jws) in [
        "not-a-jws".to_owned(),
        format!("{}.payload.{}", "a", "b"),
        Key::k1().sign_with_header(&rs256, &doc),
        Key::k1().sign_with_header(&b64, &doc),
    ]
    .into_iter()
    .enumerate()
    {
        // A distinct document each time, so no refusal is answered from an earlier row.
        let doc = document("1.1.0", json!({"voice": {"terms_use": [format!("t{i}")]}}));
        r.site.serve_raw(&doc, &jws).await;
        assert_eq!(
            r.service.refresh().await.unwrap(),
            refused("PROF_SIGNATURE_INVALID"),
            "{i}"
        );
    }
    assert_code(
        &ProfileError::SignatureInvalid,
        "PROF_SIGNATURE_INVALID",
        false,
        None,
    );
}

// Row: `kid` names no installer key
#[tokio::test]
async fn kid_names_no_installer_key() {
    let r = running_with("1.0.0", json!({})).await;
    let doc = document("1.1.0", json!({}));
    r.site.serve(&doc, &Key::k2()).await;
    assert_eq!(
        r.service.refresh().await.unwrap(),
        refused("PROF_KEY_UNKNOWN")
    );
    assert_code(&ProfileError::KeyUnknown, "PROF_KEY_UNKNOWN", false, None);
}

// Row: `format_version` not read by this build
#[tokio::test]
async fn format_version_not_read_by_this_build() {
    let r = running_with("1.0.0", json!({})).await;
    assert_eq!(
        refresh_with(&r, "1.1.0", json!({"format_version": 7})).await,
        refused("PROF_FORMAT_UNSUPPORTED")
    );
    assert_code(
        &ProfileError::FormatUnsupported,
        "PROF_FORMAT_UNSUPPORTED",
        false,
        None,
    );
}

// Row: Unknown key, wrong type, or a bound broken
#[tokio::test]
async fn unknown_key_wrong_type_or_bound_broken() {
    let r = running_with("1.0.0", json!({})).await;
    for (i, extra) in [
        json!({"usage": {"unknown": 1}}),
        json!({"limits": {"crew": {"rounds": "ten"}}}),
        json!({"sandbox": {"session_idle_minutes": 0}}),
        json!({"engineering": {"coding_tasks": {"max_attempts": 0}}}),
    ]
    .into_iter()
    .enumerate()
    {
        let outcome = refresh_with(&r, &format!("1.1.{i}"), extra).await;
        assert_eq!(outcome, refused("PROF_CONTENT_INVALID"));
    }
    let reasons: Vec<String> = row_states(&r.pool)
        .await
        .into_iter()
        .filter_map(|(_, _, reason)| reason)
        .collect();
    assert_eq!(
        reasons,
        [
            "PROF_CONTENT_INVALID usage.unknown",
            "PROF_CONTENT_INVALID limits.crew.rounds",
            "PROF_CONTENT_INVALID sandbox.session_idle_minutes",
            "PROF_CONTENT_INVALID engineering.coding_tasks.max_attempts"
        ]
    );
    let error = ProfileError::ContentInvalid {
        key_path: "x".into(),
    };
    assert_code(&error, "PROF_CONTENT_INVALID", false, None);
}

// Row: Version not newer than the active one
#[tokio::test]
async fn version_not_newer_than_the_active_one() {
    let r = running_with("1.0.0", json!({})).await;
    // Equal version, different digest: the same refusal.
    assert_eq!(
        refresh_with(&r, "1.0.0", json!({"voice": {"terms_use": ["new"]}})).await,
        refused("PROF_VERSION_NOT_NEWER")
    );
    assert_code(
        &ProfileError::VersionNotNewer,
        "PROF_VERSION_NOT_NEWER",
        false,
        None,
    );
}

// Row: Activation of a row that is not `verified`
#[tokio::test]
async fn activation_of_a_row_that_is_not_verified() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    let id = OrganisationProfileId::new();
    crate::store::insert_fetched(
        &pool,
        id,
        "h..s",
        &"d".repeat(64),
        "/tmp/profile",
        Timestamp::now(),
    )
    .await
    .unwrap();
    let doc: OrganisationProfileDocument =
        serde_json::from_slice(&document("1.0.0", json!({}))).unwrap();
    let error = crate::store::activate(&pool, id, &doc, Timestamp::now())
        .await
        .unwrap_err();
    assert_code(&error, "PROF_UNVERIFIED_ACTIVATION", false, None);
    // Rolled back: the row is still `fetched`, nothing is active.
    assert_eq!(row_states(&pool).await[0].1, "fetched");
}

// Row: A second `active` row (unique index)
#[tokio::test]
async fn a_second_active_row() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    // Only a defect could write two; the test removes the index to make one.
    sqlx::raw_sql("DROP INDEX organisation_profiles_one_active")
        .execute(&pool)
        .await
        .unwrap();
    for digest in ["a", "b"] {
        sqlx::query(
            "INSERT INTO organisation_profiles (id, profile_version, signature, document_digest, \
             cached_path, state, fetched_at, verified_at, activated_at) \
             VALUES (?, '1.0.0', 'h..s', ?, '/tmp', 'active', ?, ?, ?)",
        )
        .bind(OrganisationProfileId::new().to_string())
        .bind(digest.repeat(64))
        .bind(Timestamp::now().to_wire())
        .bind(Timestamp::now().to_wire())
        .bind(Timestamp::now().to_wire())
        .execute(&pool)
        .await
        .unwrap();
    }
    let error = start_with_defaults(
        root.path(),
        &pool,
        TestClock::new(),
        InstallerSettings::default(),
    )
    .await
    .err()
    .unwrap();
    assert_code(&error, "PROF_MULTIPLE_ACTIVE", false, None);
}

// Row: Datastore read or write fails
#[tokio::test]
async fn datastore_read_or_write_fails() {
    let root = TempRoot::new();
    let pool = empty_pool(root.path()).await;
    let error = start_with_defaults(
        root.path(),
        &pool,
        TestClock::new(),
        InstallerSettings::default(),
    )
    .await
    .err()
    .unwrap();
    assert_code(
        &error,
        "PROF_DATASTORE_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Row: `installer.json` unreadable or invalid
#[tokio::test]
async fn installer_json_unreadable_or_invalid() {
    let (lines, _guard) = Lines::capture();
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    for invalid in [
        json!({"profile_url": "http://it.example.com/profile.json"}),
        json!({"profile_signing_keys": ["a", "b", "c"]}),
        json!({"profile_signing_keys": ["too-short"]}),
        json!({"unknown": true}),
    ] {
        std::fs::write(root.path().join("installer.json"), invalid.to_string()).unwrap();
        let defaults = InstallerSettings {
            profile_url: Some("https://build.example.com/profile.json".into()),
            profile_signing_keys: vec![Key::k1().public()],
            update_feed_url: None,
        };
        let service = start_with_defaults(root.path(), &pool, TestClock::new(), defaults.clone())
            .await
            .unwrap();
        // The build defaults are used.
        assert_eq!(service.installer_settings(), defaults);
        service.shutdown().await;
    }
    let logged = lines.event("profile.installer.invalid");
    assert_eq!(logged.len(), 4);
    assert!(logged.iter().all(|l| l["code"] == "PROF_INSTALLER_INVALID"));
    assert_code(
        &ProfileError::InstallerInvalid,
        "PROF_INSTALLER_INVALID",
        false,
        None,
    );
}

// Row: Setup command gives a non-`https` or over-long address
#[tokio::test]
async fn setup_command_gives_a_non_https_or_over_long_address() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    let service = start(root.path(), &pool, TestClock::new()).await;
    let long = format!("https://it.example.com/{}", "a".repeat(2048));
    for url in ["ftp://it.example.com/p.json", "not a url", long.as_str()] {
        let error = service
            .set_profile_address(url, vec![Key::k1().public()])
            .await
            .unwrap_err();
        assert_code(&error, "PROF_ADDRESS_INVALID", false, None);
    }
}

// Row: `installer.json` cannot be written
#[tokio::test]
async fn installer_json_cannot_be_written() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    // The data root is a file, so nothing can be written under it.
    let blocked = root.path().join("blocked");
    std::fs::write(&blocked, b"").unwrap();
    let service = start(&blocked, &pool, TestClock::new()).await;
    let site = Site::new().await;
    let error = service
        .set_profile_address(&site.url(), vec![Key::k1().public()])
        .await
        .unwrap_err();
    assert_code(&error, "PROF_INSTALLER_WRITE_FAILED", true, None);
}

// Row: Cached profile fails verification at start
#[tokio::test]
async fn cached_profile_fails_verification_at_start() {
    let r = running_with("1.0.0", json!({})).await;
    r.service.shutdown().await;
    let (lines, _guard) = Lines::capture();
    // The installer now carries only K2: the cached profile, signed by K1, no longer verifies.
    write_installer(
        r.root.path(),
        "http://127.0.0.1:1/profile.json",
        &[&Key::k2()],
    );
    let service = start(r.root.path(), &r.pool, r.clock.clone()).await;
    assert_eq!(service.current().source, SnapshotSource::Defaults);
    assert_eq!(service.status().state, ProfileState::NeverFetched);
    let logged = lines.event("profile.cache.invalid");
    assert_eq!(logged[0]["code"], "PROF_CACHE_INVALID");
    assert_eq!(logged[0]["cause"], "PROF_KEY_UNKNOWN");
    // The row stays active until a fetched profile replaces it.
    assert_eq!(row_states(&r.pool).await[0].1, "active");
    assert_code(
        &ProfileError::CacheInvalid,
        "PROF_CACHE_INVALID",
        false,
        None,
    );
    service.shutdown().await;
}

// Row: Command used in the agent profile
#[tokio::test]
async fn command_used_in_the_agent_profile() {
    let service = OrganisationProfileService::start(
        ProfileMode::NotUsed,
        ProfileOptions {
            refresh_interval: Duration::from_secs(3600),
            clock: TestClock::new(),
        },
    )
    .await
    .unwrap();
    let error = service.refresh().await.unwrap_err();
    assert_code(&error, "PROF_NOT_USED", false, None);
}

// Row: Remote agent id unknown
#[tokio::test]
async fn remote_agent_id_unknown() {
    let r = running_with("1.0.0", json!({})).await;
    let unknown = RemoteAgentId::new();
    let error = r.service.remote_agent(unknown).await.unwrap_err();
    assert_code(&error, "PROF_REMOTE_AGENT_NOT_FOUND", false, None);
    assert_eq!(
        r.service.mark_remote_agent_enabled(unknown).await,
        Err(ProfileError::RemoteAgentNotFound)
    );
}

fn hosted(name: &str) -> serde_json::Value {
    json!({"name": name, "route": "hosted_agent", "address": "https://agent.example.com",
           "identity": {"issuer": "https://idp.example.com", "client_id": "c", "audience": "a"}})
}

// Row: Status change on a withdrawn remote agent
#[tokio::test]
async fn status_change_on_a_withdrawn_remote_agent() {
    let r = running_with("1.0.0", json!({"remote_agents": [hosted("Old")]})).await;
    let old = r.service.remote_agents().await.unwrap().remove(0);
    refresh_with(&r, "1.1.0", json!({})).await;
    let error = r
        .service
        .mark_remote_agent_enabled(old.id)
        .await
        .unwrap_err();
    assert_code(&error, "PROF_REMOTE_AGENT_WITHDRAWN", false, None);
    assert_eq!(
        r.service.mark_remote_agent_verified(old.id).await,
        Err(ProfileError::RemoteAgentWithdrawn)
    );
}

// Row: `verified` asked for a `listed` remote agent
#[tokio::test]
async fn verified_asked_for_a_listed_remote_agent() {
    let r = running_with("1.0.0", json!({"remote_agents": [hosted("New")]})).await;
    let agent = r.service.remote_agents().await.unwrap().remove(0);
    let error = r
        .service
        .mark_remote_agent_verified(agent.id)
        .await
        .unwrap_err();
    assert_code(&error, "PROF_REMOTE_AGENT_NOT_ENABLED", false, None);
}

// Row: A remote agent row breaks a 2C rule
#[test]
fn a_remote_agent_row_breaks_a_2c_rule() {
    let base = RemoteAgent {
        id: RemoteAgentId::new(),
        display_name: "Agent".into(),
        route: RemoteRoute::Http,
        deployment_address: Some("https://agent.example.com".into()),
        slack_account: None,
        slack_channel: None,
        identity_provider_ref: Some("Agent".into()),
        status: RemoteAgentStatus::Listed,
        listed_at: Timestamp::now(),
    };
    assert_eq!(crate::store::check_remote_agent(&base), Ok(()));
    let cases = [
        (
            RemoteAgent {
                display_name: String::new(),
                ..base.clone()
            },
            "RAG_NAME_INVALID",
        ),
        (
            RemoteAgent {
                deployment_address: None,
                ..base.clone()
            },
            "RAG_HTTP_FIELDS_INVALID",
        ),
        (
            RemoteAgent {
                slack_channel: Some("C0123ABCD45".into()),
                ..base.clone()
            },
            "RAG_HTTP_FIELDS_INVALID",
        ),
        (
            RemoteAgent {
                route: RemoteRoute::Slack,
                deployment_address: None,
                identity_provider_ref: None,
                slack_account: Some("agent-bot".into()),
                slack_channel: Some("C0123ABCD45".into()),
                ..base.clone()
            },
            "RAG_SLACK_FIELDS_INVALID",
        ),
        (
            RemoteAgent {
                route: RemoteRoute::Slack,
                slack_account: Some("U0123ABCD45".into()),
                slack_channel: Some("C0123ABCD45".into()),
                ..base.clone()
            },
            "RAG_SLACK_FIELDS_INVALID",
        ),
    ];
    for (agent, code) in cases {
        let error = crate::store::check_remote_agent(&agent).unwrap_err();
        assert_code(&error, code, false, None);
    }
}
