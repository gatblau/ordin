//! The scenarios of *Acceptance Criteria (Gherkin)* (feature "Memory Service") and of the ADR-104
//! amendment, each a test of the same name.

use std::time::Duration;

use chrono::Utc;
use ordin_mcp_front::ToolCaller;
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, KnowledgePageId, MemoryFactId, RunId, ScopeId, WorkspaceId};
use serde_json::json;

use super::support::*;
use crate::config::ProgramKind;
use crate::db::q;
use crate::model::*;
use crate::MemoryService;

#[tokio::test]
async fn happy_path_a_crew_member_remembers_and_ordin_recalls_in_the_same_workspace() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    let output = rig
        .crew_call(
            "remember",
            rig.w1,
            agent,
            json!({ "statement": "Kestrel's Q3 quote is valid until 30 November.", "about": "Kestrel" }),
        )
        .await
        .expect("remember");
    let structured = output.structured.expect("structured");
    assert_eq!(structured["state"], "pending");
    ordin_types::MemoryFactId::parse(structured["id"].as_str().expect("id")).expect("a UUIDv7");
    let started = std::time::Instant::now();
    let id = rig.keep_held_writes(rig.w1, 1).await[0];
    rig.kept(id).await;
    assert!(started.elapsed() <= Duration::from_secs(2));

    let result = rig
        .recall_text(rig.w1, "When does the Kestrel quote expire?")
        .await;
    let first = &result.facts[0];
    assert_eq!(
        said(first),
        "Kestrel's Q3 quote is valid until 30 November."
    );
    assert!(matches!(first.came_from, FactOrigin::Agent { .. }));
    assert_eq!(first.workspace_id, Some(rig.w1));
    rig.stop().await;
}

#[tokio::test]
async fn edge_a_fact_from_one_workspace_is_not_recalled_in_another() {
    let rig = Rig::desktop().await;
    rig.fact(rig.w1, "Kestrel's Q3 quote is valid until 30 November.")
        .await;
    let result = rig.recall_text(rig.w2, "Kestrel quote").await;
    assert!(!statements(&result)
        .iter()
        .any(|s| s.contains("Kestrel's Q3")));
    rig.stop().await;
}

#[tokio::test]
async fn edge_remember_everywhere_widens_a_fact_to_every_workspace() {
    let rig = Rig::desktop().await;
    let f = rig.fact(rig.w1, "Prefers pricing tables in euros").await;
    let view = rig.svc.remember_everywhere(f).await.expect("widen");
    rig.kept(view.id).await;
    rig.wait_state(f, FactState::Replaced).await;
    let result = rig.recall_text(rig.w2, "pricing tables").await;
    assert_eq!(
        statements(&result),
        vec!["Prefers pricing tables in euros".to_owned()]
    );
    assert_eq!(result.facts[0].workspace_id, None);
    rig.stop().await;
}

#[tokio::test]
async fn edge_the_same_statement_twice_in_one_run_is_one_fact() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    let run = RunId::new();
    let caller = ToolCaller::CrewMember {
        crew_run_id: ordin_types::CrewRunId::new(),
    };
    let args = json!({ "statement": "Board meets on the first Tuesday." });
    let first = rig
        .call_in_run("remember", caller, run, rig.w1, agent, None, args.clone())
        .await
        .expect("first");
    let second = rig
        .call_in_run("remember", caller, run, rig.w1, agent, None, args)
        .await
        .expect("second");
    assert_eq!(
        first.structured.expect("first")["id"],
        second.structured.expect("second")["id"]
    );
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_fact_refs")
            .await,
        1
    );
    rig.stop().await;
}

async fn ruled_agent(rig: &Rig) -> AgentInstanceId {
    let agent = AgentInstanceId::new();
    rig.svc
        .set_agent_rules(
            agent,
            rules(&["decisions", "conventions"], &["contact-details"], None),
        )
        .expect("rules");
    agent
}

#[tokio::test]
async fn error_an_agents_memory_rules_exclude_a_class() {
    let rig = Rig::desktop().await;
    let agent = ruled_agent(&rig).await;
    let error = rig
        .crew_call(
            "remember",
            rig.w1,
            agent,
            json!({ "statement": "Call Ann on 01.", "fact_class": "contact-details" }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_CLASS_EXCLUDED");
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_fact_refs")
            .await,
        0
    );
    rig.stop().await;
}

#[tokio::test]
async fn error_an_agents_memory_rules_require_a_retained_class() {
    let rig = Rig::desktop().await;
    let agent = ruled_agent(&rig).await;
    let error = rig
        .crew_call(
            "remember",
            rig.w1,
            agent,
            json!({ "statement": "Nothing classed." }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_CLASS_REQUIRED");
    let error = rig
        .crew_call(
            "remember",
            rig.w1,
            agent,
            json!({ "statement": "Nothing classed.", "fact_class": "gossip" }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_CLASS_NOT_RETAINED");
    rig.stop().await;
}

#[tokio::test]
async fn edge_retention_removes_facts_older_than_the_agents_limit() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    rig.svc
        .set_agent_rules(agent, rules(&[], &[], Some(90)))
        .expect("rules");
    let output = rig
        .crew_call(
            "remember",
            rig.w1,
            agent,
            json!({ "statement": "Quotes are kept for a quarter." }),
        )
        .await
        .expect("remember");
    let id = ordin_types::MemoryFactId::parse(
        output.structured.expect("structured")["id"]
            .as_str()
            .expect("id"),
    )
    .expect("id");
    let id = {
        assert!(rig.row(id).await.retain_until.is_some());
        rig.keep_held_writes(rig.w1, 1).await[0]
    };
    assert!(rig.row(id).await.retain_until.is_some());
    // Written 91 days ago.
    let past = Timestamp(Utc::now() - chrono::Duration::days(1));
    rig.svc
        .inner
        .db
        .exec(
            q("UPDATE memory_fact_refs SET retain_until = $1 WHERE id = $2")
                .ts(past)
                .uuid(id.0),
        )
        .await
        .expect("age the row");
    rig.svc.inner.sweep_once().await;
    assert_eq!(rig.state(id).await, FactState::Forgotten);
    let entries = rig.entries("memory_fact_expired").await;
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].data["proof_digest"]
            .as_str()
            .expect("digest")
            .len(),
        64
    );
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_the_person_forgets_a_fact() {
    let rig = Rig::desktop().await;
    let f = rig.fact(rig.w1, "Kestrel's rate is confidential.").await;
    let receipt = rig.svc.forget_fact(f).await.expect("forget");
    assert_eq!(receipt.proof_digest.len(), 64);
    assert_eq!(receipt.id, f);
    assert_eq!(receipt.summaries_removed, 0);
    let result = rig.recall_text(rig.w1, "Kestrel rate").await;
    assert!(result.facts.is_empty());
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_the_person_corrects_a_fact() {
    let rig = Rig::desktop().await;
    let f = rig.fact(rig.w1, "Prefers pricing tables in dollars").await;
    let view = rig
        .svc
        .correct_fact(
            f,
            crate::model::ObjectInput::Text("Prefers pricing tables in euros".to_owned()),
        )
        .await
        .expect("correct");
    assert_eq!(view.replaces, Some(f));
    rig.kept(view.id).await;
    rig.wait_state(f, FactState::Replaced).await;
    let result = rig.recall_text(rig.w1, "pricing tables").await;
    assert_eq!(
        statements(&result),
        vec!["Prefers pricing tables in euros".to_owned()]
    );
    rig.stop().await;
}

#[tokio::test]
async fn edge_a_cited_document_changes() {
    let rig = Rig::desktop().await;
    let origin = "SharePoint — Procurement/Kestrel Q3 quote.pdf";
    let mut fact = input("Kestrel's quote covers packaging.", None);
    fact.source = Some(FactSource {
        origin_ref: origin.to_owned(),
        version: "7".to_owned(),
    });
    let receipt = rig
        .svc
        .remember_text(rig.ordin(rig.w1), fact)
        .await
        .expect("remember");
    // A document's trust is 0.5, so the fact passes the gate at the 0.8 the write pipeline scores.
    rig.kept(receipt.id).await;
    assert_eq!(
        rig.svc.source_changed(origin, "8").await.expect("changed"),
        1
    );
    assert_eq!(rig.svc.source_changed(origin, "7").await.expect("same"), 0);
    let page = rig.svc.list_facts(rig.w1, None).await.expect("list");
    assert!(page.items[0].may_be_out_of_date);
    assert!(matches!(
        page.items[0].came_from,
        FactOrigin::Document { .. }
    ));
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_a_knowledge_page_is_indexed_by_section() {
    let rig = Rig::desktop().await;
    let p = KnowledgePageId::new();
    rig.page(
        rig.w1,
        p,
        &[
            ("One", "Invoices are paid monthly."),
            ("Two", "Rates are reviewed each quarter."),
            ("Three", "Exceptions need a director."),
            ("Four", "Suppliers sign the code of conduct."),
        ],
        "3",
        graph("Supplier policy"),
    )
    .await;
    assert_eq!(
        rig.count(
            "SELECT COUNT(*) AS n FROM memory_fact_refs WHERE origin_kind = 'knowledge_page' \
             AND state = 'kept'"
        )
        .await,
        4
    );
    let result = rig.recall_text(rig.w1, "rates reviewed quarter").await;
    assert_eq!(result.facts[0].page_id, Some(p));

    rig.page(
        rig.w1,
        p,
        &[
            ("One", "Invoices are paid fortnightly."),
            ("Two", "Rates are reviewed each year."),
            ("Three", "Exceptions need a manager."),
        ],
        "4",
        graph("Supplier policy"),
    )
    .await;
    assert_eq!(
        rig.count(
            "SELECT COUNT(*) AS n FROM memory_fact_refs WHERE origin_version = '3' \
             AND state = 'replaced'"
        )
        .await,
        4
    );
    assert_eq!(
        rig.count(
            "SELECT COUNT(*) AS n FROM memory_fact_refs WHERE origin_version = '4' \
             AND state = 'kept'"
        )
        .await,
        3
    );
    rig.svc.remove_page(rig.w1, None, p).await.expect("remove");
    rig.svc
        .remove_page(rig.w1, None, p)
        .await
        .expect("idempotent");
    assert_eq!(rig.entries("memory_page_removed").await.len(), 2);
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_fact_refs WHERE state = 'kept'")
            .await,
        0
    );
    rig.stop().await;
}

#[tokio::test]
async fn agent_profile_a_direct_chats_facts_stay_in_its_scope() {
    let rig = Rig::agent_profile().await;
    let c = ScopeId::new();
    let d = ScopeId::new();
    let receipt = rig
        .svc
        .remember_text(rig.hosted(d), input("Prefers replies in Spanish", None))
        .await
        .expect("remember");
    rig.kept(receipt.id).await;
    let in_c = rig
        .svc
        .recall(rig.hosted(c), query("replies Spanish"))
        .await
        .expect("recall");
    assert!(in_c.facts.is_empty());
    let in_d = rig
        .svc
        .recall(rig.hosted(d), query("replies Spanish"))
        .await
        .expect("recall");
    assert_eq!(
        statements(&in_d),
        vec!["Prefers replies in Spanish".to_owned()]
    );
    rig.stop().await;
}

#[tokio::test]
async fn error_agent_profile_write_without_a_scope() {
    let rig = Rig::agent_profile().await;
    let error = rig
        .call(
            "remember",
            ToolCaller::HostedAgent,
            rig.w1,
            AgentInstanceId::new(),
            None,
            json!({ "statement": "No conversation here." }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_SCOPE_MISSING");
    assert!(!error.retryable);
    rig.stop().await;
}

#[tokio::test]
async fn agent_profile_a_dry_run_counts_a_scopes_facts_without_removing_them() {
    let rig = Rig::agent_profile().await;
    let d = ScopeId::new();
    let who = rig.hosted(d);
    let kept = rig
        .svc
        .remember_text(who, input("First fact in the chat.", None))
        .await
        .expect("kept");
    rig.kept(kept.id).await;
    let retired = rig
        .svc
        .remember_text(who, input("Second fact in the chat.", None))
        .await
        .expect("retired");
    rig.kept(retired.id).await;
    rig.svc
        .inner
        .db
        .exec(q("UPDATE memory_fact_refs SET state = 'retired' WHERE id = $1").uuid(retired.id.0))
        .await
        .expect("retire");
    // Pending: the row is written, and its fact never arrives under this id.
    rig.svc
        .inner
        .db
        .exec(
            q(
                "INSERT INTO memory_fact_refs (id, memory_fact_id, scope_key, scope_id, \
               origin_kind, job_id, state, written_at) \
               VALUES ($1, $2, $3, $4, 'conversation', 'job', 'pending', $5)",
            )
            .uuid(uuid::Uuid::now_v7())
            .text(format!("fact:{}", uuid::Uuid::now_v7()))
            .text(format!("scope:{d}"))
            .uuid(d.0)
            .ts(Timestamp::now()),
        )
        .await
        .expect("a pending row");
    assert_eq!(rig.svc.count_scope(d).await.expect("count"), 3);
    let receipt = rig.svc.erase_scope(d).await.expect("erase");
    assert!(receipt.rows_removed >= 3);
    assert_eq!(receipt.digest.len(), 64);
    assert_eq!(rig.svc.count_scope(d).await.expect("count"), 0);
    rig.stop().await;
}

#[tokio::test]
async fn error_count_scope_on_the_desktop() {
    let rig = Rig::desktop().await;
    assert_eq!(
        code_of(rig.svc.count_scope(ScopeId::new()).await),
        ("MEM_NOT_SUPPORTED", false)
    );
    rig.stop().await;
}

/// The journal's SQLite file held by another writer past the busy timeout (5 s).
#[tokio::test]
async fn error_the_store_is_unreachable() {
    let rig = Rig::desktop().await;
    rig.fact(rig.w1, "Kestrel delivers on Fridays.").await;
    let file = rig.dir.0.join("data").join("journal.db");
    let other = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&file))
        .await
        .expect("a second connection");
    let mut lock = other.begin_with("BEGIN IMMEDIATE").await.expect("the lock");
    sqlx::query("UPDATE memory_fact_refs SET job_id = 'held'")
        .execute(&mut *lock)
        .await
        .expect("hold the write lock");
    let result = rig.svc.recall(rig.ordin(rig.w1), query("Kestrel")).await;
    assert_eq!(code_of(result), ("MEM_STORE_TIMEOUT", true));
    lock.rollback().await.expect("release");
    rig.stop().await;
}

/// A subscriber that keeps each event's message and `file` field.
#[derive(Clone, Default)]
pub(crate) struct Captured(pub(crate) std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap()
            .push(String::from_utf8_lossy(buf).into_owned());
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Captured {
        self.clone()
    }
}

#[tokio::test]
async fn error_models_missing() {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish();
    let _default = tracing::subscriber::set_default(subscriber);

    let dir = TempDir::new();
    let (_store, journal) = open(&dir).await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    // The real `start`, with the model folder empty.
    let svc = MemoryService::start(
        config(ProgramKind::Desktop, None, None),
        journal,
        shutdown.clone(),
    )
    .await
    .expect("the service still starts");
    let lines = captured.0.lock().unwrap().join("\n");
    // MEM-MD-6: an empty model folder is "not downloaded yet", one info line, not an error per file.
    assert!(lines.contains("memory.models_not_downloaded"), "{lines}");
    assert!(!lines.contains("memory.model_missing"), "{lines}");

    let provider = svc.tool_provider();
    let agent = AgentInstanceId::new();
    let run = RunId::new();
    let w = ordin_types::WorkspaceId::new();
    let error = provider
        .call(
            context(
                ToolCaller::CrewMember {
                    crew_run_id: ordin_types::CrewRunId::new(),
                },
                run,
                w,
                agent,
                None,
            ),
            &key("recall"),
            json!({ "query": "Kestrel" }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_MODEL_UNAVAILABLE");
    assert!(!error.retryable);
    assert_eq!(
        error.message,
        "Memory is not available on this computer right now."
    );
    shutdown.cancel();
}

#[tokio::test]
async fn error_rate_limit() {
    // At the real 600 a minute; the bucket's burst is what a burst of calls meets first.
    let rig = Rig::with(Options {
        rate: 600,
        ..Options::default()
    })
    .await;
    let agent = AgentInstanceId::new();
    let mut refused = None;
    for n in 0..100 {
        let answer = rig
            .crew_call(
                "remember",
                rig.w1,
                agent,
                json!({ "statement": format!("Fact number {n} of the burst.") }),
            )
            .await;
        if let Err(error) = answer {
            refused = Some(error);
            break;
        }
    }
    let error = refused.expect("the bucket ran out");
    assert_eq!(error.code, "MEM_RATE_LIMITED");
    assert!(error.retryable);
    rig.stop().await;
}

#[tokio::test]
async fn error_forget_cannot_be_verified() {
    let rig = Rig::desktop().await;
    let f = rig.fact(rig.w1, "Kestrel's bank details changed.").await;
    // The store's delete leaves the row, so its verification finds a row left.
    rig.execute(
        "CREATE TRIGGER keep_fact BEFORE DELETE ON mem_fact BEGIN SELECT RAISE(IGNORE); END;",
    )
    .await;
    assert_eq!(
        code_of(rig.svc.forget_fact(f).await),
        ("MEM_DELETE_UNVERIFIED", false)
    );
    assert_eq!(rig.state(f).await, FactState::Kept);
    rig.stop().await;
}

/// `cargo tree` reads this lock file: SurrealDB is not in the workspace's build. The memory
/// component's suite on `SqlStorage` is the memory crate's own gate (reported with this step).
#[test]
fn build_no_surrealdb_in_ordins_build() {
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"))
        .expect("the lock file");
    assert!(lock.contains("name = \"memory\""));
    assert!(!lock.to_lowercase().contains("name = \"surrealdb"));
}

#[tokio::test]
async fn happy_path_a_recalled_page_fact_names_its_section() {
    let rig = Rig::desktop().await;
    let p = KnowledgePageId::new();
    rig.page(
        rig.w1,
        p,
        &[
            ("Review cycle", "Rates are reviewed each quarter."),
            ("Exceptions", "Kestrel has a fixed rate until March."),
        ],
        "1",
        graph("Rates"),
    )
    .await;
    let result = rig
        .recall_text(rig.w1, "how often are rates reviewed")
        .await;
    let first = &result.facts[0];
    assert_eq!(first.page_id, Some(p));
    assert_eq!(first.page_section.as_deref(), Some("Review cycle"));
    rig.stop().await;
}

#[tokio::test]
async fn edge_a_fact_not_from_a_page_has_no_section() {
    let rig = Rig::desktop().await;
    rig.fact(rig.w1, "Ordin said this in a conversation.").await;
    let result = rig.recall_text(rig.w1, "conversation").await;
    let value = serde_json::to_value(&result.facts[0]).expect("JSON");
    assert!(value.get("page_id").is_none());
    assert!(value.get("page_section").is_none());
    rig.stop().await;
}

#[tokio::test]
async fn edge_the_sections_heading_changed_after_indexing() {
    let rig = Rig::desktop().await;
    let p = KnowledgePageId::new();
    let text = "Rates are reviewed each quarter.";
    rig.page(rig.w1, p, &[("Review cycle", text)], "1", graph("Rates"))
        .await;
    rig.page(rig.w1, p, &[("Review", text)], "2", graph("Rates"))
        .await;
    let result = rig
        .recall_text(rig.w1, "how often are rates reviewed")
        .await;
    assert_eq!(result.facts[0].page_section.as_deref(), Some("Review"));
    rig.stop().await;
}

/// An embedder as slow as a model load, past a limit of one second.
struct SlowEmbedder;

#[async_trait::async_trait]
impl memory::types::ports::EmbeddingClient for SlowEmbedder {
    async fn embed(
        &self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, memory::types::ports::ProviderError> {
        tokio::time::sleep(Duration::from_secs(5)).await;
        Ok(texts.iter().map(|_| vec![0.1; 384]).collect())
    }
}

#[tokio::test]
async fn error_a_recall_that_passes_its_time_limit() {
    let dir = TempDir::new();
    let (_store, journal) = open(&dir).await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let mut slow = seams(600);
    slow.embedder = std::sync::Arc::new(SlowEmbedder);
    slow.recall_limit_secs = 1;
    let svc = MemoryService::start_with(
        config(ProgramKind::Desktop, None, None),
        journal,
        shutdown.clone(),
        slow,
    )
    .await
    .expect("start");
    let who = MemoryCaller::Ordin {
        workspace_id: ordin_types::WorkspaceId::new(),
        run_id: RunId::new(),
    };
    assert_eq!(
        code_of(svc.recall(who, query("Kestrel")).await),
        ("MEM_MODEL_UNAVAILABLE", true)
    );
    shutdown.cancel();
}

#[tokio::test]
async fn error_more_folders_than_one_recall_can_search() {
    let rig = Rig::desktop().await;
    for n in 0..33 {
        rig.got(
            rig.w1,
            &format!("A read fact of folder {n}."),
            Some(ordin_types::TeamFolderId::new()),
        )
        .await;
    }
    assert_eq!(
        code_of(rig.svc.recall(rig.ordin(rig.w1), query("read fact")).await),
        ("MEM_TOO_MANY_FOLDERS", false)
    );
    rig.stop().await;
}

// MEM-REP-1: found in the Phase 5 walk — "we report costs in GBP" said in four conversations was kept
// four times.
// spec: ordin-desktop/components/MemoryService#MEM-REP-1
async fn said_in_a_new_conversation(rig: &Rig, w: WorkspaceId, statement: &str) -> MemoryFactId {
    let receipt = rig
        .svc
        .remember_text(
            MemoryCaller::Ordin {
                workspace_id: w,
                run_id: RunId::new(),
            },
            input(statement, None),
        )
        .await
        .expect("remember");
    rig.kept(receipt.id).await;
    receipt.id
}

#[tokio::test]
// ADR-118 (T1 step 4): a fact's key names its run, so the same fact said in two conversations is two
// facts of two sources; MEM-REP-1 now binds only the statement form, Ordin's summaries.
// spec: ordin-desktop/components/MemoryService#MEM-TR-4
async fn edge_the_same_statement_in_two_conversations_is_two_facts() {
    let rig = Rig::desktop().await;
    let first = said_in_a_new_conversation(&rig, rig.w1, "We report costs in GBP.").await;
    let second = said_in_a_new_conversation(&rig, rig.w1, "We report costs in GBP.").await;
    assert_ne!(first, second);
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_fact_refs")
            .await,
        2
    );
    // Another workspace keeps its own.
    let other = said_in_a_new_conversation(&rig, rig.w2, "We report costs in GBP.").await;
    assert_ne!(other, first);
    rig.stop().await;
}

// MEM-REP-1: a statement said again in a later conversation after it was forgotten is kept again.
#[tokio::test]
async fn edge_a_forgotten_statement_said_again_is_kept_again() {
    let rig = Rig::desktop().await;
    let first = said_in_a_new_conversation(&rig, rig.w1, "Kestrel's rate is confidential.").await;
    rig.svc.forget_fact(first).await.expect("forget");
    let again = said_in_a_new_conversation(&rig, rig.w1, "Kestrel's rate is confidential.").await;
    assert_ne!(again, first);
    let result = rig.recall_text(rig.w1, "Kestrel rate").await;
    assert_eq!(
        statements(&result),
        vec!["Kestrel's rate is confidential.".to_owned()]
    );
    rig.stop().await;
}
