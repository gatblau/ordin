//! Paths of Internal Logic 2 to 9 that the Gherkin scenarios and the Error Table rows do not reach:
//! removals and moves in the delta, a file link, the unknown-outcome checks of every write, the audience
//! lines and the lookups.

mod support;

use ordin_sharepoint::{LinkScope, SharePointLocation};
use ordin_types::domain::places::SourceLinkKind;
use ordin_types::domain::runs::ExternalActionKind;
use ordin_types::time::Timestamp;
use ordin_types::CodedError;
use serde_json::json;
use sqlx::Row as _;
use support::*;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

fn item_path() -> String {
    api(&format!("/drives/{DRIVE}/items/{FILE}"))
}

fn kestrel(ctag: &str) -> serde_json::Value {
    file_json(
        FILE,
        "Kestrel MSA.docx",
        ctag,
        "/Suppliers/Contracts",
        "Sam Lee",
    )
}

fn slow(status: u16) -> ResponseTemplate {
    ResponseTemplate::new(status).set_delay(std::time::Duration::from_secs(3))
}

fn rates() -> SharePointLocation {
    SharePointLocation {
        item_id: Some("01RATES".to_owned()),
        path: "Rates".to_owned(),
        ..contracts_folder()
    }
}

#[tokio::test]
async fn a_removal_is_reported_once_and_a_return_is_reported_as_a_change() {
    let env = Env::new().await;
    let link = track_contracts(&env, "c4").await;
    mount_delta(
        &env,
        "T1",
        vec![json!({"id": FILE, "deleted": {}})],
        "T2",
        true,
    )
    .await;
    let changes = env.adapter.check_changes(&link).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert!(changes[0].removed);
    assert_eq!(changes[0].new_version, "");
    assert_eq!(changes[0].origin_path, "Kestrel MSA.docx");
    // The same deletion again, and a deletion of something never held: nothing new.
    mount_delta(
        &env,
        "T2",
        vec![
            json!({"id": FILE, "deleted": {}}),
            json!({"id": "01UNKNOWN", "deleted": {}}),
        ],
        "T3",
        true,
    )
    .await;
    assert!(env.adapter.check_changes(&link).await.unwrap().is_empty());
    // The file comes back at the cTag it had: it is a change, because it was removed.
    mount_delta(&env, "T3", vec![kestrel("c4")], "T4", true).await;
    let changes = env.adapter.check_changes(&link).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert!(!changes[0].removed);
    // Seen again unchanged, with only its eTag moved: no change.
    let mut touched = kestrel("c4");
    touched["eTag"] = json!("\"{6D1A},9\"");
    mount_delta(&env, "T4", vec![touched], "T5", true).await;
    assert!(env.adapter.check_changes(&link).await.unwrap().is_empty());
}

#[tokio::test]
async fn an_item_moved_out_of_the_link_is_reported_as_removed() {
    let env = Env::new().await;
    let link = track_contracts(&env, "c4").await;
    let moved = file_json(FILE, "Kestrel MSA.docx", "c4", "/Archive", "Sam Lee");
    mount_delta(&env, "T1", vec![moved], "T2", true).await;
    let changes = env.adapter.check_changes(&link).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert!(changes[0].removed);
    assert_eq!(changes[0].origin_path, "Kestrel MSA.docx");
    // Moved again, still outside: nothing more to say.
    let moved = file_json(FILE, "Kestrel MSA.docx", "c4", "/Elsewhere", "Sam Lee");
    mount_delta(&env, "T2", vec![moved], "T3", true).await;
    assert!(env.adapter.check_changes(&link).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_new_file_and_an_entry_without_a_path_are_placed_by_their_metadata() {
    let env = Env::new().await;
    let link = track_contracts(&env, "c4").await;
    // Graph may omit `parentReference.path` on a delta entry; the adapter asks for the item.
    let mut bare = file_json(
        "01NEWFILE",
        "New.docx",
        "c1",
        "/Suppliers/Contracts/Old",
        "Sam Lee",
    );
    bare["parentReference"] = json!({"driveId": DRIVE});
    let mut gone = file_json(
        "01GONE",
        "Gone.docx",
        "c1",
        "/Suppliers/Contracts",
        "Sam Lee",
    );
    gone["parentReference"] = json!({"driveId": DRIVE});
    get_json(
        &env,
        &api(&format!("/drives/{DRIVE}/items/01NEWFILE")),
        200,
        file_json(
            "01NEWFILE",
            "New.docx",
            "c1",
            "/Suppliers/Contracts/Old",
            "Sam Lee",
        ),
    )
    .await;
    status_only(
        &env,
        "GET",
        &api(&format!("/drives/{DRIVE}/items/01GONE")),
        404,
    )
    .await;
    let long_name = "n".repeat(2100);
    let long = file_json(
        "01LONG",
        &long_name,
        "c1",
        "/Suppliers/Contracts",
        "Sam Lee",
    );
    mount_delta(&env, "T1", vec![bare, gone, long], "T2", true).await;
    let changes = env.adapter.check_changes(&link).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].origin_path, "Old/New.docx");
}

#[tokio::test]
async fn a_delta_that_pages_is_followed_and_a_link_to_another_host_is_refused() {
    let env = Env::new().await;
    let link = track_contracts(&env, "c4").await;
    let delta = api(&format!("/drives/{DRIVE}/root/delta"));
    Mock::given(method("GET"))
        .and(path(delta.as_str()))
        .and(query_param("token", "T1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": [],
            "@odata.nextLink": format!("{}{delta}?token=page2", env.mock.uri()),
        })))
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    mount_delta(&env, "page2", vec![kestrel("c5")], "T2", true).await;
    assert_eq!(env.adapter.check_changes(&link).await.unwrap().len(), 1);

    Mock::given(method("GET"))
        .and(path(delta.as_str()))
        .and(query_param("token", "T2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": [],
            "@odata.nextLink": "https://evil.example/v1.0/steal",
        })))
        .mount(&env.mock)
        .await;
    let error = env.adapter.check_changes(&link).await.unwrap_err();
    assert_eq!(error.code(), "SP_RESPONSE_INVALID");
    assert_eq!(env.count("GET", "/v1.0/steal").await, 0);
    let last_error: Option<String> = sqlx::query("SELECT last_error FROM sharepoint_link_state")
        .fetch_one(&env.pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        last_error.as_deref(),
        Some("SP_RESPONSE_INVALID"),
        "the code only"
    );
}

#[tokio::test]
async fn a_delta_with_neither_next_nor_delta_link_is_invalid() {
    let env = Env::new().await;
    let link = track_contracts(&env, "c4").await;
    get_json(
        &env,
        &api(&format!("/drives/{DRIVE}/root/delta")),
        200,
        json!({"value": []}),
    )
    .await;
    let error = env.adapter.check_changes(&link).await.unwrap_err();
    assert_eq!(error.code(), "SP_RESPONSE_INVALID");
    // Tracking has the same refusal.
    let fresh = Env::new().await;
    mount_origin(&fresh).await;
    get_json(
        &fresh,
        &api(&format!("/drives/{DRIVE}/root/delta")),
        200,
        json!({"value": []}),
    )
    .await;
    let error = fresh
        .adapter
        .track_link(&link_for(&contracts_folder()))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_RESPONSE_INVALID");
}

#[tokio::test]
async fn a_file_link_reports_its_own_change_by_name() {
    let env = Env::new().await;
    // The link is the file itself: its location is the file, its path prefix the file's path.
    get_json(&env, &item_path(), 200, kestrel("c4")).await;
    mount_labels_and_site(&env).await;
    mount_delta(&env, "latest", vec![], "T1", true).await;
    let location = env
        .adapter
        .resolve_origin(&format!("graph:drives/{DRIVE}/items/{FILE}"))
        .await
        .unwrap();
    assert!(!location.is_folder);
    assert_eq!(location.path, "Suppliers/Contracts/Kestrel MSA.docx");
    let link = link_for(&location);
    env.adapter.track_link(&link).await.unwrap();
    mount_delta(&env, "T1", vec![kestrel("c5")], "T2", true).await;
    let changes = env.adapter.check_changes(&link).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].origin_path, "Kestrel MSA.docx");
}

#[tokio::test]
async fn tracking_walks_folders_to_a_depth_and_untracking_removes_the_rows() {
    let env = Env::new().await;
    mount_origin(&env).await;
    mount_delta(&env, "latest", vec![], "T1", false).await;
    mount_children(
        &env,
        FOLDER,
        vec![
            kestrel("c4"),
            folder_json("01SUB", "Old", "/Suppliers/Contracts"),
        ],
        false,
    )
    .await;
    mount_children(
        &env,
        "01SUB",
        vec![file_json(
            "01DEEP",
            "Deep.docx",
            "c1",
            "/Suppliers/Contracts/Old",
            "Sam Lee",
        )],
        false,
    )
    .await;
    let link = link_for(&contracts_folder());
    env.adapter.track_link(&link).await.unwrap();
    assert_eq!(env.rows("sharepoint_link_state").await, 1);
    assert_eq!(
        env.rows("sharepoint_items").await,
        2,
        "files only, from every level"
    );
    // Tracking again replaces the rows rather than adding to them.
    env.adapter.track_link(&link).await.unwrap();
    assert_eq!(env.rows("sharepoint_items").await, 2);
    mount_delta(&env, "T1", vec![kestrel("c5")], "T2", true).await;
    env.adapter.check_changes(&link).await.unwrap();
    assert_eq!(env.rows("sharepoint_origin_changes").await, 1);

    env.adapter.untrack_link(link.id).await.unwrap();
    assert_eq!(env.rows("sharepoint_link_state").await, 0);
    assert_eq!(env.rows("sharepoint_items").await, 0);
    assert_eq!(env.rows("sharepoint_origin_changes").await, 0);
    assert_eq!(
        env.adapter.check_changes(&link).await.unwrap_err().code(),
        "SP_NOT_TRACKED"
    );
}

#[tokio::test]
async fn only_a_sharepoint_link_can_be_tracked() {
    let env = Env::new().await;
    let mut link = link_for(&contracts_folder());
    link.kind = SourceLinkKind::LocalFolder;
    assert_eq!(
        env.adapter.track_link(&link).await.unwrap_err().code(),
        "SP_LOCATION_INVALID"
    );
    assert!(env.requests().await.is_empty());
}

#[tokio::test]
async fn origin_changes_older_than_ninety_days_are_deleted_at_each_check() {
    let env = Env::new().await;
    let link = track_contracts(&env, "c4").await;
    let old =
        Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 91 * 86_400_000).unwrap();
    let recent =
        Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 89 * 86_400_000).unwrap();
    for (n, at) in [("old", old), ("recent", recent)] {
        sqlx::query(
            "INSERT INTO sharepoint_origin_changes (id, source_link_id, origin_path, new_version, changed_at, removed, found_at) \
             VALUES (?1, ?2, ?3, 'c1', ?4, 0, ?4)",
        )
        .bind(format!("id-{n}"))
        .bind(link.id.to_string())
        .bind(n)
        .bind(at.to_wire())
        .execute(&env.pool)
        .await
        .unwrap();
    }
    mount_delta(&env, "T1", vec![], "T2", true).await;
    env.adapter.check_changes(&link).await.unwrap();
    let kept: Vec<String> = sqlx::query("SELECT origin_path FROM sharepoint_origin_changes")
        .fetch_all(&env.pool)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(kept, ["recent"]);
}

#[tokio::test]
async fn changes_since_merges_links_newest_first_and_ignores_older_marks() {
    let env = Env::new().await;
    let a = track_contracts(&env, "c4").await;
    assert!(env
        .adapter
        .changes_since(&[], Timestamp::now())
        .await
        .unwrap()
        .is_empty());
    let long_ago = Timestamp::from_unix_millis(1_000).unwrap();
    for (link, path, found) in [
        (a.id, "first", "2026-01-10T10:00:00.000Z"),
        (a.id, "second", "2026-01-10T11:00:00.000Z"),
    ] {
        sqlx::query(
            "INSERT INTO sharepoint_origin_changes (id, source_link_id, origin_path, new_version, changed_at, removed, found_at) \
             VALUES (?1, ?2, ?3, 'c1', ?4, 0, ?4)",
        )
        .bind(format!("id-{path}"))
        .bind(link.to_string())
        .bind(path)
        .bind(found)
        .execute(&env.pool)
        .await
        .unwrap();
    }
    let got = env.adapter.changes_since(&[a.id], long_ago).await.unwrap();
    let paths: Vec<&str> = got.iter().map(|c| c.origin_path.as_str()).collect();
    assert_eq!(paths, ["second", "first"]);
    let after = ordin_types::time::parse_rfc3339_ms_utc("2026-01-10T10:30:00.000Z").unwrap();
    let got = env
        .adapter
        .changes_since(&[a.id], Timestamp(after))
        .await
        .unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].origin_path, "second");
}

#[tokio::test]
async fn a_stale_server_answer_for_a_listing_row_without_tags_is_invalid() {
    let env = Env::new().await;
    let mut bare = kestrel("c1");
    bare.as_object_mut().unwrap().remove("cTag");
    mount_children(&env, FOLDER, vec![bare], false).await;
    let error = env
        .adapter
        .list_children(&contracts_folder(), None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_RESPONSE_INVALID");
    // A file location and a folder-less fetch are refused before anything is read.
    let mut file_loc = contracts_folder();
    file_loc.is_folder = false;
    assert_eq!(
        env.adapter
            .list_children(&file_loc, None)
            .await
            .unwrap_err()
            .code(),
        "SP_LOCATION_INVALID"
    );
    assert_eq!(
        env.adapter
            .item_at(&file_loc, "a.docx")
            .await
            .unwrap_err()
            .code(),
        "SP_LOCATION_INVALID"
    );
    assert_eq!(
        env.adapter
            .item_at(&contracts_folder(), "a//b")
            .await
            .unwrap_err()
            .code(),
        "SP_LOCATION_INVALID"
    );
}

#[tokio::test]
async fn the_library_root_lists_and_a_folder_item_has_no_version() {
    let env = Env::new().await;
    get_json(
        &env,
        &api(&format!("/drives/{DRIVE}/root/children")),
        200,
        json!({"value": [folder_json("01SUB", "Suppliers", ""), kestrel("c4")]}),
    )
    .await;
    let root = SharePointLocation {
        item_id: None,
        path: String::new(),
        ..contracts_folder()
    };
    let page = env.adapter.list_children(&root, None).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(page.items[0].is_folder);
    assert_eq!(page.items[0].path, "Suppliers/");
    assert!(page.items[0].version.is_none());
    assert!(!page.items[1].is_folder);
    // The root's cursor names `root`, so it cannot be used on a folder.
    get_json(
        &env,
        &api(&format!("/drives/{DRIVE}/items/{FOLDER}/children")),
        200,
        json!({"value": []}),
    )
    .await;
}

#[tokio::test]
async fn find_locations_orders_libraries_before_folders_follows_pages_and_cuts_at_the_limit() {
    let env = Env::new().await;
    let sites = api("/sites");
    Mock::given(method("GET"))
        .and(path(sites.as_str()))
        .and(query_param("search", "supplier's"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": [{"id": SITE, "displayName": "Procurement"}],
            "@odata.nextLink": format!("{}{sites}?search=supplier%27s&page=2", env.mock.uri()),
        })))
        .mount(&env.mock)
        .await;
    Mock::given(method("GET"))
        .and(path(sites.as_str()))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "value": [{"id": "other.sharepoint.com,1,2", "displayName": "Legal"}]
        })))
        .with_priority(1)
        .mount(&env.mock)
        .await;
    get_json(
        &env,
        &api(&format!("/sites/{SITE}/drives")),
        200,
        json!({
            "value": [
                {"id": DRIVE, "name": "Supplier's files", "webUrl": "https://acme.sharepoint.com/a"},
                {"id": "b!second", "name": "Archive"}
            ],
            "@odata.nextLink": format!("{}/v1.0/sites/{SITE}/drives?page=2", env.mock.uri()),
        }),
    )
    .await;
    // The second page of libraries ends the list (the mock above answers the first and the next alike
    // unless told otherwise, so serve the next link from a more specific mock first).
    Mock::given(method("GET"))
        .and(path(api(&format!("/sites/{SITE}/drives")).as_str()))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": []})))
        .with_priority(1)
        .mount(&env.mock)
        .await;
    get_json(
        &env,
        &api("/sites/other.sharepoint.com,1,2/drives"),
        200,
        json!({"value": []}),
    )
    .await;
    Mock::given(method("GET"))
        .and(wiremock::matchers::path_regex(
            r"^/v1\.0/drives/[^/]+/root/search\(q='supplier''s'\)$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
            folder_json("01Z", "Zeta", "/Suppliers"),
            folder_json("01A", "Alpha", "/Suppliers"),
            kestrel("c1"),
        ]})))
        .mount(&env.mock)
        .await;

    let found = env
        .adapter
        .find_locations("  supplier's ", 50)
        .await
        .unwrap();
    // One library named for the search, then the folders of the libraries, each ordered by name.
    assert_eq!(found[0].drive_name, "Supplier's files");
    assert_eq!(found[0].item_id, None);
    assert_eq!(found[0].path, "");
    let paths: Vec<&str> = found[1..].iter().map(|l| l.path.as_str()).collect();
    assert!(
        paths
            .windows(2)
            .all(|w| w[0].to_lowercase() <= w[1].to_lowercase()),
        "{paths:?}"
    );
    assert!(found
        .iter()
        .skip(1)
        .all(|l| l.item_id.is_some() && l.is_folder));
    let cut = env.adapter.find_locations("supplier's", 3).await.unwrap();
    assert_eq!(cut.len(), 3);
    let one = env.adapter.find_locations("supplier's", 0).await.unwrap();
    assert_eq!(one.len(), 1, "a limit of 0 is raised to 1");
}

#[tokio::test]
async fn resolve_origin_reads_the_item_its_drive_and_its_site() {
    let env = Env::new().await;
    mount_origin(&env).await;
    let loc = env
        .adapter
        .resolve_origin(&format!("graph:drives/{DRIVE}/items/{FOLDER}"))
        .await
        .unwrap();
    assert_eq!(loc, contracts_folder_with_web());
    assert_eq!(
        ordin_sharepoint::SharePointAdapter::origin_location(&loc),
        format!("graph:drives/{DRIVE}/items/{FOLDER}")
    );
    // A reply without a site id cannot be turned into a location.
    let bare = Env::new().await;
    let mut item = folder_json(FOLDER, "Contracts", "/Suppliers");
    item["parentReference"] =
        json!({"driveId": DRIVE, "path": format!("/drives/{DRIVE}/root:/Suppliers")});
    get_json(
        &bare,
        &api(&format!("/drives/{DRIVE}/items/{FOLDER}")),
        200,
        item,
    )
    .await;
    assert_eq!(
        bare.adapter
            .resolve_origin(&format!("graph:drives/{DRIVE}/items/{FOLDER}"))
            .await
            .unwrap_err()
            .code(),
        "SP_RESPONSE_INVALID"
    );
}

fn contracts_folder_with_web() -> SharePointLocation {
    SharePointLocation {
        web_url:
            "https://acme.sharepoint.com/sites/Procurement/Shared%20Documents/Suppliers/Contracts"
                .to_owned(),
        ..contracts_folder()
    }
}

#[tokio::test]
async fn the_library_root_resolves_without_an_item_id() {
    let env = Env::new().await;
    let mut root = folder_json("01ROOT", "root", "");
    root["parentReference"] = json!({"driveId": DRIVE, "siteId": SITE});
    get_json(&env, &api(&format!("/drives/{DRIVE}/root")), 200, root).await;
    mount_labels_and_site(&env).await;
    let loc = env
        .adapter
        .resolve_origin(&format!("graph:drives/{DRIVE}/items/root"))
        .await
        .unwrap();
    assert_eq!(
        (loc.item_id, loc.path.as_str(), loc.is_folder),
        (None, "", true)
    );
}

#[tokio::test]
async fn exists_answers_true_and_false_and_item_at_reads_a_folder() {
    let env = Env::new().await;
    get_json(
        &env,
        "/v1.0/drives/b!x9Fq/items/01ABCDEF2GHIJ:/there.docx",
        200,
        file_json("01T", "there.docx", "c1", "/Suppliers/Contracts", "Sam Lee"),
    )
    .await;
    assert!(env
        .adapter
        .exists(&contracts_folder(), "there.docx")
        .await
        .unwrap());
    assert!(!env
        .adapter
        .exists(&contracts_folder(), "absent.docx")
        .await
        .unwrap());
    let root = SharePointLocation {
        item_id: None,
        path: String::new(),
        ..contracts_folder()
    };
    get_json(
        &env,
        "/v1.0/drives/b!x9Fq/root:/Suppliers",
        200,
        folder_json("01S", "Suppliers", ""),
    )
    .await;
    let found = env
        .adapter
        .item_at(&root, "Suppliers")
        .await
        .unwrap()
        .unwrap();
    assert!(found.is_folder);
    assert_eq!(found.path, "Suppliers/");
    assert!(found.version.is_none());
}

#[tokio::test]
async fn a_folder_is_created_in_the_library_root_and_a_name_with_a_slash_is_refused() {
    let env = Env::new().await;
    let root = SharePointLocation {
        item_id: None,
        path: String::new(),
        ..contracts_folder()
    };
    Mock::given(method("POST"))
        .and(path("/v1.0/drives/b!x9Fq/root/children"))
        .respond_with(ResponseTemplate::new(201).set_body_json(folder_json(
            "01NEW",
            "Handovers",
            "",
        )))
        .mount(&env.mock)
        .await;
    let action = write(ExternalActionKind::HandoverWritten);
    let created = env
        .adapter
        .create_folder(&root, "Handovers", action)
        .await
        .unwrap();
    assert_eq!(created.path, "Handovers");
    let body: serde_json::Value = serde_json::from_slice(
        &env.mock
            .received_requests()
            .await
            .unwrap()
            .iter()
            .find(|r| r.method.as_str() == "POST")
            .unwrap()
            .body,
    )
    .unwrap();
    assert_eq!(
        body,
        json!({"name": "Handovers", "folder": {}, "@microsoft.graph.conflictBehavior": "fail"})
    );
    let mut file_parent = root.clone();
    file_parent.is_folder = false;
    for (parent, name) in [(&file_parent, "ok"), (&root, "a/b")] {
        let error = env
            .adapter
            .create_folder(parent, name, write(ExternalActionKind::HandoverWritten))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "SP_LOCATION_INVALID");
    }
    let error = env
        .adapter
        .create_file(
            &file_parent,
            "a.docx",
            vec![],
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_LOCATION_INVALID");
}

#[tokio::test]
async fn a_create_whose_connection_is_lost_is_retried_once_when_the_name_is_absent() {
    let env = Env::new().await;
    let put = "/v1.0/drives/b!x9Fq/items/01RATES:/a.docx:/content";
    Mock::given(method("PUT"))
        .and(path(put))
        .respond_with(slow(201))
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    Mock::given(method("PUT"))
        .and(path(put))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(file_json("01N", "a.docx", "c1", "/Rates", "Sam Lee")),
        )
        .mount(&env.mock)
        .await;
    let (item, _) = env
        .adapter
        .create_file(
            &rates(),
            "a.docx",
            b"x".to_vec(),
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap();
    assert_eq!(item.item_id, "01N");
    assert_eq!(env.count("PUT", put).await, 2);
}

#[tokio::test]
async fn a_create_whose_connection_is_lost_is_unknown_when_the_name_is_present() {
    let env = Env::new().await;
    let put = "/v1.0/drives/b!x9Fq/items/01RATES:/a.docx:/content";
    Mock::given(method("PUT"))
        .and(path(put))
        .respond_with(slow(201))
        .mount(&env.mock)
        .await;
    get_json(
        &env,
        "/v1.0/drives/b!x9Fq/items/01RATES:/a.docx",
        200,
        file_json("01N", "a.docx", "c1", "/Rates", "Sam Lee"),
    )
    .await;
    let error = env
        .adapter
        .create_file(
            &rates(),
            "a.docx",
            b"x".to_vec(),
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_OUTCOME_UNKNOWN");
    assert_eq!(env.count("PUT", put).await, 1, "never resent");
}

#[tokio::test]
async fn a_folder_create_whose_connection_is_lost_is_checked_then_retried_or_unknown() {
    let env = Env::new().await;
    let post = "/v1.0/drives/b!x9Fq/items/01RATES/children";
    Mock::given(method("POST"))
        .and(path(post))
        .respond_with(slow(201))
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    Mock::given(method("POST"))
        .and(path(post))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(folder_json("01NEWF", "sub", "/Rates")),
        )
        .mount(&env.mock)
        .await;
    let created = env
        .adapter
        .create_folder(&rates(), "sub", write(ExternalActionKind::HandoverWritten))
        .await
        .unwrap();
    assert_eq!(created.item_id.as_deref(), Some("01NEWF"));

    // Present after the loss: the outcome is unknown.
    let env = Env::new().await;
    Mock::given(method("POST"))
        .and(path(post))
        .respond_with(slow(201))
        .mount(&env.mock)
        .await;
    get_json(
        &env,
        "/v1.0/drives/b!x9Fq/items/01RATES:/sub",
        200,
        folder_json("01NEWF", "sub", "/Rates"),
    )
    .await;
    let error = env
        .adapter
        .create_folder(&rates(), "sub", write(ExternalActionKind::HandoverWritten))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_OUTCOME_UNKNOWN");
}

#[tokio::test]
async fn a_delete_whose_connection_is_lost_is_settled_by_reading_the_item() {
    // Gone afterwards: the removal landed.
    let env = Env::new().await;
    get_json_once(&env, &item_path(), 200, kestrel("c3")).await;
    mount_labels_and_site(&env).await;
    status_only(&env, "GET", &item_path(), 404).await;
    Mock::given(method("DELETE"))
        .and(path(item_path().as_str()))
        .respond_with(slow(204))
        .mount(&env.mock)
        .await;
    env.adapter
        .delete(&file_ref(), None, write(ExternalActionKind::CopyPublish))
        .await
        .unwrap();
    let state: String = sqlx::query("SELECT state FROM external_actions")
        .fetch_one(&env.pool)
        .await
        .unwrap()
        .get(0);
    assert_eq!(state, "confirmed");

    // Still there with the same cTag: retried once, then unknown if lost again.
    let env = Env::new().await;
    mount_file(&env, "c3").await;
    Mock::given(method("DELETE"))
        .and(path(item_path().as_str()))
        .respond_with(slow(204))
        .mount(&env.mock)
        .await;
    let error = env
        .adapter
        .delete(
            &file_ref(),
            Some("c3"),
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_OUTCOME_UNKNOWN");
    assert_eq!(
        env.count("DELETE", &item_path()).await,
        2,
        "one retry, no more"
    );

    // Changed afterwards: unknown at once. A 412 is an origin change.
    let env = Env::new().await;
    get_json_once(&env, &item_path(), 200, kestrel("c3")).await;
    get_json(&env, &item_path(), 200, kestrel("c9")).await;
    mount_labels_and_site(&env).await;
    Mock::given(method("DELETE"))
        .and(path(item_path().as_str()))
        .respond_with(slow(204))
        .mount(&env.mock)
        .await;
    let error = env
        .adapter
        .delete(
            &file_ref(),
            Some("c3"),
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_OUTCOME_UNKNOWN");
    let env = Env::new().await;
    mount_file(&env, "c3").await;
    status_only(&env, "DELETE", &item_path(), 412).await;
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
}

#[tokio::test]
async fn a_delete_of_a_missing_item_is_not_found_and_sends_nothing() {
    let env = Env::new().await;
    status_only(&env, "GET", &item_path(), 404).await;
    let error = env
        .adapter
        .delete(&file_ref(), None, write(ExternalActionKind::CopyPublish))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_NOT_FOUND");
    assert_eq!(env.rows("external_actions").await, 0);
}

#[tokio::test]
async fn an_upload_whose_connection_is_lost_is_retried_once_when_the_origin_is_unchanged() {
    let env = Env::new().await;
    mount_file(&env, "c4").await;
    let put = format!("{}/content", item_path());
    Mock::given(method("PUT"))
        .and(path(put.as_str()))
        .respond_with(slow(200))
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    Mock::given(method("PUT"))
        .and(path(put.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(kestrel("c5")))
        .mount(&env.mock)
        .await;
    let version = env
        .adapter
        .upload_version(
            &file_ref(),
            b"x".to_vec(),
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await
        .unwrap();
    // The item still reads c4 in the stand-in, so the fresh read is what is returned.
    assert_eq!(version.tag, "c4");
    assert_eq!(
        env.count("PUT", &put).await,
        2,
        "the same key, sent once more"
    );

    // Lost twice: unknown.
    let env = Env::new().await;
    mount_file(&env, "c4").await;
    Mock::given(method("PUT"))
        .and(path(put.as_str()))
        .respond_with(slow(200))
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
    assert_eq!(env.count("PUT", &put).await, 2);
}

#[tokio::test]
async fn a_push_whose_bookkeeping_fails_after_graph_accepted_still_succeeds() {
    let env = Env::new().await;
    mount_file(&env, "c4").await;
    let put = format!("{}/content", item_path());
    // The Journal is closed by the PUT's own answer: begin has happened, confirm and the entry fail.
    let datastore = env.journal.datastore().clone();
    struct ClosesJournal(ordin_journal::Datastore, serde_json::Value);
    impl wiremock::Respond for ClosesJournal {
        fn respond(&self, _: &wiremock::Request) -> ResponseTemplate {
            let store = self.0.clone();
            std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async move { store.close().await })
            })
            .join()
            .unwrap();
            ResponseTemplate::new(200).set_body_json(self.1.clone())
        }
    }
    Mock::given(method("PUT"))
        .and(path(put.as_str()))
        .respond_with(ClosesJournal(datastore, kestrel("c5")))
        .mount(&env.mock)
        .await;
    let version = env
        .adapter
        .upload_version(
            &file_ref(),
            b"x".to_vec(),
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await;
    assert!(version.is_ok(), "{version:?}");
}

#[tokio::test]
async fn an_audience_reads_groups_people_and_sharing_links() {
    let env = Env::new().await;
    let permissions = api(&format!("/drives/{DRIVE}/items/{FOLDER}/permissions"));
    get_json(&env, &permissions, 200, json!({"value": [
        {"grantedToV2": {"siteGroup": {"id": "g1", "displayName": "Procurement team"}}},
        {"grantedToIdentitiesV2": [{"user": {"id": "u1"}}, {"user": {"id": "u2"}}, {"user": {"id": "u3"}}]},
        {"link": {"scope": "users"}}
    ]}))
    .await;
    let audience = env.adapter.audience(&contracts_folder()).await.unwrap();
    assert_eq!(audience.line, "Procurement team and 3 people");
    assert_eq!(audience.principals, ["g1", "u1", "u2", "u3"]);
    assert_eq!(audience.link_scope, Some(LinkScope::Users));
    assert!(!audience.everyone_who_can_open);

    let env = Env::new().await;
    get_json(
        &env,
        &api(&format!("/drives/{DRIVE}/root/permissions")),
        200,
        json!({"value": [{"link": {"scope": "anonymous"}}, {"link": {"scope": "organization"}}]}),
    )
    .await;
    let root = SharePointLocation {
        item_id: None,
        path: String::new(),
        ..contracts_folder()
    };
    let audience = env.adapter.audience(&root).await.unwrap();
    assert_eq!(audience.line, "Anyone with the link");
    assert_eq!(audience.link_scope, Some(LinkScope::Anonymous));

    // Nothing readable is the fallback.
    let env = Env::new().await;
    get_json(&env, &permissions, 200, json!({"value": []})).await;
    let audience = env.adapter.audience(&contracts_folder()).await.unwrap();
    assert!(audience.everyone_who_can_open);
}

#[test]
fn the_error_type_and_the_value_types_have_the_wire_forms_of_the_spec() {
    let location = contracts_folder();
    let text = serde_json::to_value(&location).unwrap();
    assert_eq!(text["item_id"], FOLDER);
    let root = SharePointLocation {
        item_id: None,
        ..location
    };
    assert!(serde_json::to_value(&root)
        .unwrap()
        .get("item_id")
        .is_none());
    assert_eq!(
        serde_json::to_value(LinkScope::Organization).unwrap(),
        "organization"
    );
    assert!(
        LinkScope::Anonymous > LinkScope::Organization
            && LinkScope::Organization > LinkScope::Users
    );
}
