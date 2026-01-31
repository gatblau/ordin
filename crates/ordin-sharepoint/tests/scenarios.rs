//! X2: one test per Gherkin scenario of SharePointAdapter.md *Acceptance Criteria*, named as the scenario
//! (snake_case, the leading kind kept). The Background is `support::Env`: a Graph stand-in on loopback and
//! Identity returning a Microsoft token.

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ordin_journal::{EntryFilter, NewExternalAction};
use ordin_sharepoint::{SharePointError, SharePointLocation};
use ordin_types::domain::runs::ExternalActionKind;
use ordin_types::time::Timestamp;
use ordin_types::{CodedError, ExternalActionKey};
use serde_json::{json, Value};
use sqlx::Row as _;
use sqlx::SqlitePool;
use support::*;
use wiremock::matchers::{header, method, path, path_regex, query_param};
use wiremock::{Mock, Request, Respond, ResponseTemplate};

fn code<T: std::fmt::Debug>(result: Result<T, SharePointError>) -> &'static str {
    result.expect_err("an error was expected").code()
}

/// The states of every external action, as `kind:state`.
async fn actions(pool: &SqlitePool) -> Vec<String> {
    sqlx::query("SELECT kind, state FROM external_actions ORDER BY sent_at, action_key")
        .fetch_all(pool)
        .await
        .expect("actions")
        .iter()
        .map(|r| format!("{}:{}", r.get::<String, _>(0), r.get::<String, _>(1)))
        .collect()
}

// Scenario: Happy path — find and list a supplier folder
#[tokio::test]
async fn happy_path_find_and_list_a_supplier_folder() {
    let env = Env::new().await;
    get_json(&env, &api("/sites"), 200, json!({"value": [
        {"id": SITE, "displayName": "Procurement", "webUrl": "https://acme.sharepoint.com/sites/Procurement"}
    ]}))
    .await;
    get_json(&env, &api(&format!("/sites/{SITE}/drives")), 200, json!({"value": [
        {"id": DRIVE, "name": "Documents", "webUrl": "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents"}
    ]}))
    .await;
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/v1\.0/drives/b!x9Fq/root/search\(q='supplier%20contracts'\)$",
        ))
        .and(query_param("$top", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
            folder_json(FOLDER, "Contracts", "/Suppliers"),
            file_json(FILE, "supplier contracts.txt", "c1", "/Suppliers", "Sam Lee"),
        ]})))
        .mount(&env.mock)
        .await;
    mount_children(
        &env,
        FOLDER,
        vec![
            file_json("01A", "a.docx", "c1", "/Suppliers/Contracts", "Sam Lee"),
            file_json("01B", "b.docx", "c2", "/Suppliers/Contracts", "Sam Lee"),
            file_json("01C", "c.docx", "c3", "/Suppliers/Contracts", "Sam Lee"),
        ],
        false,
    )
    .await;

    let found = env
        .adapter
        .find_locations("supplier contracts", 10)
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "Suppliers/Contracts");
    assert_eq!(found[0].item_id.as_deref(), Some(FOLDER));
    assert_eq!(found[0].site_name, "Procurement");
    assert!(found[0].is_folder);

    let page = env.adapter.list_children(&found[0], None).await.unwrap();
    assert_eq!(page.items.len(), 3);
    assert!(page
        .items
        .iter()
        .all(|i| i.version.as_ref().is_some_and(|v| !v.tag.is_empty())));
    assert!(page.next_cursor.is_none());
}

// Scenario: Edge case — a listing longer than one Graph page is read to the end
#[tokio::test]
async fn edge_case_a_listing_longer_than_one_graph_page_is_read_to_the_end() {
    let env = Env::new().await;
    let children = api(&format!("/drives/{DRIVE}/items/{FOLDER}/children"));
    let page_of = |from: usize, count: usize, next: Option<&str>| {
        let items: Vec<Value> = (from..from + count)
            .map(|i| {
                file_json(
                    &format!("f{i}"),
                    &format!("file-{i}.docx"),
                    "c1",
                    "/Suppliers/Contracts",
                    "Sam Lee",
                )
            })
            .collect();
        let mut body = json!({"value": items});
        if let Some(token) = next {
            body["@odata.nextLink"] = json!(format!(
                "{}{children}?$top=100&$skiptoken={token}",
                env.mock.uri()
            ));
        }
        body
    };
    for (token, from, next) in [
        ("p2", 100, Some("p3")),
        ("p3", 200, Some("p4")),
        ("p4", 300, Some("p5")),
        ("p5", 400, None),
    ] {
        let count = if next.is_none() { 50 } else { 100 };
        Mock::given(method("GET"))
            .and(path(children.as_str()))
            .and(query_param("$skiptoken", token))
            .respond_with(ResponseTemplate::new(200).set_body_json(page_of(from, count, next)))
            .mount(&env.mock)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(children.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_of(0, 100, Some("p2"))))
        .mount(&env.mock)
        .await;

    let folder = contracts_folder();
    let mut cursor = None;
    let mut total = 0;
    let mut pages = 0;
    loop {
        let page = env.adapter.list_children(&folder, cursor).await.unwrap();
        total += page.items.len();
        pages += 1;
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!((total, pages), (450, 5));
}

// Scenario: Error path — a cursor for another folder is refused
#[tokio::test]
async fn error_path_a_cursor_for_another_folder_is_refused() {
    let env = Env::new().await;
    let children = api(&format!("/drives/{DRIVE}/items/{FOLDER}/children"));
    get_json(
        &env,
        &children,
        200,
        json!({
            "value": [file_json("f1", "a.docx", "c1", "/Suppliers/Contracts", "Sam Lee")],
            "@odata.nextLink": format!("{}{children}?$skiptoken=p2", env.mock.uri()),
        }),
    )
    .await;
    let folder_a = contracts_folder();
    let cursor = env
        .adapter
        .list_children(&folder_a, None)
        .await
        .unwrap()
        .next_cursor
        .expect("a cursor");

    let folder_b = SharePointLocation {
        item_id: Some("01FOLDERB".to_owned()),
        path: "Suppliers/Other".to_owned(),
        ..folder_a
    };
    let before = env.requests().await.len();
    let error = env
        .adapter
        .list_children(&folder_b, Some(cursor))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
    assert_eq!(
        env.requests().await.len(),
        before,
        "no Graph request is sent"
    );
}

/// Records the state of the external action at the moment the PUT arrives.
struct SeesAction {
    handle: tokio::runtime::Handle,
    pool: SqlitePool,
    seen: Arc<Mutex<Vec<String>>>,
    reply: Value,
}

impl Respond for SeesAction {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        // The stand-in answers on its own runtime; the Journal is read from a plain thread on the test's.
        let (handle, pool) = (self.handle.clone(), self.pool.clone());
        let states =
            std::thread::spawn(move || handle.block_on(async move { actions(&pool).await }))
                .join()
                .expect("read the actions");
        self.seen.lock().unwrap().extend(states);
        ResponseTemplate::new(200).set_body_json(self.reply.clone())
    }
}

// Scenario: Happy path — an approved push uploads a new version
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_path_an_approved_push_uploads_a_new_version() {
    let env = Env::new().await;
    let item = api(&format!("/drives/{DRIVE}/items/{FILE}"));
    get_json_once(
        &env,
        &item,
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
        &item,
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
    let seen = Arc::new(Mutex::new(Vec::new()));
    Mock::given(method("PUT"))
        .and(path(format!("{item}/content").as_str()))
        .and(IfMatch(etag_of("c4")))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(SeesAction {
            handle: tokio::runtime::Handle::current(),
            pool: env.pool.clone(),
            seen: Arc::clone(&seen),
            reply: file_json(
                FILE,
                "Kestrel MSA.docx",
                "c5",
                "/Suppliers/Contracts",
                "Sam Lee",
            ),
        })
        .expect(1)
        .mount(&env.mock)
        .await;

    let version = env
        .adapter
        .upload_version(
            &file_ref(),
            b"new text".to_vec(),
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await
        .unwrap();

    assert_eq!(version.tag, "c5", "a new cTag");
    assert_eq!(version.label.as_deref(), Some("4.0"));
    assert_eq!(
        *seen.lock().unwrap(),
        ["origin_push:sent"],
        "begun before the PUT"
    );
    assert_eq!(
        actions(&env.pool).await,
        ["origin_push:confirmed"],
        "confirmed after it"
    );
    let entries = env
        .journal
        .list_entries(
            EntryFilter {
                run_id: None,
                kind: Some("sharepoint_pushed".to_owned()),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(entries.items.len(), 1);
    assert_eq!(
        entries.items[0].data,
        json!({"kind": "origin_push", "version_label": "4.0"})
    );
}

// Scenario: Error path — the origin changed since it was read
#[tokio::test]
async fn error_path_the_origin_changed_since_it_was_read() {
    let env = Env::new().await;
    mount_file(&env, "c5").await;
    let error = env
        .adapter
        .upload_version(
            &file_ref(),
            b"x".to_vec(),
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await
        .unwrap_err();
    match error {
        SharePointError::OriginChanged { current } => assert_eq!(current.tag, "c5"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        env.count(
            "PUT",
            &api(&format!("/drives/{DRIVE}/items/{FILE}/content"))
        )
        .await,
        0
    );
    assert!(
        actions(&env.pool).await.is_empty(),
        "no external action was begun"
    );
}

// Scenario: Error path — the origin changes between the check and the PUT
#[tokio::test]
async fn error_path_the_origin_changes_between_the_check_and_the_put() {
    let env = Env::new().await;
    mount_file(&env, "c4").await;
    let content = api(&format!("/drives/{DRIVE}/items/{FILE}/content"));
    status_only(&env, "PUT", &content, 412).await;
    let error = env
        .adapter
        .upload_version(
            &file_ref(),
            b"x".to_vec(),
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_ORIGIN_CHANGED");
    assert_eq!(env.count("PUT", &content).await, 1, "nothing was resent");
    assert_eq!(
        actions(&env.pool).await,
        ["origin_push:sent"],
        "the Push Service resolves it as not pushed"
    );
}

// Scenario: Error path — a timeout after sending is never retried when the origin changed
#[tokio::test]
async fn error_path_a_timeout_after_sending_is_never_retried_when_the_origin_changed() {
    let env = Env::new().await;
    let item = api(&format!("/drives/{DRIVE}/items/{FILE}"));
    get_json_once(
        &env,
        &item,
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
        &item,
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
        .and(path(format!("{item}/content").as_str()))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
        .mount(&env.mock)
        .await;
    let error = env
        .adapter
        .upload_version(
            &file_ref(),
            b"x".to_vec(),
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_OUTCOME_UNKNOWN");
    assert_eq!(
        env.count("PUT", &format!("{item}/content")).await,
        1,
        "exactly one PUT"
    );
}

// Scenario: Happy path — a copy's first publish creates the file
#[tokio::test]
async fn happy_path_a_copys_first_publish_creates_the_file() {
    let env = Env::new().await;
    let rates = rates_folder();
    Mock::given(method("PUT"))
        .and(path(
            "/v1.0/drives/b!x9Fq/items/01RATES:/Supplier%20day%20rates.docx:/content",
        ))
        .and(query_param("@microsoft.graph.conflictBehavior", "fail"))
        .respond_with(ResponseTemplate::new(201).set_body_json(file_json(
            "01NEW",
            "Supplier day rates.docx",
            "c1",
            "/Rates",
            "Sam Lee",
        )))
        .expect(1)
        .mount(&env.mock)
        .await;
    let (item, version) = env
        .adapter
        .create_file(
            &rates,
            "Supplier day rates.docx",
            b"rates".to_vec(),
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap();
    assert_eq!(item.item_id, "01NEW");
    assert_eq!(version.tag, "c1");
    assert_eq!(actions(&env.pool).await, ["copy_publish:confirmed"]);
}

fn rates_folder() -> SharePointLocation {
    SharePointLocation {
        item_id: Some("01RATES".to_owned()),
        path: "Rates".to_owned(),
        ..contracts_folder()
    }
}

// Scenario: Error path — the name is taken at the destination
#[tokio::test]
async fn error_path_the_name_is_taken_at_the_destination() {
    let env = Env::new().await;
    status_only(
        &env,
        "PUT",
        "/v1.0/drives/b!x9Fq/items/01RATES:/Supplier%20day%20rates.docx:/content",
        409,
    )
    .await;
    let result = env
        .adapter
        .create_file(
            &rates_folder(),
            "Supplier day rates.docx",
            b"x".to_vec(),
            write(ExternalActionKind::CopyPublish),
        )
        .await;
    assert_eq!(code(result), "SP_NAME_EXISTS");
    assert_eq!(
        actions(&env.pool).await,
        ["copy_publish:sent"],
        "nothing was written; the action stays sent"
    );
}

// Scenario: Happy path — a change by someone else is found at the next check
#[tokio::test]
async fn happy_path_a_change_by_someone_else_is_found_at_the_next_check() {
    let env = Env::new().await;
    let link = track_contracts(&env, "c4").await;
    let before = Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 1000).unwrap();
    mount_delta(
        &env,
        "T1",
        vec![
            file_json(
                FILE,
                "Kestrel MSA.docx",
                "c5",
                "/Suppliers/Contracts",
                "Sam Lee",
            ),
            folder_json("01SUB", "Old", "/Suppliers/Contracts"),
            file_json("01ELSE", "Elsewhere.docx", "c1", "/Other", "Sam Lee"),
        ],
        "T2",
        false,
    )
    .await;
    let changes = env.adapter.check_changes(&link).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].origin_path, "Kestrel MSA.docx");
    assert_eq!(changes[0].changed_by.as_deref(), Some("Sam Lee"));
    assert_eq!(changes[0].new_version, "c5");
    assert!(!changes[0].removed);
    let since = env.adapter.changes_since(&[link.id], before).await.unwrap();
    assert_eq!(since.len(), 1);
    assert_eq!(since[0].origin_path, "Kestrel MSA.docx");
}

// Scenario: Edge case — ordin's own push is not reported as a change
#[tokio::test]
async fn edge_case_ordins_own_push_is_not_reported_as_a_change() {
    let env = Env::new().await;
    let link = track_contracts(&env, "c4").await;
    let item = api(&format!("/drives/{DRIVE}/items/{FILE}"));
    get_json_once(
        &env,
        &item,
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
        &item,
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
        .and(path(format!("{item}/content").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(file_json(
            FILE,
            "Kestrel MSA.docx",
            "c5",
            "/Suppliers/Contracts",
            "Sam Lee",
        )))
        .mount(&env.mock)
        .await;
    env.adapter
        .upload_version(
            &file_ref(),
            b"new".to_vec(),
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await
        .unwrap();
    mount_delta(
        &env,
        "T1",
        vec![file_json(
            FILE,
            "Kestrel MSA.docx",
            "c5",
            "/Suppliers/Contracts",
            "Ordin",
        )],
        "T2",
        false,
    )
    .await;
    assert!(env.adapter.check_changes(&link).await.unwrap().is_empty());
}

// Scenario: Edge case — an expired delta token resynchronises
#[tokio::test]
async fn edge_case_an_expired_delta_token_resynchronises() {
    let env = Env::new().await;
    // The first walk holds Kestrel at c4; the second finds it at c5 and Other unchanged.
    mount_origin(&env).await;
    mount_delta(&env, "latest", vec![], "T1", true).await;
    mount_delta(&env, "latest", vec![], "T9", false).await;
    let kestrel = |ctag: &str| {
        file_json(
            FILE,
            "Kestrel MSA.docx",
            ctag,
            "/Suppliers/Contracts",
            "Sam Lee",
        )
    };
    let other = file_json(
        "01OTHER",
        "Other.docx",
        "c1",
        "/Suppliers/Contracts",
        "Sam Lee",
    );
    mount_children(&env, FOLDER, vec![kestrel("c4"), other.clone()], true).await;
    mount_children(&env, FOLDER, vec![kestrel("c5"), other], false).await;
    let link = link_for(&contracts_folder());
    env.adapter.track_link(&link).await.unwrap();
    Mock::given(method("GET"))
        .and(path(api(&format!("/drives/{DRIVE}/root/delta"))))
        .and(query_param("token", "T1"))
        .respond_with(ResponseTemplate::new(410))
        .mount(&env.mock)
        .await;

    let changes = env.adapter.check_changes(&link).await.unwrap();
    assert_eq!(changes.len(), 1, "only the item whose cTag differs");
    assert_eq!(changes[0].origin_path, "Kestrel MSA.docx");
    assert_eq!(changes[0].new_version, "c5");
    // The new starting point is in use: a second check asks T9, which the stand-in answers empty.
    mount_delta(&env, "T9", vec![], "T10", false).await;
    assert!(env.adapter.check_changes(&link).await.unwrap().is_empty());
}

// Scenario: Edge case — permissions cannot be read (C-42 fallback)
#[tokio::test]
async fn edge_case_permissions_cannot_be_read_c_42_fallback() {
    let env = Env::new().await;
    status_only(
        &env,
        "GET",
        &api(&format!("/drives/{DRIVE}/items/{FOLDER}/permissions")),
        403,
    )
    .await;
    let audience = env.adapter.audience(&contracts_folder()).await.unwrap();
    assert_eq!(audience.line, "everyone who can open it");
    assert!(audience.everyone_who_can_open);
    assert!(audience.principals.is_empty());
}

// Scenario: Error path — no Microsoft sign-in
#[tokio::test]
async fn error_path_no_microsoft_sign_in() {
    let env = Env::with(SignIn::Nobody).await;
    let error = env.adapter.find_locations("supplier", 5).await.unwrap_err();
    assert_eq!(error.code(), "SP_SIGN_IN_NEEDED");
    assert_eq!(error.repair(), Some(ordin_types::RepairAction::SignInAgain));
    assert!(env.requests().await.is_empty(), "no Graph request is sent");
}

fn handovers_folder() -> SharePointLocation {
    SharePointLocation {
        item_id: Some("01HAND".to_owned()),
        path: "Handovers".to_owned(),
        ..contracts_folder()
    }
}

// Scenario: Happy path — a handover folder is created with its files
#[tokio::test]
async fn happy_path_a_handover_folder_is_created_with_its_files() {
    let env = Env::new().await;
    Mock::given(method("POST"))
        .and(path("/v1.0/drives/b!x9Fq/items/01HAND/children"))
        .respond_with(ResponseTemplate::new(201).set_body_json(folder_json(
            "01NEWF",
            "2026-01-10-northwind",
            "/Handovers",
        )))
        .mount(&env.mock)
        .await;
    for (id, name) in [("01MD", "handover.md"), ("01YAML", "handover.yaml")] {
        Mock::given(method("PUT"))
            .and(path(
                format!("/v1.0/drives/b!x9Fq/items/01NEWF:/{name}:/content").as_str(),
            ))
            .respond_with(ResponseTemplate::new(201).set_body_json(file_json(
                id,
                name,
                "c1",
                "/Handovers/2026-01-10-northwind",
                "Sam Lee",
            )))
            .mount(&env.mock)
            .await;
    }
    let kind = ExternalActionKind::HandoverWritten;
    let folder = env
        .adapter
        .create_folder(&handovers_folder(), "2026-01-10-northwind", write(kind))
        .await
        .unwrap();
    assert_eq!(folder.path, "Handovers/2026-01-10-northwind");
    assert_eq!(folder.item_id.as_deref(), Some("01NEWF"));
    let md = env
        .adapter
        .create_file(&folder, "handover.md", b"# hand over".to_vec(), write(kind))
        .await
        .unwrap();
    let yaml = env
        .adapter
        .create_file(&folder, "handover.yaml", b"a: 1".to_vec(), write(kind))
        .await
        .unwrap();
    assert_eq!(
        (md.0.item_id.as_str(), yaml.0.item_id.as_str()),
        ("01MD", "01YAML")
    );
    assert_eq!(actions(&env.pool).await, ["handover_written:confirmed"; 3]);
}

// Scenario: Happy path — a handover record is read and written conditionally
#[tokio::test]
async fn happy_path_a_handover_record_is_read_and_written_conditionally() {
    let env = Env::new().await;
    let record = file_json(
        "01YAML",
        "handover.yaml",
        "c2",
        "/Handovers/2026-01-10-northwind",
        "Sam Lee",
    );
    get_json(
        &env,
        "/v1.0/drives/b!x9Fq/items/01HAND:/2026-01-10-northwind/handover.yaml",
        200,
        record.clone(),
    )
    .await;
    get_json(
        &env,
        &api(&format!("/drives/{DRIVE}/items/01YAML")),
        200,
        record.clone(),
    )
    .await;
    get_json(
        &env,
        &api(&format!("/drives/{DRIVE}/items/01YAML/versions")),
        200,
        json!({"value": [{"id": "2.0"}]}),
    )
    .await;
    mount_labels_and_site(&env).await;
    Mock::given(method("PUT"))
        .and(path(
            api(&format!("/drives/{DRIVE}/items/01YAML/content")).as_str(),
        ))
        .and(IfMatch(etag_of("c2")))
        .respond_with(ResponseTemplate::new(200).set_body_json(record))
        .expect(1)
        .mount(&env.mock)
        .await;

    let found = env
        .adapter
        .item_at(&handovers_folder(), "2026-01-10-northwind/handover.yaml")
        .await
        .unwrap()
        .expect("the record");
    assert_eq!(found.version.as_ref().map(|v| v.tag.as_str()), Some("c2"));
    env.adapter
        .upload_version(
            &found.item,
            b"appended".to_vec(),
            "c2",
            write(ExternalActionKind::HandoverWritten),
        )
        .await
        .unwrap();
}

// Scenario: Edge case — item_at on a missing path
#[tokio::test]
async fn edge_case_item_at_on_a_missing_path() {
    let env = Env::new().await;
    status_only(
        &env,
        "GET",
        "/v1.0/drives/b!x9Fq/items/01HAND:/2026-01-10-northwind/missing.yaml",
        404,
    )
    .await;
    let found = env
        .adapter
        .item_at(&handovers_folder(), "2026-01-10-northwind/missing.yaml")
        .await
        .unwrap();
    assert!(found.is_none());
}

// Scenario: Happy path — a published copy is removed to the recycle bin
#[tokio::test]
async fn happy_path_a_published_copy_is_removed_to_the_recycle_bin() {
    let env = Env::new().await;
    mount_file(&env, "c3").await;
    Mock::given(method("DELETE"))
        .and(path(api(&format!("/drives/{DRIVE}/items/{FILE}")).as_str()))
        .and(IfMatch(etag_of("c3")))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&env.mock)
        .await;
    env.adapter
        .delete(
            &file_ref(),
            Some("c3"),
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap();
    assert_eq!(actions(&env.pool).await, ["copy_publish:confirmed"]);
}

// Scenario: Error path — a copy changed at its destination is not removed
#[tokio::test]
async fn error_path_a_copy_changed_at_its_destination_is_not_removed() {
    let env = Env::new().await;
    mount_file(&env, "c4").await;
    let error = env
        .adapter
        .delete(
            &file_ref(),
            Some("c3"),
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_ORIGIN_CHANGED");
    assert_eq!(
        env.count("DELETE", &api(&format!("/drives/{DRIVE}/items/{FILE}")))
            .await,
        0
    );
}

// Scenario: Error path — a caller that opened the record itself is refused
#[tokio::test]
async fn error_path_a_caller_that_opened_the_record_itself_is_refused() {
    let env = Env::new().await;
    let key = ExternalActionKey::new();
    env.journal
        .begin_external_action(NewExternalAction {
            action_key: key,
            run_id: None,
            kind: ExternalActionKind::CopyPublish,
            destination: "SharePoint".to_owned(),
        })
        .await
        .unwrap();
    let action = ordin_sharepoint::ExternalWrite {
        key,
        run_id: None,
        kind: ExternalActionKind::CopyPublish,
    };
    let result = env
        .adapter
        .create_file(
            &rates_folder(),
            "Supplier day rates.docx",
            b"x".to_vec(),
            action,
        )
        .await;
    assert_eq!(code(result), "SP_ACTION_KEY_USED");
    assert!(env.requests().await.is_empty(), "nothing is sent");
}

// Scenario: Error path — throttled beyond the retries
#[tokio::test]
async fn error_path_throttled_beyond_the_retries() {
    let env = Env::with_scale(SignIn::Token, 0.01).await;
    let item = api(&format!("/drives/{DRIVE}/items/{FILE}"));
    Mock::given(method("GET"))
        .and(path(item.as_str()))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "2"))
        .mount(&env.mock)
        .await;
    let started = Instant::now();
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    assert!(
        matches!(error, SharePointError::Throttled { retry_after_s: 2 }),
        "{error:?}"
    );
    assert_eq!(
        env.count("GET", &item).await,
        4,
        "the first attempt and three retries"
    );
    // Three waits of 2 s, scaled by 0.01 in the stand-in: at least 60 ms in all.
    assert!(
        started.elapsed() >= Duration::from_millis(55),
        "{:?}",
        started.elapsed()
    );
}

// Scenario: Happy path — a handover place given as a web address is resolved
#[tokio::test]
async fn happy_path_a_handover_place_given_as_a_web_address_is_resolved() {
    let env = Env::new().await;
    let address = "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents/Handovers";
    let share = format!("u!{}", URL_SAFE_NO_PAD.encode(address));
    get_json(
        &env,
        &api(&format!("/shares/{share}/driveItem")),
        200,
        folder_json("01HAND", "Handovers", ""),
    )
    .await;
    mount_labels_and_site(&env).await;
    let found = env.adapter.resolve_web_url(address).await.unwrap();
    assert_eq!(found.path, "Handovers");
    assert_eq!(found.item_id.as_deref(), Some("01HAND"));
    assert_eq!(found.site_name, "Procurement");
    assert_eq!(found.drive_name, "Documents");
}

// Scenario: Error path — a web address outside SharePoint is refused
#[tokio::test]
async fn error_path_a_web_address_outside_sharepoint_is_refused() {
    let env = Env::new().await;
    let result = env.adapter.resolve_web_url("https://example.com/x").await;
    assert_eq!(code(result), "SP_LOCATION_INVALID");
    assert!(env.requests().await.is_empty(), "no Graph request is sent");
}
