//! Published copies that need a conversion app: SharePoint copies in Word, PDF and slides, and
//! `PUSH_COPY_MAKER_FAILED` (ADR-048, C-28; follow-up FU-105). A test computer has no conversion app, so
//! the Craft Toolkit runs a fake one (`support::Maker`): a shell program the toolkit starts over MCP, the
//! way it starts the real one. The toolkit's own code runs: the profile gate, the path checks, the MCP
//! session and the read of the output. SharePoint is a `wiremock` stand-in on loopback.
// The fake conversion app is a POSIX shell program, so these tests run on macOS and Linux only.
#![cfg(unix)]

mod support;

use ordin_types::domain::places::{CopyDestination, CopyFormat, PublishedCopyState};
use serde_json::json;
use support::*;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, ResponseTemplate};

const ORIGINAL: &str = "# Rates\n\nrates v1\n";
const BANNER_START: &str = "Published copy of rates.md, from Supplier quotes.";

fn with(app: &'static str, format: CopyFormat, does: &'static str) -> Options {
    Options {
        maker: Some(Maker { app, format, does }),
        ..Options::default()
    }
}

/// A request for a `format` copy of `rates.md` in the stand-in SharePoint folder.
fn request(stack: &Stack, format: CopyFormat, file_name: &str) -> ordin_push::PublishCopyRequest {
    let mut request = stack.copy_request(false);
    request.format = format;
    request.destination = CopyDestination::SharepointFile {
        site: "s".to_owned(),
        library: DRIVE.to_owned(),
        folder: FOLDER.to_owned(),
    };
    request.file_name = file_name.to_owned();
    request
}

/// The folder, its audience (403: the C-42 fallback), and an upload that creates `name`.
async fn mount_folder_and_upload(stack: &Stack, name: &str) {
    stack.mount_sharepoint_origin().await;
    Mock::given(method("GET"))
        .and(path_regex(r"/permissions$"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&stack.graph)
        .await;
    Mock::given(method("PUT"))
        .and(path_regex(r"/content$"))
        .respond_with(ResponseTemplate::new(201).set_body_json(file_json("01NEWCOPY", name, "c1")))
        .mount(&stack.graph)
        .await;
    stack
        .graph_get(
            &graph(&format!("/drives/{DRIVE}/items/01NEWCOPY/versions")),
            200,
            json!({"value": [{"id": "1.0", "lastModifiedDateTime": "2026-01-10T13:40:00Z"}]}),
        )
        .await;
}

/// Asks for the copy and presses Publish on its card; returns the copy's id.
async fn publish_and_approve(
    stack: &Stack,
    request: ordin_push::PublishCopyRequest,
) -> ordin_types::PublishedCopyId {
    let copy = stack
        .service
        .publish_copy(request)
        .await
        .expect("publish_copy");
    let card: String =
        sqlx::query_scalar("SELECT approval_id FROM copy_operations WHERE kind = 'first_publish'")
            .fetch_one(&stack.pool)
            .await
            .expect("the card");
    stack
        .approve(ordin_types::ApprovalId::parse(&card).expect("id"))
        .await;
    copy
}

/// What the stand-in SharePoint received as uploads.
async fn uploads(stack: &Stack) -> Vec<(String, String)> {
    stack
        .graph
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.method.as_str() == "PUT")
        .map(|r| {
            (
                r.url.path().to_owned(),
                String::from_utf8_lossy(&r.body).into_owned(),
            )
        })
        .collect()
}

// Scenario: A Word, PDF or slides copy is made by the conversion app and created in SharePoint (ADR-048)
#[tokio::test]
async fn a_copy_is_made_by_its_conversion_app_and_created_in_sharepoint() {
    let cases = [
        (CopyFormat::Word, "wordcraft", "rates.docx"),
        (CopyFormat::Pdf, "pdfcraft", "rates.pdf"),
        (CopyFormat::Slides, "deckcraft", "rates.pptx"),
    ];
    for (format, app, file_name) in cases {
        let stack = Stack::with(with(app, format, MAKER_ECHOES)).await;
        mount_folder_and_upload(&stack, file_name).await;
        stack.write_original(ORIGINAL).await;
        let copy = publish_and_approve(&stack, request(&stack, format, file_name)).await;
        stack
            .until("the copy is current", || async {
                stack
                    .files
                    .published_copy(copy)
                    .await
                    .map(|c| c.state == PublishedCopyState::Current)
                    .unwrap_or(false)
            })
            .await;
        let sent = uploads(&stack).await;
        assert_eq!(sent.len(), 1, "{format:?}: one upload");
        assert!(
            sent[0].0.contains(file_name),
            "{format:?}: uploaded as {file_name}, was {}",
            sent[0].0
        );
        // The app was sent the banner and the original's text, and what it wrote is what was uploaded.
        assert!(
            sent[0].1.contains(BANNER_START),
            "{format:?}: {}",
            sent[0].1
        );
        assert!(sent[0].1.contains("rates v1"), "{format:?}: {}", sent[0].1);
        let published = stack.files.published_copy(copy).await.expect("copy");
        assert_eq!(published.destination_item_id.as_deref(), Some("01NEWCOPY"));
        assert_eq!(published.original_version_published.as_deref(), Some("1"));
    }
}

// Scenario: The conversion app fails (PUSH_COPY_MAKER_FAILED)
#[tokio::test]
async fn push_copy_maker_failed_when_the_conversion_app_reports_a_failure() {
    let stack = Stack::with(with("wordcraft", CopyFormat::Word, MAKER_FAILS)).await;
    mount_folder_and_upload(&stack, "rates.docx").await;
    stack.write_original(ORIGINAL).await;
    let copy = publish_and_approve(&stack, request(&stack, CopyFormat::Word, "rates.docx")).await;
    // The copy is marked failed and its operation row is updated by separate statements, so wait for
    // both before reading either.
    stack
        .until("the copy and its operation fail", || async {
            let copy_failed = stack
                .files
                .published_copy(copy)
                .await
                .map(|c| c.state == PublishedCopyState::Failed)
                .unwrap_or(false);
            let operation: Option<String> = sqlx::query_scalar(
                "SELECT state FROM copy_operations WHERE published_copy_id = ?1",
            )
            .bind(copy.to_string())
            .fetch_optional(&stack.pool)
            .await
            .unwrap_or(None);
            copy_failed && operation.as_deref() == Some("failed")
        })
        .await;
    let published = stack.files.published_copy(copy).await.expect("copy");
    assert_eq!(
        published.failure_reason.as_deref(),
        Some("The copy couldn't be made.")
    );
    assert!(
        uploads(&stack).await.is_empty(),
        "nothing reached SharePoint"
    );
    let state: String =
        sqlx::query_scalar("SELECT state FROM copy_operations WHERE published_copy_id = ?1")
            .bind(copy.to_string())
            .fetch_one(&stack.pool)
            .await
            .expect("the operation");
    assert_eq!(state, "failed");
}

// Scenario: Format not ready (C-28): only the formats an installed app makes are offered
#[tokio::test]
async fn a_format_without_an_installed_app_is_not_ready() {
    let stack = Stack::with(with("wordcraft", CopyFormat::Word, MAKER_ECHOES)).await;
    mount_folder_and_upload(&stack, "rates.pdf").await;
    stack.write_original(ORIGINAL).await;
    let error = stack
        .service
        .publish_copy(request(&stack, CopyFormat::Pdf, "rates.pdf"))
        .await
        .expect_err("refused");
    assert_eq!(error.code, "PUSH_FORMAT_NOT_READY");
    assert!(
        stack
            .service
            .publish_copy(request(&stack, CopyFormat::Word, "rates.docx"))
            .await
            .is_ok(),
        "the format its app makes is accepted"
    );
}
