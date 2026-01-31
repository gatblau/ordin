//! Gherkin: "Knowledge Service — hosted agent knowledge (agent profile)" and the ADR-107 lookup,
//! on SQLite (single shape) and, separately, on the PostgreSQL 18 container (scaled shape); and
//! the agent-profile rows of the Error Table (X1). Each scenario is one function run by a test of
//! the same name in module `sqlite` and in module `pg`. The PostgreSQL runs need a container
//! runtime through `DOCKER_HOST`; a run without one fails.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use ordin_journal::Datastore;
use ordin_knowledge::{
    ChatHistoryPage, IngestItem, ItemKind, KnowledgeError, KnowledgeIndex, KnowledgeService,
    KnowledgeSourceConfig, ProgramKind, ScopeChat,
};
use ordin_mcp_front::{CallVia, ProgressSink, ServerName, ToolCallContext, ToolCaller, ToolKey};
use ordin_types::{AgentInstanceId, RunId, ScopeId, ToolRequestId, WorkspaceId};
use serde_json::{json, Value};
use support::fakes::{journal_on, sqlite_journal, Fakes, TempDir};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Engine {
    Sqlite,
    Pg,
}

fn sources(notes: bool) -> Vec<KnowledgeSourceConfig> {
    let mut list = vec![
        json!({"name": "channel-history", "kind": "chat-history", "connection": "team-slack", "language": "english",
               "expose": ["search", "recent", "open"], "hidden-senders-setting": "hidden-senders",
               "own-messages-setting": "own-messages", "ranking": {"thread-replies": true}}),
        json!({"name": "team-faq", "kind": "entries", "expose": ["search"]}),
    ];
    if notes {
        list.push(json!({"name": "agent-notes", "kind": "notes", "max-notes-per-scope": 3, "max-note-chars": 100}));
    }
    list.into_iter()
        .map(|v| serde_json::from_value(v).unwrap())
        .collect()
}

struct Agent {
    svc: Arc<KnowledgeService>,
    f: Fakes,
    store: Datastore,
    a: ScopeId,
    b: ScopeId,
    engine: Engine,
    _dir: TempDir,
}

async fn agent_with(
    engine: Engine,
    sources: Vec<KnowledgeSourceConfig>,
    note_tools: bool,
) -> Result<Agent, KnowledgeError> {
    let dir = TempDir::new();
    let (store, journal, program) = match engine {
        Engine::Sqlite => {
            let (s, j) = sqlite_journal(&dir).await;
            (s, j, ProgramKind::AgentSingle)
        }
        Engine::Pg => {
            let s = support::pg::migrated_database().await;
            (s.clone(), journal_on(s).await, ProgramKind::AgentScaled)
        }
    };
    let f = Fakes::new();
    let svc = KnowledgeService::new(program, f.deps(journal, Some((sources, note_tools)))).await?;
    let a = ScopeId::new();
    let b = ScopeId::new();
    f.chat.chats.lock().unwrap().insert(
        a,
        ScopeChat {
            connection: "team-slack".into(),
            chat: "C0456XYZ".into(),
            ingest_enabled: true,
        },
    );
    f.chat.chats.lock().unwrap().insert(
        b,
        ScopeChat {
            connection: "team-slack".into(),
            chat: "D0999".into(),
            ingest_enabled: false,
        },
    );
    Ok(Agent {
        svc,
        f,
        store,
        a,
        b,
        engine,
        _dir: dir,
    })
}

async fn agent(engine: Engine) -> Agent {
    agent_with(engine, sources(true), true).await.unwrap()
}

fn item(origin: &str, text: &str, sender: &str) -> IngestItem {
    IngestItem {
        origin_id: origin.into(),
        origin_time: Utc::now(),
        sender: sender.into(),
        sender_name: Some(format!("Name {sender}")),
        from_program: false,
        own: false,
        text: text.into(),
        thread: None,
    }
}

impl Agent {
    fn index(&self) -> Arc<dyn KnowledgeIndex> {
        self.svc.index().unwrap()
    }

    fn ctx(&self, scope: Option<ScopeId>) -> ToolCallContext {
        ToolCallContext {
            run_id: RunId::new(),
            workspace_id: WorkspaceId::new(),
            agent_instance_id: AgentInstanceId::new(),
            caller: ToolCaller::HostedAgent,
            scope_id: scope,
            coding_task_id: None,
            call_id: ToolRequestId::new(),
            correlation_id: "test".into(),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
            cancel: tokio_util::sync::CancellationToken::new(),
            progress: ProgressSink::detached(),
            only_needed_parts: false,
            via: CallVia::Harness,
        }
    }

    async fn tool(&self, scope: Option<ScopeId>, tool: &str, args: Value) -> String {
        let key = ToolKey {
            server: ServerName::new("knowledge").unwrap(),
            tool: tool.into(),
        };
        self.svc
            .tool_provider()
            .call(self.ctx(scope), &key, args)
            .await
            .unwrap()
            .text
    }

    async fn upsert(&self, scope: ScopeId, items: &[IngestItem]) -> u64 {
        self.index()
            .upsert("channel-history", scope, items)
            .await
            .unwrap()
    }

    async fn close(&self) {
        match &self.store {
            Datastore::Sqlite(p) => p.close().await,
            Datastore::Postgres(p) => p.close().await,
        }
    }
}

macro_rules! scenarios {
    ($($name:ident),* $(,)?) => {
        mod sqlite {
            $( #[tokio::test(flavor = "current_thread")] async fn $name() { super::$name(super::Engine::Sqlite).await } )*
        }
        mod pg {
            $( #[tokio::test(flavor = "current_thread")] async fn $name() { super::$name(super::Engine::Pg).await } )*
        }
    };
}

scenarios!(
    happy_path_a_search_matches_any_word_of_the_query,
    word_forms_match,
    a_note_is_written_to_the_runs_scope_only,
    agent_wide_notes_are_visible_everywhere,
    error_path_the_note_limit,
    error_path_a_note_too_long,
    unverified_entries_are_hidden,
    hidden_senders,
    upsert_skips_unchanged_items,
    erasure_by_scope,
    edge_case_entries_are_paged_newest_first,
    error_path_the_datastore_is_unavailable,
    ingest_step_reads_one_page_of_chat_history_and_indexes_it,
    ingest_step_for_a_scope_that_is_not_enabled,
    ingest_step_when_the_chat_system_cannot_be_reached,
    edge_case_a_refused_ingest_step_counts_nothing,
    thread_replies_are_ingested,
    one_thread_failing_does_not_stop_the_job,
    a_checkpoint_written_before_thread_reading_still_loads,
    agent_profile_lookup_searches_one_source_for_the_question_with_no_model,
    row_source_name_unknown_or_of_the_wrong_kind,
    row_ingest_step_source_unknown_or_not_chat_history,
    row_ingest_step_the_scope_has_no_enabled_chat_or_the_chat_system_refuses,
    row_ingest_step_the_chat_history_cannot_be_read,
    row_ingest_step_reading_or_indexing_fails_is_counted,
    row_ingest_step_checkpoint_of_another_shape,
    row_a_question_type_pattern_does_not_compile,
    row_note_beyond_max_notes_per_scope,
    row_note_over_max_note_chars,
    row_note_with_no_text,
    row_replace_or_retire_an_unknown_note_or_a_non_uuid_id,
    row_note_tools_wanted_with_no_notes_source,
    row_entry_text_empty_or_over_20000_characters,
    row_entry_id_unknown,
    row_datastore_or_settings_unavailable_during_an_agent_profile_call,
    row_malformed_cursor_on_notes_or_entries,
);

async fn happy_path_a_search_matches_any_word_of_the_query(engine: Engine) {
    let ag = agent(engine).await;
    ag.upsert(
        ag.a,
        &[
            item("1.1", "the deploy key lives in the vault", "U1"),
            item("1.2", "lunch is at noon", "U2"),
        ],
    )
    .await;
    let text = ag
        .tool(
            Some(ag.a),
            "search_channel_history",
            json!({"query": "rotate deploy key staging"}),
        )
        .await;
    assert!(text.starts_with("1 results."), "{text}");
    assert!(text.contains("the deploy key lives in the vault"));
}

async fn word_forms_match(engine: Engine) {
    let ag = agent(engine).await;
    ag.upsert(ag.a, &[item("1.1", "we rotated the keys yesterday", "U1")])
        .await;
    let results = ag
        .index()
        .search(
            &["channel-history".into()],
            Some(ag.a),
            "rotating key",
            None,
            5,
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn edge_case_a_stop_word_query_on_postgresql() {
    let ag = agent(Engine::Pg).await;
    ag.upsert(ag.a, &[item("1.1", "the deploy key", "U1")])
        .await;
    let text = ag
        .tool(
            Some(ag.a),
            "search_channel_history",
            json!({"query": "the and of to"}),
        )
        .await;
    assert_eq!(text, "No results.");
}

async fn a_note_is_written_to_the_runs_scope_only(engine: Engine) {
    let ag = agent(engine).await;
    let text = ag
        .tool(
            Some(ag.b),
            "note_write",
            json!({"text": "Asha prefers Tuesday calls"}),
        )
        .await;
    let id = text.strip_prefix("Note written: ").unwrap();
    let note = ag.index().open(id, Some(ag.b)).await.unwrap().unwrap();
    assert_eq!(note.scope_id, Some(ag.b));
    let found = ag
        .tool(
            Some(ag.a),
            "search_channel_history",
            json!({"query": "Asha Tuesday"}),
        )
        .await;
    assert_eq!(found, "No results.");
}

async fn agent_wide_notes_are_visible_everywhere(engine: Engine) {
    let ag = agent(engine).await;
    ag.tool(
        None,
        "note_write",
        json!({"text": "The release train leaves on Thursdays"}),
    )
    .await;
    let text = ag
        .tool(
            Some(ag.a),
            "search_channel_history",
            json!({"query": "release train"}),
        )
        .await;
    assert!(text.contains("| agent-note |"), "{text}");
}

async fn error_path_the_note_limit(engine: Engine) {
    let ag = agent(engine).await;
    for i in 0..3 {
        ag.tool(
            Some(ag.a),
            "note_write",
            json!({"text": format!("note {i}")}),
        )
        .await;
    }
    let text = ag
        .tool(Some(ag.a), "note_write", json!({"text": "one more"}))
        .await;
    assert_eq!(text, "Refused: this place already holds 3 notes, the limit of the configuration (max-notes-per-scope: 3). Retire a note before you write another.");
}

async fn error_path_a_note_too_long(engine: Engine) {
    let ag = agent(engine).await;
    let text = ag
        .tool(Some(ag.a), "note_write", json!({"text": "n".repeat(117)}))
        .await;
    assert_eq!(
        text,
        "Refused: a note holds at most 100 characters (max-note-chars: 100); this text holds 117."
    );
}

async fn unverified_entries_are_hidden(engine: Engine) {
    let ag = agent(engine).await;
    let id = ag
        .index()
        .entry_add(
            "team-faq",
            ag.a,
            "expense claims go through the finance portal",
            "op",
        )
        .await
        .unwrap();
    let text = ag
        .tool(
            Some(ag.a),
            "search_channel_history",
            json!({"query": "expense claims"}),
        )
        .await;
    assert_eq!(text, "No results.");
    ag.index()
        .entry_verify(&id.to_string(), "op")
        .await
        .unwrap();
    let results = ag
        .index()
        .search(
            &["channel-history".into(), "team-faq".into()],
            Some(ag.a),
            "expense claims",
            None,
            5,
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].label, ordin_knowledge::ResultLabel::Entry);
}

async fn hidden_senders(engine: Engine) {
    let ag = agent(engine).await;
    ag.upsert(
        ag.a,
        &[
            item("1.1", "secret plan from u01", "U01"),
            item("1.2", "public plan", "U02"),
        ],
    )
    .await;
    ag.f.settings
        .hidden
        .lock()
        .unwrap()
        .insert(ag.a, vec!["U01".into()]);
    let search = ag
        .tool(
            Some(ag.a),
            "search_channel_history",
            json!({"query": "plan"}),
        )
        .await;
    assert!(!search.contains("u01"), "{search}");
    let recent = ag
        .tool(Some(ag.a), "recent_channel_history", json!({}))
        .await;
    assert!(!recent.contains("u01"), "{recent}");
    let hidden = ag
        .index()
        .recent("channel-history", Some(ag.a), 50)
        .await
        .unwrap();
    assert!(hidden.iter().all(|i| i.sender.as_deref() != Some("U01")));
    let id = ag
        .svc
        .index()
        .unwrap()
        .search(&["channel-history".into()], Some(ag.a), "public", None, 5)
        .await
        .unwrap()[0]
        .item
        .item_id;
    let _ = id;
    ag.f.settings.hidden.lock().unwrap().clear();
    let all = ag
        .index()
        .recent("channel-history", Some(ag.a), 50)
        .await
        .unwrap();
    let secret = all
        .iter()
        .find(|i| i.sender.as_deref() == Some("U01"))
        .unwrap()
        .item_id;
    ag.f.settings
        .hidden
        .lock()
        .unwrap()
        .insert(ag.a, vec!["U01".into()]);
    let open = ag
        .tool(
            Some(ag.a),
            "open_channel_history",
            json!({"item_id": secret.to_string()}),
        )
        .await;
    assert_eq!(open, format!("No item {secret} is available here."));
}

async fn upsert_skips_unchanged_items(engine: Engine) {
    let ag = agent(engine).await;
    let items = [item("1.1", "a", "U1"), item("1.2", "b", "U2")];
    assert_eq!(ag.upsert(ag.a, &items).await, 2);
    assert_eq!(ag.upsert(ag.a, &items).await, 0);
}

async fn erasure_by_scope(engine: Engine) {
    let ag = agent(engine).await;
    ag.upsert(ag.a, &[item("1.1", "a", "U1")]).await;
    ag.tool(Some(ag.a), "note_write", json!({"text": "note in A"}))
        .await;
    ag.tool(Some(ag.b), "note_write", json!({"text": "note in B"}))
        .await;
    ag.index().delete_scope(Some(ag.a)).await.unwrap();
    assert_eq!(ag.index().count_scope(Some(ag.a)).await.unwrap(), 0);
    assert_eq!(
        ag.index()
            .notes(Some(ag.b), None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
}

async fn set_created(ag: &Agent, id: uuid::Uuid, at: chrono::DateTime<Utc>) {
    match &ag.store {
        Datastore::Sqlite(p) => {
            sqlx::query("UPDATE knowledge_items SET created_at = ?1 WHERE item_id = ?2")
                .bind(at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
                .bind(id.to_string())
                .execute(p)
                .await
                .unwrap();
        }
        Datastore::Postgres(p) => {
            sqlx::query("UPDATE knowledge_items SET created_at = $1 WHERE item_id = $2")
                .bind(at)
                .bind(id)
                .execute(p)
                .await
                .unwrap();
        }
    }
}

async fn edge_case_entries_are_paged_newest_first(engine: Engine) {
    let ag = agent(engine).await;
    let start = Utc::now() - ChronoDuration::hours(1);
    let mut ids = Vec::new();
    for i in 0..120 {
        let id = ag
            .index()
            .entry_add("team-faq", ag.a, &format!("entry {i}"), "op")
            .await
            .unwrap();
        set_created(&ag, id, start + ChronoDuration::seconds(i)).await;
        ids.push(id);
    }
    let first = ag.index().entries(ag.a, None).await.unwrap();
    assert_eq!(first.items.len(), 100);
    assert_eq!(first.items[0].item_id, ids[119]);
    let cursor = serde_json::to_value(first.next_cursor.unwrap()).unwrap();
    let second = ag
        .index()
        .entries(ag.a, Some(cursor.as_str().unwrap()))
        .await
        .unwrap();
    assert_eq!(second.items.len(), 20);
    assert_eq!(second.items[19].item_id, ids[0]);
    assert!(second.next_cursor.is_none());
}

async fn error_path_the_datastore_is_unavailable(engine: Engine) {
    let ag = agent(engine).await;
    ag.close().await;
    let text = ag
        .tool(Some(ag.a), "note_write", json!({"text": "x"}))
        .await;
    assert_eq!(text, "The knowledge index cannot be reached at the moment, and whether the note was changed is not known (KNOW_DB_UNAVAILABLE).");
}

fn page(items: Vec<IngestItem>, next: Option<&str>, threads: &[&str]) -> ChatHistoryPage {
    ChatHistoryPage {
        items,
        newest_position: None,
        next_cursor: next.map(str::to_owned),
        threads: threads.iter().map(|t| (*t).to_owned()).collect(),
    }
}

async fn ingest_step_reads_one_page_of_chat_history_and_indexes_it(engine: Engine) {
    let ag = agent(engine).await;
    ag.f.chat.pages.lock().unwrap().insert(
        String::new(),
        page(
            vec![
                item("1.1", "a", "U1"),
                item("1.2", "b", "U2"),
                item("1.3", "c", "U3"),
            ],
            None,
            &[],
        ),
    );
    let step = ag
        .svc
        .ingest()
        .unwrap()
        .ingest_step("channel-history", ag.a, None)
        .await
        .unwrap();
    assert_eq!(ag.index().count_scope(Some(ag.a)).await.unwrap(), 3);
    assert_eq!((step.items_read, step.total, step.done), (3, None, true));
    assert_eq!(ag.f.metrics.count("items_written", "channel-history"), 3);
    assert_eq!(
        ag.f.metrics
            .count("ingest_thread_failed", "channel-history"),
        0
    );
}

async fn ingest_step_for_a_scope_that_is_not_enabled(engine: Engine) {
    let ag = agent(engine).await;
    let e = ag
        .svc
        .ingest()
        .unwrap()
        .ingest_step("channel-history", ag.b, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_SCOPE_NOT_ENABLED");
    assert!(ag.f.chat.reads.lock().unwrap().is_empty());
}

async fn ingest_step_when_the_chat_system_cannot_be_reached(engine: Engine) {
    let ag = agent(engine).await;
    *ag.f.chat.unavailable.lock().unwrap() = true;
    let e = ag
        .svc
        .ingest()
        .unwrap()
        .ingest_step("channel-history", ag.a, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_HISTORY_UNAVAILABLE");
    assert!(e.retryable());
    assert_eq!(
        ag.f.metrics
            .count("ingest_thread_failed", "channel-history"),
        1
    );
    assert_eq!(ag.f.metrics.count("items_written", "channel-history"), 0);
}

async fn edge_case_a_refused_ingest_step_counts_nothing(engine: Engine) {
    let ag = agent(engine).await;
    let e = ag
        .svc
        .ingest()
        .unwrap()
        .ingest_step("channel-history", ag.b, None)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_SCOPE_NOT_ENABLED");
    assert!(ag.f.metrics.of("items_written").is_empty());
    assert!(ag.f.metrics.of("ingest_thread_failed").is_empty());
}

async fn thread_replies_are_ingested(engine: Engine) {
    let ag = agent(engine).await;
    let mut parent = item("1.0", "parent message", "U1");
    parent.thread = Some("1.0".into());
    ag.f.chat
        .pages
        .lock()
        .unwrap()
        .insert(String::new(), page(vec![parent], None, &["1.0"]));
    let replies: Vec<IngestItem> = (1..=3)
        .map(|i| {
            let mut r = item(&format!("1.{i}"), &format!("reply {i}"), "U2");
            r.thread = Some("1.0".into());
            r
        })
        .collect();
    ag.f.chat
        .replies
        .lock()
        .unwrap()
        .insert(("1.0".into(), String::new()), page(replies, None, &[]));
    let ingest = ag.svc.ingest().unwrap();
    let first = ingest
        .ingest_step("channel-history", ag.a, None)
        .await
        .unwrap();
    assert!(!first.done);
    assert_eq!(first.checkpoint["threads"], json!(["1.0"]));
    let second = ingest
        .ingest_step("channel-history", ag.a, Some(&first.checkpoint))
        .await
        .unwrap();
    assert!(ag
        .f
        .chat
        .reads
        .lock()
        .unwrap()
        .contains(&"replies:1.0".to_owned()));
    let all = ag
        .index()
        .recent("channel-history", Some(ag.a), 50)
        .await
        .unwrap();
    assert_eq!(
        all.iter()
            .filter(|i| i.thread.as_deref() == Some("1.0") && i.origin_id.as_deref() != Some("1.0"))
            .count(),
        3
    );
    assert_eq!(second.checkpoint["threads"], json!([]));
    ingest
        .ingest_step("channel-history", ag.a, Some(&second.checkpoint))
        .await
        .unwrap();
    assert_eq!(ag.f.chat.reads.lock().unwrap().last().unwrap(), "page:");
}

async fn one_thread_failing_does_not_stop_the_job(engine: Engine) {
    let ag = agent(engine).await;
    ag.f.chat
        .failing_threads
        .lock()
        .unwrap()
        .insert("1.0".into());
    let cp = json!({"oldest": null, "cursor": null, "items_read": 0, "threads": ["1.0", "2.0"], "thread_cursor": null});
    let step = ag
        .svc
        .ingest()
        .unwrap()
        .ingest_step("channel-history", ag.a, Some(&cp))
        .await
        .unwrap();
    assert_eq!(step.items_read, 0);
    assert_eq!(step.checkpoint["threads"], json!(["2.0"]));
    assert_eq!(
        ag.f.metrics
            .count("ingest_thread_failed", "channel-history"),
        1
    );
}

async fn a_checkpoint_written_before_thread_reading_still_loads(engine: Engine) {
    let ag = agent(engine).await;
    ag.f.chat
        .pages
        .lock()
        .unwrap()
        .insert("c2".into(), page(vec![item("2.1", "x", "U1")], None, &[]));
    let cp = json!({"oldest": null, "cursor": "c2", "items_read": 200});
    let step = ag
        .svc
        .ingest()
        .unwrap()
        .ingest_step("channel-history", ag.a, Some(&cp))
        .await
        .unwrap();
    assert!(ag
        .f
        .chat
        .reads
        .lock()
        .unwrap()
        .contains(&"page:c2".to_owned()));
    assert_eq!(step.items_read, 201);
}

async fn agent_profile_lookup_searches_one_source_for_the_question_with_no_model(engine: Engine) {
    let (logs, _guard) = support::logs::capture();
    let ag = agent(engine).await;
    let items: Vec<IngestItem> = (0..8)
        .map(|i| {
            item(
                &format!("1.{i}"),
                &format!("to reset password open the portal step {i}"),
                "U1",
            )
        })
        .collect();
    ag.upsert(ag.a, &items).await;
    ag.tool(
        Some(ag.a),
        "note_write",
        json!({"text": "reset password notes"}),
    )
    .await;
    let results = ag
        .index()
        .lookup("channel-history", Some(ag.a), "how do I reset password", 5)
        .await
        .unwrap();
    assert!(!results.is_empty() && results.len() <= 5);
    assert!(results
        .iter()
        .all(|r| r.item.source == "channel-history" && r.item.kind != ItemKind::Notes));
    let lines = logs.named("knowledge_lookup");
    assert_eq!(lines.len(), 1);
    assert!(lines[0].fields.values().all(|v| !v.contains("reset")));
}

// ---------------------------------------------------------------- X1: agent-profile rows

async fn row_source_name_unknown_or_of_the_wrong_kind(engine: Engine) {
    let ag = agent(engine).await;
    let e = ag
        .index()
        .search(&["nope".into()], Some(ag.a), "x", None, 5)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_UNKNOWN_SOURCE");
    let e = ag
        .index()
        .entry_add("channel-history", ag.a, "x", "op")
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_UNKNOWN_SOURCE");
    assert!(!e.retryable());
}

async fn row_ingest_step_source_unknown_or_not_chat_history(engine: Engine) {
    let ag = agent(engine).await;
    let ingest = ag.svc.ingest().unwrap();
    assert_eq!(
        ingest
            .ingest_step("nope", ag.a, None)
            .await
            .unwrap_err()
            .code(),
        "KNOW_UNKNOWN_SOURCE"
    );
    assert_eq!(
        ingest
            .ingest_step("team-faq", ag.a, None)
            .await
            .unwrap_err()
            .code(),
        "KNOW_UNKNOWN_KIND"
    );
}

async fn row_ingest_step_the_scope_has_no_enabled_chat_or_the_chat_system_refuses(engine: Engine) {
    let ag = agent(engine).await;
    let ingest = ag.svc.ingest().unwrap();
    for scope in [ag.b, ScopeId::new()] {
        let e = ingest
            .ingest_step("channel-history", scope, None)
            .await
            .unwrap_err();
        assert_eq!(e.code(), "KNOW_SCOPE_NOT_ENABLED");
        assert!(!e.retryable());
    }
}

async fn row_ingest_step_the_chat_history_cannot_be_read(engine: Engine) {
    let ag = agent(engine).await;
    *ag.f.chat.unavailable.lock().unwrap() = true;
    let e = ag
        .svc
        .ingest()
        .unwrap()
        .ingest_step("channel-history", ag.a, None)
        .await
        .unwrap_err();
    assert_eq!(
        (e.code(), e.retryable()),
        ("KNOW_HISTORY_UNAVAILABLE", true)
    );
}

async fn row_ingest_step_reading_or_indexing_fails_is_counted(engine: Engine) {
    let ag = agent(engine).await;
    ag.f.chat
        .pages
        .lock()
        .unwrap()
        .insert(String::new(), page(vec![item("1.1", "a", "U1")], None, &[]));
    ag.close().await;
    let e = ag
        .svc
        .ingest()
        .unwrap()
        .ingest_step(
            "channel-history",
            ag.a,
            Some(&json!({"oldest": null, "cursor": null, "items_read": 0})),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_DB_UNAVAILABLE");
    assert_eq!(
        ag.f.metrics
            .count("ingest_thread_failed", "channel-history"),
        1
    );
}

async fn row_ingest_step_checkpoint_of_another_shape(engine: Engine) {
    let ag = agent(engine).await;
    let e = ag
        .svc
        .ingest()
        .unwrap()
        .ingest_step("channel-history", ag.a, Some(&json!({"page": 3})))
        .await
        .unwrap_err();
    assert_eq!(
        (e.code(), e.retryable()),
        ("KNOW_CHECKPOINT_INVALID", false)
    );
}

async fn row_a_question_type_pattern_does_not_compile(engine: Engine) {
    let mut list = sources(true);
    list[0] = serde_json::from_value(
        json!({"name": "channel-history", "kind": "chat-history", "connection": "team-slack",
        "ranking": {"favour-links-and-steps": [{"name": "how-to", "patterns": ["(unclosed"]}]}}),
    )
    .unwrap();
    let Err(e) = agent_with(engine, list, true).await else {
        panic!("started")
    };
    assert_eq!(e.code(), "KNOW_BAD_PATTERN");
    let KnowledgeError::BadPattern { at } = e else {
        panic!()
    };
    assert_eq!(
        at,
        "knowledge[channel-history].ranking.favour-links-and-steps[how-to]"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn row_language_is_no_postgresql_text_search_configuration() {
    let mut list = sources(true);
    list[0] = serde_json::from_value(json!({"name": "channel-history", "kind": "chat-history", "connection": "team-slack", "language": "klingon"})).unwrap();
    let ag = agent_with(Engine::Pg, list, true).await.unwrap();
    let e = ag
        .index()
        .upsert("channel-history", ag.a, &[item("1", "x", "U1")])
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_LANGUAGE_UNKNOWN");
    let KnowledgeError::LanguageUnknown { language } = e else {
        panic!()
    };
    assert_eq!(language, "klingon");
}

async fn row_note_beyond_max_notes_per_scope(engine: Engine) {
    let ag = agent(engine).await;
    for i in 0..3 {
        ag.index()
            .note_write("agent-notes", Some(ag.a), &format!("n{i}"), RunId::new())
            .await
            .unwrap();
    }
    let e = ag
        .index()
        .note_write("agent-notes", Some(ag.a), "n", RunId::new())
        .await
        .unwrap_err();
    assert!(matches!(e, KnowledgeError::NoteLimit { max: 3 }));
    let text = ag
        .tool(Some(ag.a), "note_write", json!({"text": "n"}))
        .await;
    assert!(text.starts_with("Refused: this place already holds 3 notes"));
    assert_eq!(ag.f.metrics.count("note_refused:limit", "agent-notes"), 1);
}

async fn row_note_over_max_note_chars(engine: Engine) {
    let ag = agent(engine).await;
    let e = ag
        .index()
        .note_write("agent-notes", Some(ag.a), &"x".repeat(101), RunId::new())
        .await
        .unwrap_err();
    assert!(matches!(
        e,
        KnowledgeError::NoteTooLong { max: 100, len: 101 }
    ));
    ag.tool(Some(ag.a), "note_write", json!({"text": "x".repeat(101)}))
        .await;
    assert_eq!(
        ag.f.metrics.count("note_refused:too-long", "agent-notes"),
        1
    );
}

async fn row_note_with_no_text(engine: Engine) {
    let ag = agent(engine).await;
    assert_eq!(
        ag.index()
            .note_write("agent-notes", Some(ag.a), "   ", RunId::new())
            .await
            .unwrap_err()
            .code(),
        "KNOW_NOTE_EMPTY"
    );
    assert_eq!(
        ag.tool(Some(ag.a), "note_write", json!({"text": "  "}))
            .await,
        "Refused: a note needs a text."
    );
}

async fn row_replace_or_retire_an_unknown_note_or_a_non_uuid_id(engine: Engine) {
    let ag = agent(engine).await;
    let unknown = uuid::Uuid::now_v7().to_string();
    for id in [unknown.as_str(), "5b2e-not-a-uuid"] {
        assert_eq!(
            ag.index()
                .note_retire(id, Some(ag.a))
                .await
                .unwrap_err()
                .code(),
            "KNOW_NOTE_NOT_FOUND"
        );
        assert_eq!(
            ag.index()
                .note_replace(id, Some(ag.a), "x", RunId::new())
                .await
                .unwrap_err()
                .code(),
            "KNOW_NOTE_NOT_FOUND"
        );
        let text = ag
            .tool(Some(ag.a), "note_retire", json!({"note_id": id}))
            .await;
        assert_eq!(text, format!("No note {id} of yours exists here."));
    }
    // A note of another scope is not one of yours here.
    let other = ag
        .index()
        .note_write("agent-notes", Some(ag.b), "b", RunId::new())
        .await
        .unwrap();
    let text = ag
        .tool(
            Some(ag.a),
            "note_replace",
            json!({"note_id": other.to_string(), "text": "y"}),
        )
        .await;
    assert_eq!(text, format!("No note {other} of yours exists here."));
    let mine = ag
        .index()
        .note_write("agent-notes", Some(ag.a), "a", RunId::new())
        .await
        .unwrap();
    assert_eq!(
        ag.tool(
            Some(ag.a),
            "note_replace",
            json!({"note_id": mine.to_string(), "text": "y"})
        )
        .await,
        format!("Note replaced: {mine}")
    );
    assert_eq!(
        ag.tool(
            Some(ag.a),
            "note_retire",
            json!({"note_id": mine.to_string()})
        )
        .await,
        format!("Note retired: {mine}")
    );
}

async fn row_note_tools_wanted_with_no_notes_source(engine: Engine) {
    let Err(e) = agent_with(engine, sources(false), true).await else {
        panic!("started")
    };
    assert_eq!(e.code(), "KNOW_NO_NOTES_SOURCE");
}

async fn row_entry_text_empty_or_over_20000_characters(engine: Engine) {
    let ag = agent(engine).await;
    for text in ["".to_owned(), "e".repeat(20_001)] {
        assert_eq!(
            ag.index()
                .entry_add("team-faq", ag.a, &text, "op")
                .await
                .unwrap_err()
                .code(),
            "KNOW_ENTRY_INVALID"
        );
    }
}

async fn row_entry_id_unknown(engine: Engine) {
    let ag = agent(engine).await;
    let id = uuid::Uuid::now_v7().to_string();
    assert_eq!(
        ag.index().entry_verify(&id, "op").await.unwrap_err().code(),
        "KNOW_ENTRY_NOT_FOUND"
    );
    assert_eq!(
        ag.index().entry_remove(&id).await.unwrap_err().code(),
        "KNOW_ENTRY_NOT_FOUND"
    );
}

async fn row_datastore_or_settings_unavailable_during_an_agent_profile_call(engine: Engine) {
    let ag = agent(engine).await;
    *ag.f.settings.unavailable.lock().unwrap() = true;
    let text = ag
        .tool(Some(ag.a), "search_channel_history", json!({"query": "x"}))
        .await;
    assert_eq!(
        text,
        "The knowledge index cannot be read at the moment (KNOW_DB_UNAVAILABLE)."
    );
    *ag.f.settings.unavailable.lock().unwrap() = false;
    ag.close().await;
    let e = ag.index().notes(Some(ag.a), None).await.unwrap_err();
    assert_eq!((e.code(), e.retryable()), ("KNOW_DB_UNAVAILABLE", true));
    let text = ag
        .tool(Some(ag.a), "recent_channel_history", json!({}))
        .await;
    assert_eq!(
        text,
        "The knowledge index cannot be read at the moment (KNOW_DB_UNAVAILABLE)."
    );
}

async fn row_malformed_cursor_on_notes_or_entries(engine: Engine) {
    let ag = agent(engine).await;
    assert_eq!(
        ag.index()
            .notes(Some(ag.a), Some("!!not-a-cursor"))
            .await
            .unwrap_err()
            .code(),
        "SRV_BAD_CURSOR"
    );
    assert_eq!(
        ag.index()
            .entries(ag.a, Some("e30"))
            .await
            .unwrap_err()
            .code(),
        "SRV_BAD_CURSOR"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_tools_are_offered_to_the_hosted_agent_with_wrightruns_names() {
    let ag = agent(Engine::Sqlite).await;
    let names: Vec<String> = ag.svc.tool_provider().servers()[0]
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(
        names,
        [
            "search_channel_history",
            "recent_channel_history",
            "open_channel_history",
            "search_team_faq",
            "note_write",
            "note_replace",
            "note_retire"
        ]
    );
    assert!(ag.engine == Engine::Sqlite);
}
