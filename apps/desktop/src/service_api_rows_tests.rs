//! §7.2 for the rows of ServiceApi.md SVC-R1-1 to SVC-R1-3 (FU-149): each row answered by the
//! program reaches its owner through the real modules, and each row kept refused answers
//! `SVC_NOT_IN_THIS_VERSION`.

use ordin_service_api::{command, dispatch, ServiceError};
use ordin_types::{AgentInstanceId, ConfluenceLinkId, PlaceId, PublishedCopyId, WorkspaceId};
use serde_json::{json, Value};

use crate::test_support::TempDir;
use crate::wiring_tests::{context, wire_all};

/// One command through the program's Service API with the real modules.
async fn call(
    api: &std::sync::Arc<crate::service_api::DesktopServiceApi>,
    name: &'static str,
    input: Value,
) -> Result<Value, ServiceError> {
    dispatch(&**api, &context(), command(name).expect("row"), input).await
}

fn id<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).expect("an id")
}

// SVC-R1-1: the rows that reach an owner with nothing added.
#[tokio::test]
async fn service_api_answers_the_rows_of_svc_r1_1() {
    let dir = TempDir::new("service-api-r1-1");
    let runner = wire_all(&dir).await;
    let api = runner.modules.service_api.clone().expect("row 3.48");

    // brand.get_asset: a font the pack does not have is the owner's own code.
    let font = call(
        &api,
        "brand.get_asset",
        json!({ "asset": { "kind": "font", "index": 9 } }),
    )
    .await
    .expect_err("no such font");
    assert_eq!(font.code, "BRAND_ASSET_UNKNOWN");
    // …and the logo is bytes with a media type.
    let logo = call(
        &api,
        "brand.get_asset",
        json!({ "asset": { "kind": "logo_light" } }),
    )
    .await;
    if let Ok(bytes) = logo {
        assert!(bytes["media_type"].is_string() && bytes["base64"].is_string());
    }

    // ordin.draft_language: the owner answers, with its own code or the count of messages.
    let draft = call(
        &api,
        "ordin.draft_language",
        json!({ "locale": "fr-FR", "name": "French" }),
    )
    .await;
    match draft {
        Ok(reply) => {
            assert_eq!(reply["accepted"], json!(true));
            assert!(reply["messages"].is_u64());
        }
        Err(error) => assert!(error.code.starts_with("OH_"), "{}", error.code),
    }

    // knowledge.write: a person's new page in a workspace, then the owner's refusal for an unknown one.
    let workspace = call(&api, "workspaces.create", json!({ "name": "Northwind" }))
        .await
        .expect("created");
    let written = call(
        &api,
        "knowledge.write",
        json!({ "workspace_id": workspace["id"], "title": "Supplier quotes", "body_markdown": "Quotes arrive by Friday." }),
    )
    .await
    .expect("written");
    assert!(written["page_id"].is_string());
    let unknown = call(
        &api,
        "knowledge.write",
        json!({ "workspace_id": id(WorkspaceId::new()), "title": "Lost", "body_markdown": "Nowhere." }),
    )
    .await
    .expect_err("an unknown workspace");
    assert!(
        unknown.code.starts_with("KNOW_") || unknown.code.starts_with("WS_"),
        "{}",
        unknown.code
    );

    // files.add_place: a folder the person picked; a relative path is the owner's `PLACE_PATH_INVALID`.
    let folder = dir.path().join("Supplier quotes");
    std::fs::create_dir_all(&folder).expect("folder");
    let place = call(
        &api,
        "files.add_place",
        json!({ "path": folder.to_string_lossy(), "friendly_name": "Supplier quotes" }),
    )
    .await
    .expect("added");
    assert_eq!(place["kind"], json!("persons_folder"));
    let relative = call(
        &api,
        "files.add_place",
        json!({ "path": "Supplier quotes", "friendly_name": null }),
    )
    .await
    .expect_err("a relative path");
    assert_eq!(relative.code, "PLACE_PATH_INVALID");

    // craft.open: an unknown place is the owner's refusal.
    let open = call(
        &api,
        "craft.open",
        json!({ "place_id": id(PlaceId::new()), "path": "quote.docx" }),
    )
    .await
    .expect_err("an unknown place");
    assert!(
        open.code.starts_with("CRAFT_") || open.code.starts_with("PLACE_"),
        "{}",
        open.code
    );

    // workspaces.add_email_link: proposed and confirmed in one call.
    let link = call(
        &api,
        "workspaces.add_email_link",
        json!({
            "workspace_id": workspace["id"],
            "kind": "people_and_domains",
            "folder_or_label": null,
            "addresses": ["asha@example.com"],
            "domains": [],
            "display_name": "Asha"
        }),
    )
    .await
    .expect("added");
    assert_eq!(link["state"], json!("active"));
    let missing = call(
        &api,
        "workspaces.add_email_link",
        json!({
            "workspace_id": id(WorkspaceId::new()),
            "kind": "people_and_domains",
            "folder_or_label": null,
            "addresses": ["asha@example.com"],
            "domains": [],
            "display_name": "Asha"
        }),
    )
    .await
    .expect_err("an unknown workspace");
    assert!(missing.code.starts_with("WS_"), "{}", missing.code);

    // The adapters: with no sign-in each answers its own code.
    let sharepoint = call(
        &api,
        "sharepoint.find",
        json!({ "query": "Procurement", "limit": 10 }),
    )
    .await
    .expect_err("not signed in");
    assert!(sharepoint.code.starts_with("SP_"), "{}", sharepoint.code);
    let confluence = call(
        &api,
        "confluence.find",
        json!({ "query": "Procurement", "limit": 10 }),
    )
    .await
    .expect_err("not signed in");
    assert!(confluence.code.starts_with("CONF_"), "{}", confluence.code);
    let kept = call(
        &api,
        "confluence.keep_mine",
        json!({ "published_copy_id": id(PublishedCopyId::new()) }),
    )
    .await
    .expect_err("an unknown copy");
    assert!(kept.code.starts_with("CONF_"), "{}", kept.code);

    // Jira: no credential is `JIRA_NOT_CONNECTED`; a bad site is `JIRA_ADDRESS_INVALID`.
    let issues = call(
        &api,
        "jira.find_issues",
        json!({ "text": "invoice", "limit": 5 }),
    )
    .await
    .expect_err("not connected");
    assert_eq!(issues.code, "JIRA_NOT_CONNECTED");
    let connect = call(
        &api,
        "jira.connect_token",
        json!({ "site": "not a host", "email": "asha@example.com", "token": "secret-token" }),
    )
    .await
    .expect_err("a bad site");
    assert_eq!(connect.code, "JIRA_ADDRESS_INVALID");
    assert!(!connect.message.contains("secret-token"));

    // Confluence with an API token (SVC-CONF-TOKEN-1): a bad site is `CONF_SITE_INVALID` and the token is
    // never echoed; with no Jira API token, "Use my Jira sign-in" is `JIRA_NOT_CONNECTED`.
    let confluence = call(
        &api,
        "confluence.connect_token",
        json!({ "site": "not a host", "email": "asha@example.com", "token": "secret-token" }),
    )
    .await
    .expect_err("a bad site");
    assert_eq!(confluence.code, "CONF_SITE_INVALID");
    assert!(!confluence.message.contains("secret-token"));
    let from_jira = call(&api, "confluence.connect_with_jira", json!({}))
        .await
        .expect_err("no Jira token");
    assert_eq!(from_jira.code, "JIRA_NOT_CONNECTED");
}

// SVC-R1-2: the rows whose arguments the program fills.
#[tokio::test]
async fn service_api_fills_what_the_screens_never_send() {
    let dir = TempDir::new("service-api-r1-2");
    let runner = wire_all(&dir).await;
    let api = runner.modules.service_api.clone().expect("row 3.48");
    let unknown = id(AgentInstanceId::new());

    let files = call(
        &api,
        "agents.installed_files",
        json!({ "agent_instance_id": unknown }),
    )
    .await
    .expect_err("an unknown instance");
    assert_eq!(files.code, "PKG_INSTANCE_UNKNOWN");
    let file = call(
        &api,
        "agents.installed_file",
        json!({ "agent_instance_id": unknown, "path": "SKILL.md" }),
    )
    .await
    .expect_err("an unknown instance");
    assert_eq!(file.code, "PKG_INSTANCE_UNKNOWN");
    let page = call(
        &api,
        "agents.part_page",
        json!({ "agent_instance_id": unknown, "part_id": "workspaces:advisor", "detail": "business_user" }),
    )
    .await
    .expect_err("an unknown instance");
    assert!(
        page.code == "INST_FOLDER_UNAVAILABLE" || page.code.starts_with("PKG_"),
        "{}",
        page.code
    );
    let checked = call(
        &api,
        "checker.check_installed",
        json!({ "agent_instance_id": unknown }),
    )
    .await
    .expect_err("an unknown instance");
    assert!(
        checked.code == "INST_FOLDER_UNAVAILABLE" || checked.code.starts_with("PKG_"),
        "{}",
        checked.code
    );
    // A library item of the bundled catalogue: the Checker answers a report or its own code.
    let library = call(
        &api,
        "checker.check_library_item",
        json!({ "agent_repository_id": null, "kind": "WorkspaceProfile", "identifier": "urn:ordin:default:workspaces:coordinator" }),
    )
    .await;
    if let Err(error) = library {
        assert!(
            error.code.starts_with("CHK_") || error.code.starts_with("CAT_"),
            "{}",
            error.code
        );
    }
}

// SVC-R1-4 (D-1 to D-5): the rows the five decisions answer.
#[tokio::test]
async fn service_api_answers_the_rows_of_svc_r1_4() {
    let dir = TempDir::new("service-api-r1-4");
    let runner = wire_all(&dir).await;
    let api = runner.modules.service_api.clone().expect("row 3.48");

    // identity.sign_in: a kind with no profile section, and a kind this version does not offer.
    let microsoft = call(&api, "identity.sign_in", json!({ "kind": "microsoft" }))
        .await
        .expect_err("no microsoft section");
    assert_eq!(microsoft.code, "CRED_PROVIDER_NOT_CONFIGURED");
    // GitHub: this build has no GitHub App client id, which is Identity's own refusal (ID-R1-4).
    let github = call(&api, "identity.sign_in", json!({ "kind": "github" }))
        .await
        .expect_err("no app");
    assert_eq!(github.code, "CRED_PROVIDER_NOT_CONFIGURED");
    let google = call(&api, "identity.sign_in", json!({ "kind": "google" }))
        .await
        .expect_err("no google section");
    assert_eq!(google.code, "CRED_PROVIDER_NOT_CONFIGURED");
    // The mail server's single sign-on: the Service API's input check requires the account (covered in
    // `ordin-service-api`, `sign_in_mail_account_checks`); a profile without an identity provider is
    // not configured (ID-R1-7).
    let no_provider = call(
        &api,
        "identity.sign_in",
        json!({ "kind": "mail_server_single_sign_on", "mail_account": "asha@example.com" }),
    )
    .await
    .expect_err("no identity provider");
    assert_eq!(no_provider.code, "CRED_PROVIDER_NOT_CONFIGURED");
    // The account is ignored for every other kind.
    let ignored = call(
        &api,
        "identity.sign_in",
        json!({ "kind": "github", "mail_account": "not an address" }),
    )
    .await
    .expect_err("no app");
    assert_eq!(ignored.code, "CRED_PROVIDER_NOT_CONFIGURED");
    // identity.sign_in_wait: a challenge nobody started is the owner's own code.
    let wait = call(
        &api,
        "identity.sign_in_wait",
        json!({ "challenge_id": id(uuid::Uuid::now_v7()) }),
    )
    .await
    .expect_err("an unknown challenge");
    assert_eq!(wait.code, "CRED_CHALLENGE_UNKNOWN");
    // identity.save_mail_password: a view without the password, or the owner's own code.
    let saved = call(
        &api,
        "identity.save_mail_password",
        json!({ "account_label": "asha@example.com", "kind": "mail_password", "password": "secret-password" }),
    )
    .await;
    match saved {
        Ok(view) => {
            assert_eq!(view["account_label"], json!("asha@example.com"));
            assert!(!view.to_string().contains("secret-password"));
        }
        Err(error) => {
            assert!(error.code.starts_with("CRED_"), "{}", error.code);
            assert!(!error.message.contains("secret-password"));
        }
    }

    // confluence.publish_checked: an unknown link is the Workspace Service's own code.
    let publish = call(
        &api,
        "confluence.publish_checked",
        json!({ "confluence_link_id": id(ConfluenceLinkId::new()), "items": [] }),
    )
    .await
    .expect_err("an unknown link");
    assert_eq!(publish.code, "WS_LINK_NOT_FOUND");

    // files.save_edit: a person's edit of a file in a place becomes a new version.
    let folder = dir.path().join("Notes");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join("note.md"), "first").expect("file");
    let place = call(
        &api,
        "files.add_place",
        json!({ "path": folder.to_string_lossy(), "friendly_name": "Notes" }),
    )
    .await
    .expect("added");
    let edited = call(
        &api,
        "files.save_edit",
        json!({ "place_id": place["id"], "path": "note.md", "text": "second" }),
    )
    .await
    .expect("edited");
    assert!(edited["version"].is_u64());
    assert_eq!(
        std::fs::read_to_string(folder.join("note.md")).expect("read"),
        "second"
    );
    let outside = call(
        &api,
        "files.save_edit",
        json!({ "place_id": id(PlaceId::new()), "path": "note.md", "text": "x" }),
    )
    .await
    .expect_err("an unknown place");
    assert!(
        outside.code.starts_with("FILE_") || outside.code.starts_with("PLACE_"),
        "{}",
        outside.code
    );
}

// SVC-R1-4: the two rows kept refused.
#[tokio::test]
async fn service_api_keeps_the_relay_rows_refused() {
    let dir = TempDir::new("service-api-relay");
    let runner = wire_all(&dir).await;
    let api = runner.modules.service_api.clone().expect("row 3.48");
    let refused = call(
        &api,
        "identity.complete_relayed_sign_in",
        json!({ "challenge_id": id(uuid::Uuid::now_v7()), "code": "c", "state": "s" }),
    )
    .await
    .expect_err("refused");
    assert_eq!(refused.code, "SVC_NOT_IN_THIS_VERSION");
    assert!(!refused.message.is_empty());
}

// ID-R1-1, ID-R1-4 to ID-R1-6: the provider and scopes of each kind, read from the profile alone.
#[tokio::test]
async fn sign_in_provider_follows_the_profile_for_each_kind() {
    use crate::service_api::owners::sign_in_provider;
    use ordin_identity::ProviderConfig;
    use ordin_organisation_profile::{GoogleSection, MicrosoftSection};
    use ordin_types::domain::identity::CredentialKind as K;

    let dir = TempDir::new("sign-in-provider");
    let runner = wire_all(&dir).await;
    let profile = runner.modules.profile.clone().expect("profile");
    let mut document = profile.current().document.clone();

    // No sections: the kinds that need one are not configured; GitHub needs none of the profile.
    for kind in [
        K::Microsoft,
        K::Confluence,
        K::Slack,
        K::Oidc,
        K::Google,
        K::MailServerSingleSignOn,
    ] {
        assert_eq!(
            sign_in_provider(&document, None, kind, None, None).unwrap_err(),
            "CRED_PROVIDER_NOT_CONFIGURED",
            "{kind:?}"
        );
    }
    let (github, scopes) =
        sign_in_provider(&document, None, K::Github, None, None).expect("github");
    assert!(matches!(github, ProviderConfig::Github { ref host } if host == "github.com"));
    assert!(scopes.is_empty());
    for kind in [K::MailServerPassword, K::MailServerAppPassword] {
        assert_eq!(
            sign_in_provider(&document, None, kind, None, None).unwrap_err(),
            "CRED_FLOW_NOT_OFFERED",
            "{kind:?}"
        );
    }

    // Google: the section's client id and secret, no scopes from the caller.
    document.google = Some(GoogleSection {
        client_id: "google-client".to_owned(),
        client_secret: Some("google-secret".to_owned()),
    });
    let (google, scopes) =
        sign_in_provider(&document, None, K::Google, None, None).expect("google");
    assert!(matches!(
        google,
        ProviderConfig::Google { ref client_id, ref client_secret }
            if client_id == "google-client" && client_secret.as_deref() == Some("google-secret")
    ));
    assert!(scopes.is_empty());

    // ID-MS-BUILT-2: no profile section, the build's own app with the `organizations` authority.
    let (built_in, scopes) =
        sign_in_provider(&document, Some("ms-app"), K::Microsoft, None, None).expect("built in");
    assert!(matches!(
        built_in,
        ProviderConfig::Microsoft { ref tenant_id, ref client_id }
            if tenant_id == "organizations" && client_id == "ms-app"
    ));
    assert_eq!(scopes.len(), ordin_sharepoint::SCOPES.len());

    // Microsoft: the SharePoint Adapter's scopes.
    document.microsoft = Some(MicrosoftSection {
        tenant_id: "tenant".to_owned(),
        client_id: "microsoft-client".to_owned(),
    });
    let (microsoft, scopes) =
        sign_in_provider(&document, None, K::Microsoft, None, None).expect("microsoft");
    assert!(
        matches!(microsoft, ProviderConfig::Microsoft { ref tenant_id, .. } if tenant_id == "tenant")
    );
    // ID-MS-BUILT-2: a profile that names Microsoft wins over the build's own app.
    let (company, _) =
        sign_in_provider(&document, Some("ms-app"), K::Microsoft, None, None).expect("company");
    assert!(
        matches!(company, ProviderConfig::Microsoft { ref client_id, .. } if client_id == "microsoft-client")
    );
    assert_eq!(
        scopes,
        ordin_sharepoint::SCOPES
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>()
    );

    // Jira needs the site from the input and the Atlassian client id of the Confluence section.
    assert_eq!(
        sign_in_provider(
            &document,
            None,
            K::Jira,
            Some("https://acme.atlassian.net"),
            None
        )
        .unwrap_err(),
        "CRED_PROVIDER_NOT_CONFIGURED"
    );

    // The mail single sign-on: the identity provider section and the person's address (ID-R1-7).
    document.identity_provider = Some(ordin_organisation_profile::IdentityProviderSection {
        issuer: "https://login.example.com".to_owned(),
        client_id: "mail-client".to_owned(),
        scopes: vec!["mail.read".to_owned()],
    });
    let (mail, scopes) = sign_in_provider(
        &document,
        None,
        K::MailServerSingleSignOn,
        None,
        Some("asha@example.com"),
    )
    .expect("mail single sign-on");
    assert!(matches!(
        mail,
        ProviderConfig::MailServerSso { ref issuer, ref client_id, ref scopes, ref mail_account }
            if issuer == "https://login.example.com"
                && client_id == "mail-client"
                && scopes == &vec!["mail.read".to_owned()]
                && mail_account == "asha@example.com"
    ));
    assert_eq!(scopes, vec!["mail.read".to_owned()]);
}

// Found on Windows in the Phase 5 walk: Settings › Sign-ins showed nothing, because
// `profile.get_document` answered `SVC_INTERNAL`. The Service API restates the profile document
// field for field, and its copy lacked `engineering.coding_tasks`, so even the default profile
// failed. Any later field the copy misses fails here.
#[tokio::test]
async fn profile_get_document_restates_the_default_profile() {
    let dir = TempDir::new("service-api-profile");
    let runner = wire_all(&dir).await;
    let api = runner.modules.service_api.clone().expect("row 3.48");
    let result = call(&api, "profile.get_document", json!({}))
        .await
        .expect("profile.get_document answers");
    assert_eq!(result["source"], "defaults");
    assert_eq!(
        result["document"]["engineering"]["coding_tasks"]["max_attempts"],
        3
    );
}
