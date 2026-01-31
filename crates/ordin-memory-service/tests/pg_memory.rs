//! The `[pg]` scenarios: the migrations on PostgreSQL 18, and the agent profile's scenarios of
//! criterion 17 on the scaled shape (Gherkin *Agent profile — the same scenarios on PostgreSQL*).
//! Needs a container runtime through `DOCKER_HOST`; a run without one fails.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_journal::{Datastore, Journal, JournalOptions, SystemClock};
use ordin_memory_service::{
    DocumentStanding, FactState, KnowledgeDocumentFacts, KnowledgeFactInput, LinkInput,
    MemoryCaller, MemoryPlace, MemoryService, ProgramKind, RecallInput, RememberInput, SourceKind,
    ThingCount, ThingInput, WiderMode,
};
use ordin_types::{MemoryFactId, RunId, ScopeId};
use tokio_util::sync::CancellationToken;

struct Scaled {
    svc: MemoryService,
    store: Datastore,
    shutdown: CancellationToken,
}

async fn scaled() -> Scaled {
    let store = support::pg::migrated_database().await;
    let journal = Journal::start(
        store.clone(),
        JournalOptions {
            automatic_pruning: false,
            prune_interval: Duration::from_secs(86_400),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .expect("the journal");
    let shutdown = CancellationToken::new();
    let svc = MemoryService::start_with(
        support::config(ProgramKind::AgentScaled),
        Arc::new(journal),
        shutdown.clone(),
        support::seams(),
    )
    .await
    .expect("the service starts on PostgreSQL");
    Scaled {
        svc,
        store,
        shutdown,
    }
}

fn hosted(scope: ScopeId) -> MemoryCaller {
    MemoryCaller::HostedAgent {
        scope_id: scope,
        run_id: RunId::new(),
    }
}

fn write(statement: &str, things: &[&str], links: &[(&str, &str, &str)]) -> RememberInput {
    RememberInput {
        statement: statement.to_owned(),
        about: None,
        fact_class: None,
        source: None,
        entities: things
            .iter()
            .map(|name| ThingInput {
                name: (*name).to_owned(),
                other_names: Vec::new(),
            })
            .collect(),
        links: links
            .iter()
            .map(|(from, kind, to)| LinkInput {
                from: (*from).to_owned(),
                kind: (*kind).to_owned(),
                to: (*to).to_owned(),
                ended: false,
                confidence_permille: None,
            })
            .collect(),
        made_from: Vec::new(),
    }
}

fn query(text: &str) -> RecallInput {
    RecallInput {
        query: Some(text.to_owned()),
        searches: None,
        folders: None,
        result_cap: 10,
        wider: WiderMode::Auto,
    }
}

impl Scaled {
    /// Waits until the rows are kept and their jobs are done.
    async fn settled(&self, id: MemoryFactId) {
        let pool = self.store.postgres().expect("PostgreSQL");
        let started = Instant::now();
        loop {
            let (state,): (String,) =
                sqlx::query_as("SELECT state FROM memory_fact_refs WHERE id = $1")
                    .bind(id.0)
                    .fetch_one(pool)
                    .await
                    .expect("the row");
            let (jobs,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM mem_work_job WHERE status IN ('pending', 'leased')",
            )
            .fetch_one(pool)
            .await
            .expect("the jobs");
            if state == "kept" && jobs == 0 {
                return;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "the row stayed {state}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

#[tokio::test]
async fn agent_profile_the_same_scenarios_on_postgresql_criterion_17() {
    let rig = scaled().await;
    let (s, other) = (ScopeId::new(), ScopeId::new());
    let place = MemoryPlace::Scope(s);

    // Criterion 1: a fact with things and a link, and the same call again is one write.
    let who = MemoryCaller::HostedAgent {
        scope_id: s,
        run_id: RunId::new(),
    };
    let link = [("Borealis", "supplies", "packaging")];
    let first = rig
        .svc
        .remember(
            who,
            write(
                "Borealis supplies packaging",
                &["Borealis", "packaging"],
                &link,
            ),
        )
        .await
        .expect("remember");
    rig.settled(first.id).await;
    let again = rig
        .svc
        .remember(
            who,
            write(
                "Borealis supplies packaging",
                &["Borealis", "packaging"],
                &link,
            ),
        )
        .await
        .expect("again");
    assert_eq!(again.id, first.id);
    assert_eq!(again.state, FactState::Kept);
    let things = rig
        .svc
        .list_things(place, Some("borealis".to_owned()), None)
        .await
        .expect("things");
    let borealis = things.items[0].id.clone();
    let detail = rig
        .svc
        .get_thing(place, borealis.clone())
        .await
        .expect("thing");
    assert_eq!(detail.links.len(), 1);
    assert_eq!(detail.facts.len(), 1);

    // Criterion 3: recall finds a thing the question names.
    let result = rig
        .svc
        .recall(hosted(s), query("who supplies packaging?"))
        .await
        .expect("recall");
    assert_eq!(result.facts[0].id, first.id);

    // Criterion 6, across two scopes.
    let other_fact = rig
        .svc
        .remember(
            hosted(other),
            write("Cardboard is ordered monthly", &["packaging"], &[]),
        )
        .await
        .expect("other");
    rig.settled(other_fact.id).await;
    let elsewhere = rig
        .svc
        .recall(hosted(other), query("who supplies packaging?"))
        .await
        .expect("recall");
    assert!(elsewhere.links.is_empty());
    assert!(!elsewhere.facts.iter().any(|f| f.id == first.id));

    // Criterion 2: a link no longer true closes and stays in history.
    let mut ended = write(
        "Borealis no longer supplies packaging",
        &["Borealis", "packaging"],
        &link,
    );
    ended.links[0].ended = true;
    let closing = rig.svc.remember(who, ended).await.expect("closing");
    rig.settled(closing.id).await;
    let history = rig.svc.fact_history(closing.id).await.expect("history");
    assert!(history.links.iter().any(|l| l.until.is_some()));

    // Criterion 4: one hop.
    let reached = rig
        .svc
        .remember(who, write("Boxes are stacked", &["packaging"], &[]))
        .await
        .expect("reached");
    rig.settled(reached.id).await;

    // The recall log, on the PostgreSQL column types.
    let used = rig
        .svc
        .memory_used(place, vec![result.recall_id])
        .await
        .expect("used");
    assert!(!used.facts.is_empty() || used.gone > 0);

    // Criterion 14: forget everything about a thing.
    let count = rig
        .svc
        .count_thing(place, borealis.clone())
        .await
        .expect("count");
    assert_eq!(count.facts, 2);
    assert_eq!(
        count,
        ThingCount {
            facts: 2,
            links: count.links,
            summaries: 0
        }
    );
    let receipt = rig
        .svc
        .forget_thing(place, borealis, count)
        .await
        .expect("forget");
    assert_eq!(receipt.facts_removed, 2);
    assert_eq!(receipt.proof_digest.len(), 64);

    // §11a and §11.
    assert!(rig.svc.count_scope(other).await.expect("count") >= 1);
    let erased = rig.svc.erase_scope(other).await.expect("erase");
    assert_eq!(erased.digest.len(), 64);
    assert_eq!(rig.svc.count_scope(other).await.expect("count"), 0);
    rig.shutdown.cancel();
}

#[tokio::test]
async fn a_knowledge_scope_works_on_postgresql() {
    let store = support::pg::migrated_database().await;
    let journal = Journal::start(
        store.clone(),
        JournalOptions {
            automatic_pruning: false,
            prune_interval: Duration::from_secs(86_400),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .expect("the journal");
    let k = ScopeId::new();
    let shutdown = CancellationToken::new();
    let mut config = support::config(ProgramKind::AgentScaled);
    config.knowledge_scope = Some(k);
    let svc = MemoryService::start_with(
        config,
        Arc::new(journal),
        shutdown.clone(),
        support::seams(),
    )
    .await
    .expect("the service starts");
    let rig = Scaled {
        svc,
        store,
        shutdown,
    };
    let fact = |statement: &str| KnowledgeFactInput {
        statement: statement.to_owned(),
        about: None,
        entities: vec![
            ThingInput {
                name: "Borealis".to_owned(),
                other_names: Vec::new(),
            },
            ThingInput {
                name: "Packaging".to_owned(),
                other_names: Vec::new(),
            },
        ],
        links: vec![LinkInput {
            from: "Borealis".to_owned(),
            kind: "supplies".to_owned(),
            to: "Packaging".to_owned(),
            ended: false,
            confidence_permille: None,
        }],
    };
    let document = |prefix: &str, version: &str| KnowledgeDocumentFacts {
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
        facts: vec![fact("Borealis supplies Packaging by the pallet.")],
    };
    let prefix = "git:acme/handbook_v1@main/terms.md";
    let receipt = rig
        .svc
        .index_knowledge_document(k, document(prefix, "a1b2c3d"))
        .await
        .expect("index");
    assert_eq!(receipt.written, 1);
    let pool = rig.store.postgres().expect("PostgreSQL");
    let started = Instant::now();
    loop {
        let (kept, jobs): (i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM memory_fact_refs WHERE state = 'kept'), \
             (SELECT COUNT(*) FROM mem_work_job WHERE status IN ('pending', 'leased'))",
        )
        .fetch_one(pool)
        .await
        .expect("the counts");
        if kept == 1 && jobs == 0 {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "never kept");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let place = MemoryPlace::Scope(k);
    let top = rig.svc.top_things(place, 10).await.expect("top");
    assert_eq!(top.len(), 2);
    assert_eq!(top[0].link_count, 1);
    let within = rig
        .svc
        .top_things_within(place, 10, vec!["git:acme/handbook_v1@main/".to_owned()])
        .await
        .expect("within");
    assert_eq!(within.len(), 2);
    assert_eq!((within[0].link_count, within[0].fact_count), (1, 1));
    let outside = rig
        .svc
        .top_things_within(place, 10, vec!["git:acme/handbookXv1".to_owned()])
        .await
        .expect("an escaped prefix");
    assert!(outside.is_empty());
    let result = rig
        .svc
        .recall(
            MemoryCaller::KnowledgeAgent { scope_id: k },
            RecallInput {
                result_cap: 50,
                wider: WiderMode::Never,
                ..query("Borealis pallet")
            },
        )
        .await
        .expect("recall");
    assert_eq!(result.facts.len(), 1);
    let replaced = rig
        .svc
        .index_knowledge_document(k, document(prefix, "9fceb02"))
        .await
        .expect("the next version");
    assert_eq!(replaced.retired, 1);
    let removed = rig
        .svc
        .remove_knowledge_document(k, &format!("{prefix}#"))
        .await
        .expect("remove");
    assert_eq!(removed, 1);
    rig.shutdown.cancel();
}

#[tokio::test]
async fn the_migrations_apply_on_postgresql_with_the_widened_commit_check() {
    let store = support::pg::migrated_database().await;
    let pool = store.postgres().expect("PostgreSQL");
    let (definition,): (String,) = sqlx::query_as(
        "SELECT pg_get_constraintdef(oid) FROM pg_constraint \
         WHERE conname = 'memory_recall_wider_commit_id_check'",
    )
    .fetch_one(pool)
    .await
    .expect("the constraint");
    assert!(definition.contains(">= 1"), "{definition}");
    let (tables,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM information_schema.tables \
         WHERE table_name IN ('memory_fact_refs', 'memory_recalls', 'memory_recall_items', \
         'memory_made_from', 'memory_consolidations', 'memory_recall_wider', 'mem_fact')",
    )
    .fetch_one(pool)
    .await
    .expect("the tables");
    assert_eq!(tables, 7);
}

/// PostgreSQL holds every step under the number `migrations()` gives it (MEM-MIG-3), the same
/// numbers as SQLite.
// spec: ordin-desktop/components/MemoryService#MEM-MIG-3
#[tokio::test]
async fn postgresql_holds_the_fixed_step_numbers() {
    let store = support::pg::migrated_database().await;
    let pool = store.postgres().expect("PostgreSQL");
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT version, description FROM schema_migrations \
         WHERE module = 'memory_service' ORDER BY version",
    )
    .fetch_all(pool)
    .await
    .expect("the bookkeeping rows");
    let migrations = MemoryService::migrations();
    let numbered = |steps: &[ordin_journal::MigrationStep]| -> Vec<(i64, String)> {
        steps
            .iter()
            .map(|(n, d, _)| (*n, (*d).to_owned()))
            .collect()
    };
    assert_eq!(rows, numbered(migrations.postgres));
    assert_eq!(rows, numbered(migrations.sqlite));
    assert_eq!(rows.len(), 12);
}
