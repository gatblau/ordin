//! X2: one test per Gherkin scenario of OrganisationProfile.md and its amendments (ADR-102, ADR-110,
//! ADR-113), each named after its scenario; the exact title is the comment above each test.
//!
//! The Background's address `https://it.example.com/ordin/profile.json` is served by a `wiremock`
//! stand-in on loopback; unit tests admit its plain-HTTP loopback address (`ALLOW_LOOPBACK_HTTP`), no
//! real network is used.

use std::time::Duration;

use ordin_types::domain::agents::RemoteAgentStatus;
use serde_json::json;

use super::support::*;
use crate::*;

fn version_of(snapshot: &ProfileSnapshot) -> Option<String> {
    match &snapshot.source {
        SnapshotSource::Verified {
            profile_version, ..
        } => Some(profile_version.clone()),
        SnapshotSource::Defaults => None,
    }
}

fn refused(code: &str) -> RefreshOutcome {
    RefreshOutcome::Refused { code: code.into() }
}

// Scenario: First start activates a validly signed profile
#[tokio::test]
async fn first_start_activates_a_validly_signed_profile() {
    let r = running_with("1.0.0", json!({})).await;
    let status = r.service.status();
    assert_eq!(status.state, ProfileState::Active);
    assert_eq!(status.active_version.as_deref(), Some("1.0.0"));
    assert_eq!(version_of(&r.service.current()).as_deref(), Some("1.0.0"));
    let states = row_states(&r.pool).await;
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].1, "active");
    for file in ["active.json", "active.jws"] {
        let path = r.root.path().join("profile").join(file);
        assert!(path.exists(), "{file}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{file}");
        }
    }
}

// Scenario: An offline start uses the cached profile
#[tokio::test]
async fn an_offline_start_uses_the_cached_profile() {
    let r = running_with("1.0.0", json!({})).await;
    r.service.shutdown().await;
    // The address now cannot be reached: a closed loopback port.
    write_installer(
        r.root.path(),
        "http://127.0.0.1:1/profile.json",
        &[&Key::k1()],
    );
    let service = start(r.root.path(), &r.pool, r.clock.clone()).await;
    assert_eq!(version_of(&service.current()).as_deref(), Some("1.0.0"));
    settle(&service).await;
    let failure = service.status().last_failure.expect("the fetch failed");
    assert_eq!(failure.code, "PROF_UNREACHABLE");
    assert_eq!(version_of(&service.current()).as_deref(), Some("1.0.0"));
}

// Scenario: A newer profile replaces the active one and subscribers are told
#[tokio::test]
async fn a_newer_profile_replaces_the_active_one_and_subscribers_are_told() {
    let r = running_with("1.0.0", json!({})).await;
    let mut subscriber = r.service.subscribe();
    assert!(subscriber.has_changed().unwrap(), "told once on subscribe");
    subscriber.borrow_and_update();
    let outcome = refresh_with(&r, "1.1.0", json!({})).await;
    assert_eq!(
        outcome,
        RefreshOutcome::Activated {
            profile_version: "1.1.0".into()
        }
    );
    let states = row_states(&r.pool).await;
    assert_eq!(states[0], ("1.0.0".into(), "replaced".into(), None));
    assert_eq!(states[1].1, "active");
    assert!(subscriber.has_changed().unwrap());
    assert_eq!(
        version_of(&subscriber.borrow_and_update()).as_deref(),
        Some("1.1.0")
    );
}

// Scenario: A profile with a bad signature is ignored
#[tokio::test]
async fn a_profile_with_a_bad_signature_is_ignored() {
    let r = running_with("1.0.0", json!({})).await;
    let doc = document("1.1.0", json!({}));
    // Signed by K2 under K1's kid: the signature does not verify with K1.
    let header = ordin_types::encoding::b64url_encode(
        format!(r#"{{"alg":"EdDSA","kid":"{}"}}"#, Key::k1().kid()).as_bytes(),
    );
    r.site
        .serve_raw(&doc, &Key::k2().sign_with_header(&header, &doc))
        .await;
    let outcome = r.service.refresh().await.unwrap();
    assert_eq!(outcome, refused("PROF_SIGNATURE_INVALID"));
    assert_eq!(version_of(&r.service.current()).as_deref(), Some("1.0.0"));
    let states = row_states(&r.pool).await;
    assert!(states
        .iter()
        .any(|(_, state, reason)| state == "failed_verification"
            && reason.as_deref() == Some("PROF_SIGNATURE_INVALID")));
}

// Scenario: The same bad profile is not recorded twice
#[tokio::test]
async fn the_same_bad_profile_is_not_recorded_twice() {
    let r = running_with("1.0.0", json!({})).await;
    let doc = document("1.1.0", json!({}));
    // No `kid`: every installer key is tried, and K1 does not verify K2's signature.
    let header = ordin_types::encoding::b64url_encode(br#"{"alg":"EdDSA"}"#);
    r.site
        .serve_raw(&doc, &Key::k2().sign_with_header(&header, &doc))
        .await;
    assert_eq!(
        r.service.refresh().await.unwrap(),
        refused("PROF_SIGNATURE_INVALID")
    );
    let rows = row_states(&r.pool).await.len();
    assert_eq!(
        r.service.refresh().await.unwrap(),
        refused("PROF_SIGNATURE_INVALID")
    );
    assert_eq!(row_states(&r.pool).await.len(), rows);
}

// Scenario: An older version is refused
#[tokio::test]
async fn an_older_version_is_refused() {
    let r = running_with("2.0.0", json!({})).await;
    assert_eq!(
        refresh_with(&r, "1.9.0", json!({})).await,
        refused("PROF_VERSION_NOT_NEWER")
    );
}

// Scenario Outline: Content outside the bounds is refused
#[tokio::test]
async fn content_outside_the_bounds_is_refused() {
    let sha = "a".repeat(64);
    let examples = [
        (
            json!({"usage": {"agents_at_once_max": 4}}),
            "usage.agents_at_once_max",
        ),
        (
            json!({"limits": {"crew": {"rounds": 31}}}),
            "limits.crew.rounds",
        ),
        (
            json!({"locales": {"offered": ["en-GB", "de-DE"], "fallback": "fr-FR"}}),
            "locales.fallback",
        ),
        (json!({"marketplace": {}}), "marketplace"),
        (
            json!({"updates": {"feed_url": "http://x.example"}}),
            "updates.feed_url",
        ),
        (
            json!({"email": {"system": "company_server", "smtp": {"host": "smtp.example.com"},
                             "company_domains": ["example.com"]}}),
            "email.imap",
        ),
        (
            json!({"craft": {"apps": [{"app_id": "krita", "version": "5.2.6", "platforms": {
                "macos_arm64": {"asset_url": "https://mirror.example/k.dmg", "sha256": "abc",
                                "file_name": "k.dmg"}}}]}}),
            "craft.apps[0].platforms.macos_arm64.sha256",
        ),
        (
            json!({"language_servers": [
                {"language": "go", "name": "gopls", "version": "0.16.2", "platforms": {"linux_x86_64":
                    {"asset_url": "https://m.example/g.tgz", "sha256": sha, "file_name": "g.tgz", "binary_path": "bin/gopls"}}},
                {"language": "go", "name": "gopls", "version": "0.16.3", "platforms": {"linux_x86_64":
                    {"asset_url": "https://m.example/g.tgz", "sha256": sha, "file_name": "g.tgz", "binary_path": "bin/gopls"}}}]}),
            "language_servers",
        ),
        (
            json!({"gitlab": {"host": "https://gitlab.example.com"}}),
            "gitlab.host",
        ),
        (
            json!({"slack": {"workspace": "acme", "dedicated_channel": "#agents"}}),
            "slack.dedicated_channel",
        ),
        (
            json!({"slack": {"workspace": "acme", "dedicated_channel": "C0123ABCD45"},
                   "remote_agents": [{"name": "Bot", "route": "slack", "address": "agent-bot"}]}),
            "remote_agents[0].address",
        ),
    ];
    let r = running_with("1.0.0", json!({})).await;
    for (patch, (defect, key_path)) in examples.into_iter().enumerate() {
        // Each example is a distinct document; the version stays "1.0.1" as in the outline.
        let mut defect = defect;
        defect["voice"] = json!({"terms_use": [format!("example-{patch}")]});
        let outcome = refresh_with(&r, "1.0.1", defect).await;
        assert_eq!(outcome, refused("PROF_CONTENT_INVALID"), "{key_path}");
        let reason = row_states(&r.pool)
            .await
            .into_iter()
            .filter_map(|(_, _, reason)| reason)
            .next_back()
            .unwrap();
        assert_eq!(reason, format!("PROF_CONTENT_INVALID {key_path}"));
        assert_eq!(version_of(&r.service.current()).as_deref(), Some("1.0.0"));
    }
}

// Scenario: Craft apps, GitLab and language servers in the closing-round shape are read
#[tokio::test]
async fn craft_apps_gitlab_and_language_servers_in_the_closing_round_shape_are_read() {
    let sha = "0".repeat(64);
    let r = running_with(
        "1.0.0",
        json!({
            "craft": {"apps": [{"app_id": "krita", "version": "5.2.6", "platforms": {"macos_arm64": {
                "asset_url": "https://mirror.example/krita-5.2.6.dmg", "sha256": sha,
                "file_name": "krita-5.2.6.dmg"}}}]},
            "gitlab": {"host": "gitlab.example.com", "client_id": "a1b2c3"},
            "language_servers": [{"language": "go", "name": "gopls", "version": "0.16.2", "platforms": {
                "linux_x86_64": {"asset_url": "https://mirror.example/gopls.tar.gz", "sha256": sha,
                                 "file_name": "gopls.tar.gz", "binary_path": "bin/gopls"}}}]
        }),
    )
    .await;
    let current = r.service.current();
    assert_eq!(current.document.craft.apps[0].allowed_tools, None);
    assert_eq!(
        current.document.gitlab.as_ref().unwrap().host,
        "gitlab.example.com"
    );
    assert_eq!(
        current.document.language_servers[0].language,
        ServerLanguage::Go
    );
}

// Scenario: A newer format is reported as such
#[tokio::test]
async fn a_newer_format_is_reported_as_such() {
    let r = running_with("1.0.0", json!({})).await;
    assert_eq!(
        refresh_with(
            &r,
            "1.1.0",
            json!({"format_version": 2, "a_key_of_format_2": true})
        )
        .await,
        refused("PROF_FORMAT_UNSUPPORTED")
    );
}

// Scenario: Unchanged profile answered with 304
#[tokio::test]
async fn unchanged_profile_answered_with_304() {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, ResponseTemplate};
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    let site = Site::new().await;
    let doc = document("1.0.0", json!({}));
    Mock::given(method("GET"))
        .and(path("/profile.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("ETag", "abc")
                .set_body_bytes(doc.clone()),
        )
        .mount(&site.server)
        .await;
    Mock::given(method("GET"))
        .and(path("/profile.json.jws"))
        .respond_with(ResponseTemplate::new(200).set_body_string(Key::k1().sign(&doc)))
        .mount(&site.server)
        .await;
    write_installer(root.path(), &site.url(), &[&Key::k1()]);
    let clock = TestClock::new();
    let service = start(root.path(), &pool, clock.clone()).await;
    settle(&service).await;
    assert_eq!(service.status().state, ProfileState::Active);
    site.server.reset().await;
    Mock::given(method("GET"))
        .and(path("/profile.json"))
        .and(header("If-None-Match", "abc"))
        .respond_with(ResponseTemplate::new(304))
        .mount(&site.server)
        .await;
    clock.advance(Duration::from_secs(60));
    assert_eq!(service.refresh().await.unwrap(), RefreshOutcome::Unchanged);
    assert_eq!(service.status().last_success_at, Some(clock.now()));
}

// Scenario: No address on the desktop
#[tokio::test]
async fn no_address_on_the_desktop() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    let service = start(root.path(), &pool, TestClock::new()).await;
    assert_eq!(service.status().state, ProfileState::NoAddress);
    let current = service.current();
    assert_eq!(current.source, SnapshotSource::Defaults);
    assert_eq!(current.document.harnesses.len(), 1);
    assert_eq!(current.document.harnesses[0].name, HarnessName::ClaudeCode);
    assert_eq!(current.document.locales.fallback, "en-GB");
}

fn agents(entries: serde_json::Value) -> serde_json::Value {
    json!({"remote_agents": entries})
}

fn hosted(name: &str, address: &str) -> serde_json::Value {
    json!({"name": name, "route": "hosted_agent", "address": address,
           "identity": {"issuer": "https://example.okta.com", "client_id": "0oa1b2c3",
                        "audience": "api://agent"}})
}

// Scenario: Remote agents follow the profile
#[tokio::test]
async fn remote_agents_follow_the_profile() {
    let r = running_with(
        "1.0.0",
        agents(json!([
            hosted("Finance assistant", "https://finance-agent.example.com"),
            hosted("Legal assistant", "https://legal-agent.example.com")
        ])),
    )
    .await;
    let finance = r
        .service
        .remote_agents()
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.display_name == "Finance assistant")
        .unwrap();
    r.service
        .mark_remote_agent_enabled(finance.id)
        .await
        .unwrap();
    let verified = r
        .service
        .mark_remote_agent_verified(finance.id)
        .await
        .unwrap();
    assert_eq!(verified.status, RemoteAgentStatus::Verified);
    assert_eq!(
        r.service
            .remote_agent_identity(finance.id)
            .unwrap()
            .client_id,
        "0oa1b2c3"
    );

    refresh_with(
        &r,
        "1.1.0",
        agents(json!([hosted(
            "Finance assistant",
            "https://finance2.example.com"
        )])),
    )
    .await;
    let all = r.service.remote_agents().await.unwrap();
    let finance = all
        .iter()
        .find(|a| a.display_name == "Finance assistant")
        .unwrap();
    assert_eq!(finance.status, RemoteAgentStatus::Listed);
    assert_eq!(
        finance.deployment_address.as_deref(),
        Some("https://finance2.example.com")
    );
    let legal = all
        .iter()
        .find(|a| a.display_name == "Legal assistant")
        .unwrap();
    assert_eq!(legal.status, RemoteAgentStatus::Withdrawn);
}

// Scenario: Remote agent status moves forward only through enabled
#[tokio::test]
async fn remote_agent_status_moves_forward_only_through_enabled() {
    let r = running_with(
        "1.0.0",
        json!({
            "slack": {"workspace": "acme", "dedicated_channel": "C0123ABCD45"},
            "remote_agents": [{"name": "R", "route": "slack", "address": "U0123ABCD45"}]
        }),
    )
    .await;
    let agent = r.service.remote_agents().await.unwrap().remove(0);
    assert_eq!(agent.status, RemoteAgentStatus::Listed);
    assert_eq!(agent.slack_channel.as_deref(), Some("C0123ABCD45"));
    assert_eq!(r.service.remote_agent_identity(agent.id), None);
    assert_eq!(
        r.service.mark_remote_agent_verified(agent.id).await,
        Err(ProfileError::RemoteAgentNotEnabled)
    );
    r.service.mark_remote_agent_enabled(agent.id).await.unwrap();
    // Enabling twice leaves it enabled.
    r.service.mark_remote_agent_enabled(agent.id).await.unwrap();
    r.service
        .mark_remote_agent_verified(agent.id)
        .await
        .unwrap();
    assert_eq!(
        r.service.remote_agent(agent.id).await.unwrap().status,
        RemoteAgentStatus::Verified
    );
}

// Scenario: The personal server's setup command sets the address
#[tokio::test]
async fn the_personal_servers_setup_command_sets_the_address() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    let site = Site::new().await;
    site.serve(&document("1.0.0", json!({})), &Key::k1()).await;
    let service = start(root.path(), &pool, TestClock::new()).await;
    assert_eq!(service.status().state, ProfileState::NoAddress);
    service
        .set_profile_address(&site.url(), vec![Key::k1().public()])
        .await
        .unwrap();
    let written: InstallerSettings =
        serde_json::from_slice(&std::fs::read(root.path().join("installer.json")).unwrap())
            .unwrap();
    assert_eq!(written.profile_url.as_deref(), Some(site.url().as_str()));
    assert_eq!(service.installer_settings(), written);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(root.path().join("installer.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    assert!(site.requests().await >= 1, "a refresh is attempted");
    assert!(service.status().last_attempt_at.is_some());
    service.shutdown().await;
}

// Scenario: The setup command refuses an http address
#[tokio::test]
async fn the_setup_command_refuses_an_http_address() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    let service = start(root.path(), &pool, TestClock::new()).await;
    assert_eq!(
        service
            .set_profile_address(
                "http://it.example.com/profile.json",
                vec![Key::k1().public()]
            )
            .await,
        Err(ProfileError::AddressInvalid)
    );
}

// Scenario: The agent profile does not read a profile
#[tokio::test]
async fn the_agent_profile_does_not_read_a_profile() {
    let site = Site::new().await;
    let service = OrganisationProfileService::start(
        ProfileMode::NotUsed,
        ProfileOptions {
            refresh_interval: Duration::from_secs(3600),
            clock: TestClock::new(),
        },
    )
    .await
    .unwrap();
    assert_eq!(service.status().state, ProfileState::NotUsed);
    assert_eq!(service.refresh().await, Err(ProfileError::NotUsed));
    assert_eq!(
        service
            .set_profile_address(&site.url(), vec![Key::k1().public()])
            .await,
        Err(ProfileError::NotUsed)
    );
    assert_eq!(service.remote_agents().await.unwrap(), Vec::new());
    assert_eq!(service.current().source, SnapshotSource::Defaults);
    assert_eq!(service.installer_settings(), InstallerSettings::default());
    service.shutdown().await;
    assert_eq!(site.requests().await, 0, "no HTTP request is made");
}

// Scenario: Retry with backoff while nothing is active
#[test]
fn retry_with_backoff_while_nothing_is_active() {
    use crate::service::retry_delay;
    // Attempt times while every attempt fails: the first at once, then after each wait.
    let schedule = |jitter: f64| {
        let mut at = vec![Duration::ZERO];
        for failures in 0..5 {
            let next = *at.last().unwrap() + retry_delay(failures, jitter);
            at.push(next);
        }
        at
    };
    let exact: Vec<u64> = schedule(0.0).iter().map(Duration::as_secs).collect();
    assert_eq!(exact, [0, 5, 15, 35, 75, 155]);
    // With ±20 % jitter, "about" those times, and five attempts within the 2 minutes.
    for jitter in [-0.2, 0.2] {
        let at = schedule(jitter);
        for (got, want) in at.iter().zip(&exact) {
            let want = *want;
            let want = want as f64;
            assert!((got.as_secs_f64() - want).abs() <= want * 0.2 + 1e-9);
        }
        let within = at
            .iter()
            .filter(|t| **t <= Duration::from_secs(120))
            .count();
        assert_eq!(within, 5);
    }
}

// Scenario: The default harness is the first harness entry
#[tokio::test]
async fn the_default_harness_is_the_first_harness_entry() {
    let r = running_with(
        "1.0.0",
        json!({"harnesses": [{"name": "copilot"}, {"name": "claude_code"}]}),
    )
    .await;
    assert_eq!(r.service.current().default_harness(), HarnessName::Copilot);
    assert_eq!(
        ProfileSnapshot::defaults().default_harness(),
        HarnessName::ClaudeCode
    );
}

// Scenario: A Telegram chat id that is not an integer is refused
#[tokio::test]
async fn a_telegram_chat_id_that_is_not_an_integer_is_refused() {
    let r = running_with("1.0.0", json!({})).await;
    assert_eq!(
        refresh_with(
            &r,
            "1.1.0",
            json!({"telegram": {"allowed_chats": ["team-chat"]}})
        )
        .await,
        refused("PROF_CONTENT_INVALID")
    );
    assert_eq!(version_of(&r.service.current()).as_deref(), Some("1.0.0"));
}

// Scenario: The server setup command needs a signing key with the address
#[tokio::test]
async fn the_server_setup_command_needs_a_signing_key_with_the_address() {
    let root = TempRoot::new();
    let pool = migrated_pool(root.path()).await;
    std::fs::write(root.path().join("installer.json"), b"{}").unwrap();
    let service = start(root.path(), &pool, TestClock::new()).await;
    assert_eq!(
        service
            .set_profile_address("https://it.example.com/ordin/profile.json", vec![])
            .await,
        Err(ProfileError::InstallerInvalid)
    );
    assert_eq!(
        std::fs::read(root.path().join("installer.json")).unwrap(),
        b"{}"
    );
}

// ---- Amendment January 2026 (ADR-102)

// Scenario: a profile with no slack section
#[tokio::test]
async fn a_profile_with_no_slack_section() {
    let r = running_with("1.0.0", json!({})).await;
    let current = r.service.current();
    assert_eq!(current.document.slack, None);
    let rules = current.document.slack.clone().unwrap_or_default();
    assert!(rules.tools_enabled);
    assert_eq!(rules.allowed_channels, None);
    assert!(rules.rules_allowed);
}

// Scenario: IT limits Slack without the Slack route
#[tokio::test]
async fn it_limits_slack_without_the_slack_route() {
    let r = running_with(
        "1.0.0",
        json!({"slack": {"tools_enabled": true, "allowed_channels": ["C0123ABCD45"], "rules_allowed": false}}),
    )
    .await;
    assert_eq!(r.service.status().state, ProfileState::Active);
    let slack = r.service.current().document.slack.clone().unwrap();
    assert_eq!(slack.workspace, None);
    assert_eq!(slack.dedicated_channel, None);
    assert!(!slack.rules_allowed);
}

async fn refused_at(extra: serde_json::Value, key_path: &str) {
    let r = running_with("1.0.0", json!({})).await;
    assert_eq!(
        refresh_with(&r, "1.1.0", extra).await,
        refused("PROF_CONTENT_INVALID")
    );
    let reason = row_states(&r.pool)
        .await
        .into_iter()
        .find_map(|(_, _, reason)| reason)
        .unwrap();
    assert_eq!(reason, format!("PROF_CONTENT_INVALID {key_path}"));
    assert_eq!(
        version_of(&r.service.current()).as_deref(),
        Some("1.0.0"),
        "the last verified profile stays in force"
    );
}

// Scenario: an allowed channel that is not a channel id
#[tokio::test]
async fn an_allowed_channel_that_is_not_a_channel_id() {
    refused_at(
        json!({"slack": {"allowed_channels": ["#design-reviews"]}}),
        "slack.allowed_channels[0]",
    )
    .await;
}

// Scenario: the Slack route half configured
#[tokio::test]
async fn the_slack_route_half_configured() {
    refused_at(
        json!({"slack": {"workspace": "acme"}}),
        "slack.dedicated_channel",
    )
    .await;
}

// ---- Amendment January 2026 (ADR-110)

fn folder(key: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut entry = json!({"key": key, "display_name": key, "host": "github.com",
                           "repository": "example-co/standards", "branch": "main",
                           "folder": format!("knowledge/{key}")});
    if let (Some(e), serde_json::Value::Object(x)) = (entry.as_object_mut(), extra) {
        e.extend(x);
    }
    entry
}

// Scenario: no team_knowledge section
#[tokio::test]
async fn no_team_knowledge_section() {
    let r = running_with("1.0.0", json!({})).await;
    assert!(r
        .service
        .current()
        .document
        .team_knowledge
        .read_folders
        .is_empty());
}

// Scenario: a named folder with its defaults
#[tokio::test]
async fn a_named_folder_with_its_defaults() {
    let r = running_with(
        "1.0.0",
        json!({"team_knowledge": {"read_folders": [{"key": "security-standards",
            "display_name": "Security standards", "host": "github.com",
            "repository": "example-co/standards", "branch": "main"}]}}),
    )
    .await;
    assert_eq!(r.service.status().state, ProfileState::Active);
    let entry = r.service.current().document.team_knowledge.read_folders[0].clone();
    assert_eq!(entry.folder, "");
    assert_eq!(entry.applies_to, AppliesTo::TeamKnowledge);
    assert!(!entry.review_required);
    assert!(!entry.person_may_stop);
}

// Scenario: a host that is neither GitHub nor the company's GitLab
#[tokio::test]
async fn a_host_that_is_neither_github_nor_the_companys_gitlab() {
    refused_at(
        json!({"gitlab": {"host": "gitlab.example.com"},
               "team_knowledge": {"read_folders": [folder("a", json!({"host": "git.other.example"}))]}}),
        "team_knowledge.read_folders[0].host",
    )
    .await;
    // The company's own GitLab host is accepted.
    let r = running_with(
        "1.0.0",
        json!({"gitlab": {"host": "gitlab.example.com"},
               "team_knowledge": {"read_folders": [folder("a", json!({"host": "gitlab.example.com"}))]}}),
    )
    .await;
    assert_eq!(r.service.status().state, ProfileState::Active);
}

// Scenario: two entries with one key
#[tokio::test]
async fn two_entries_with_one_key() {
    refused_at(
        json!({"team_knowledge": {"read_folders": [
            folder("architecture", json!({"display_name": "One"})),
            folder("architecture", json!({"display_name": "Two", "folder": "other"}))]}}),
        "team_knowledge.read_folders[1].key",
    )
    .await;
}

// Scenario: a folder path that leaves its folder
#[tokio::test]
async fn a_folder_path_that_leaves_its_folder() {
    refused_at(
        json!({"team_knowledge": {"read_folders": [folder("a", json!({"folder": "knowledge/../secrets"}))]}}),
        "team_knowledge.read_folders[0].folder",
    )
    .await;
}

// Scenario: too many named folders
#[tokio::test]
async fn too_many_named_folders() {
    let folders: Vec<_> = (0..21)
        .map(|i| folder(&format!("k{i}"), json!({})))
        .collect();
    refused_at(
        json!({"team_knowledge": {"read_folders": folders}}),
        "team_knowledge.read_folders",
    )
    .await;
}

// Scenario: an unsigned profile adds no folder
#[tokio::test]
async fn an_unsigned_profile_adds_no_folder() {
    let r = running_with("1.0.0", json!({})).await;
    let mut subscriber = r.service.subscribe();
    subscriber.borrow_and_update();
    let doc = document(
        "1.1.0",
        json!({"team_knowledge": {"read_folders": [folder("security-standards", json!({}))]}}),
    );
    // Signed by K2 under K1's kid: the signature fails.
    let header = ordin_types::encoding::b64url_encode(
        format!(r#"{{"alg":"EdDSA","kid":"{}"}}"#, Key::k1().kid()).as_bytes(),
    );
    r.site
        .serve_raw(&doc, &Key::k2().sign_with_header(&header, &doc))
        .await;
    assert_eq!(
        r.service.refresh().await.unwrap(),
        refused("PROF_SIGNATURE_INVALID")
    );
    assert!(!subscriber.has_changed().unwrap());
    assert!(r
        .service
        .current()
        .document
        .team_knowledge
        .read_folders
        .is_empty());
}

// Scenario: a newer profile drops a folder
#[tokio::test]
async fn a_newer_profile_drops_a_folder() {
    let r = running_with(
        "1.0.0",
        json!({"team_knowledge": {"read_folders": [
            folder("security-standards", json!({})), folder("architecture", json!({}))]}}),
    )
    .await;
    let mut subscriber = r.service.subscribe();
    subscriber.borrow_and_update();
    refresh_with(
        &r,
        "1.1.0",
        json!({"team_knowledge": {"read_folders": [folder("architecture", json!({}))]}}),
    )
    .await;
    assert!(subscriber.has_changed().unwrap());
    let keys: Vec<String> = subscriber
        .borrow_and_update()
        .document
        .team_knowledge
        .read_folders
        .iter()
        .map(|f| f.key.clone())
        .collect();
    assert_eq!(keys, ["architecture"]);
}

// ---- Amendment January 2026 (ADR-113)

fn door(key: &str, url: &str) -> serde_json::Value {
    json!({"key": key, "display_name": key, "url": url})
}

// Scenario: no knowledge_agents section
#[tokio::test]
async fn no_knowledge_agents_section() {
    let r = running_with("1.0.0", json!({})).await;
    assert!(r
        .service
        .current()
        .document
        .knowledge_agents
        .front_doors
        .is_empty());
}

// Scenario: a named front door with its defaults
#[tokio::test]
async fn a_named_front_door_with_its_defaults() {
    let r = running_with(
        "1.0.0",
        json!({"knowledge_agents": {"front_doors": [{"key": "procurement-knowledge",
            "display_name": "Procurement knowledge", "url": "https://knowledge-procurement.example.org"}]}}),
    )
    .await;
    assert_eq!(r.service.status().state, ProfileState::Active);
    assert_eq!(
        r.service.current().document.knowledge_agents.front_doors[0].applies_to,
        AppliesTo::TeamKnowledge
    );
}

// Scenario: a front door over plain HTTP
#[tokio::test]
async fn a_front_door_over_plain_http() {
    refused_at(
        json!({"knowledge_agents": {"front_doors": [door("k", "http://knowledge.example.org")]}}),
        "knowledge_agents.front_doors[0].url",
    )
    .await;
}

// Scenario: an address carrying a query
#[tokio::test]
async fn an_address_carrying_a_query() {
    refused_at(
        json!({"knowledge_agents": {"front_doors": [door("k", "https://knowledge.example.org?token=x")]}}),
        "knowledge_agents.front_doors[0].url",
    )
    .await;
}

// Scenario: too many front doors
#[tokio::test]
async fn too_many_front_doors() {
    let doors: Vec<_> = (0..9)
        .map(|i| door(&format!("k{i}"), &format!("https://k{i}.example.org")))
        .collect();
    refused_at(
        json!({"knowledge_agents": {"front_doors": doors}}),
        "knowledge_agents.front_doors",
    )
    .await;
}

// Scenario: a newer profile drops a front door
#[tokio::test]
async fn a_newer_profile_drops_a_front_door() {
    let r = running_with(
        "1.0.0",
        json!({"knowledge_agents": {"front_doors": [
            door("procurement-knowledge", "https://p.example.org"),
            door("finance-knowledge", "https://f.example.org")]}}),
    )
    .await;
    let mut subscriber = r.service.subscribe();
    subscriber.borrow_and_update();
    refresh_with(
        &r,
        "1.1.0",
        json!({"knowledge_agents": {"front_doors": [door("finance-knowledge", "https://f.example.org")]}}),
    )
    .await;
    let keys: Vec<String> = subscriber
        .borrow_and_update()
        .document
        .knowledge_agents
        .front_doors
        .iter()
        .map(|d| d.key.clone())
        .collect();
    assert_eq!(keys, ["finance-knowledge"]);
}
