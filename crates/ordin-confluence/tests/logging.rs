//! Logging (ConfluenceAdapter.md *Never logged*, *Observability*): the log lines of a read, a push, a
//! sync and an unknown outcome carry no token, no title, no file name, no search text, no author name and
//! no page body; the events named in the spec appear.
//!
//! This is the only log-capturing test binary of the crate: the subscriber is installed per test on the
//! test's own thread with `set_default`, so no global subscriber is touched.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_types::domain::places::{ConfluenceLinkState, FileVersionOrigin};
use serde_json::Value;
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

const NEVER_LOGGED: [&str; 8] = [
    TOKEN,
    "Secret Rates Title",
    "rates.md",
    "Day rate",
    "Sam Lee",
    "supplier day rates",
    "Bearer",
    "<p>Body</p>",
];

fn check_clean(capture: &Capture) {
    let text = capture.text();
    for word in NEVER_LOGGED {
        assert!(!text.contains(word), "{word} was logged: {text}");
    }
}

fn event_names(capture: &Capture) -> Vec<String> {
    capture
        .lines()
        .iter()
        .filter_map(|line| {
            line.get("event")
                .or_else(|| line.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

const PAGE: &str = "4411";

#[tokio::test(flavor = "current_thread")]
async fn a_sync_with_a_push_logs_the_named_events_and_nothing_private() {
    let env = Env::new().await;
    mount_author(&env).await;
    mount_search(&env, vec![]).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Day rate", FileVersionOrigin::Person)
        .await;
    let hash = env.hash_of("rates.md").await;
    env.page_copy(&link, "rates.md", PAGE, "7", &hash).await;
    env.version("rates.md", "# Day rate 2", FileVersionOrigin::Person)
        .await;
    mount_page(&env, PAGE, "Secret Rates Title", 7).await;
    mount_put(&env, PAGE, "Secret Rates Title", 8).await;
    let capture = Capture::default();
    let guard = capture.install();

    env.adapter
        .find_locations("supplier day rates", 5)
        .await
        .unwrap();
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    drop(guard);
    let events = event_names(&capture);
    for wanted in [
        "confluence_request",
        "confluence_pushed",
        "confluence_link_synced",
    ] {
        assert!(events.iter().any(|e| e == wanted), "{wanted} in {events:?}");
    }
    check_clean(&capture);
}

#[tokio::test(flavor = "current_thread")]
async fn an_unknown_outcome_logs_the_key_and_nothing_private() {
    let env = Env::with_timeout(Duration::from_millis(250)).await;
    reply_once(
        &env,
        "GET",
        &api(&format!("/api/v2/pages/{PAGE}")),
        200,
        page_json(PAGE, "Secret Rates Title", 7),
    )
    .await;
    mount_page(&env, PAGE, "Secret Rates Title", 8).await;
    Mock::given(method("PUT"))
        .and(path(api(&format!("/api/v2/pages/{PAGE}"))))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .mount(&env.mock)
        .await;
    let capture = Capture::default();
    let guard = capture.install();
    let action = key();

    let error = env
        .adapter
        .update_page(
            PAGE,
            "Secret Rates Title",
            "Day rate",
            None,
            "7",
            action,
            None,
        )
        .await
        .unwrap_err();

    drop(guard);
    assert_eq!(error.code(), "CONF_OUTCOME_UNKNOWN");
    assert!(event_names(&capture)
        .iter()
        .any(|e| e == "confluence_outcome_unknown"));
    assert!(capture.text().contains(&action.to_string()));
    check_clean(&capture);
}
