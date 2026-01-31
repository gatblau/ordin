//! Amendment January 2026 — Ordin remembers facts as triples (ADR-118): one test per Gherkin
//! scenario, named as the scenario, and one per Error Table row.

use std::sync::Arc;

use ordin_mcp_front::{ServerName, ToolKey};
use ordin_memory_service::{
    FactSourceInput, MemoryCaller, ObjectInput, ObjectKindInput, TermOrigin, TermView,
    VocabularyView,
};
use ordin_types::MemoryFactId;
use serde_json::json;

use super::support::*;
use crate::ports::SourceDocument;

fn remember_key() -> ToolKey {
    ToolKey {
        server: ServerName::new("ordin").unwrap(),
        tool: "remember".to_owned(),
    }
}

fn term(key: &str, label: &str, object: ObjectKindInput, single_valued: bool) -> TermView {
    TermView {
        key: key.to_owned(),
        label: label.to_owned(),
        meaning: format!("What {label} means."),
        object,
        single_valued,
        added_by: TermOrigin::Seed,
        fact_count: 0,
        seed_unrenamed: true,
    }
}

fn one_fact() -> serde_json::Value {
    json!([{ "subject": { "name": "Fast Track Fund" }, "predicate": "amount", "object": { "money": { "amount": "3000000", "currency": "GBP" } } }])
}

fn document_of(input: &ordin_memory_service::RememberFactsInput) -> (String, String) {
    match &input.source {
        Some(FactSourceInput::Document(s)) => (s.origin_ref.clone(), s.version.clone()),
        other => panic!("not a document source: {other:?}"),
    }
}

// Scenario: Happy path — Ordin keeps two facts from one document (ADR-118)
// spec: ordin-desktop/components/OrdinHost#OH-TR-2
#[tokio::test]
async fn happy_path_ordin_keeps_two_facts_from_one_document() {
    let f = fixture().await;
    let run = f.open().await;
    *f.workspaces.documents.lock().unwrap() = vec![
        SourceDocument {
            origin_path: "Funding/Other.docx".into(),
            origin_version_read: "v1".into(),
            local_version: 1,
        },
        SourceDocument {
            origin_path: "Funding/Fast Track.docx".into(),
            origin_version_read: "v7".into(),
            local_version: 2,
        },
    ];
    let args = json!({
        "source": "Funding/Fast Track.docx",
        "facts": [
            {"subject":{"name":"Fast Track Fund"},"predicate":"amount","object":{"money":{"amount":"3000000","currency":"GBP"}}},
            {"subject":{"name":"Fast Track Fund"},"predicate":"startDate","object":{"date":"2026-04"}}
        ]
    });
    f.host
        .tool_provider()
        .check_arguments(&remember_key(), &args)
        .expect("the arguments pass");
    let out = f.tool("remember", args).await.unwrap();
    assert_eq!(out.text, "Kept 2.");
    let ids = out.structured.as_ref().unwrap()["fact_ids"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(ids.len(), 2);
    let calls = f.memory.facts_remembered.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "memory.remember_facts is called once");
    let (who, input) = &calls[0];
    assert!(
        matches!(who, MemoryCaller::Ordin { workspace_id, run_id } if *workspace_id == f.ws && *run_id == run)
    );
    assert_eq!(
        document_of(input),
        ("Funding/Fast Track.docx".to_owned(), "v7/2".to_owned())
    );
    assert_eq!(input.facts.len(), 2);
    assert!(input.fact_class.is_none());
    assert_eq!(
        input.facts[1].object,
        ObjectInput::Date("2026-04".to_owned())
    );
    // The lookup is read once for the call.
    assert_eq!(f.f.count("workspaces.source_documents"), 1);
    // Each kept receipt joins the run's own facts (OH-AM-3).
    let own = crate::host::lock(&f.host.state)
        .convs
        .get(&f.ws)
        .map(|c| c.own_facts.len());
    assert_eq!(own, Some(2));
}

// Scenario: Edge — Ordin says when a fact became true
#[tokio::test]
async fn edge_ordin_says_when_a_fact_became_true() {
    let f = fixture().await;
    f.open().await;
    let facts = json!([{ "subject": { "name": "Kestrel" }, "predicate": "foundingDate", "object": { "date": "1998" }, "valid_from": "2024" }]);
    f.tool(
        "remember",
        json!({ "source": "conversation", "facts": facts }),
    )
    .await
    .unwrap();
    let calls = f.memory.facts_remembered.lock().unwrap().clone();
    assert_eq!(calls[0].1.facts[0].valid_from.as_deref(), Some("2024"));
    assert_eq!(calls[0].1.source, Some(FactSourceInput::Conversation));
    // A conversation source reads no working copy.
    assert_eq!(f.f.count("workspaces.source_documents"), 0);
}

// Scenario: Edge — a source that is not in the working copy (OH-TR-2)
// spec: ordin-desktop/components/OrdinHost#OH-TR-2
#[tokio::test]
async fn edge_a_source_that_is_not_in_the_working_copy() {
    let f = fixture().await;
    f.open().await;
    f.tool(
        "remember",
        json!({ "source": "Attached: notes.pdf", "facts": one_fact() }),
    )
    .await
    .unwrap();
    let calls = f.memory.facts_remembered.lock().unwrap().clone();
    assert_eq!(
        document_of(&calls[0].1),
        ("Attached: notes.pdf".to_owned(), "as read".to_owned())
    );
}

// Error row: the working copy cannot be read while the source's version is looked up (OH-TR-2) —
// the remember goes ahead with version "as read".
#[tokio::test]
async fn the_working_copy_cannot_be_read_while_the_version_is_looked_up() {
    let f = fixture().await;
    f.open().await;
    f.f.always("workspaces.source_documents", "WS_STORE_UNAVAILABLE", true);
    let out = f
        .tool(
            "remember",
            json!({ "source": "Funding/Fast Track.docx", "facts": one_fact() }),
        )
        .await
        .unwrap();
    assert_eq!(out.text, "Kept 1.");
    let calls = f.memory.facts_remembered.lock().unwrap().clone();
    assert_eq!(document_of(&calls[0].1).1, "as read");
}

// The version is cut at 256 characters (OH-TR-2).
#[tokio::test]
async fn a_long_version_is_cut_at_256_characters() {
    let f = fixture().await;
    f.open().await;
    *f.workspaces.documents.lock().unwrap() = vec![SourceDocument {
        origin_path: "a.docx".into(),
        origin_version_read: "v".repeat(300),
        local_version: 3,
    }];
    f.tool(
        "remember",
        json!({ "source": "a.docx", "facts": one_fact() }),
    )
    .await
    .unwrap();
    let calls = f.memory.facts_remembered.lock().unwrap().clone();
    assert_eq!(document_of(&calls[0].1).1.chars().count(), 256);
}

// Scenario: Edge — a summary from the consolidation turn names the conversation
#[tokio::test]
async fn edge_a_summary_from_the_consolidation_turn_names_the_conversation() {
    let f = fixture().await;
    f.open().await;
    let ids = [MemoryFactId::new(), MemoryFactId::new()];
    let args = json!({ "source": "conversation", "summary": "Borealis is our packaging supplier.", "made_from": ids });
    f.host
        .tool_provider()
        .check_arguments(&remember_key(), &args)
        .expect("the arguments pass");
    let out = f.tool("remember", args).await.unwrap();
    assert_eq!(out.text, "Kept.");
    let remembered = f.memory.remembered.lock().unwrap().clone();
    assert_eq!(remembered.len(), 1);
    let input = &remembered[0].1;
    assert!(input.source.is_none());
    assert_eq!(input.made_from.len(), 2);
    assert_eq!(input.statement, "Borealis is our packaging supplier.");
    assert!(f.memory.facts_remembered.lock().unwrap().is_empty());
}

// Scenario: Error — a value of the wrong kind is returned to the model
#[tokio::test]
async fn error_a_value_of_the_wrong_kind_is_returned_to_the_model() {
    let f = fixture().await;
    f.open().await;
    f.f.once("memory.remember_facts", "MEM_TERM_OBJECT_MISMATCH", true);
    let e = f
        .tool(
            "remember",
            json!({ "source": "conversation", "facts": [{ "subject": { "name": "Order 7731" }, "predicate": "price", "object": { "text": "cheap" } }] }),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "MEM_TERM_OBJECT_MISMATCH");
    assert!(!e.retryable);
}

// Scenario: Error — facts and a summary together
// Error row: both or neither of `facts` and `summary`, `made_from` without `summary`, or `summary`
// out of bounds.
// spec: ordin-desktop/components/OrdinHost#OH-TR-2
#[tokio::test]
async fn error_facts_and_a_summary_together() {
    let f = fixture().await;
    f.open().await;
    let ids = [MemoryFactId::new(), MemoryFactId::new()];
    for args in [
        json!({ "source": "conversation", "facts": one_fact(), "summary": "x" }),
        json!({ "source": "conversation" }),
        json!({ "source": "conversation", "facts": one_fact(), "made_from": ids }),
        json!({ "source": "conversation", "summary": "x" }),
        json!({ "source": "conversation", "summary": "", "made_from": ids }),
        json!({ "source": "conversation", "summary": "s".repeat(501), "made_from": ids }),
    ] {
        let p = f
            .host
            .tool_provider()
            .check_arguments(&remember_key(), &args)
            .expect_err("an argument problem");
        assert_eq!(p.at, "/facts", "{args}");
        assert_eq!(
            p.reason,
            "remember takes facts, or a summary with made_from."
        );
    }
    // Neither the facts nor the summary reach the Memory Service.
    assert!(f.memory.facts_remembered.lock().unwrap().is_empty());
    assert!(f.memory.remembered.lock().unwrap().is_empty());
    // A source is required, and 17 facts are too many.
    let p = f
        .host
        .tool_provider()
        .check_arguments(
            &remember_key(),
            &json!({ "source": "", "facts": one_fact() }),
        )
        .unwrap_err();
    assert_eq!(p.at, "/source");
    let many: Vec<serde_json::Value> = (0..17).map(|_| one_fact()[0].clone()).collect();
    assert!(f
        .host
        .tool_provider()
        .check_arguments(
            &remember_key(),
            &json!({ "source": "conversation", "facts": many })
        )
        .is_err());
}

// Scenario: Edge — the seed lists the link words
// spec: ordin-desktop/components/OrdinHost#OH-TR-2
#[tokio::test]
async fn edge_the_seed_lists_the_link_words() {
    let f = fixture().await;
    let mut terms: Vec<TermView> = (0..26)
        .map(|i| {
            term(
                &format!("seed{i:02}"),
                &format!("seed word {i:02}"),
                ObjectKindInput::Thing,
                false,
            )
        })
        .collect();
    terms.push(term(
        "accountManager",
        "account manager",
        ObjectKindInput::Thing,
        true,
    ));
    *f.memory.vocabulary.lock().unwrap() = Some(VocabularyView { terms });
    f.host
        .send_message(f.ws, "hello".to_owned(), vec![])
        .await
        .unwrap();
    let seed = f.supervisor.starts.lock().unwrap()[0].seed.clone();
    let words = seed.find("## Link words\n").expect("a Link words section");
    let workspace = seed.find("## Workspace\n").unwrap();
    let voice = seed.find("## Voice\n").unwrap();
    assert!(
        workspace < words && words < voice,
        "after the workspace summary, before the voice"
    );
    let section = &seed[words..voice];
    assert_eq!(section.lines().count(), 1 + 27);
    assert!(section
        .lines()
        .any(|l| l == "accountManager — account manager; thing; one at a time"));
    assert!(section.lines().any(|l| l.ends_with("; several")));
}

// The Link words section's bounds: at most 500 lines, then "… and <n> more".
#[test]
fn the_link_words_section_is_bounded() {
    let terms: Vec<TermView> = (0..503)
        .map(|i| {
            term(
                &format!("t{i:03}"),
                &format!("word {i:03}"),
                ObjectKindInput::Text,
                false,
            )
        })
        .collect();
    let text = crate::seed::link_words_text(&VocabularyView { terms });
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1 + 500 + 1);
    assert_eq!(*lines.last().unwrap(), "… and 3 more");
    assert!(text.chars().count() <= 24_000 + 20);
    // By label.
    assert_eq!(lines[1], "t000 — word 000; text; several");
}

// SPEC-GAP-1: a vocabulary that cannot be read leaves the section out; the conversation starts.
#[tokio::test]
async fn a_vocabulary_that_cannot_be_read_leaves_the_section_out() {
    let f = fixture().await;
    f.f.always("memory.vocabulary", "MEM_STORE_UNAVAILABLE", true);
    f.host
        .send_message(f.ws, "hello".to_owned(), vec![])
        .await
        .unwrap();
    let seed = f.supervisor.starts.lock().unwrap()[0].seed.clone();
    assert!(!seed.contains("## Link words"));
}

// Scenario: Edge — kept points are turned into facts by Ordin (OH-TR-1)
// spec: ordin-desktop/components/OrdinHost#OH-TR-1
#[tokio::test]
async fn edge_kept_points_are_turned_into_facts_by_ordin() {
    let f = fixture().await;
    let run = f.open().await;
    f.host
        .start_fresh(
            f.ws,
            vec!["Kestrel is cheapest".into(), "Quote due Friday".into()],
        )
        .await
        .expect("started fresh");
    let turns: Vec<String> = f
        .supervisor
        .turn_texts()
        .into_iter()
        .filter(|t| t.starts_with(crate::conversation::KEEP_POINTS_TEXT))
        .collect();
    assert_eq!(turns.len(), 1, "one keep_points_turn");
    assert_eq!(
        turns[0].lines().skip(1).collect::<Vec<_>>(),
        vec!["Kestrel is cheapest", "Quote due Friday"]
    );
    assert!(
        f.memory.remembered.lock().unwrap().is_empty(),
        "memory.remember is not called by the host"
    );
    // The run stops when that turn ends.
    assert!(f.supervisor.cancelled.lock().unwrap().contains(&run));
}

// Error row: the keep-points turn does not end within its bound — the run stops anyway.
// spec: ordin-desktop/components/OrdinHost#OH-TR-1
#[tokio::test]
async fn the_keep_points_turn_does_not_end_in_time() {
    let f = fixture().await;
    let run = f.open().await;
    f.supervisor.respond(Arc::new(|_, text| {
        Box::pin(async move {
            if text.starts_with(crate::conversation::KEEP_POINTS_TEXT) {
                None
            } else {
                Some("Ready.".to_owned())
            }
        })
    }));
    f.host
        .start_fresh(f.ws, vec!["Kestrel is cheapest".into()])
        .await
        .expect("started fresh");
    assert!(f.supervisor.cancelled.lock().unwrap().contains(&run));
}

// Step 33: at most 5 points go to the keep-points turn.
#[tokio::test]
async fn at_most_five_points_go_to_the_keep_points_turn() {
    let f = fixture().await;
    f.open().await;
    let points: Vec<String> = (0..7).map(|i| format!("Point {i}")).collect();
    f.host.start_fresh(f.ws, points).await.unwrap();
    let turn = f
        .supervisor
        .turn_texts()
        .into_iter()
        .find(|t| t.starts_with(crate::conversation::KEEP_POINTS_TEXT))
        .unwrap();
    assert_eq!(turn.lines().count(), 1 + 5);
}

// Scenario: Happy path — the tool texts stay within the MCP Front's limits
#[tokio::test]
async fn happy_path_the_tool_texts_stay_within_the_mcp_fronts_limits() {
    let f = fixture().await;
    let servers = f.host.tool_provider().servers();
    let remember = servers
        .iter()
        .flat_map(|s| s.tools.iter())
        .find(|t| t.name == "remember")
        .unwrap();
    assert_eq!(remember.description.chars().count(), 119);
    assert_eq!(remember.argument_form.chars().count(), 298);
    assert_eq!(remember.plain.chars().count(), 67);
    let facts = &remember.input_schema["properties"]["facts"];
    assert_eq!(facts["maxItems"], 16);
    assert!(facts["items"]["properties"]["predicate"]["oneOf"].is_array());
    // OH-TOP-1: the subject takes an optional topic of 1–40 characters; an object's thing does not.
    let topic = &facts["items"]["properties"]["subject"]["properties"]["topic"];
    assert_eq!(topic["maxLength"], 40);
    assert_eq!(topic["minLength"], 1);
    assert!(
        facts["items"]["properties"]["object"]["oneOf"][0]["properties"]["thing"]["properties"]
            .get("topic")
            .is_none()
    );
}

// Scenario: Edge — the fixed instructions version (OH-REC-2, OH-TR-3)
// spec: ordin-desktop/components/OrdinHost#OH-TR-3
#[test]
fn edge_the_fixed_instructions_version() {
    // Advanced to 1.3.0 by OH-TF-6, then to 1.4.0 by OH-TOP-2 (topics of things).
    assert_eq!(crate::FIXED_INSTRUCTIONS_VERSION, "1.4.0");
}

// Scenario: Happy path — the remember schema states the one-claim rule (OH-TR-3)
// spec: ordin-desktop/components/OrdinHost#OH-TR-3
#[tokio::test]
async fn happy_path_the_remember_schema_states_the_one_claim_rule() {
    let f = fixture().await;
    let servers = f.host.tool_provider().servers();
    let remember = servers
        .iter()
        .flat_map(|s| s.tools.iter())
        .find(|t| t.name == "remember")
        .unwrap();
    let item = &remember.input_schema["properties"]["facts"]["items"]["properties"];
    let text = item["object"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|o| o["properties"].get("text"))
        .unwrap();
    assert_eq!(text["maxLength"], 120);
    assert!(text["description"]
        .as_str()
        .unwrap()
        .contains("several claims are several facts"));
    let label = item["predicate"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|o| o["properties"].get("propose"))
        .unwrap()["properties"]["label"]
        .clone();
    assert!(label["description"]
        .as_str()
        .unwrap()
        .contains("never a kind of note"));
}

// Error Table row: remember whose facts the Memory Service refuses — Ordin reads what to do instead
// (OH-TR-3, MemoryService.md MEM-TR-13).
// spec: ordin-desktop/components/OrdinHost#OH-TR-3
#[test]
fn a_refused_fact_reaches_ordin_with_the_memory_services_sentence() {
    let long = ordin_memory_service::MemoryError::Fact {
        code: "MEM_TRIPLE_INVALID",
        index: 0,
        field: "object.text",
    };
    let e = crate::ports::memory_refusal(&long);
    assert_eq!(e.code, "MEM_TRIPLE_INVALID");
    assert!(!e.retryable);
    assert!(
        e.message.starts_with("facts[0].object.text: "),
        "{}",
        e.message
    );
    assert!(
        e.message.contains("Split it into separate facts"),
        "{}",
        e.message
    );
    let note = ordin_memory_service::MemoryError::Fact {
        code: "MEM_TERM_PROPOSAL_INVALID",
        index: 2,
        field: "predicate.propose.label",
    };
    let e = crate::ports::memory_refusal(&note);
    assert_eq!(e.code, "MEM_TERM_PROPOSAL_INVALID");
    assert!(
        e.message.starts_with("facts[2].predicate.propose.label: "),
        "{}",
        e.message
    );
    assert!(
        e.message.contains("how the thing relates to the value"),
        "{}",
        e.message
    );
}
