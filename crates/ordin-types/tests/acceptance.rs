//! X2: one test per Gherkin scenario of SharedTypes.md (and its knowledge-agents amendment), each
//! named after its scenario; the exact title is the comment above each test.

use ordin_types::domain::agent_profile::ScopeKind;
use ordin_types::domain::runs::Run;
use ordin_types::paging::Page;
use ordin_types::prelude::*;
use ordin_types::session::ToolCaller;
use ordin_types::text::{check_len, normalise_text};
use ordin_types::wire::knowledge_query::{PageStatus, PageTier};
use ordin_types::wire::run::ModelChoice;
use ordin_types::yaml::{read_restricted, read_restricted_node, YamlError, YamlNode};
use serde::Deserialize;
use serde_json::json;

/// The serde error a refused value produces carries the `TypesError`'s text, which names its code's rule.
fn refused_with<T: serde::de::DeserializeOwned + std::fmt::Debug>(json: &str, error: TypesError) {
    let refusal = serde_json::from_str::<T>(json).expect_err("the value is refused");
    assert!(
        refusal.to_string().starts_with(&error.to_string()),
        "{refusal} is not {}",
        error.code()
    );
}

// Scenario: Happy path — an identifier and a timestamp round-trip
#[test]
fn happy_path_an_identifier_and_a_timestamp_round_trip() {
    let run = RunId::new();
    let at: Timestamp = serde_json::from_str("\"2026-01-10T09:41:07.312Z\"").unwrap();
    let run_back: RunId = serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
    let at_back: Timestamp = serde_json::from_str(&serde_json::to_string(&at).unwrap()).unwrap();
    assert_eq!(run_back, run);
    assert_eq!(at_back, at);
    assert_eq!(run.as_uuid().get_version_num(), 7);
}

// Scenario: Edge case — a version-4 UUID is refused
#[test]
fn edge_case_a_version_4_uuid_is_refused() {
    refused_with::<WorkspaceId>(
        "\"0192a1c2-3344-4556-8677-8899aabbccdd\"",
        TypesError::IdNotUuidV7,
    );
    assert_eq!(TypesError::IdNotUuidV7.code(), "ID_NOT_UUIDV7");
}

// Scenario: Edge case — an uppercase identifier is refused
#[test]
fn edge_case_an_uppercase_identifier_is_refused() {
    refused_with::<RunId>(
        "\"0192A1C2-3344-7556-8677-8899AABBCCDD\"",
        TypesError::IdNotUuidV7,
    );
}

// Scenario: Error path — a timestamp without milliseconds is refused
#[test]
fn error_path_a_timestamp_without_milliseconds_is_refused() {
    refused_with::<Timestamp>(
        "\"2026-01-10T09:41:07Z\"",
        TypesError::TimestampNotRfc3339MsUtc,
    );
    assert_eq!(
        TypesError::TimestampNotRfc3339MsUtc.code(),
        "TS_NOT_RFC3339_MS_UTC"
    );
}

// Scenario: Error path — a timestamp with an offset is refused
#[test]
fn error_path_a_timestamp_with_an_offset_is_refused() {
    refused_with::<Timestamp>(
        "\"2026-01-10T10:41:07.312+01:00\"",
        TypesError::TimestampNotRfc3339MsUtc,
    );
}

// Scenario: Happy path — text is NFC-normalised before its length is counted
#[test]
fn happy_path_text_is_nfc_normalised_before_its_length_is_counted() {
    let decomposed = "Cafe\u{301}";
    assert_eq!(decomposed.chars().count(), 5);
    let normalised = normalise_text(decomposed).unwrap();
    assert_eq!(normalised, "Caf\u{e9}");
    assert_eq!(normalised.chars().count(), 4);
    check_len(&normalised, 1, 4, "X_TOO_LONG").unwrap();
}

// Scenario: Error path — a carriage return is refused
#[test]
fn error_path_a_carriage_return_is_refused() {
    let refusal = normalise_text("line one\r\nline two").unwrap_err();
    assert_eq!(refusal.code(), "TEXT_CONTROL_CHAR");
}

// Scenario: Error path — a malformed decimal string
#[test]
fn error_path_a_malformed_decimal_string() {
    refused_with::<DecimalString>("\"1e5\"", TypesError::NumNotDecimal);
    assert_eq!(TypesError::NumNotDecimal.code(), "NUM_NOT_DECIMAL");
}

// Scenario: Error path — a forged cursor
#[test]
fn error_path_a_forged_cursor() {
    let forged: Cursor = serde_json::from_value(json!("A".repeat(1025))).unwrap();
    let refusal = forged.decode::<Vec<u32>>().unwrap_err();
    assert_eq!(refusal.code(), "SRV_BAD_CURSOR");
}

// Scenario: Edge case — the last page carries next_cursor null
#[test]
fn edge_case_the_last_page_carries_next_cursor_null() {
    let page: Page<RunId> = Page {
        items: vec![RunId::new()],
        next_cursor: None,
    };
    let value = serde_json::to_value(&page).unwrap();
    let object = value.as_object().unwrap();
    assert!(object.contains_key("next_cursor"));
    assert_eq!(object["next_cursor"], serde_json::Value::Null);
}

// Scenario: Happy path — a model choice serialises in the 2C form
#[test]
fn happy_path_a_model_choice_serialises_in_the_2c_form() {
    let choice = ModelChoice {
        level: ModelLevel::Standard,
        model: Some("sonnet".into()),
    };
    assert_eq!(
        serde_json::to_string(&choice).unwrap(),
        r#"{"level":"standard","model":"sonnet"}"#
    );
    // "ordin_runtime_adapter::ModelChoice is the same type as ordin_types::wire::run::ModelChoice":
    // the runtime-adapter crate re-exports this module; that half is checked in its own Step.
    let _: ordin_types::prelude::ModelChoice = choice;
}

// Scenario: Error path — an unknown field in a model choice
#[test]
fn error_path_an_unknown_field_in_a_model_choice() {
    let refused = serde_json::from_str::<ModelChoice>(
        r#"{"level":"standard","model":"sonnet","thinking":"high"}"#,
    );
    assert!(refused.is_err());
}

// Scenario: Happy path — every event kind round-trips through its wire name
#[test]
fn happy_path_every_event_kind_round_trips_through_its_wire_name() {
    use EventKind::*;
    let all = [
        RunStateChanged,
        RunProgress,
        PlanChanged,
        StepMarked,
        StartUpReportReady,
        ApprovalRequested,
        ApprovalDecided,
        CrewRoundChanged,
        BlockShown,
        Reset,
        ServerStateChanged,
        WorkspaceChanged,
        NeedsYouChanged,
        BlockAvailable,
        LocaleChanged,
        BrandChanged,
        SetupProgress,
        SignInChanged,
        LanguageChanged,
        KnowledgeAgentsChanged,
        MoreFound,
    ];
    assert_eq!(all.len(), 21);
    for kind in all {
        assert_eq!(EventKind::parse(kind.as_str()), Some(kind));
        // The serde name is the same wire name.
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(kind.as_str()));
    }
    assert_eq!(LanguageChanged.as_str(), "language_changed");
    assert_eq!(KnowledgeAgentsChanged.as_str(), "knowledge_agents_changed");
    assert_eq!(MoreFound.as_str(), "more_found");
    assert_eq!(EventKind::parse("job_finished"), None);
}

// Scenario: Happy path — a crew member's tool caller serialises in the 2C form
#[test]
fn happy_path_a_crew_members_tool_caller_serialises_in_the_2c_form() {
    let caller = ToolCaller::CrewMember {
        crew_run_id: CrewRunId::parse("0192a1c5-0000-7000-8000-000000000001").unwrap(),
    };
    assert_eq!(
        serde_json::to_string(&caller).unwrap(),
        r#"{"kind":"crew_member","crew_run_id":"0192a1c5-0000-7000-8000-000000000001"}"#
    );
    // The re-export by `ordin_runtime_adapter` is checked in that crate's own Step.
}

// Scenario: Happy path — the page form lives at ordin_types::paging
#[test]
fn happy_path_the_page_form_lives_at_ordin_types_paging() {
    let page: ordin_types::paging::Page<RunId> = ordin_types::paging::Page {
        items: vec![RunId::new()],
        next_cursor: Some(ordin_types::paging::Cursor::encode(&[42u32])),
    };
    let cursor = page.next_cursor.unwrap();
    assert_eq!(cursor.decode::<Vec<u32>>().unwrap(), vec![42]);
}

// Scenario: Happy path — a plain YAML document is read
#[test]
fn happy_path_a_plain_yaml_document_is_read() {
    #[derive(Deserialize)]
    struct Doc {
        name: String,
        steps: Vec<String>,
    }
    let doc: Doc = read_restricted("name: Rates\nsteps:\n  - one\n  - two\n", 1024).unwrap();
    assert_eq!(doc.name, "Rates");
    assert_eq!(doc.steps, vec!["one".to_owned(), "two".to_owned()]);
}

// Scenario: Happy path — the node form keeps a scalar's source text
#[test]
fn happy_path_the_node_form_keeps_a_scalars_source_text() {
    let node = read_restricted_node("version: 1.10\n", 1024).unwrap();
    assert_eq!(
        node,
        YamlNode::Map(vec![(
            "version".into(),
            YamlNode::Scalar {
                text: "1.10".into(),
                quoted: false
            }
        )])
    );
}

// Scenario: Error path — an alias is refused
#[test]
fn error_path_an_alias_is_refused() {
    let refusal =
        read_restricted::<serde_json::Value>("base: &b {a: 1}\nother: *b\n", 1024).unwrap_err();
    assert_eq!(refusal.code(), "YAML_FEATURE_REFUSED");
}

// Scenario: Error path — a custom tag is refused
#[test]
fn error_path_a_custom_tag_is_refused() {
    let refusal =
        read_restricted::<serde_json::Value>("value: !python/object foo\n", 1024).unwrap_err();
    assert_eq!(refusal.code(), "YAML_FEATURE_REFUSED");
}

// Scenario: Error path — a duplicate key is malformed
#[test]
fn error_path_a_duplicate_key_is_malformed() {
    let refusal = read_restricted::<serde_json::Value>("a: 1\na: 2\n", 1024).unwrap_err();
    assert_eq!(refusal.code(), "YAML_MALFORMED");
}

// Scenario: Edge case — input one byte over the limit
#[test]
fn edge_case_input_one_byte_over_the_limit() {
    // Not YAML at all: a parse would fail as malformed, so `TooLarge` proves no parse ran.
    let text = "[".repeat(1025);
    let refusal = read_restricted::<serde_json::Value>(&text, 1024).unwrap_err();
    assert_eq!(refusal, YamlError::TooLarge { max_bytes: 1024 });
    assert_eq!(refusal.code(), "YAML_TOO_LARGE");
}

// Scenario: Happy path — one AdapterKind across domain and wire
#[test]
fn happy_path_one_adapter_kind_across_domain_and_wire() {
    let run = json!({
        "id": RunId::new(),
        "origin": {"kind": "task", "task_id": TaskId::new()},
        "agent": {"kind": "remote_agent", "remote_agent_id": RemoteAgentId::new()},
        "adapter": "hosted_agent",
        "state": "running",
        "limits": {"wall_clock_secs": 3600, "idle_timeout_secs": 600},
        "usage": {"model_turns": 0, "tool_calls": 0},
        "inputs_used": [],
        "queued_at": "2026-01-10T09:41:07.312Z"
    });
    let parsed: Run = serde_json::from_value(run).expect("a Run in the 2C form");
    assert_eq!(
        parsed.adapter,
        ordin_types::wire::run::AdapterKind::HostedAgent
    );
    let written = serde_json::to_value(&parsed).unwrap();
    assert_eq!(written["adapter"], "hosted_agent");
    // `domain::runs::AdapterKind` is the wire type itself, so this assignment compiles.
    let same: ordin_types::domain::runs::AdapterKind =
        ordin_types::wire::run::AdapterKind::HostedAgent;
    assert_eq!(same, parsed.adapter);
}

// ---- Feature: Shared types — knowledge agents (amendment January 2026, ADR-113) ----

// Scenario: Happy path — a connection id round-trips
#[test]
fn happy_path_a_connection_id_round_trips() {
    let id = KnowledgeConnectionId::new();
    let text = serde_json::to_string(&id).unwrap();
    let back: KnowledgeConnectionId = serde_json::from_str(&text).unwrap();
    assert_eq!(back, id);
    let shown = id.to_string();
    assert_eq!(shown.len(), 36);
    assert_eq!(shown, shown.to_lowercase());
    assert_eq!(id.as_uuid().get_version_num(), 7);
}

// Scenario: Error — an answer id that is not version 7
#[test]
fn error_an_answer_id_that_is_not_version_7() {
    let refusal = KnowledgeAnswerId::parse("0192b7a0-5c1d-4e2f-8a3b-4c5d6e7f8091").unwrap_err();
    assert_eq!(refusal.to_string(), "identifier is not a version-7 UUID");
}

// Scenario: Happy path — the knowledge scope kind
#[test]
fn happy_path_the_knowledge_scope_kind() {
    assert_eq!(
        serde_json::to_string(&ScopeKind::Knowledge).unwrap(),
        "\"knowledge\""
    );
}

// Scenario: Happy path — page tiers carry their trust
#[test]
fn happy_path_page_tiers_carry_their_trust() {
    assert_eq!(PageTier::HumanReviewed.trust_signal(), 1.0);
    assert_eq!(PageTier::MachineConfirmed.trust_signal(), 0.9);
    assert_eq!(PageTier::Unverified.trust_signal(), 0.8);
    assert_eq!(PageTier::default(), PageTier::Unverified);
    assert_eq!(PageStatus::default(), PageStatus::Stable);
}

// ModelPolicy.md (amended January 2026): the constructors keep the secret out of `Debug`.
#[test]
fn model_policy_constructors_keep_the_value_out_of_debug() {
    use ordin_types::domain::agent_profile::{HarnessEnvVar, OrganisationKey};
    let key = OrganisationKey::new("sk-test-key".to_owned());
    assert_eq!(key.expose(), "sk-test-key");
    assert_eq!(format!("{key:?}"), "OrganisationKey(<redacted>)");
    let var = HarnessEnvVar::new("GATEWAY_TOKEN".to_owned(), "token-value".to_owned());
    assert_eq!(var.value(), "token-value");
    assert_eq!(
        format!("{var:?}"),
        "HarnessEnvVar { name: \"GATEWAY_TOKEN\", value: <redacted> }"
    );
}

// phase-2c-shared-types-domain.md (amended January 2026): `PartKind` sorts in declaration order.
#[test]
fn part_kind_sorts_in_declaration_order() {
    use ordin_types::domain::agents::PartKind;
    let mut kinds = vec![PartKind::ControlLoop, PartKind::Rule, PartKind::SystemRole];
    kinds.sort();
    assert_eq!(
        kinds,
        vec![PartKind::SystemRole, PartKind::Rule, PartKind::ControlLoop]
    );
}
