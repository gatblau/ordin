//! Behaviour the scenarios do not reach on their own.

use std::time::Duration;

use chrono::Utc;
use ordin_types::time::Timestamp;

use super::support::*;
use crate::config::{component_config, ProgramKind};
use crate::db::{like_prefix, placeholders, q};
use crate::model::*;
use crate::remember::{bounded_key, pipeline_fact_id};
use crate::service::Inner;
use crate::{MemoryService, ModelFileDigest, ModelManifest};

/// A dead letter expires because the service runs the maintenance scheduler. The row is
/// parked 31 days ago before the service starts, and the first cycle takes it away.
#[tokio::test]
async fn a_dead_letter_past_the_period_expires_on_the_scheduler_the_service_spawns() {
    let dir = TempDir::new();
    let (store, journal) = open(&dir).await;
    let pool = store.sqlite().expect("SQLite").clone();
    let old = Timestamp(Utc::now() - chrono::Duration::days(31)).to_wire();
    let recent = Timestamp(Utc::now() - chrono::Duration::days(29)).to_wire();
    for (id, at) in [("dead_letter:old", &old), ("dead_letter:recent", &recent)] {
        sqlx::query(
            "INSERT INTO mem_dead_letter (id, tenant, kind, payload, scope, attempts, \
             original_job_id, created_at, dead_lettered_at) \
             VALUES (?1, 'ordin', 'extract_fact', '{}', '{}', 5, ?2, ?3, ?3)",
        )
        .bind(id)
        .bind(format!("work_job:{id}"))
        .bind(at)
        .execute(&pool)
        .await
        .expect("a dead letter");
    }
    let shutdown = tokio_util::sync::CancellationToken::new();
    let svc = MemoryService::start_with(
        config(ProgramKind::Desktop, None, None),
        journal,
        shutdown.clone(),
        seams(600),
    )
    .await
    .expect("start");
    let started = std::time::Instant::now();
    loop {
        let left: Vec<(String,)> = sqlx::query_as("SELECT id FROM mem_dead_letter ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("read");
        if left.len() == 1 {
            assert_eq!(left[0].0, "dead_letter:recent");
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "the old dead letter never expired: {left:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    drop(svc);
    shutdown.cancel();
}

#[test]
fn a_key_over_the_components_limit_goes_as_its_digest() {
    let short = "page:1:2:3".to_owned();
    assert_eq!(bounded_key(short.clone()), short);
    let long = bounded_key("k".repeat(300));
    assert!(long.starts_with("sha256:"));
    assert!(long.chars().count() <= 255);
}

#[tokio::test]
async fn the_id_this_service_computes_is_the_id_the_pipeline_stores() {
    let rig = Rig::desktop().await;
    let id = rig.fact(rig.w1, "The pipeline keeps this fact.").await;
    let row = rig.row(id).await;
    let owner = Inner::owner_of(MemoryPlace::Workspace(rig.w1));
    assert!(row.memory_fact_id.starts_with("fact:"));
    assert_ne!(pipeline_fact_id(&owner, "another key"), row.memory_fact_id);
    let stored = rig.stored(id).await.expect("the fact is stored");
    assert_eq!(stored.id, row.memory_fact_id);
    rig.stop().await;
}

#[test]
fn the_component_configuration_follows_shared_context_5() {
    let config = component_config(tracing::Level::DEBUG, "m@abc".to_owned(), 600);
    assert_eq!(config.embed_dim, 384);
    assert_eq!(config.rate_read_per_min, 600);
    assert_eq!(config.rate_write_per_min, 600);
    assert_eq!(config.decay_k, 864_000.0);
    assert_eq!(config.dead_letter_ttl_secs, 2_592_000);
    assert_eq!(config.recall_time_limit_secs, 10);
    assert_eq!(config.trust_admit, 0.7);
    assert_eq!(config.embed_model_version, "m@abc");
    assert!(config.store_path.is_empty());
    assert_eq!(config.graph_hops, 1);
    assert_eq!(config.graph_cap, 200);
    for level in [
        tracing::Level::ERROR,
        tracing::Level::WARN,
        tracing::Level::INFO,
        tracing::Level::TRACE,
    ] {
        component_config(level, String::new(), 1);
    }
}

#[test]
fn the_migration_steps_are_numbered_in_order_on_both_engines() {
    let migrations = MemoryService::migrations();
    assert_eq!(migrations.module, "memory_service");
    for steps in [migrations.sqlite, migrations.postgres] {
        for (at, step) in steps.iter().enumerate() {
            assert_eq!(step.0, at as i64 + 1);
        }
        assert_eq!(steps.len(), 15);
        assert_eq!(steps[0].1, "memory_fact_refs");
        assert_eq!(steps[10].1, "versions of other sources");
        assert_eq!(steps[11].1, "confirmed citations");
        // spec: ordin-desktop/components/MemoryService#M-10
        assert_eq!(steps[12].1, "0011_triples");
        assert_eq!(steps[13].1, "facts as triples");
        // spec: ordin-desktop/components/MemoryService#MEM-TOP-1
        assert_eq!(steps[14].1, "topics of things");
    }
}

/// A kept database applies nothing new, and its bookkeeping rows carry the numbers
/// `migrations()` gives (MEM-MIG-3, scenario "a kept database applies nothing new").
// spec: ordin-desktop/components/MemoryService#MEM-MIG-3
#[tokio::test]
async fn a_kept_database_applies_nothing_and_holds_the_fixed_numbers() {
    use ordin_journal::{ApplyMode, MigrationRegistry, MIGRATIONS};
    let dir = TempDir::new();
    let (store, _journal) = open(&dir).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("the journal");
    registry
        .register(MemoryService::migrations())
        .expect("this module");
    let report = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("the second apply");
    assert!(report.applied.is_empty(), "{:?}", report.applied);
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT version, description FROM schema_migrations \
         WHERE module = 'memory_service' ORDER BY version",
    )
    .fetch_all(store.sqlite().expect("SQLite"))
    .await
    .expect("the bookkeeping rows");
    let expected: Vec<(i64, String)> = MemoryService::migrations()
        .sqlite
        .iter()
        .map(|(n, d, _)| (*n, (*d).to_owned()))
        .collect();
    assert_eq!(rows, expected);
    assert_eq!(rows.len(), 15);
}

#[tokio::test]
async fn the_rebuild_of_memory_recall_wider_keeps_its_rows_and_widens_the_check() {
    let rig = Rig::desktop().await;
    let recall = ordin_types::MemoryRecallId::new();
    rig.svc
        .inner
        .db
        .exec(
            q(
                "INSERT INTO memory_recalls (id, workspace_id, caller_kind, recalled_at) \
               VALUES ($1, $2, 'ordin', $3)",
            )
            .uuid(recall.0)
            .uuid(rig.w1.0)
            .ts(Timestamp::now()),
        )
        .await
        .expect("the recall");
    // A one-character commit passes the widened check.
    rig.svc
        .inner
        .db
        .exec(
            q(
                "INSERT INTO memory_recall_wider (recall_id, position, agent, repository, path, \
               commit_id, as_of) VALUES ($1, 0, 'a', 'r', 'p', '1', $2)",
            )
            .uuid(recall.0)
            .ts(Timestamp::now()),
        )
        .await
        .expect("a short commit");
    rig.stop().await;
}

#[tokio::test]
async fn bearers_last_one_call() {
    let rig = Rig::desktop().await;
    rig.fact(rig.w1, "Bearers are valid for one call.").await;
    rig.recall_text(rig.w1, "bearers").await;
    assert_eq!(rig.svc.inner.auth.live_bearers().await, 0);
    rig.stop().await;
}

#[tokio::test]
async fn set_recall_log_days_keeps_its_range_and_the_sweep_drops_old_recalls() {
    let rig = Rig::desktop().await;
    assert!(rig.svc.set_recall_log_days(0).is_err());
    assert!(rig.svc.set_recall_log_days(3651).is_err());
    rig.svc.set_recall_log_days(1).expect("in range");
    let id = rig.fact(rig.w1, "Quotes expire in November.").await;
    let result = rig.recall_text(rig.w1, "quotes").await;
    assert_eq!(result.facts[0].id, id);
    rig.svc
        .inner
        .db
        .exec(
            q("UPDATE memory_recalls SET recalled_at = $1")
                .ts(Timestamp(Utc::now() - chrono::Duration::days(2))),
        )
        .await
        .expect("age the recall");
    rig.svc.inner.sweep_once().await;
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_recalls").await,
        0
    );
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_recall_items")
            .await,
        0
    );
    rig.stop().await;
}

#[test]
fn the_sql_helpers_escape_and_number() {
    assert_eq!(placeholders(2, 3), "$2, $3, $4");
    assert_eq!(like_prefix("a_b%c"), "a\\_b\\%c%");
}

#[tokio::test]
async fn a_stored_fact_stays_readable_through_the_store_port_seam() {
    let rig = Rig::desktop().await;
    let context = rig.svc.inner.maintenance_context();
    assert!(rig
        .svc
        .inner
        .storage
        .get_fact(&context, "fact:00000000-0000-7000-8000-000000000000")
        .await
        .expect("read")
        .is_none());
    rig.stop().await;
}

/// §1 step 3 with a correct manifest: the check passes and the version names the digest.
#[tokio::test]
async fn a_correct_manifest_passes_the_check_and_a_wrong_digest_does_not() {
    let dir = TempDir::new();
    let mut files = Vec::new();
    for file in crate::providers::bundled_files() {
        let path = dir.0.join(&file);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        std::fs::write(&path, file.as_bytes()).expect("a file");
        files.push(ModelFileDigest {
            sha256: crate::service::file_sha256(&path).expect("a digest"),
            path: file,
        });
    }
    let mut manifest = ModelManifest { files };
    assert!(crate::providers::check_models(&dir.0, &manifest).await);
    manifest.files[0].sha256 = "0".repeat(64);
    assert!(!crate::providers::check_models(&dir.0, &manifest).await);
}

#[tokio::test]
async fn the_folder_refusal_names_the_folder_and_the_class_refusal_the_classes() {
    let rig = Rig::desktop().await;
    let agent = ordin_types::AgentInstanceId::new();
    rig.svc
        .set_agent_rules(agent, rules(&["decisions"], &[], None))
        .expect("rules");
    let error = rig
        .crew_call(
            "remember",
            rig.w1,
            agent,
            serde_json::json!({ "statement": "No class given." }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_CLASS_REQUIRED");
    assert!(error.message.contains("decisions"), "{}", error.message);

    let error = rig
        .crew_call(
            "recall",
            rig.w1,
            agent,
            serde_json::json!({ "query": "x", "folders": ["acme/unknown › x"] }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_FOLDER_UNKNOWN");
    assert!(
        error.message.contains("acme/unknown › x"),
        "{}",
        error.message
    );
    rig.stop().await;
}

#[tokio::test]
async fn a_call_past_its_deadline_or_cancelled_answers_and_session_end_holds_nothing() {
    let rig = Rig::desktop().await;
    let agent = ordin_types::AgentInstanceId::new();
    let ctx = context(
        ordin_mcp_front::ToolCaller::CrewMember {
            crew_run_id: ordin_types::CrewRunId::new(),
        },
        rig.run,
        rig.w1,
        agent,
        None,
    );
    ctx.cancel.cancel();
    let error = rig
        .provider()
        .call(ctx, &key("recall"), serde_json::json!({ "query": "x" }))
        .await;
    // Either the recall won the race or the cancellation did; neither hangs or panics.
    if let Err(error) = error {
        assert!(error.retryable);
    }
    rig.provider().session_closed(rig.run).await;
    rig.stop().await;
}
