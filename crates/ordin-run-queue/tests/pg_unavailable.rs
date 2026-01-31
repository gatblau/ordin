//! `Error path - the datastore does not answer` with the PostgreSQL container paused. The only
//! test of this binary, so the freeze touches no other test (plan 01, "PostgreSQL 18 in tests",
//! item 5); it has its own container.

mod support;

use ordin_run_queue::RunQueue;
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn error_path_the_datastore_does_not_answer() {
    let logs = logs();
    let h = Harness::postgres().await;
    let url = h.url.clone().expect("the PostgreSQL url");
    // The queue works before the freeze.
    enqueue(&h.queue, go_no_go()).await;

    let server = pg::server().await;
    server.pause().await;
    let result = h.queue.enqueue(go_no_go()).await;
    server.unpause().await;

    let error = result.expect_err("the paused database does not answer");
    assert_eq!(error.code(), "DB_UNAVAILABLE");
    assert!(error.retryable());
    let text = logs.text();
    assert!(
        text.contains("run_queue_unavailable"),
        "the failure was logged: {text}"
    );
    // The connection string, its password and its host and port never reach a log line.
    let password = url
        .split(':')
        .nth(2)
        .and_then(|rest| rest.split('@').next())
        .unwrap()
        .to_owned();
    let authority = url
        .split('@')
        .nth(1)
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .to_owned();
    for secret in [url.as_str(), password.as_str(), authority.as_str()] {
        assert!(!text.contains(secret), "a log line holds {secret:?}");
    }
    // Once the server answers again the queue recovers.
    enqueue(&h.queue, go_no_go()).await;
}
