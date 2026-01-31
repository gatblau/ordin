//! The scenarios of the ADR-112/113 amendment (trusted pages and knowledge agents) and of the
//! ADR-115 amendment (facts that inherit their readers), each a test of the same name.

use std::sync::Arc;
use std::time::Duration;

use ordin_journal::JournalEvent;
use ordin_mcp_front::ToolCaller;
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::{UnansweredReason, UnansweredSource};
use ordin_types::{AgentInstanceId, KnowledgeAnswerId, KnowledgePageId, MemoryFactId, ScopeId};
use serde_json::json;

use super::support::*;
use crate::db::q;
use crate::model::*;

fn standing(tier: PageTier, status: PageStatus, past: bool) -> PageGraph {
    PageGraph {
        standing: PageStanding {
            tier,
            status,
            past_stale_after: past,
        },
        ..graph("A page")
    }
}

#[tokio::test]
async fn happy_path_checked_knowledge_ranks_first() {
    let rig = Rig::desktop().await;
    let text = "The rate review cycle is quarterly.";
    let p1 = KnowledgePageId::new();
    let p2 = KnowledgePageId::new();
    let p3 = KnowledgePageId::new();
    for (page, tier) in [
        (p1, PageTier::Unverified),
        (p2, PageTier::HumanReviewed),
        (p3, PageTier::MachineConfirmed),
    ] {
        rig.page(
            rig.w1,
            page,
            &[("", text)],
            "1",
            standing(tier, PageStatus::Stable, false),
        )
        .await;
    }
    let result = rig.recall_text(rig.w1, "rate review cycle").await;
    let pages: Vec<_> = result.facts.iter().filter_map(|f| f.page_id).collect();
    assert_eq!(pages[..3], [p2, p3, p1]);
    rig.stop().await;
}

#[tokio::test]
async fn edge_deprecated_only_when_nothing_else_matches() {
    let rig = Rig::desktop().await;
    let d = KnowledgePageId::new();
    let s = KnowledgePageId::new();
    rig.page(
        rig.w1,
        d,
        &[("", "The packaging supplier used to be Kestrel.")],
        "1",
        standing(PageTier::Unverified, PageStatus::Deprecated, false),
    )
    .await;
    rig.page(
        rig.w1,
        s,
        &[("", "The packaging supplier is Borealis.")],
        "1",
        standing(PageTier::Unverified, PageStatus::Stable, false),
    )
    .await;
    let result = rig.recall_text(rig.w1, "packaging supplier").await;
    assert!(result.facts.iter().all(|f| f.page_id != Some(d)));
    rig.svc.remove_page(rig.w1, None, s).await.expect("remove");
    rig.settle().await;
    let result = rig.recall_text(rig.w1, "packaging supplier").await;
    assert_eq!(result.facts[0].page_id, Some(d));
    assert_eq!(result.facts[0].page_status, Some(PageStatus::Deprecated));
    rig.stop().await;
}

#[tokio::test]
async fn edge_draft_and_stale_pages_are_marked() {
    let rig = Rig::desktop().await;
    let r = KnowledgePageId::new();
    let qq = KnowledgePageId::new();
    rig.page(
        rig.w1,
        r,
        &[("", "Freight insurance draft rule.")],
        "1",
        standing(PageTier::Unverified, PageStatus::Draft, false),
    )
    .await;
    rig.page(
        rig.w1,
        qq,
        &[("", "Freight insurance old rule.")],
        "1",
        standing(PageTier::Unverified, PageStatus::Stable, true),
    )
    .await;
    let result = rig.recall_text(rig.w1, "freight insurance").await;
    let of = |page| {
        result
            .facts
            .iter()
            .find(|f| f.page_id == Some(page))
            .expect("the fact")
    };
    assert_eq!(of(r).page_status, Some(PageStatus::Draft));
    assert!(of(qq).may_be_out_of_date);
    rig.stop().await;
}

fn client(rig: &Rig, connections: &[&str]) -> Arc<FakeClient> {
    let client = Arc::new(FakeClient::default());
    *client.connections.lock().unwrap() = connections.iter().map(|c| (*c).to_owned()).collect();
    rig.svc.set_fake_client(client.clone());
    client
}

#[tokio::test]
async fn happy_path_a_confident_local_answer_sends_nothing() {
    let rig = Rig::desktop().await;
    let client = client(&rig, &["Buying knowledge"]);
    *client.names.lock().unwrap() = vec!["Borealis Ltd".to_owned()];
    for n in 0..5 {
        let write = RememberInput {
            entities: vec![thing("Borealis Ltd")],
            ..input(&format!("Borealis delivery terms number {n}."), None)
        };
        rig.svc
            .remember(rig.ordin(rig.w1), write)
            .await
            .expect("remember");
    }
    rig.settle().await;
    let result = rig.recall_text(rig.w1, "Borealis delivery terms").await;
    assert!(client.asks.lock().unwrap().is_empty());
    assert!(!result.wider_status.asked);
    assert_eq!(result.wider_status.reason, WiderReason::Confident);
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_a_name_the_workspace_does_not_know_goes_wider() {
    let rig = Rig::desktop().await;
    let client = client(&rig, &["Buying knowledge"]);
    *client.names.lock().unwrap() = vec!["Kestrel Freight".to_owned()];
    for n in 0..4 {
        rig.fact(rig.w1, &format!("Invoices are paid on day {n}."))
            .await;
    }
    let question = "who pays Kestrel Freight invoices?";
    rig.recall_text(rig.w1, question).await;
    {
        let asks = client.asks.lock().unwrap();
        assert_eq!(asks.len(), 1);
        assert!(asks[0].workspace);
        assert_eq!(asks[0].query.as_deref(), Some(question));
        assert_eq!(asks[0].searches, 0);
        assert_eq!(asks[0].limit, 10);
    }
    rig.stop().await;
}

fn always(text: &str) -> RecallInput {
    RecallInput {
        wider: WiderMode::Always,
        ..query(text)
    }
}

#[tokio::test]
async fn happy_path_outside_facts_are_merged_and_recorded_by_source() {
    let rig = Rig::desktop().await;
    let client = client(&rig, &["Buying knowledge"]);
    rig.fact(rig.w1, "Kestrel invoices go to accounts.").await;
    let a1 = KnowledgeAnswerId::new();
    let mut first = hit("Kestrel is paid monthly.", "9fceb02");
    first.answer_id = Some(a1);
    let mut second = hit("Kestrel is paid in euros.", "9fceb02");
    second.answer_id = Some(a1);
    *client.first.lock().unwrap() = Some(batch(vec![first, second], true));
    let result = rig
        .svc
        .recall(rig.ordin(rig.w1), always("Kestrel invoices"))
        .await
        .expect("recall");
    assert_eq!(result.wider.len(), 2);
    // Equal fused scores: page before document, so the outside document leads the plain local fact.
    assert_eq!(
        result.order,
        vec![
            RecallSlot::Wider(0),
            RecallSlot::Fact(0),
            RecallSlot::Wider(1)
        ]
    );
    let used = rig
        .svc
        .memory_used(MemoryPlace::Workspace(rig.w1), vec![result.recall_id])
        .await
        .expect("used");
    assert_eq!(used.wider.len(), 1, "the two share a source");
    assert_eq!(used.wider[0].agent, "Buying knowledge");
    assert_eq!(used.wider[0].repository, "acme/buying");
    assert_eq!(used.wider[0].path, "suppliers/kestrel.md");
    assert_eq!(used.wider[0].commit, "9fceb02");
    assert_eq!(used.wider[0].answer_id, Some(a1));
    let entries = rig.entries("memory_wider_answer_ids").await;
    assert_eq!(entries[0].data["answer_ids"], json!([a1.to_string()]));
    assert!(!entries[0].data.to_string().contains("paid"));
    assert_eq!(rig.entries("memory_wider_asked").await.len(), 1);
    rig.stop().await;
}

/// The events appended so far of one kind.
async fn events(rig: &Rig, kind: &str) -> Vec<JournalEvent> {
    rig.journal
        .events_since(0, 1000)
        .await
        .expect("events")
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

#[tokio::test]
async fn edge_a_slow_front_door_is_named_and_more_found_follows() {
    let rig = Rig::desktop().await;
    let client = client(&rig, &["Buying knowledge", "Legal knowledge"]);
    let mut first = batch(vec![hit("Buying says net thirty.", "9fceb02")], false);
    first.unanswered = vec![UnansweredSource {
        agent: "Legal knowledge".to_owned(),
        reason: UnansweredReason::NotInTime,
    }];
    *client.first.lock().unwrap() = Some(first);
    let mut late = batch(vec![hit("Legal says net sixty.", "1a2b3c4")], true);
    late.answered = vec!["Legal knowledge".to_owned()];
    *client.later.lock().unwrap() = vec![(Duration::from_millis(30), late)];
    let result = rig
        .svc
        .recall(rig.ordin(rig.w1), always("payment terms"))
        .await
        .expect("recall");
    assert!(result.wider_status.more_found_pending);
    assert_eq!(result.wider_status.unanswered[0].agent, "Legal knowledge");

    let started = std::time::Instant::now();
    loop {
        let view = rig
            .svc
            .more_found(rig.w1, result.recall_id)
            .await
            .expect("more found");
        if view.done && !view.wider.is_empty() {
            assert_eq!(view.wider[0].statement, "Legal says net sixty.");
            assert!(!view.expired);
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let appended = events(&rig, "more_found").await;
    assert_eq!(appended.len(), 1);
    assert_eq!(appended[0].data["added"], 1);
    assert_eq!(
        code_of(rig.svc.more_found(rig.w2, result.recall_id).await),
        ("MEM_RECALL_IDS_INVALID", false)
    );
    rig.stop().await;
}

#[tokio::test]
async fn error_a_failed_front_door_never_fails_the_recall() {
    let rig = Rig::desktop().await;
    let client = client(&rig, &["Buying knowledge"]);
    *client.fail.lock().unwrap() = true;
    rig.fact(rig.w1, "Kestrel invoices go to accounts.").await;
    let result = rig
        .svc
        .recall(rig.ordin(rig.w1), always("Kestrel invoices"))
        .await
        .expect("the local answer");
    assert_eq!(result.facts.len(), 1);
    assert_eq!(result.wider_status.reason, WiderReason::Failed);
    assert_eq!(result.wider_status.unanswered[0].agent, "Buying knowledge");
    rig.stop().await;
}

#[tokio::test]
async fn edge_a_recall_restricted_to_folders_stays_local() {
    let rig = Rig::desktop().await;
    let client = client(&rig, &["Buying knowledge"]);
    rig.svc.set_folders(Arc::new(FakeFolders::default()));
    let mut input = query("anything");
    input.folders = Some(vec!["own".to_owned()]);
    let result = rig
        .svc
        .recall(rig.ordin(rig.w1), input)
        .await
        .expect("recall");
    assert!(client.asks.lock().unwrap().is_empty());
    assert_eq!(result.wider_status.reason, WiderReason::NotAsked);
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_a_desktop_glossary_resolves_another_name() {
    let rig = Rig::desktop().await;
    let folder = ordin_types::TeamFolderId::new();
    rig.svc
        .index_read_page_sections(
            rig.w1,
            folder,
            KnowledgePageId::new(),
            sections(&[("Borealis Ltd", "Borealis Ltd is the packaging supplier.")]),
            "1".to_owned(),
            PageGraph {
                other_names: vec!["Borealis".to_owned(), "BRL".to_owned()],
                ..graph("Borealis Ltd")
            },
        )
        .await
        .expect("the concept");
    rig.settle().await;
    let write = RememberInput {
        entities: vec![thing("Borealis Ltd")],
        ..input("Delivery terms take five days.", None)
    };
    let receipt = rig
        .svc
        .remember(rig.ordin(rig.w1), write)
        .await
        .expect("remember");
    rig.kept(receipt.id).await;
    let result = rig.recall_text(rig.w1, "BRL delivery terms").await;
    let at = result
        .facts
        .iter()
        .position(|f| f.id == receipt.id)
        .expect("the fact, through the other name");
    assert!(at < 5);
    rig.stop().await;
}

#[tokio::test]
async fn error_wider_from_a_hosted_agent() {
    let rig = Rig::agent_profile().await;
    let error = rig
        .call(
            "recall",
            ToolCaller::HostedAgent,
            rig.w1,
            AgentInstanceId::new(),
            Some(ScopeId::new()),
            json!({ "query": "anything", "wider": true }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_WIDER_NOT_SUPPORTED");
    // The agent profile does not offer the property.
    let tools = rig.provider().servers();
    let recall = &tools[0].tools[0];
    assert!(recall.input_schema["properties"].get("wider").is_none());
    rig.stop().await;
}

#[tokio::test]
async fn error_a_knowledge_agent_writes_only_whole_documents() {
    let rig = Rig::agent_profile().await;
    let who = MemoryCaller::KnowledgeAgent {
        scope_id: rig.scope_k,
    };
    assert_eq!(
        code_of(rig.svc.remember(who, input("Direct write.", None)).await),
        ("MEM_TRUST_NOT_ALLOWED", false)
    );
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_fact_refs")
            .await,
        0
    );
    rig.stop().await;
}

fn document(prefix: &str, version: &str, statements: &[&str]) -> KnowledgeDocumentFacts {
    KnowledgeDocumentFacts {
        origin_prefix: prefix.to_owned(),
        version: version.to_owned(),
        trust_permille: 500,
        standing: DocumentStanding {
            kind: SourceKind::Document,
            tier: None,
            status: None,
            stale_after: None,
            page_title: None,
        },
        facts: statements
            .iter()
            .map(|statement| KnowledgeFactInput {
                statement: (*statement).to_owned(),
                about: None,
                entities: Vec::new(),
                links: Vec::new(),
            })
            .collect(),
    }
}

#[tokio::test]
async fn happy_path_a_glossary_concept_is_written_at_the_highest_trust() {
    let rig = Rig::agent_profile().await;
    let doc = KnowledgeDocumentFacts {
        trust_permille: 1000,
        standing: DocumentStanding {
            kind: SourceKind::Glossary,
            tier: Some(PageTier::Unverified),
            status: None,
            stale_after: None,
            page_title: Some("Borealis Ltd".to_owned()),
        },
        ..document(
            "git:acme/glossary/borealis.md",
            "a1b2c3d",
            &["Borealis Ltd supplies packaging."],
        )
    };
    let receipt = rig
        .svc
        .index_knowledge_document(rig.scope_k, doc.clone())
        .await
        .expect("index");
    assert_eq!(
        receipt,
        KnowledgeIndexReceipt {
            written: 1,
            retired: 0
        }
    );
    rig.settle().await;
    let row: (f64,) = sqlx::query_as("SELECT trust_signal FROM mem_source")
        .fetch_one(rig.pool())
        .await
        .expect("the source");
    assert_eq!(row.0, 1.0);
    let again = rig
        .svc
        .index_knowledge_document(rig.scope_k, doc)
        .await
        .expect("again");
    assert_eq!(
        again,
        KnowledgeIndexReceipt {
            written: 0,
            retired: 0
        }
    );
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_a_knowledge_agents_scope_stays_apart() {
    let rig = Rig::agent_profile().await;
    let channel = ScopeId::new();
    let receipt = rig
        .svc
        .remember(
            rig.hosted(channel),
            input("the Q3 budget is 2 million", None),
        )
        .await
        .expect("remember");
    rig.kept(receipt.id).await;
    let wide = rig
        .svc
        .remember(
            rig.hosted(rig.scope_a),
            input("the Q3 budget is agreed", None),
        )
        .await
        .expect("agent-wide");
    rig.kept(wide.id).await;
    let result = rig
        .svc
        .recall(
            MemoryCaller::KnowledgeAgent {
                scope_id: rig.scope_k,
            },
            RecallInput {
                wider: WiderMode::Never,
                ..query("Q3 budget")
            },
        )
        .await
        .expect("recall");
    assert!(result.facts.is_empty());
    assert!(result.recall_id.0.is_nil());
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_a_changed_documents_old_facts_are_retired() {
    let rig = Rig::agent_profile().await;
    let k = rig.scope_k;
    let prefix = "git:acme/buying/suppliers/kestrel.txt";
    let old = [
        "Old term one.",
        "Old term two.",
        "Old term three.",
        "Old term four.",
    ];
    let first = rig
        .svc
        .index_knowledge_document(k, document(prefix, "a1b2c3d", &old))
        .await
        .expect("first");
    assert_eq!(first.written, 4);
    rig.settle().await;
    let new = ["New term one.", "New term two.", "New term three."];
    let second = rig
        .svc
        .index_knowledge_document(k, document(prefix, "9fceb02", &new))
        .await
        .expect("second");
    assert_eq!(
        second,
        KnowledgeIndexReceipt {
            written: 3,
            retired: 4
        }
    );
    rig.settle().await;
    let who = MemoryCaller::KnowledgeAgent { scope_id: k };
    let result = rig.svc.recall(who, query("term")).await.expect("recall");
    assert_eq!(result.facts.len(), 3);
    assert!(statements(&result).iter().all(|s| s.starts_with("New")));
    let removed = rig
        .svc
        .remove_knowledge_document(k, &format!("{prefix}#"))
        .await
        .expect("remove");
    assert_eq!(removed, 3);
    rig.settle().await;
    let result = rig.svc.recall(who, query("term")).await.expect("recall");
    assert!(result.facts.is_empty());
    rig.stop().await;
}

fn fact_with(
    statement: &str,
    things: Vec<ThingInput>,
    links: Vec<LinkInput>,
) -> KnowledgeFactInput {
    KnowledgeFactInput {
        statement: statement.to_owned(),
        about: None,
        entities: things,
        links,
    }
}

#[tokio::test]
async fn happy_path_the_most_connected_things() {
    let rig = Rig::agent_profile().await;
    let mut facts = Vec::new();
    for (name, links) in [("Alpha", 12), ("Bravo", 3), ("Charlie", 7)] {
        for n in 0..links {
            let peer = format!("{name} peer {n}");
            facts.push(fact_with(
                &format!("{name} works with {peer}."),
                vec![thing(name), thing(&peer)],
                vec![link(name, "works with", &peer)],
            ));
        }
    }
    let doc = KnowledgeDocumentFacts {
        facts,
        ..document("git:acme/org/people.md", "a1b2c3d", &[])
    };
    rig.svc
        .index_knowledge_document(rig.scope_k, doc)
        .await
        .expect("index");
    rig.settle().await;
    let top = rig
        .svc
        .top_things(MemoryPlace::Scope(rig.scope_k), 2)
        .await
        .expect("top");
    let names: Vec<&str> = top.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["alpha", "charlie"]);
    assert_eq!((top[0].link_count, top[0].fact_count), (12, 12));
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_a_knowledge_scope_shortlist_larger_than_the_answer() {
    let rig = Rig::agent_profile().await;
    let statements_owned: Vec<String> = (0..80)
        .map(|n| format!("The packaging supplier fact {n}."))
        .collect();
    let refs: Vec<&str> = statements_owned.iter().map(String::as_str).collect();
    // Eighty writes against one SQLite file: on a machine loaded by the whole suite, one of them can
    // wait past the datastore's lock timeout (`MEM_STORE_TIMEOUT`, retryable). Indexing the same
    // version again is safe (each fact is written under its own key), so the test retries as a
    // caller would.
    let mut attempts = 0;
    loop {
        attempts += 1;
        match rig
            .svc
            .index_knowledge_document(
                rig.scope_k,
                document("git:acme/buying/packaging.md", "a1b2c3d", &refs),
            )
            .await
        {
            Ok(_) => break,
            Err(e) if e.code() == "MEM_STORE_TIMEOUT" && attempts < 5 => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => panic!("index: {e:?}"),
        }
    }
    rig.settle().await;
    let result = rig
        .svc
        .recall(
            MemoryCaller::KnowledgeAgent {
                scope_id: rig.scope_k,
            },
            RecallInput {
                result_cap: 30,
                ..query("packaging supplier")
            },
        )
        .await
        .expect("recall");
    assert_eq!(result.facts.len(), 30);
    assert!(result.facts.iter().all(|f| matches!(
        &f.came_from,
        FactOrigin::Document { origin_ref, .. } if origin_ref.starts_with("git:acme/buying/packaging.md#")
    )));
    rig.stop().await;
}

#[tokio::test]
async fn error_the_shortlist_cap() {
    let rig = Rig::agent_profile().await;
    let who = MemoryCaller::KnowledgeAgent {
        scope_id: rig.scope_k,
    };
    let over = RecallInput {
        result_cap: 51,
        ..query("anything")
    };
    assert_eq!(
        code_of(rig.svc.recall(who, over).await),
        ("MEM_RESULT_CAP_INVALID", false)
    );
    let desktop = Rig::desktop().await;
    let over = RecallInput {
        result_cap: 21,
        ..query("anything")
    };
    assert_eq!(
        code_of(desktop.svc.recall(desktop.ordin(desktop.w1), over).await),
        ("MEM_RESULT_CAP_INVALID", false)
    );
    rig.stop().await;
    desktop.stop().await;
}

#[tokio::test]
async fn invariant_no_summary_in_a_knowledge_scope() {
    let rig = Rig::agent_profile().await;
    for n in 0..5 {
        rig.svc
            .index_knowledge_document(
                rig.scope_k,
                document(
                    &format!("git:acme/docs/{n}.md"),
                    "a1b2c3d",
                    &[format!("Supplier document number {n}.").as_str()],
                ),
            )
            .await
            .expect("index");
    }
    rig.settle().await;
    let result = rig
        .svc
        .recall(
            MemoryCaller::KnowledgeAgent {
                scope_id: rig.scope_k,
            },
            RecallInput {
                result_cap: 50,
                ..query("supplier document")
            },
        )
        .await
        .expect("recall");
    assert_eq!(result.facts.len(), 5);
    assert!(result
        .facts
        .iter()
        .all(|f| f.derived_from.is_empty() && f.derived_origins.is_empty()));
    rig.stop().await;
}

/// F1 a document fact, F2 a page fact, S Ordin's summary of them.
async fn summary_of_two(rig: &Rig) -> (MemoryFactId, MemoryFactId, MemoryFactId, KnowledgePageId) {
    let mut doc = input("Terms are net thirty.", None);
    doc.source = Some(FactSource {
        origin_ref: "git:acme/buying/terms.md#1".to_owned(),
        version: "a1b2c3d".to_owned(),
    });
    let f1 = rig
        .svc
        .remember(rig.ordin(rig.w1), doc)
        .await
        .expect("F1")
        .id;
    rig.kept(f1).await;
    let p = KnowledgePageId::new();
    rig.page(
        rig.w1,
        p,
        &[("Intro", "Terms overview."), ("Rates", "Terms rates vary.")],
        "1",
        graph("Terms"),
    )
    .await;
    let f2 = rig
        .svc
        .list_facts(rig.w1, None)
        .await
        .expect("list")
        .items
        .into_iter()
        .find(|f| f.statement == "Terms rates vary.")
        .expect("F2")
        .id;
    let s = rig
        .svc
        .remember(
            rig.ordin(rig.w1),
            RememberInput {
                made_from: vec![f1, f2],
                ..input("Terms summary line.", None)
            },
        )
        .await
        .expect("S")
        .id;
    rig.kept(s).await;
    (f1, f2, s, p)
}

#[tokio::test]
async fn happy_path_a_summary_names_the_origins_it_was_made_from() {
    let rig = Rig::desktop().await;
    let (_, _, s, p) = summary_of_two(&rig).await;
    let result = rig.recall_text(rig.w1, "terms summary").await;
    let fact = result.facts.iter().find(|f| f.id == s).expect("S");
    assert_eq!(fact.derived_from.len(), 2);
    let mut origins = fact.derived_origins.clone();
    origins.sort();
    let mut wanted = vec![
        "git:acme/buying/terms.md#1".to_owned(),
        format!("knowledge-page:{p}#2"),
    ];
    wanted.sort();
    assert_eq!(origins, wanted);
    rig.stop().await;
}

#[tokio::test]
async fn edge_an_origin_with_no_row_leaves_the_lists_different() {
    let rig = Rig::desktop().await;
    let (f1, f2, s, _) = summary_of_two(&rig).await;
    let f3 = rig.fact(rig.w1, "Terms third origin.").await;
    let s3 = rig
        .svc
        .remember(
            rig.ordin(rig.w1),
            RememberInput {
                made_from: vec![f1, f2, f3],
                ..input("Terms wider summary line.", None)
            },
        )
        .await
        .expect("S3")
        .id;
    rig.kept(s3).await;
    let _ = s;
    // F3's row is gone, and the mirror with it.
    rig.execute("DELETE FROM memory_consolidations").await;
    rig.execute("DELETE FROM memory_made_from").await;
    rig.execute(&format!(
        "UPDATE memory_fact_refs SET replaces_id = NULL WHERE replaces_id = '{f3}'"
    ))
    .await;
    rig.execute(&format!("DELETE FROM memory_fact_refs WHERE id = '{f3}'"))
        .await;
    let result = rig.recall_text(rig.w1, "terms wider summary").await;
    let fact = result.facts.iter().find(|f| f.id == s3).expect("S");
    assert_eq!(fact.derived_from.len(), 3);
    assert_eq!(fact.derived_origins.len(), 2);
    rig.stop().await;
}

/// The handbook and contracts documents of scope K.
async fn audience_scope(rig: &Rig) -> ordin_types::ScopeId {
    let k = rig.scope_k;
    let handbook = "git:acme/handbook@main/terms.md";
    let contracts = "git:acme/contracts@main/terms.md";
    let borealis = |n: u32| {
        fact_with(
            &format!("Borealis Ltd clause {n}."),
            vec![thing("Borealis Ltd")],
            Vec::new(),
        )
    };
    rig.svc
        .index_knowledge_document(
            k,
            KnowledgeDocumentFacts {
                facts: vec![borealis(1), borealis(2)],
                ..document(handbook, "a1b2c3d", &[])
            },
        )
        .await
        .expect("handbook");
    let mut facts: Vec<_> = (3..=6).map(borealis).collect();
    facts.push(fact_with(
        "Project Falcon is confidential.",
        vec![thing("Project Falcon")],
        Vec::new(),
    ));
    facts.push(fact_with(
        "Borealis Ltd supplies Packaging.",
        vec![thing("Borealis Ltd"), thing("Packaging")],
        vec![link("Borealis Ltd", "supplies", "Packaging")],
    ));
    rig.svc
        .index_knowledge_document(
            k,
            KnowledgeDocumentFacts {
                facts,
                ..document(contracts, "a1b2c3d", &[])
            },
        )
        .await
        .expect("contracts");
    rig.settle().await;
    k
}

#[tokio::test]
async fn happy_path_things_counted_over_audience_open_facts_only() {
    let rig = Rig::agent_profile().await;
    let k = audience_scope(&rig).await;
    let top = rig
        .svc
        .top_things_within(
            MemoryPlace::Scope(k),
            10,
            vec!["git:acme/handbook@main/".to_owned()],
        )
        .await
        .expect("top");
    let names: Vec<&str> = top.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["borealis ltd"]);
    assert_eq!(top[0].fact_count, 2);
    rig.stop().await;
}

#[tokio::test]
async fn edge_a_link_counts_only_when_a_counted_fact_asserted_it() {
    let rig = Rig::agent_profile().await;
    let k = audience_scope(&rig).await;
    let top = rig
        .svc
        .top_things_within(
            MemoryPlace::Scope(k),
            10,
            vec!["git:acme/handbook@main/".to_owned()],
        )
        .await
        .expect("top");
    assert_eq!(top[0].link_count, 0);
    let all = rig
        .svc
        .top_things_within(MemoryPlace::Scope(k), 10, vec!["git:acme/".to_owned()])
        .await
        .expect("top");
    assert_eq!(all[0].name, "borealis ltd");
    assert_eq!(all[0].link_count, 1);
    rig.stop().await;
}

#[tokio::test]
async fn error_top_things_within_limits() {
    let rig = Rig::agent_profile().await;
    assert_eq!(
        code_of(
            rig.svc
                .top_things_within(MemoryPlace::Scope(rig.scope_k), 10, Vec::new())
                .await
        ),
        ("MEM_LIMIT_INVALID", false)
    );
    let desktop = Rig::desktop().await;
    assert_eq!(
        code_of(
            desktop
                .svc
                .top_things_within(
                    MemoryPlace::Workspace(desktop.w1),
                    10,
                    vec!["git:".to_owned()]
                )
                .await
        ),
        ("MEM_NOT_SUPPORTED", false)
    );
    rig.stop().await;
    desktop.stop().await;
}

#[tokio::test]
async fn happy_path_confluence_and_sharepoint_origins_are_accepted() {
    let rig = Rig::agent_profile().await;
    let k = rig.scope_k;
    let one = ["One fact."];
    assert_eq!(
        rig.svc
            .index_knowledge_document(
                k,
                document("confluence:acme.atlassian.net/pages/101", "12", &one)
            )
            .await
            .expect("confluence"),
        KnowledgeIndexReceipt {
            written: 1,
            retired: 0
        }
    );
    assert_eq!(
        rig.svc
            .index_knowledge_document(
                k,
                document(
                    "sharepoint:b!x1y2/01ABC",
                    "\"c:{3F2C8A10-6B1E-4D2A-9C33-0A7B5E4D1F20},4\"",
                    &["Another fact."]
                )
            )
            .await
            .expect("sharepoint"),
        KnowledgeIndexReceipt {
            written: 1,
            retired: 0
        }
    );
    assert_eq!(
        code_of(
            rig.svc
                .index_knowledge_document(
                    k,
                    document("confluence:acme.atlassian.net/pages/102", "12a", &one)
                )
                .await
        ),
        ("MEM_SOURCE_INVALID", false)
    );
    assert_eq!(
        code_of(
            rig.svc
                .index_knowledge_document(k, document("file:/etc/passwd", "a1b2c3d", &one))
                .await
        ),
        ("MEM_SOURCE_INVALID", false)
    );
    rig.stop().await;
}

#[tokio::test]
async fn desktop_a_confluence_version_of_two_characters_is_recorded_for_what_ordin_used() {
    let rig = Rig::desktop().await;
    let client = client(&rig, &["Buying knowledge"]);
    let mut outside = hit("Confluence says so.", "12");
    outside.repository = "confluence:acme.atlassian.net".to_owned();
    outside.path = "pages/101".to_owned();
    *client.first.lock().unwrap() = Some(batch(vec![outside], true));
    let result = rig
        .svc
        .recall(rig.ordin(rig.w1), always("anything"))
        .await
        .expect("recall");
    let used = rig
        .svc
        .memory_used(MemoryPlace::Workspace(rig.w1), vec![result.recall_id])
        .await
        .expect("used");
    assert_eq!(used.wider[0].commit, "12");
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_unchecked_sources_reach_what_ordin_used_naming_none() {
    let rig = Rig::desktop().await;
    let client = client(&rig, &["Buying knowledge"]);
    let mut first = batch(Vec::new(), true);
    first.unchecked_sources = true;
    *client.first.lock().unwrap() = Some(first);
    let result = rig
        .svc
        .recall(rig.ordin(rig.w1), always("anything"))
        .await
        .expect("recall");
    assert!(result.wider_status.unchecked_sources);
    let used = rig
        .svc
        .memory_used(MemoryPlace::Workspace(rig.w1), vec![result.recall_id])
        .await
        .expect("used");
    assert!(used.wider_unchecked);
    let row = rig
        .svc
        .inner
        .db
        .first(q("SELECT wider FROM memory_recalls"))
        .await
        .expect("read")
        .expect("a row");
    let wider: serde_json::Value =
        serde_json::from_str(&row.text("wider").expect("JSON")).expect("JSON");
    assert_eq!(wider["unchecked"], true);
    assert_eq!(
        wider,
        json!({ "answered": ["Buying knowledge"], "unanswered": [], "unchecked": true })
    );
    rig.stop().await;
}

#[tokio::test]
async fn edge_unchecked_arriving_under_more_found() {
    let rig = Rig::desktop().await;
    let client = client(&rig, &["Buying knowledge", "Legal knowledge"]);
    let mut first = batch(Vec::new(), false);
    first.unanswered = vec![UnansweredSource {
        agent: "Legal knowledge".to_owned(),
        reason: UnansweredReason::NotInTime,
    }];
    *client.first.lock().unwrap() = Some(first);
    let mut late = batch(Vec::new(), true);
    late.unchecked_sources = true;
    *client.later.lock().unwrap() = vec![(Duration::from_millis(10), late)];
    let result = rig
        .svc
        .recall(rig.ordin(rig.w1), always("anything"))
        .await
        .expect("recall");
    assert!(!result.wider_status.unchecked_sources);
    let place = MemoryPlace::Workspace(rig.w1);
    let started = std::time::Instant::now();
    loop {
        let used = rig
            .svc
            .memory_used(place, vec![result.recall_id])
            .await
            .expect("used");
        if used.wider_unchecked {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut appended = events(&rig, "more_found").await;
    while appended.is_empty() {
        assert!(started.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(10)).await;
        appended = events(&rig, "more_found").await;
    }
    assert_eq!(appended.len(), 1);
    let text = appended[0].data.to_string();
    assert!(!text.contains("unchecked"));
    assert!(appended[0].data.get("added").is_some());
    let _ = Timestamp::now();
    rig.stop().await;
}

#[tokio::test]
async fn the_store_port_has_a_source_for_each_document_fact() {
    let rig = Rig::agent_profile().await;
    rig.svc
        .index_knowledge_document(rig.scope_k, document("git:a/b.md", "a1b2c3d", &["A fact."]))
        .await
        .expect("index");
    rig.settle().await;
    let row = crate::store::rows_with_prefix(
        &rig.svc.inner.db,
        &format!("scope:{}", rig.scope_k),
        "git:a/b.md#",
        10,
    )
    .await
    .expect("rows");
    assert_eq!(row[0].state, FactState::Kept);
    let context = rig.svc.inner.maintenance_context();
    let fact = rig
        .svc
        .inner
        .storage
        .get_fact(&context, &row[0].memory_fact_id)
        .await
        .expect("read")
        .expect("the fact");
    assert!(fact.source_id.is_some());
    rig.stop().await;
}
