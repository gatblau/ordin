//! X1: one test per row of SharePointAdapter.md's *Error Table*, provoking the condition and asserting the
//! code, the retryability and the repair action of the Response Body. The row `SP_NOT_CONFIGURED` needs
//! Identity to answer `CRED_PROVIDER_NOT_CONFIGURED`, which a stored credential cannot produce; its test
//! is the unit test `not_configured` in `src/error.rs`.

mod support;

use ordin_sharepoint::{ExternalWrite, SharePointError};
use ordin_types::domain::runs::ExternalActionKind;
use ordin_types::{CodedError, Cursor, RepairAction};
use serde_json::json;
use support::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

fn check(error: &SharePointError, code: &str, retryable: bool, repair: Option<RepairAction>) {
    assert_eq!(error.code(), code);
    assert_eq!(error.retryable(), retryable, "{code}");
    assert_eq!(error.repair(), repair, "{code}");
    assert_eq!(error.to_string(), code, "Display is the code only");
}

fn item_path() -> String {
    api(&format!("/drives/{DRIVE}/items/{FILE}"))
}

fn push() -> ExternalWrite {
    write(ExternalActionKind::OriginPush)
}

// Row: no Microsoft sign-in, refused, or a second 401.
#[tokio::test]
async fn sign_in_needed() {
    let env = Env::with(SignIn::Nobody).await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    check(
        &error,
        "SP_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
    // A credential without the scopes is the same row (CRED_SCOPE_MISSING).
    let narrow = Env::with(SignIn::WrongScopes).await;
    let error = narrow.adapter.item_version(&file_ref()).await.unwrap_err();
    check(
        &error,
        "SP_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
    // A second 401 is the same row; the refresh token is refused the first time.
    let env = Env::with(SignIn::Token).await;
    status_only(&env, "GET", &item_path(), 401).await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    check(
        &error,
        "SP_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
}

// Row: Graph 403.
#[tokio::test]
async fn access_denied() {
    let env = Env::new().await;
    status_only(&env, "GET", &item_path(), 403).await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    check(&error, "SP_ACCESS_DENIED", false, None);
}

// Row: Graph 404.
#[tokio::test]
async fn not_found() {
    let env = Env::new().await;
    status_only(&env, "GET", &item_path(), 404).await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    check(&error, "SP_NOT_FOUND", false, None);
}

// Row: stored origin form or a file name is malformed.
#[tokio::test]
async fn location_invalid() {
    let env = Env::new().await;
    let error = env
        .adapter
        .resolve_origin("not an origin")
        .await
        .unwrap_err();
    check(&error, "SP_LOCATION_INVALID", false, None);
    let error = env
        .adapter
        .exists(&contracts_folder(), "a/b")
        .await
        .unwrap_err();
    check(&error, "SP_LOCATION_INVALID", false, None);
    assert!(env.requests().await.is_empty());
}

// Row: search text empty or over 200 characters.
#[tokio::test]
async fn query_invalid() {
    let env = Env::new().await;
    for query in ["", "   ", &"q".repeat(201)] {
        let error = env.adapter.find_locations(query, 10).await.unwrap_err();
        check(&error, "SP_QUERY_INVALID", false, None);
    }
    assert!(env.requests().await.is_empty());
}

// Row: origin version differs from the one read (before sending, or 412).
#[tokio::test]
async fn origin_changed() {
    let env = Env::new().await;
    mount_file(&env, "c5").await;
    let error = env
        .adapter
        .upload_version(&file_ref(), b"x".to_vec(), "c4", push())
        .await
        .unwrap_err();
    check(&error, "SP_ORIGIN_CHANGED", false, None);
}

// Row: a file of that name exists (409 on create).
#[tokio::test]
async fn name_exists() {
    let env = Env::new().await;
    status_only(
        &env,
        "PUT",
        "/v1.0/drives/b!x9Fq/items/01FOLDER:/a.docx:/content",
        409,
    )
    .await;
    let folder = ordin_sharepoint::SharePointLocation {
        item_id: Some("01FOLDER".to_owned()),
        ..contracts_folder()
    };
    let error = env
        .adapter
        .create_file(
            &folder,
            "a.docx",
            b"x".to_vec(),
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap_err();
    check(&error, "SP_NAME_EXISTS", false, None);
    // A folder that exists is the same row.
    status_only(
        &env,
        "POST",
        "/v1.0/drives/b!x9Fq/items/01FOLDER/children",
        409,
    )
    .await;
    let error = env
        .adapter
        .create_folder(&folder, "sub", write(ExternalActionKind::HandoverWritten))
        .await
        .unwrap_err();
    check(&error, "SP_NAME_EXISTS", false, None);
}

// Row: file above 250 MiB, or Graph 413.
#[tokio::test]
async fn too_large() {
    let env = Env::new().await;
    let oversize = vec![0u8; 250 * 1024 * 1024 + 1];
    let error = env
        .adapter
        .upload_version(&file_ref(), oversize, "c4", push())
        .await
        .unwrap_err();
    check(&error, "SP_TOO_LARGE", false, None);
    assert!(
        env.requests().await.is_empty(),
        "refused before anything is sent"
    );
    // Graph's own 413.
    mount_file(&env, "c4").await;
    status_only(&env, "PUT", &format!("{}/content", item_path()), 413).await;
    let error = env
        .adapter
        .upload_version(&file_ref(), b"x".to_vec(), "c4", push())
        .await
        .unwrap_err();
    check(&error, "SP_TOO_LARGE", false, None);
    // A listed file above the limit is not read.
    let big = file_json(FILE, "Big.iso", "c1", "/Suppliers/Contracts", "Sam Lee");
    let mut big = big;
    big["size"] = json!(251u64 * 1024 * 1024);
    let env = Env::new().await;
    get_json(&env, &item_path(), 200, big).await;
    let error = env.adapter.fetch(&file_ref()).await.unwrap_err();
    check(&error, "SP_TOO_LARGE", false, None);
}

// Row: 429 after three retries.
#[tokio::test]
async fn throttled() {
    let env = Env::new().await;
    Mock::given(method("GET"))
        .and(path(item_path().as_str()))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .mount(&env.mock)
        .await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    check(&error, "SP_THROTTLED", true, Some(RepairAction::TryAgain));
}

// Row: 5xx, 503, 504 or network failure after retries; Identity unreachable.
#[tokio::test]
async fn unavailable() {
    let env = Env::new().await;
    status_only(&env, "GET", &item_path(), 500).await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    check(&error, "SP_UNAVAILABLE", true, Some(RepairAction::TryAgain));
    assert_eq!(
        env.count("GET", &item_path()).await,
        4,
        "the first attempt and three retries"
    );
    // 503 without Retry-After, beyond the retries.
    let env = Env::new().await;
    status_only(&env, "GET", &item_path(), 503).await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    check(&error, "SP_UNAVAILABLE", true, Some(RepairAction::TryAgain));
    // Identity unreachable: the token is about to expire and the provider answers 503.
    let env = Env::with(SignIn::ProviderDown).await;
    status_only(&env, "POST", "/token-down", 503).await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    check(&error, "SP_UNAVAILABLE", true, Some(RepairAction::TryAgain));
}

// Row: upload or create timed out after sending and the origin shows a change.
#[tokio::test]
async fn outcome_unknown() {
    let env = Env::new().await;
    get_json_once(
        &env,
        &item_path(),
        200,
        file_json(
            FILE,
            "Kestrel MSA.docx",
            "c4",
            "/Suppliers/Contracts",
            "Sam Lee",
        ),
    )
    .await;
    get_json(
        &env,
        &item_path(),
        200,
        file_json(
            FILE,
            "Kestrel MSA.docx",
            "c5",
            "/Suppliers/Contracts",
            "Sam Lee",
        ),
    )
    .await;
    mount_labels_and_site(&env).await;
    Mock::given(method("PUT"))
        .and(path(format!("{}/content", item_path()).as_str()))
        .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(3)))
        .mount(&env.mock)
        .await;
    let error = env
        .adapter
        .upload_version(&file_ref(), b"x".to_vec(), "c4", push())
        .await
        .unwrap_err();
    check(&error, "SP_OUTCOME_UNKNOWN", false, None);
}

// Row: Graph body does not match the expected shape.
#[tokio::test]
async fn response_invalid() {
    let env = Env::new().await;
    Mock::given(method("GET"))
        .and(path(item_path().as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&env.mock)
        .await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    check(&error, "SP_RESPONSE_INVALID", false, None);
    // A next link with no $skiptoken is the same row (Internal Logic 1.4).
    let env = Env::new().await;
    let children = api(&format!("/drives/{DRIVE}/items/{FOLDER}/children"));
    get_json(
        &env,
        &children,
        200,
        json!({
            "value": [],
            "@odata.nextLink": format!("{}{children}?$top=100", env.mock.uri()),
        }),
    )
    .await;
    let error = env
        .adapter
        .list_children(&contracts_folder(), None)
        .await
        .unwrap_err();
    check(&error, "SP_RESPONSE_INVALID", false, None);
}

// Row: `check_changes` on a link with no tracking state.
#[tokio::test]
async fn not_tracked() {
    let env = Env::new().await;
    let error = env
        .adapter
        .check_changes(&link_for(&contracts_folder()))
        .await
        .unwrap_err();
    check(&error, "SP_NOT_TRACKED", false, None);
}

// Row: the caller's external-action key was already used.
#[tokio::test]
async fn action_key_used() {
    let env = Env::new().await;
    mount_file(&env, "c4").await;
    status_only(&env, "PUT", &format!("{}/content", item_path()), 200).await;
    let action = push();
    env.journal
        .begin_external_action(ordin_journal::NewExternalAction {
            action_key: action.key,
            run_id: None,
            kind: action.kind,
            destination: "SharePoint".to_owned(),
        })
        .await
        .unwrap();
    let error = env
        .adapter
        .upload_version(&file_ref(), b"x".to_vec(), "c4", action)
        .await
        .unwrap_err();
    check(&error, "SP_ACTION_KEY_USED", false, None);
    assert_eq!(
        env.count("PUT", &format!("{}/content", item_path())).await,
        0
    );
}

// Row: own tables unreachable.
#[tokio::test]
async fn datastore_unavailable() {
    let env = Env::new().await;
    mount_file(&env, "c4").await;
    env.journal.datastore().close().await;
    let error = env
        .adapter
        .upload_version(&file_ref(), b"x".to_vec(), "c4", push())
        .await
        .unwrap_err();
    check(
        &error,
        "SP_DATASTORE_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
    assert_eq!(
        env.count("PUT", &format!("{}/content", item_path())).await,
        0,
        "nothing is sent"
    );
    // The tracking tables are the same row.
    let error = env
        .adapter
        .check_changes(&link_for(&contracts_folder()))
        .await
        .unwrap_err();
    check(
        &error,
        "SP_DATASTORE_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Row: malformed paging cursor.
#[tokio::test]
async fn bad_cursor() {
    let env = Env::new().await;
    let cursor: Cursor = serde_json::from_value(json!("!!not-a-cursor!!")).unwrap();
    let error = env
        .adapter
        .list_children(&contracts_folder(), Some(cursor))
        .await
        .unwrap_err();
    check(
        &error,
        "SRV_BAD_CURSOR",
        false,
        Some(RepairAction::TryAgain),
    );
    assert!(env.requests().await.is_empty());
}
