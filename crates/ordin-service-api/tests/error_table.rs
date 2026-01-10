//! X1: one test per row of ServiceApi.md's *Error Table* (and the ADR-114 row), provoking the
//! condition and asserting the code, the retryability (from the registry every program reads) and the
//! HTTP status where the row states one.

mod support;

use std::time::Duration;

use ordin_service_api::blocks::*;
use ordin_service_api::*;
use ordin_types::error_registry::lookup;
use ordin_types::prelude::*;
use ordin_types::wire::ContentRef;
use serde_json::{json, Value};
use support::*;

/// The code's registry entry says it is (not) retryable.
fn retryable(code: &str) -> bool {
    lookup(code)
        .unwrap_or_else(|| panic!("{code} registered"))
        .retryable
}

fn press() -> Value {
    json!({"card_ref": "q8Zt3VxN0pLk2mRb7sYw1A", "decision": "approve"})
}

/// `handle` of a raw body routed as `approval.decide` from the screens.
async fn refused(body: &[u8]) -> (HandledReply, Value) {
    let api = Mock::answering(json!({}));
    handle_with(&api, body, "approval.decide", CallOrigin::LocalScreens, 10).await
}

// Row: command body over 1 MiB (9 MiB for the text-save commands, import_language and save_export).
#[tokio::test]
async fn body_too_large() {
    let body = vec![b' '; MAX_BODY_BYTES + 1];
    let (reply, json) = refused(&body).await;
    assert_eq!(reply.code, Some("SVC_BODY_TOO_LARGE"));
    assert_eq!(reply.http_status, 422);
    assert!(json["request_id"].is_null());
    assert!(!retryable("SVC_BODY_TOO_LARGE"));
    for routed in [
        "files.save_edit",
        "knowledge.save_edit",
        "projects.save_file",
        "localisation.import_language",
        "files.save_export",
    ] {
        let ok = accept_with(&body, routed, CallOrigin::LocalScreens).unwrap_err();
        assert_ne!(ok.code, "SVC_BODY_TOO_LARGE", "{routed}");
        let big = vec![b' '; MAX_TEXT_SAVE_BODY_BYTES + 1];
        let refusal = accept_with(&big, routed, CallOrigin::LocalScreens).unwrap_err();
        assert_eq!(refusal.code, "SVC_BODY_TOO_LARGE", "{routed}");
    }
}

// Row: body not an envelope, unknown envelope field, input shape or rule fails.
#[tokio::test]
async fn input_invalid() {
    let (reply, json) = refused(b"not json").await;
    assert_eq!(reply.code, Some("SVC_INPUT_INVALID"));
    assert_eq!(reply.http_status, 422);
    assert_eq!(json["error"]["action"], "try_again");
    assert!(json["request_id"].is_null());
    // An unknown envelope field: the request id alone still reads, so it is carried.
    let mut env: Value = serde_json::from_slice(&envelope("approval.decide", press())).unwrap();
    env["extra"] = json!(1);
    let (reply, json) = refused(&serde_json::to_vec(&env).unwrap()).await;
    assert_eq!(reply.code, Some("SVC_INPUT_INVALID"));
    assert_eq!(json["request_id"], REQUEST);
    // An input of the wrong shape, and a control character in a text field.
    let refusal = accept_local(
        "approval.decide",
        json!({"card_ref": 5, "decision": "approve"}),
    )
    .unwrap_err();
    assert_eq!(
        (refusal.code, refusal.at.as_deref()),
        ("SVC_INPUT_INVALID", Some("/input/card_ref"))
    );
    let refusal = accept_local(
        "approval.decide",
        json!({"card_ref": "a\u{7}", "decision": "approve"}),
    )
    .unwrap_err();
    assert_eq!(
        (refusal.code, refusal.at.as_deref()),
        ("SVC_INPUT_INVALID", Some("/input/card_ref"))
    );
    assert!(!retryable("SVC_INPUT_INVALID"));
}

fn with(field: &str, value: Value) -> Vec<u8> {
    let mut env: Value = serde_json::from_slice(&envelope("approval.decide", press())).unwrap();
    env[field] = value;
    serde_json::to_vec(&env).unwrap()
}

// Row: request_id not UUIDv7.
#[tokio::test]
async fn request_id_malformed() {
    let (reply, _) = refused(&with(
        "request_id",
        json!("0192a1c0-0d1e-4f20-8a31-425364758697"),
    ))
    .await;
    assert_eq!(reply.code, Some("SVC_REQUEST_ID_MALFORMED"));
    assert_eq!(reply.http_status, 422);
    assert!(!retryable("SVC_REQUEST_ID_MALFORMED"));
}

// Row: version numbers out of range.
#[tokio::test]
async fn version_malformed() {
    for version in [
        json!({"major": 0, "minor": 0}),
        json!({"major": 1, "minor": 1000}),
    ] {
        let (reply, _) = refused(&with("api_version", version)).await;
        assert_eq!(reply.code, Some("SVC_VERSION_MALFORMED"));
        assert_eq!(reply.http_status, 422);
    }
    assert!(!retryable("SVC_VERSION_MALFORMED"));
}

// Row: version pair not accepted.
#[tokio::test]
async fn version_unsupported() {
    let (reply, _) = refused(&with("api_version", json!({"major": 2, "minor": 0}))).await;
    assert_eq!(reply.code, Some("SVC_VERSION_UNSUPPORTED"));
    assert_eq!(reply.http_status, 409);
    assert!(!retryable("SVC_VERSION_UNSUPPORTED"));
}

// Row: command name malformed, unknown, or not the routed one.
#[tokio::test]
async fn command_unknown() {
    for command in ["Approval.decide", "approval.no_such_verb", "runs.stop"] {
        let (reply, _) = refused(&with("command", json!(command))).await;
        assert_eq!(reply.code, Some("SVC_COMMAND_UNKNOWN"), "{command}");
        assert_eq!(reply.http_status, 404);
    }
    assert!(!retryable("SVC_COMMAND_UNKNOWN"));
}

// Row: local-only command from a paired desktop.
#[tokio::test]
async fn command_local_only() {
    let api = Mock::answering(json!({}));
    let (reply, _) = handle_with(
        &api,
        &envelope("app.quit", json!({})),
        "app.quit",
        paired(),
        10,
    )
    .await;
    assert_eq!(reply.code, Some("SVC_COMMAND_LOCAL_ONLY"));
    assert_eq!(reply.http_status, 404);
    assert!(!retryable("SVC_COMMAND_LOCAL_ONLY"));
}

// Row: backend id is not this core (and the malformed-backend validation row).
#[tokio::test]
async fn backend_unknown() {
    let (reply, _) = refused(&with("backend", json!(BackendId::new().to_string()))).await;
    assert_eq!(reply.code, Some("SVC_BACKEND_UNKNOWN"));
    assert_eq!(reply.http_status, 404);
    assert!(!retryable("SVC_BACKEND_UNKNOWN"));
    let (reply, _) = refused(&with("backend", json!("not-a-backend"))).await;
    assert_eq!(reply.code, Some("SVC_BACKEND_ID_MALFORMED"));
}

// Row: malformed cursor.
#[tokio::test]
async fn bad_cursor() {
    let api = Mock::answering(json!({"items": [], "next_cursor": null}));
    let body = envelope("diagnostics.lines", json!({"cursor": "c".repeat(1025)}));
    let (reply, json) = handle_with(
        &api,
        &body,
        "diagnostics.lines",
        CallOrigin::LocalScreens,
        10,
    )
    .await;
    assert_eq!(reply.code, Some("SRV_BAD_CURSOR"));
    assert_eq!(reply.http_status, 422);
    assert_eq!(json["error"]["code"], "SRV_BAD_CURSOR");
    assert!(!retryable("SRV_BAD_CURSOR"));
}

// Row: implementation did not answer by the deadline.
#[tokio::test(start_paused = true)]
async fn deadline() {
    let api = Mock::new(|_, _| Box::pin(futures::future::pending()));
    let (reply, json) = handle_with(
        &api,
        &envelope("approval.decide", press()),
        "approval.decide",
        CallOrigin::LocalScreens,
        10,
    )
    .await;
    assert_eq!(reply.code, Some("SVC_DEADLINE"));
    assert_eq!(reply.http_status, 503);
    assert_eq!(json["error"]["action"], "try_again");
    assert!(retryable("SVC_DEADLINE"));
}

// Row: implementation panicked, or returned an unregistered code.
#[tokio::test]
async fn internal() {
    let api = Mock::new(|_, _| Box::pin(async { Err(module_error("NOT_A_REGISTERED_CODE")) }));
    let (reply, json) = handle_local(&api, "approval.decide", press()).await;
    assert_eq!(reply.code, Some("SVC_INTERNAL"));
    assert_eq!(reply.http_status, 500);
    assert_eq!(json["error"]["action"], "send_to_it");
    assert!(!retryable("SVC_INTERNAL"));
    let api = Mock::new(|_, _| Box::pin(async { panic!("defect") }));
    let (reply, _) = handle_local(&api, "approval.decide", press()).await;
    assert_eq!(reply.code, Some("SVC_INTERNAL"));
}

// Row: journal record not decodable, or event rule fails.
#[test]
fn event_invalid() {
    let cases: [(&str, Value); 6] = [
        ("no_such_kind", json!({})),
        ("workspace_changed", json!("not an object")),
        ("workspace_changed", json!({"workspace_id": "nope"})),
        ("reset", json!({})),
        (
            "step_marked",
            json!({"run_id": RunId::new(), "plan_version": 1, "position": 8, "mark": "now"}),
        ),
        (
            "approval_decided",
            json!({"approval_id": ApprovalId::new(), "state": "approved", "decided_on": ""}),
        ),
    ];
    for (kind, data) in cases {
        let error = ServiceEvent::from_journal(kind, data).unwrap_err();
        assert_eq!(error.code, "SVC_EVENT_INVALID", "{kind}");
    }
    assert!(!retryable("SVC_EVENT_INVALID"));
}

// Row: non-block event over 256 KiB.
#[test]
fn event_too_large() {
    let envelope = EventEnvelope {
        api_version: ServiceApiVersion::CURRENT,
        backend: backend(),
        seq: 3,
        at: Timestamp::now(),
        event: ServiceEvent::ApprovalDecided {
            approval_id: ApprovalId::new(),
            state: ordin_types::domain::ApprovalState::Approved,
            decided_on: "x".repeat(MAX_EVENT_DATA_BYTES),
        },
    };
    assert_eq!(
        encode_for_transport(&envelope).unwrap_err().code,
        "SVC_EVENT_TOO_LARGE"
    );
    assert!(!retryable("SVC_EVENT_TOO_LARGE"));
}

// Row: seq jump seen by the screens (a desktop diagnostic written by the generated rule).
#[test]
fn event_gap() {
    let files = ordin_service_api::typescript::generate();
    let commands = &files
        .iter()
        .find(|(n, _)| *n == "commands.gen.ts")
        .unwrap()
        .1;
    assert!(commands.contains("export function expectNext(backend: string, seq: number)"));
    assert!(commands.contains("SVC_EVENT_GAP"));
    assert!(lookup("SVC_EVENT_GAP").is_some());
}

struct Refs(bool);

#[async_trait::async_trait]
impl RefResolver for Refs {
    async fn content(&self, _: WorkspaceId, _: &ContentRef) -> bool {
        self.0
    }
    async fn email_message(&self, _: WorkspaceId, _: EmailLinkId, _: &str) -> bool {
        self.0
    }
    async fn entity(&self, _: WorkspaceId, _: EntityRef) -> bool {
        self.0
    }
    async fn run_agent_name(&self, _: RunId) -> Option<String> {
        Some("Supplier analyst".to_owned())
    }
    async fn plan_state(&self, _: RunId) -> PlanState {
        PlanState {
            versions: 20,
            current_len: 3,
        }
    }
    fn approval_verbs(&self) -> &[String] {
        &[]
    }
}

fn ctx() -> BlockCheckContext {
    BlockCheckContext {
        workspace_id: WorkspaceId::new(),
        run_id: Some(RunId::new()),
        now: Timestamp::now(),
    }
}

async fn codes(raw: Value, found: bool) -> Vec<(String, String)> {
    match check_block(&raw, &ctx(), &Refs(found)).await {
        Ok(_) => Vec::new(),
        Err(rejections) => rejections.into_iter().map(|r| (r.code, r.at)).collect(),
    }
}

// Row: block over 2 MiB.
#[tokio::test]
async fn block_too_large() {
    let raw = json!({"kind": "message", "text": "x".repeat(2 * 1024 * 1024), "attachments": []});
    assert_eq!(
        codes(raw, true).await,
        [("BLOCK_TOO_LARGE".to_owned(), String::new())]
    );
}

// Row: unknown block kind / field.
#[tokio::test]
async fn block_kind_or_field_unknown() {
    assert_eq!(
        codes(json!({"kind": "button"}), true).await,
        [("BLOCK_KIND_UNKNOWN".to_owned(), "/kind".to_owned())]
    );
    let raw = json!({"kind": "message", "text": "Hi", "attachments": [], "colour": "red"});
    assert_eq!(
        codes(raw, true).await,
        [("BLOCK_FIELD_UNKNOWN".to_owned(), "/colour".to_owned())]
    );
}

// Row: URL where a reference is expected.
#[tokio::test]
async fn block_link_not_allowed() {
    let raw = json!({"kind": "preview", "item": "https://example.com/a.pdf"});
    assert_eq!(
        codes(raw, true).await,
        [("BLOCK_LINK_NOT_ALLOWED".to_owned(), "/item".to_owned())]
    );
}

fn place_file(path: &str) -> Value {
    json!({"kind": "place_file", "place_id": PlaceId::new(), "relative_path": path})
}

fn documents_source() -> Value {
    json!({"kind": "documents", "documents": [{"item": place_file("a.xlsx"), "version": "7"}]})
}

fn table(columns: Value, rows: Value) -> Value {
    json!({"kind": "table", "title": "T", "columns": columns, "rows": rows, "source": documents_source()})
}

fn chart(chart: &str, columns: Value, rows: Value, x: &str, series: Value) -> Value {
    json!({"kind": "chart", "title": "C", "chart": chart, "columns": columns, "rows": rows, "x": x, "series": series, "source": documents_source()})
}

// Row: a per-kind rule fails (each code of the block rules table).
#[tokio::test]
async fn block_rule_fails() {
    let text_col = json!([{"name": "A", "type": "text"}]);
    let num_cols =
        json!([{"name": "A", "type": "text"}, {"name": "N", "type": "number", "decimals": 2}]);
    let cases: Vec<(Value, &str, &str)> = vec![
        (
            json!({"kind": "message", "text": "", "attachments": []}),
            "BLOCK_TEXT_INVALID",
            "/text",
        ),
        (
            json!({"kind": "message", "text": "a", "attachments": vec![place_file("a"); 21]}),
            "BLOCK_TOO_MANY_ITEMS",
            "/attachments",
        ),
        (
            json!({"kind": "question", "question": "Q?", "choices": ["a"], "allow_other": false}),
            "BLOCK_CHOICES_INVALID",
            "/choices",
        ),
        (
            json!({"kind": "question", "question": "Q?", "choices": ["a", "a"], "allow_other": false}),
            "BLOCK_CHOICES_INVALID",
            "/choices/1",
        ),
        (
            json!({"kind": "form", "title": "F", "verb": "Go", "target": {"kind": "agent_instance", "agent_instance_id": AgentInstanceId::new()}, "fields": []}),
            "BLOCK_FORM_INVALID",
            "/fields",
        ),
        (
            json!({"kind": "form", "title": "F", "verb": "OK", "target": {"kind": "agent_instance", "agent_instance_id": AgentInstanceId::new()}, "fields": [{"name": "Bad", "label": "L", "required": true, "input": "choice", "options": ["a"], "value": null, "filled_by_ordin": false}]}),
            "BLOCK_FORM_INVALID",
            "/fields/0/name",
        ),
        (
            json!({"kind": "documents", "workspace_id": WorkspaceId::new(), "items": []}),
            "BLOCK_TOO_MANY_ITEMS",
            "/items",
        ),
        (
            json!({"kind": "document", "item": place_file("../x")}),
            "BLOCK_PATH_INVALID",
            "/item/relative_path",
        ),
        (
            table(json!([{"name": "", "type": "text"}]), json!([])),
            "BLOCK_TABLE_INVALID",
            "/columns/0/name",
        ),
        (
            table(json!([]), json!([])),
            "BLOCK_TABLE_TOO_WIDE",
            "/columns",
        ),
        (
            table(text_col.clone(), json!(vec![json!(["a"]); 10_001])),
            "BLOCK_TABLE_TOO_LONG",
            "/rows",
        ),
        (
            table(text_col.clone(), json!([[1.5]])),
            "BLOCK_CELL_TYPE_MISMATCH",
            "/rows/0/0",
        ),
        (
            table(
                json!([{"name": "D", "type": "date"}]),
                json!([["2026-13-01"]]),
            ),
            "BLOCK_CELL_TYPE_MISMATCH",
            "/rows/0/0",
        ),
        (
            table(text_col.clone(), json!([["x".repeat(2001)]])),
            "BLOCK_CELL_TOO_LONG",
            "/rows/0/0",
        ),
        (
            table(
                json!([{"name": "M", "type": "money", "currency": "gbp"}]),
                json!([]),
            ),
            "BLOCK_CURRENCY_INVALID",
            "/columns/0/currency",
        ),
        (
            json!({"kind": "key_figures", "figures": [{"label": "S", "value": 1.0}]}),
            "BLOCK_SOURCE_MISSING",
            "/source",
        ),
        (
            json!({"kind": "key_figures", "figures": [{"label": "S", "value": 1.0}], "source": {"kind": "company_system", "system": "", "read_at": "2026-01-10T15:20:00.000Z"}}),
            "BLOCK_SOURCE_INVALID",
            "/source/system",
        ),
        (
            json!({"kind": "key_figures", "figures": [{"label": "S", "value": 1.0}], "source": {"kind": "worked_out", "by": "Someone else", "from": []}}),
            "BLOCK_SOURCE_INVALID",
            "/source/by",
        ),
        (
            chart(
                "line",
                num_cols.clone(),
                json!([["a", 1]]),
                "A",
                json!(["A"]),
            ),
            "BLOCK_CHART_SERIES_INVALID",
            "/series",
        ),
        (
            chart(
                "line",
                num_cols.clone(),
                json!(vec![json!(["a", 1]); 5001]),
                "A",
                json!(["N"]),
            ),
            "BLOCK_CHART_TOO_MANY_POINTS",
            "/rows",
        ),
        (
            chart(
                "pie",
                num_cols.clone(),
                json!(vec![json!(["a", 1]); 7]),
                "A",
                json!(["N"]),
            ),
            "BLOCK_CHART_PIE_INVALID",
            "/rows",
        ),
        (
            chart(
                "scatter",
                num_cols.clone(),
                json!([["a", 1]]),
                "A",
                json!(["N"]),
            ),
            "BLOCK_CHART_AXIS_INVALID",
            "/x",
        ),
        (
            chart(
                "line",
                num_cols.clone(),
                json!([["a", 1]]),
                "Z",
                json!(["N"]),
            ),
            "BLOCK_CHART_AXIS_INVALID",
            "/x",
        ),
        (
            json!({"kind": "chart", "title": "", "chart": "bar", "orientation": "horizontal", "arrangement": "stacked", "columns": num_cols, "rows": [["a", 1]], "x": "A", "series": ["N"], "source": documents_source()}),
            "BLOCK_CHART_INVALID",
            "/title",
        ),
        (
            json!({"kind": "key_figures", "figures": [], "source": documents_source()}),
            "BLOCK_FIGURES_COUNT",
            "/figures",
        ),
        (
            json!({"kind": "key_figures", "figures": [{"label": "S", "value": 1.0, "unit": "", "change": {"against": "Q2", "earlier_value": 2.0}}], "source": documents_source()}),
            "BLOCK_FIGURES_INVALID",
            "/figures/0/unit",
        ),
        (
            json!({"kind": "report", "title": "R", "as_of": "2026-01-10T15:20:00.000Z", "parts": []}),
            "BLOCK_REPORT_PARTS_COUNT",
            "/parts",
        ),
        (
            json!({"kind": "report", "title": "", "as_of": "2026-01-10T15:20:00.000Z", "parts": [{"heading": "H", "content": {"kind": "key_figures", "figures": [{"label": "S", "value": 1.0}], "source": documents_source()}}]}),
            "BLOCK_REPORT_INVALID",
            "/title",
        ),
        (
            json!({"kind": "report", "title": "R", "as_of": "2999-01-01T00:00:00.000Z", "parts": [{"heading": "H", "content": {"kind": "key_figures", "figures": [{"label": "S", "value": 1.0}], "source": documents_source()}}]}),
            "BLOCK_AS_OF_INVALID",
            "/as_of",
        ),
        (
            json!({"kind": "result", "of": "offer", "offer": "delete_everything", "run_id": null}),
            "BLOCK_OFFER_UNKNOWN",
            "/offer",
        ),
        (
            json!({"kind": "result", "of": "produced", "run_id": RunId::new(), "summary": "", "items": []}),
            "BLOCK_TEXT_INVALID",
            "/summary",
        ),
        (
            json!({"kind": "agent_proposal", "agent_repository_id": AgentRepositoryId::new(), "agent": "urn:ordin:a", "reason": ""}),
            "BLOCK_TEXT_INVALID",
            "/reason",
        ),
        (
            json!({"kind": "memory", "workspace_id": WorkspaceId::new(), "facts": []}),
            "BLOCK_TOO_MANY_ITEMS",
            "/facts",
        ),
    ];
    for (raw, code, at) in cases {
        let found = codes(raw.clone(), true).await;
        assert!(
            found.iter().any(|(c, a)| c == code && a == at),
            "{code} at {at}: {found:?} for {raw}"
        );
    }
}

// Row: a reference not found, or not readable by the person.
#[tokio::test]
async fn block_ref_unresolved() {
    let raw = json!({"kind": "preview", "item": place_file("a.pdf")});
    assert_eq!(
        codes(raw, false).await,
        [("BLOCK_REF_UNRESOLVED".to_owned(), "/item".to_owned())]
    );
    let raw = json!({"kind": "crew_plan", "crew_charter_id": CrewCharterId::new()});
    assert_eq!(
        codes(raw, false).await,
        [(
            "BLOCK_REF_UNRESOLVED".to_owned(),
            "/crew_charter_id".to_owned()
        )]
    );
}

// Row: email message not in the workspace's links.
#[tokio::test]
async fn block_email_not_linked() {
    let raw = json!({"kind": "email_list", "workspace_id": WorkspaceId::new(), "email_link_id": EmailLinkId::new(), "messages": ["<m1@x>"]});
    assert_eq!(
        codes(raw, false).await,
        [(
            "BLOCK_EMAIL_NOT_LINKED".to_owned(),
            "/messages/0".to_owned()
        )]
    );
}

// Row: hosted.decide_card refused by the hosted agent (the owner's codes, passed through).
#[tokio::test]
async fn hosted_card_refused() {
    for code in ["HAA_CARD_DECIDED", "HAA_NO_ACCESS", "HAA_CARD_UNKNOWN"] {
        let api = Mock::new(move |_, _| Box::pin(async move { Err(module_error(code)) }));
        let input = json!({"server_or_agent": RemoteAgentId::new(), "approval_id": ApprovalId::new(), "decision": "reject"});
        let (reply, json) = handle_local(&api, "hosted.decide_card", input).await;
        assert_eq!(json["error"]["code"], code);
        assert_eq!(reply.http_status, 200);
        assert_eq!(reply.code, Some(code));
    }
}

// Row: generated TypeScript differs from the committed files.
#[test]
fn generated_types_stale() {
    let dir = std::env::temp_dir().join(format!("ordin-svc-stale-{}", std::process::id()));
    let bin = env!("CARGO_BIN_EXE_ordin-service-api-gen");
    let out = std::process::Command::new(bin)
        .arg("check")
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("service-api types are stale: schema.gen.json"));
    let usage = std::process::Command::new(bin).output().unwrap();
    assert_eq!(usage.status.code(), Some(2));
}

// Row (ADR-114): the command's group is not among the groups this build answers.
#[tokio::test]
async fn not_in_this_version() {
    let mut api = Mock::answering(json!({}));
    api.groups = ALWAYS_ANSWERED_GROUPS;
    let (reply, json) = handle_with(
        &api,
        &envelope("mail.settings", json!({})),
        "mail.settings",
        CallOrigin::LocalScreens,
        10,
    )
    .await;
    assert_eq!(reply.code, Some("SVC_NOT_IN_THIS_VERSION"));
    assert_eq!(reply.http_status, 404);
    assert_eq!(json["request_id"], REQUEST);
    assert!(api.calls().is_empty());
    assert!(!retryable("SVC_NOT_IN_THIS_VERSION"));
    let _ = Duration::ZERO;
}
