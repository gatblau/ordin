//! X2: one test per Gherkin scenario of ServiceApi.md *Acceptance Criteria* and of each amendment,
//! named after the scenario (its exact title in the comment above it). Scenarios about the screens,
//! the Remote API or the Architecture Test are held here to the part this crate decides.

mod support;

use std::time::Duration;

use ordin_service_api::blocks::*;
use ordin_service_api::commands::{OlderSide, ServerView};
use ordin_service_api::*;
use ordin_types::domain::ServerConnectionState;
use ordin_types::prelude::*;
use ordin_types::wire::ContentRef;
use serde_json::{json, Value};
use support::*;

fn approval_press() -> Value {
    json!({"card_ref": "q8Zt3VxN0pLk2mRb7sYw1A", "decision": "approve"})
}

fn approval_outcome() -> Value {
    json!({"approval_id": "0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f", "state": "approved", "decided_on": "Asha's laptop"})
}

// Scenario: Happy path — an approval press is accepted and dispatched
#[tokio::test]
async fn happy_path_an_approval_press_is_accepted_and_dispatched() {
    let api = Mock::answering(approval_outcome());
    let (reply, json) = handle_local(&api, "approval.decide", approval_press()).await;
    assert_eq!(json["kind"], "ok");
    assert_eq!(json["result"]["state"], "approved");
    assert_eq!(json["request_id"], REQUEST);
    assert_eq!(reply.http_status, 200);
    assert_eq!(api.calls(), ["approval_decide"]);
}

// Scenario: Error path — an unknown input field
#[tokio::test]
async fn error_path_an_unknown_input_field() {
    let api = Mock::answering(approval_outcome());
    let mut input = approval_press();
    input["force"] = json!(true);
    let (reply, json) = handle_local(&api, "approval.decide", input).await;
    assert_eq!(json["kind"], "error");
    assert_eq!(json["error"]["code"], "SVC_INPUT_INVALID");
    assert_eq!(reply.http_status, 422);
    assert!(api.calls().is_empty());
}

// Scenario: Error path — a local-only command over the Remote API
#[tokio::test]
async fn error_path_a_local_only_command_over_the_remote_api() {
    let api = Mock::answering(json!({}));
    let body = envelope(
        "servers.connect",
        json!({"kind": "address_and_code", "address": "10.0.0.1:8443", "code": "K7QM-2XPA"}),
    );
    let (reply, json) = handle_with(&api, &body, "servers.connect", paired(), 10).await;
    assert_eq!(json["error"]["code"], "SVC_COMMAND_LOCAL_ONLY");
    assert_eq!(reply.http_status, 404);
}

fn versioned(major: u16, minor: u16) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "api_version": {"major": major, "minor": minor},
        "request_id": REQUEST,
        "command": "approval.decide",
        "backend": BACKEND,
        "input": approval_press(),
    }))
    .unwrap()
}

// Scenario: Error path — a desktop two minor versions ahead
#[tokio::test]
async fn error_path_a_desktop_two_minor_versions_ahead() {
    let api = Mock::answering(approval_outcome());
    let (reply, json) = handle_with(&api, &versioned(1, 2), "approval.decide", paired(), 10).await;
    assert_eq!(json["error"]["code"], "SVC_VERSION_UNSUPPORTED");
    assert_eq!(reply.http_status, 409);
}

// Scenario: Edge case — a desktop one minor version ahead is accepted
#[tokio::test]
async fn edge_case_a_desktop_one_minor_version_ahead_is_accepted() {
    let api = Mock::answering(approval_outcome());
    let (reply, _) = handle_with(&api, &versioned(1, 1), "approval.decide", paired(), 10).await;
    assert_eq!(reply.code, None);
    assert_eq!(api.calls(), ["approval_decide"]);
}

// Scenario: Error path — the path and the envelope disagree
#[test]
fn error_path_the_path_and_the_envelope_disagree() {
    let refusal = accept_with(
        &envelope("approval.decide", approval_press()),
        "runs.stop",
        paired(),
    )
    .unwrap_err();
    assert_eq!(refusal.code, "SVC_COMMAND_UNKNOWN");
}

// Scenario: Error path — a stuck module
#[tokio::test(start_paused = true)]
async fn error_path_a_stuck_module() {
    let api = Mock::new(|_, _| Box::pin(futures::future::pending()));
    let started = tokio::time::Instant::now();
    let (reply, json) = handle_local(&api, "approval.decide", approval_press()).await;
    assert!(started.elapsed() >= Duration::from_secs(10));
    assert_eq!(json["error"]["code"], "SVC_DEADLINE");
    assert_eq!(json["error"]["action"], "try_again");
    assert_eq!(reply.http_status, 503);
}

// Scenario: Error path — a panicking module
#[tokio::test]
async fn error_path_a_panicking_module() {
    let api = Mock::new(|_, _| Box::pin(async { panic!("a module defect") }));
    let (reply, json) = handle_local(&api, "approval.decide", approval_press()).await;
    assert_eq!(json["error"]["code"], "SVC_INTERNAL");
    assert_eq!(json["error"]["action"], "send_to_it");
    assert_eq!(reply.http_status, 500);
    // The process keeps running: the same handler answers the next call.
    let api = Mock::answering(approval_outcome());
    let (_, json) = handle_local(&api, "approval.decide", approval_press()).await;
    assert_eq!(json["kind"], "ok");
}

fn progress_data() -> Value {
    json!({"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","progress":{"line":"Read supplier quotes","count":{"done":12,"total":40}}})
}

// Scenario: Happy path — a journal record becomes an event
#[test]
fn happy_path_a_journal_record_becomes_an_event() {
    let at =
        Timestamp(ordin_types::time::parse_rfc3339_ms_utc("2026-01-10T09:41:07.312Z").unwrap());
    let envelope =
        EventEnvelope::new(backend(), 48213, at, "run_progress", progress_data()).unwrap();
    let json: Value = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json["kind"], "run_progress");
    assert_eq!(json["seq"], 48213);
    assert_eq!(
        serde_json::to_string(&envelope).unwrap(),
        r#"{"api_version":{"major":1,"minor":0},"backend":"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b","seq":48213,"at":"2026-01-10T09:41:07.312Z","kind":"run_progress","run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","progress":{"line":"Read supplier quotes","count":{"done":12,"total":40}}}"#
    );
}

// Scenario: Error path — a progress count above its total
#[test]
fn error_path_a_progress_count_above_its_total() {
    let mut data = progress_data();
    data["progress"]["count"] = json!({"done": 41, "total": 40});
    let error = ServiceEvent::from_journal("run_progress", data).unwrap_err();
    assert_eq!(error.code, "SVC_EVENT_INVALID");
}

// Scenario: Edge case — a large block is sent as a reference
#[test]
fn edge_case_a_large_block_is_sent_as_a_reference() {
    let workspace_id = WorkspaceId::new();
    let block_id = BlockId::new();
    let text = "x".repeat(300 * 1024);
    let data = json!({
        "workspace_id": workspace_id,
        "block_id": block_id,
        "block": {"kind": "message", "text": text, "attachments": []},
    });
    let envelope = EventEnvelope::new(backend(), 7, Timestamp::now(), "block_shown", data).unwrap();
    let frame = encode_for_transport(&envelope).unwrap();
    assert_eq!(frame.kind, EventKind::BlockAvailable);
    assert_eq!(frame.seq, 7);
    let json: Value = serde_json::from_str(&frame.json).unwrap();
    assert_eq!(json["kind"], "block_available");
    assert_eq!(json["workspace_id"], workspace_id.to_string());
    assert_eq!(json["block_id"], block_id.to_string());
    assert_eq!(json["seq"], 7);
}

/// A resolver that finds everything unless told otherwise.
pub struct Resolver {
    pub found: bool,
    pub agent: Option<String>,
    pub verbs: Vec<String>,
    pub plan: PlanState,
    pub other_workspace_link: Option<SlackLinkId>,
}

impl Default for Resolver {
    fn default() -> Self {
        Resolver {
            found: true,
            agent: Some("Supplier analyst".to_owned()),
            verbs: vec!["Approve".to_owned(), "Send".to_owned()],
            plan: PlanState {
                versions: 1,
                current_len: 3,
            },
            other_workspace_link: None,
        }
    }
}

#[async_trait::async_trait]
impl RefResolver for Resolver {
    async fn content(&self, _: WorkspaceId, _: &ContentRef) -> bool {
        self.found
    }
    async fn email_message(&self, _: WorkspaceId, _: EmailLinkId, _: &str) -> bool {
        self.found
    }
    async fn entity(&self, _: WorkspaceId, entity: EntityRef) -> bool {
        match (entity, self.other_workspace_link) {
            (EntityRef::SlackLink(id), Some(other)) if id == other => false,
            _ => self.found,
        }
    }
    async fn run_agent_name(&self, _: RunId) -> Option<String> {
        self.agent.clone()
    }
    async fn plan_state(&self, _: RunId) -> PlanState {
        self.plan
    }
    fn approval_verbs(&self) -> &[String] {
        &self.verbs
    }
}

fn ctx() -> BlockCheckContext {
    BlockCheckContext {
        workspace_id: WorkspaceId::new(),
        run_id: None,
        now: Timestamp::now(),
    }
}

/// The 2C table example.
fn table_example() -> Value {
    json!({"kind":"table","title":"Q3 rates by supplier","columns":[{"name":"Supplier","type":"text"},{"name":"Rate","type":"money","currency":"GBP"},{"name":"Change","type":"percentage"}],"rows":[["Kestrel",412.5,0.08],["Acme",398.0,-0.02]],"source":{"kind":"documents","documents":[{"item":{"kind":"place_file","place_id":"0192a0f2-2222-7333-8444-555566667777","relative_path":"Q3/rates.xlsx"},"version":"7"}]}})
}

// Scenario: Happy path — a table block passes the check
#[tokio::test]
async fn happy_path_a_table_block_passes_the_check() {
    let raw = table_example();
    let block = check_block(&raw, &ctx(), &Resolver::default())
        .await
        .unwrap();
    assert!(matches!(block, Block::Table(_)));
    assert_eq!(serde_json::to_value(&block).unwrap(), raw);
}

fn pie(series: Value) -> Value {
    json!({"kind":"chart","title":"Spend by supplier","chart":"pie","columns":[{"name":"Supplier","type":"text"},{"name":"Q2","type":"money","currency":"GBP"},{"name":"Q3","type":"money","currency":"GBP"}],"rows":[["Kestrel",382000,412000],["Acme",401000,398000]],"x":"Supplier","series":series,"source":{"kind":"company_system","system":"Finance ledger","read_at":"2026-01-10T15:20:00.000Z"}})
}

// Scenario: Error path — a pie chart with two series
#[tokio::test]
async fn error_path_a_pie_chart_with_two_series() {
    let rejections = check_block(&pie(json!(["Q2", "Q3"])), &ctx(), &Resolver::default())
        .await
        .unwrap_err();
    assert_eq!(rejections.len(), 1, "{rejections:?}");
    assert_eq!(rejections[0].code, "BLOCK_CHART_PIE_INVALID");
    assert_eq!(rejections[0].at, "/series");
    assert!(
        check_block(&pie(json!(["Q2"])), &ctx(), &Resolver::default())
            .await
            .is_ok()
    );
}

// Scenario: Error path — a link given as a URL
#[tokio::test]
async fn error_path_a_link_given_as_a_url() {
    let raw =
        json!({"kind": "message", "text": "Here", "attachments": ["https://example.com/a.pdf"]});
    let rejections = check_block(&raw, &ctx(), &Resolver::default())
        .await
        .unwrap_err();
    assert_eq!(rejections[0].code, "BLOCK_LINK_NOT_ALLOWED");
    assert_eq!(rejections[0].at, "/attachments/0");
}

fn table_part(heading: &str, with_source: bool) -> Value {
    let mut content = table_example();
    if !with_source {
        content.as_object_mut().unwrap().remove("source");
    }
    json!({"heading": heading, "content": content})
}

// Scenario: Edge case — a report keeps its good parts
#[tokio::test]
async fn edge_case_a_report_keeps_its_good_parts() {
    let raw = json!({
        "kind": "report",
        "title": "Q3 supplier rates",
        "as_of": "2026-01-10T15:20:00.000Z",
        "parts": [table_part("One", true), table_part("Two", false), table_part("Three", true)],
    });
    let checked = check_report_tool(&raw, &ctx(), &Resolver::default())
        .await
        .unwrap();
    let ReportToolArgs::Report(report) = checked.block else {
        panic!("a report");
    };
    let headings: Vec<&str> = report.parts.iter().map(|p| p.heading.as_str()).collect();
    assert_eq!(headings, ["One", "Three"]);
    assert_eq!(checked.dropped_parts.len(), 1);
    assert_eq!(checked.dropped_parts[0].code, "BLOCK_SOURCE_MISSING");
    assert_eq!(checked.dropped_parts[0].at, "/parts/1/content/source");
}

// Scenario: Error path — a form that approves
#[tokio::test]
async fn error_path_a_form_that_approves() {
    let raw = json!({"kind":"form","title":"Compare quotes","verb":"Approve","target":{"kind":"agent_instance","agent_instance_id":"0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"},"command":"compare-quotes","fields":[{"name":"arguments","label":"Quotes to compare","required":true,"input":"text","value":"Kestrel, Acme and Northwind Q3","placeholder":null,"filled_by_ordin":true}]});
    let rejections = check_block(&raw, &ctx(), &Resolver::default())
        .await
        .unwrap_err();
    assert_eq!(rejections.len(), 1, "{rejections:?}");
    assert_eq!(rejections[0].code, "BLOCK_FORM_INVALID");
    assert_eq!(rejections[0].at, "/verb");
}

// Scenario: Happy path — the merged catalogue
#[test]
fn happy_path_the_merged_catalogue() {
    // Amended for ADR-110 to ADR-114: 400 rows after ADR-114.
    assert_eq!(CATALOGUE.len(), 400);
    let mut names: Vec<&str> = CATALOGUE.iter().map(|m| m.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 400);
    for name in [
        "ordin.send_message",
        "catalogue.search",
        "identity.sign_in",
        "app.quit",
        "app.pick_folder",
        "checker.check_installed",
        "checker.check_library_item",
        "checker.report",
    ] {
        assert!(command(name).is_some(), "{name}");
    }
    let set = command("projects.set_check_commands").unwrap();
    assert_eq!((set.scope, set.effect), (Scope::Any, Effect::Change));
    for gone in ["conversation.send", "signins.list", "agents.library"] {
        assert!(command(gone).is_none(), "{gone}");
    }
}

// Scenario: Edge case — sign_in_changed carries credential_kind
#[test]
fn edge_case_sign_in_changed_carries_credential_kind() {
    let data = json!({"credential_id":"0192a1d0-2222-7333-8444-555566667777","credential_kind":"github","state":"signed_in"});
    let event = ServiceEvent::from_journal("sign_in_changed", data).unwrap();
    let ServiceEvent::SignInChanged {
        credential_kind, ..
    } = &event
    else {
        panic!("sign_in_changed");
    };
    assert_eq!(
        *credential_kind,
        ordin_types::domain::CredentialKind::Github
    );
    let envelope = EventEnvelope {
        api_version: ServiceApiVersion::CURRENT,
        backend: backend(),
        seq: 1,
        at: Timestamp::now(),
        event,
    };
    let json = serde_json::to_string(&envelope).unwrap();
    assert_eq!(json.matches("\"kind\"").count(), 1);
    assert!(json.contains("\"kind\":\"sign_in_changed\""));
}

// Scenario: Error path — a code command over the Remote API
#[tokio::test]
async fn error_path_a_code_command_over_the_remote_api() {
    let api = Mock::answering(json!({}));
    let body = envelope(
        "projects.terminal_open",
        json!({"project_id": "0192a1d0-2222-7333-8444-555566667777", "cols": 80, "rows": 24}),
    );
    let (reply, json) = handle_with(&api, &body, "projects.terminal_open", paired(), 10).await;
    assert_eq!(json["error"]["code"], "SVC_COMMAND_LOCAL_ONLY");
    assert_eq!(reply.http_status, 404);
}

// Scenario: Happy path — app.quit from the screens
#[tokio::test]
async fn happy_path_app_quit_from_the_screens() {
    let api = Mock::answering(json!({"stopping_sessions": 2}));
    let (_, json) = handle_local(&api, "app.quit", json!({})).await;
    assert_eq!(json["kind"], "ok");
    assert_eq!(json["result"], json!({"stopping_sessions": 2}));
}

// Scenario: Edge case — a server refused for its version names the older side
#[test]
fn edge_case_a_server_refused_for_its_version_names_the_older_side() {
    let mut view = ServerView {
        server_connection_id: ServerConnectionId::new(),
        display_name: "Northwind server".to_owned(),
        state: ServerConnectionState::RefusedForVersion,
        as_of: None,
        backend: None,
        older_side: Some(OlderSide::Server),
    };
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["older_side"], "server");
    view.state = ServerConnectionState::Connected;
    view.older_side = None;
    let json = serde_json::to_value(&view).unwrap();
    assert!(json.get("older_side").is_none());
}

// Scenario: Happy path — an error reply carries its Diagnostics reference
#[tokio::test]
async fn happy_path_an_error_reply_carries_its_diagnostics_reference() {
    let api = Mock::new(|_, _| {
        Box::pin(async {
            let mut error = module_error("APPR_CARD_ALREADY_DECIDED");
            error.reference = Some("r-81".to_owned());
            Err(error)
        })
    });
    let (_, json) = handle_local(&api, "approval.decide", approval_press()).await;
    assert_eq!(json["error"]["code"], "APPR_CARD_ALREADY_DECIDED");
    assert_eq!(json["error"]["reference"], "r-81");
}

// Scenario: Happy path — a large text save is accepted
#[test]
fn happy_path_a_large_text_save_is_accepted() {
    let input = json!({
        "place_id": "0192a0f2-2222-7333-8444-555566667777",
        "path": "Q3/notes.md",
        "text": "x".repeat(6 * 1024 * 1024),
    });
    let outcome = accept_local("files.save_edit", input);
    assert!(
        !matches!(&outcome, Err(r) if r.code == "SVC_BODY_TOO_LARGE"),
        "{outcome:?}"
    );
}

// Scenario: Happy path — the injected block check equals the free function
#[tokio::test]
async fn happy_path_the_injected_block_check_equals_the_free_function() {
    let raw = table_example();
    let resolver = Resolver::default();
    let context = ctx();
    let injected: &dyn BlockCatalogueCheck = &StandardBlockCheck;
    let a = injected
        .check_block(&raw, &context, &resolver)
        .await
        .unwrap();
    let b = check_block(&raw, &context, &resolver).await.unwrap();
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap()
    );
    let report = json!({"kind": "report", "title": "T", "as_of": "2026-01-10T15:20:00.000Z", "parts": [table_part("One", true)]});
    let a = injected
        .check_report_tool(&report, &context, &resolver)
        .await
        .unwrap();
    let b = check_report_tool(&report, &context, &resolver)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(a.block).unwrap(),
        serde_json::to_value(b.block).unwrap()
    );
    let plan = json!({"action": "mark", "position": 2, "mark": "now"});
    let run = RunId::new();
    let a = injected
        .check_plan_tool(&plan, run, &resolver)
        .await
        .unwrap();
    let b = check_plan_tool(&plan, run, &resolver).await.unwrap();
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap()
    );
}

// Scenario: Happy path — locale_changed carries the owner's payload
#[test]
fn happy_path_locale_changed_carries_the_owners_payload() {
    let data = json!({"active":{"locale":"fr-FR","source":"person","direction":"ltr","fallback_chain":["fr-FR","en-GB"]}});
    let event = ServiceEvent::from_journal("locale_changed", data.clone()).unwrap();
    let ServiceEvent::LocaleChanged { active } = &event else {
        panic!("locale_changed");
    };
    assert_eq!(active.locale, "fr-FR");
    assert_eq!(event.to_journal_data(), data);
}

fn decide_card_input() -> Value {
    json!({
        "server_or_agent": "0192a1d0-2222-7333-8444-555566667777",
        "approval_id": "0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f",
        "decision": "approve",
    })
}

// Scenario: Happy path — pressing a hosted agent's approver card (SA-CLOSE-05)
#[tokio::test]
async fn happy_path_pressing_a_hosted_agents_approver_card() {
    let api = Mock::new(|method, input| {
        Box::pin(async move {
            assert_eq!(method, "hosted_decide_card");
            assert_eq!(input["decision"], "approve");
            assert!(input.get("note").is_none_or(Value::is_null));
            Ok(json!({"state": "approved"}))
        })
    });
    let (_, json) = handle_local(&api, "hosted.decide_card", decide_card_input()).await;
    assert_eq!(json["result"], json!({"state": "approved"}));
}

// Scenario: Error path — hosted.decide_card from a paired desktop
#[test]
fn error_path_hosted_decide_card_from_a_paired_desktop() {
    let body = envelope("hosted.decide_card", decide_card_input());
    let refusal = accept_with(&body, "hosted.decide_card", paired()).unwrap_err();
    assert_eq!(refusal.code, "SVC_COMMAND_LOCAL_ONLY");
}

// Scenario: Error path — the card was already decided by another approver
#[tokio::test]
async fn error_path_the_card_was_already_decided_by_another_approver() {
    let api = Mock::new(|_, _| Box::pin(async { Err(module_error("HAA_CARD_DECIDED")) }));
    let (_, json) = handle_local(&api, "hosted.decide_card", decide_card_input()).await;
    assert_eq!(json["error"]["code"], "HAA_CARD_DECIDED");
}

// Scenario: Error path — the generated TypeScript is stale
#[test]
fn error_path_the_generated_typescript_is_stale() {
    let dir = std::env::temp_dir().join(format!("ordin-svc-gen-{}", std::process::id()));
    let bin = env!("CARGO_BIN_EXE_ordin-service-api-gen");
    let status = std::process::Command::new(bin)
        .args(["write"])
        .arg(&dir)
        .status()
        .unwrap();
    assert!(status.success());
    let ok = std::process::Command::new(bin)
        .args(["check"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(ok.status.success());
    // A field added to RunView without regenerating leaves the committed file behind.
    let types = dir.join("src/lib/service-api/types.gen.ts");
    let text = std::fs::read_to_string(&types).unwrap();
    std::fs::write(
        &types,
        text.replacen(
            "export interface RunView {",
            "export interface RunView {\n  added?: string;",
            1,
        ),
    )
    .unwrap();
    let stale = std::process::Command::new(bin)
        .args(["check"])
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(stale.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&stale.stdout),
        "service-api types are stale: types.gen.ts\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
