//! Logging (SharePointAdapter.md *Never logged*, *Observability*): the log lines of a versioned write, a
//! throttled read, a tracked link and an unknown outcome carry no token, no file or folder name, no search
//! text, no delta link, no `lastModifiedBy` name and no document text; the events named in the spec appear.
//!
//! This is the only log-capturing test binary of the crate (plan FU-017): the subscriber is installed per
//! test on the test's own thread with `set_default`, so no global subscriber is touched.

mod support;

use std::sync::{Arc, Mutex};

use ordin_types::domain::runs::ExternalActionKind;
use serde_json::{json, Value};
use support::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

/// A log capture with ordin's own line formatter (Logging.md).
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    fn lines(&self) -> Vec<Value> {
        self.text()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let writer = self.clone();
        let subscriber = tracing_subscriber::fmt()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }
}

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const NEVER_LOGGED: [&str; 11] = [
    TOKEN,
    "rt-secret-refresh-0001",
    "Kestrel",
    "Contracts",
    "Confidential contract text",
    "Sam Lee",
    "docx",
    "supplier day rates",
    "token=T1",
    "Bearer",
    "authorization",
];

fn events(capture: &Capture) -> Vec<String> {
    capture
        .lines()
        .iter()
        .filter(|l| l["component"] == "sharepoint")
        .filter_map(|l| l["event"].as_str().map(str::to_owned))
        .collect()
}

fn assert_clean(capture: &Capture) {
    let text = capture.text().to_lowercase();
    for needle in NEVER_LOGGED {
        assert!(
            !text.contains(&needle.to_lowercase()),
            "{needle} is in the log:\n{}",
            capture.text()
        );
    }
}

fn item_path() -> String {
    api(&format!("/drives/{DRIVE}/items/{FILE}"))
}

#[tokio::test(flavor = "current_thread")]
async fn a_versioned_write_logs_ids_and_counts_only() {
    let capture = Capture::default();
    let _guard = capture.install();
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
            b"Confidential contract text".to_vec(),
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await
        .unwrap();

    assert_clean(&capture);
    let seen = events(&capture);
    assert!(seen.contains(&"graph_request".to_owned()));
    assert!(seen.contains(&"sharepoint_pushed".to_owned()));
    let request = capture
        .lines()
        .into_iter()
        .find(|l| l["event"] == "graph_request")
        .expect("a graph_request line");
    for field in ["operation", "status", "elapsed_ms", "attempt"] {
        assert!(request.get(field).is_some(), "{field}");
    }
    let pushed = capture
        .lines()
        .into_iter()
        .find(|l| l["event"] == "sharepoint_pushed")
        .unwrap();
    assert_eq!(pushed["kind"], "version");
    assert_eq!(pushed["size_bytes"], 26);
}

#[tokio::test(flavor = "current_thread")]
async fn a_throttled_read_and_a_search_log_no_names_or_text() {
    let capture = Capture::default();
    let _guard = capture.install();
    let env = Env::with_scale(SignIn::Refreshable, 0.001).await;
    get_json(&env, &api("/sites"), 200, json!({"value": []})).await;
    env.adapter
        .find_locations("supplier day rates", 10)
        .await
        .unwrap();
    Mock::given(method("GET"))
        .and(path(item_path().as_str()))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .mount(&env.mock)
        .await;
    let _ = env.adapter.item_version(&file_ref()).await.unwrap_err();

    assert_clean(&capture);
    let seen = events(&capture);
    assert!(seen.contains(&"graph_throttled".to_owned()), "{seen:?}");
    let throttled = capture
        .lines()
        .into_iter()
        .find(|l| l["event"] == "graph_throttled")
        .unwrap();
    assert_eq!(throttled["retry_after_s"], 1);
}

#[tokio::test(flavor = "current_thread")]
async fn tracking_logs_link_ids_and_counts_only() {
    let capture = Capture::default();
    let _guard = capture.install();
    let env = Env::new().await;
    let link = track_contracts(&env, "c4").await;
    mount_delta(
        &env,
        "T1",
        vec![file_json(
            FILE,
            "Kestrel MSA.docx",
            "c5",
            "/Suppliers/Contracts",
            "Sam Lee",
        )],
        "T2",
        true,
    )
    .await;
    assert_eq!(env.adapter.check_changes(&link).await.unwrap().len(), 1);
    // The delta link expires: the resynchronisation is logged by link id.
    Mock::given(method("GET"))
        .and(path(api(&format!("/drives/{DRIVE}/root/delta"))))
        .and(wiremock::matchers::query_param("token", "T2"))
        .respond_with(ResponseTemplate::new(410))
        .mount(&env.mock)
        .await;
    mount_delta(&env, "latest", vec![], "T3", false).await;
    mount_children(
        &env,
        FOLDER,
        vec![file_json(
            FILE,
            "Kestrel MSA.docx",
            "c6",
            "/Suppliers/Contracts",
            "Sam Lee",
        )],
        false,
    )
    .await;
    env.adapter.check_changes(&link).await.unwrap();

    assert_clean(&capture);
    let seen = events(&capture);
    for event in [
        "sharepoint_link_tracked",
        "sharepoint_changes_found",
        "delta_resync",
    ] {
        assert!(seen.contains(&event.to_owned()), "{event} in {seen:?}");
    }
    let tracked = capture
        .lines()
        .into_iter()
        .find(|l| l["event"] == "sharepoint_link_tracked")
        .unwrap();
    assert_eq!(tracked["source_link_id"], link.id.to_string());
    assert_eq!(tracked["items"], 1);
}

#[tokio::test(flavor = "current_thread")]
async fn an_unknown_outcome_logs_the_action_key_and_nothing_else() {
    let capture = Capture::default();
    let _guard = capture.install();
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
    let action = write(ExternalActionKind::OriginPush);
    let key = action.key;
    let _ = env
        .adapter
        .upload_version(
            &file_ref(),
            b"Confidential contract text".to_vec(),
            "c4",
            action,
        )
        .await
        .unwrap_err();

    assert_clean(&capture);
    let unknown = capture
        .lines()
        .into_iter()
        .find(|l| l["event"] == "sharepoint_outcome_unknown")
        .expect("the event");
    assert_eq!(unknown["action_key"], key.to_string());
}
