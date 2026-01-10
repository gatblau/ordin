//! C11 integration tests — the Tenancy Benchmark Harness's safety gates, against a real SurrealDB
//! server started via testcontainers (the `tests/store_remote.rs` precedent).
//!
//! These are the tests the harness exists to earn the right to be destructive. `memory-bench` issues
//! `REMOVE NAMESPACE`, which has no rollback, so the three guards below are the whole of what stands
//! between a mistyped invocation and a dropped production namespace. They are exercised against a real
//! engine rather than an in-memory one on purpose: the pre-flight guard reads `INFO FOR ROOT`, and a
//! guard tested only against a fake root is a guard tested against nothing.
//!
//! Docker may be absent in CI; each test prints a notice and skips gracefully rather than failing.

use memory::bench::corpus;
use memory::bench::coresidency;
use memory::bench::report::{
    BenchReport, CoResidencyLeg, HostDisk, LatencyLeg, RelevanceLeg, StrategyLabel,
};
use memory::types::placement::MemoryProfile;
use memory::bench::rig::{Rig, Timing};
use memory::bench::{BenchConfig, BenchError, BenchHarness, IsolationShape};
use surrealdb::opt::auth::Root;
use testcontainers::core::{ContainerPort, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

/// The pinned SurrealDB server image tag (3.x), matching the embedded `surrealdb` crate major.
const SURREALDB_IMAGE: &str = "surrealdb/surrealdb";
const SURREALDB_TAG: &str = "v3.1.5";

/// Start a root-credentialled in-memory SurrealDB server and return its `ws://` endpoint.
///
/// Returns `None` when Docker is unavailable, so callers skip rather than fail.
async fn start_server(test: &str) -> Option<(ContainerAsync<GenericImage>, String)> {
    let image = GenericImage::new(SURREALDB_IMAGE, SURREALDB_TAG)
        .with_exposed_port(ContainerPort::Tcp(8000))
        .with_wait_for(WaitFor::message_on_stdout("Started web server"))
        .with_cmd([
            "start",
            "--user",
            "root",
            "--pass",
            "root",
            "--bind",
            "0.0.0.0:8000",
            "memory",
        ]);
    let container = match image.start().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "SKIP {test}: could not start SurrealDB container ({e}); docker may be absent"
            );
            return None;
        }
    };
    let port = match container.get_host_port_ipv4(8000.tcp()).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("SKIP {test}: could not resolve host port ({e})");
            return None;
        }
    };
    // The server is started with root credentials, so the harness's conditional signin (FU-019 shape)
    // needs the pair in the environment. Setting it here rather than per-test is what makes the
    // `refuses_on_prefix_collision` failure mode a real one: without a signin the harness cannot read
    // `INFO FOR ROOT` at all, and the guard under test would never get to run.
    std::env::set_var("MEMORY_STORE_REMOTE_USER", "root");
    std::env::set_var("MEMORY_STORE_REMOTE_PASS", "root");
    Some((container, format!("ws://127.0.0.1:{port}")))
}

fn cfg(store_url: &str) -> BenchConfig {
    BenchConfig {
        store_url: store_url.into(),
        populations: vec![4],
        tenant_prefix: "bench".into(),
        concurrency: 2,
        isolation: IsolationShape::Namespace,
        report_path: std::path::PathBuf::from("./bench-report.json"),
        sweep_samples: 2,
        retain: false,
        embed_dim: 8,
        warmup_reps: 1,
        measure_reps: 3,
        host_disk: "unspecified".into(),
        queries: 3,
        co_residents: vec![1],
        visible: 10,
        total: 40,
        fractions: vec![],
        sizes: vec![],
        rig_dir: std::path::PathBuf::from("/tmp/memory-bench-rig-unused"),
        stage1_k: 10,
    }
}

/// A unique scratch directory. `std::env::temp_dir()` plus pid and a counter, rather than adding a
/// `tempfile` dependency for one path.
fn scratch_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("memory-rig-{}-{name}-{n}", std::process::id()))
}

/// Connect a bare admin handle to the same server, for arranging and inspecting state outside the
/// harness — the tests must be able to see namespaces the harness deliberately refuses to touch.
async fn admin(endpoint: &str) -> surrealdb::Surreal<surrealdb::engine::any::Any> {
    let db = surrealdb::engine::any::connect(endpoint.to_string())
        .await
        .expect("admin connect");
    db.signin(Root {
        username: "root".into(),
        password: "root".into(),
    })
    .await
    .expect("admin signin");
    db
}

async fn namespaces(db: &surrealdb::Surreal<surrealdb::engine::any::Any>) -> Vec<String> {
    let mut resp = db.query("INFO FOR ROOT").await.expect("info for root");
    let info: Vec<serde_json::Value> = resp.take(0).expect("take info");
    info.first()
        .and_then(|v| v.get("namespaces"))
        .and_then(|v| v.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// SA-BENCH-ISOLATION-01. With no bench store URL — and a populated `MEMORY_STORE_PATH` sitting right
/// there to be inherited — the harness refuses and opens no connection at all. The refusal has to
/// come before the connection: a harness that connects first and refuses second is already attached
/// to the store it was supposed to stay away from.
#[tokio::test]
async fn refuses_without_store_url() {
    // The service's own store configuration, present and populated. Nothing may read it.
    std::env::set_var("MEMORY_STORE_PATH", "/tmp/memory-live-store");

    match BenchHarness::connect(cfg("")).await {
        Err(BenchError::StoreUrlUnset) => {}
        other => panic!("expected StoreUrlUnset, got {:?}", other.err()),
    }
    assert_eq!(
        BenchError::StoreUrlUnset.exit_code(),
        2,
        "a refusal must exit 2, distinguishably from a store failure"
    );

    std::env::remove_var("MEMORY_STORE_PATH");
}

/// SA-BENCH-ISOLATION-02. A pre-existing namespace matching the prefix aborts the run before anything
/// is created. This is the one route by which teardown destroys data it did not create even when every
/// other guard holds: `DEFINE NAMESPACE IF NOT EXISTS` adopts the colliding namespace, and teardown's
/// prefix check then passes on it.
#[tokio::test]
async fn refuses_on_prefix_collision() {
    let Some((_c, endpoint)) = start_server("refuses_on_prefix_collision").await else {
        return;
    };
    let db = admin(&endpoint).await;
    db.query("DEFINE NAMESPACE IF NOT EXISTS bench0000001")
        .await
        .expect("define colliding ns")
        .check()
        .expect("ns ok");

    let harness = BenchHarness::connect(cfg(&endpoint))
        .await
        .expect("connect to the disposable store");
    match harness.preflight().await {
        Err(BenchError::PrefixCollision { existing }) => {
            assert_eq!(existing, "bench0000001");
        }
        other => panic!("expected PrefixCollision, got {other:?}"),
    }

    // Refused means refused: the colliding namespace is still there and nothing new was created.
    let after = namespaces(&db).await;
    assert!(after.contains(&"bench0000001".to_string()));
    assert_eq!(
        after.iter().filter(|n| n.starts_with("bench")).count(),
        1,
        "the refused run must not have created a population"
    );
}

/// Teardown drops only the ids the run generated, never what it found. The `acme` namespace here
/// stands for a live tenant sharing a server with a benchmark: it is not prefixed, it is not in the
/// generated id list, and it must survive.
#[tokio::test]
async fn teardown_ignores_foreign_namespaces() {
    let Some((_c, endpoint)) = start_server("teardown_ignores_foreign_namespaces").await else {
        return;
    };
    let db = admin(&endpoint).await;

    let harness = BenchHarness::connect(cfg(&endpoint))
        .await
        .expect("connect to the disposable store");
    harness
        .preflight()
        .await
        .expect("no collision on a clean server");

    // Arrange the state a completed run would leave: the generated population, plus a foreign tenant
    // and a differently-prefixed one that the harness never minted.
    let generated = harness.generate_ids(4).expect("generate ids");
    for ns in generated
        .iter()
        .map(String::as_str)
        .chain(["acme", "other0000001"])
    {
        db.query(format!("DEFINE NAMESPACE IF NOT EXISTS {ns}"))
            .await
            .expect("define ns")
            .check()
            .expect("ns ok");
    }

    let dropped = harness.teardown(4).await.expect("teardown");
    assert_eq!(dropped, 4, "every generated id is dropped");

    let after = namespaces(&db).await;
    assert!(
        after.contains(&"acme".to_string()),
        "a foreign namespace must survive teardown"
    );
    assert!(
        after.contains(&"other0000001".to_string()),
        "a namespace outside the configured prefix must survive teardown"
    );
    for id in &generated {
        assert!(!after.contains(id), "{id} should have been dropped");
    }
}

/// `MEMORY_BENCH_RETAIN=true` skips teardown entirely, for an operator who wants the population for a
/// follow-up run and takes ownership of dropping it. The population is left exactly as it was.
#[tokio::test]
async fn retain_leaves_the_population_in_place() {
    let Some((_c, endpoint)) = start_server("retain_leaves_the_population_in_place").await else {
        return;
    };
    let db = admin(&endpoint).await;

    let mut c = cfg(&endpoint);
    c.retain = true;
    let harness = BenchHarness::connect(c).await.expect("connect");
    let generated = harness.generate_ids(3).expect("generate ids");
    for ns in &generated {
        db.query(format!("DEFINE NAMESPACE IF NOT EXISTS {ns}"))
            .await
            .expect("define ns")
            .check()
            .expect("ns ok");
    }

    assert_eq!(harness.teardown(3).await.expect("teardown"), 0);
    let after = namespaces(&db).await;
    for id in &generated {
        assert!(after.contains(id), "{id} must be retained");
    }
}

/// The rig must be a **real SurrealKV store on disk**, which is the whole difference between it and
/// the `mem://` engine FU-024 rejects. Two things prove it, and neither can be satisfied by an
/// in-memory engine: the directory exists on disk and holds the engine's `LOCK` file after a write,
/// and a second open of the same path is refused because the first holds an exclusive lock.
///
/// The plan proposed proving persistence by reopening the path in a second `Rig`. That is not
/// possible in one process: the embedded engine holds its exclusive lock for the process lifetime, so
/// the reopen fails on the lock rather than on the data, whatever the data says. Asserting on the
/// files and on the lock proves the same property — this is a real on-disk store — without depending
/// on a release that never comes.
#[tokio::test]
async fn rig_opens_a_surrealkv_store_and_round_trips() {
    let dir = scratch_dir("roundtrip");
    let f = memory::types::domain::Fact {
        id: "fact:rig1".into(),
        content: serde_json::json!({"subject": "s", "predicate": "p", "object": "o"}),
        entities: vec!["entity:e1".into()],
        source_id: None,
        memory_class: memory::types::domain::MemoryClass::Semantic,
        fact_class: None,
        visibility: memory::types::domain::Visibility::UserPrivate,
        owner: memory::types::scope::ScopeRef {
            tenant: "acme".into(),
            team: None,
            user: "u-me".into(),
        },
        valid_from: chrono::Utc::now(),
        valid_to: None,
        ingested_at: chrono::Utc::now(),
        confidence: 0.9,
        salience: 0.7,
        stability: 1.0,
        pii_review: false,
        supersedes: None,
        superseded_by: None,
        derived_from: vec![],
        last_recalled_at: None,
    };
    let ctx = memory::types::scope::ScopeContext {
        tenant: "acme".into(),
        teams: vec![],
        user: "u-me".into(),
        token_jti: "jti".into(),
        allowed_ops: memory::types::scope::OpSet {
            read: true,
            write: true,
            forget: true,
        },
        client: None,
        fact_authority: None,
        correlation_id: "c-rig".into(),
    };

    let rig = Rig::open_surrealkv(&dir, 8, memory::types::placement::MemoryProfile::Collective)
        .await
        .expect("open surrealkv rig");
    assert_eq!(rig.path(), dir.as_path());

    memory::types::ports::MemoryStore::put_fact(rig.store(), &f)
        .await
        .expect("put_fact");
    let got = memory::types::ports::MemoryStore::get_fact(rig.store(), &ctx, "fact:rig1")
        .await
        .expect("get_fact");
    assert!(got.is_some(), "the rig must round-trip a fact");

    // On disk, not in memory. `mem://` creates no directory and no lock file.
    assert!(dir.is_dir(), "a SurrealKV rig writes a real directory");
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("read the rig directory")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        entries.iter().any(|n| n == "LOCK"),
        "expected the engine's LOCK file in {entries:?}"
    );

    // The exclusive lock is the constraint every later leg has to respect: one rig per path, and
    // rigs opened sequentially rather than concurrently.
    let second =
        Rig::open_surrealkv(&dir, 8, memory::types::placement::MemoryProfile::Collective).await;
    assert!(
        second.is_err(),
        "a second rig over a held path must be refused, not silently share the store"
    );

    rig.discard().expect("discard the scratch directory");
    assert!(!dir.exists(), "discard removes the rig directory");
}

/// A bimodal workload reaches the report as a spread rather than being flattened into a median. The
/// 22x spread FU-024 names was invisible in the numbers that were reported; this instrument cannot
/// hide it.
#[tokio::test]
async fn timed_reports_spread_not_just_a_median() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let n = AtomicU32::new(0);
    let t = Timing::measure(2, 10, || async {
        if n.fetch_add(1, Ordering::Relaxed).is_multiple_of(4) {
            tokio::time::sleep(std::time::Duration::from_millis(12)).await;
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await;

    assert_eq!(t.warmup_reps, 2);
    assert_eq!(t.measure_reps, 10);
    assert!(
        t.min_max_ratio > 2.0,
        "a bimodal workload must reach the report as a spread, got {}",
        t.min_max_ratio
    );
    assert!(t.p99_millis >= t.p95_millis && t.p95_millis >= t.p50_millis);
}

// ---------------------------------------------------------------------------------------------
// Plan 06 Phase 2 — the shared corpus and the re-pointed report.
//
// These run against a real embedded SurrealKV store at a scratch path rather than a container:
// NFR-P3 is a claim about the in-process engine, and every later leg measures on this rig, so the
// corpus has to be shown correct on the rig that will carry the numbers.
// ---------------------------------------------------------------------------------------------

const CORPUS_DIM: u32 = 8;

/// A `u-me` recall through the public path, as a leg would issue it.
fn stage_one(embed_dim: u32, user: &str, tenant: &str, k: u16) -> memory::types::ports::StageOneQuery {
    memory::types::ports::StageOneQuery {
        query_vector: corpus::query_vector(embed_dim),
        // No keyword terms and no entity filter, so the vector signal is the only one contributing
        // and the returned set is the one the later legs will time.
        keyword_terms: vec![],
        filters: memory::types::ports::RecallFilters::default(),
        scope: corpus::reader_ctx(tenant, user),
        stage1_k: k,
    }
}

/// A benchmark whose corpus varies between runs cannot attribute a difference to the strategy — it
/// has measured two different data sets and called the difference a result.
///
/// Two builds of identical parameters into two separate stores must agree on their digest **and** on
/// what the store actually returns: the digest alone proves only that the generator is
/// deterministic, so the ranked `(fact_id, semantic_score)` list from a real recall on each store is
/// compared as well. That reads the stored embeddings back through the production path, which is
/// where a non-deterministic write would show up.
#[tokio::test]
async fn corpus_is_deterministic_across_builds() {
    let dir_a = scratch_dir("corpus-det-a");
    let dir_b = scratch_dir("corpus-det-b");
    // The **individual** profile, so `memory` runs the exact filter-first strategy. The HNSW graph is
    // not reproducible across two independent builds of identical data, so comparing through the
    // approximate path would test the index's determinism and not the corpus's — which is the thing
    // under test here.
    let profile = memory::types::placement::MemoryProfile::Individual;

    let rig_a = Rig::open_surrealkv(&dir_a, CORPUS_DIM, profile)
        .await
        .expect("open rig a");
    let rig_b = Rig::open_surrealkv(&dir_b, CORPUS_DIM, profile)
        .await
        .expect("open rig b");

    let a = corpus::build(&rig_a, "acme", 60, 20, 4)
        .await
        .expect("build a");
    let b = corpus::build(&rig_b, "acme", 60, 20, 4)
        .await
        .expect("build b");

    assert_eq!(a, b, "identical parameters must yield an identical corpus");
    assert_eq!(a.digest, b.digest);
    assert_eq!(a.co_resident_facts, 40);
    assert_eq!(a.visible_fraction(), 20.0 / 60.0);

    // A different corpus must not share a digest, or the field cannot tell two rows apart.
    let c = corpus::build(&rig_b, "acme", 60, 21, 4)
        .await
        .expect("build c");
    assert_ne!(a.digest, c.digest, "a different corpus must digest differently");

    // Read back through the production path: same facts, same order, same scores.
    let q = stage_one(CORPUS_DIM, corpus::OWNER, "acme", 10);
    let ctx = corpus::reader_ctx("acme", corpus::OWNER);
    let ranked = |cands: Vec<memory::types::ports::Candidate>| -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = cands
            .into_iter()
            .map(|c| (c.fact_id, c.semantic_score))
            .collect();
        v.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap().then_with(|| x.0.cmp(&y.0)));
        v
    };
    let ra = ranked(
        memory::types::ports::MemoryStore::recall(rig_a.store(), &ctx, &q)
            .await
            .expect("recall a"),
    );
    assert!(!ra.is_empty(), "the corpus must be recallable at all");

    // `rig_b` now holds corpus `c` over corpus `b`, so re-derive `b` on a third store to compare
    // like with like rather than against an overwritten one.
    let dir_c = scratch_dir("corpus-det-c");
    let rig_c = Rig::open_surrealkv(&dir_c, CORPUS_DIM, profile)
        .await
        .expect("open rig c");
    let b2 = corpus::build(&rig_c, "acme", 60, 20, 4)
        .await
        .expect("rebuild b");
    assert_eq!(a.digest, b2.digest);
    let rc = ranked(
        memory::types::ports::MemoryStore::recall(rig_c.store(), &ctx, &q)
            .await
            .expect("recall c"),
    );
    assert_eq!(
        ra, rc,
        "two builds of one corpus must return the same facts, in the same order, at the same scores"
    );

    rig_a.discard().ok();
    rig_b.discard().ok();
    rig_c.discard().ok();
}

/// The visible fraction has to be what the builder says it is, or every later leg is plotted against
/// an x-axis that does not mean what its label claims.
///
/// This is also the co-residency read-filter property (RISK-017) re-asserted on the rig: the measured
/// caller sees only their own facts, and the co-residents' facts are present in the namespace and
/// invisible to them. A leg cannot then silently measure a leaking filter and report it as relevance.
#[tokio::test]
async fn visible_fraction_is_what_the_builder_says_it_is() {
    const TOTAL: u32 = 2000;
    const VISIBLE: u32 = 1000;

    let dir = scratch_dir("visible-fraction");
    let rig = Rig::open_surrealkv(
        &dir,
        CORPUS_DIM,
        memory::types::placement::MemoryProfile::Collective,
    )
    .await
    .expect("open rig");

    let c = corpus::build(&rig, "acme", TOTAL, VISIBLE, 4)
        .await
        .expect("build the corpus");
    assert_eq!(c.visible, VISIBLE);
    assert_eq!(c.co_resident_facts, TOTAL - VISIBLE);
    assert_eq!(c.visible_fraction(), 0.5);

    // The measured caller sees their own facts and nothing else.
    let ctx = corpus::reader_ctx("acme", corpus::OWNER);
    let got = memory::types::ports::MemoryStore::recall(
        rig.store(),
        &ctx,
        &stage_one(CORPUS_DIM, corpus::OWNER, "acme", 50),
    )
    .await
    .expect("recall as the measured caller");
    assert!(!got.is_empty(), "the caller must see their own corpus");
    for cand in &got {
        assert_eq!(
            cand.fact.owner.user,
            corpus::OWNER,
            "{} leaked from {}",
            cand.fact_id,
            cand.fact.owner.user
        );
    }

    // Each co-resident's facts exist and are visible to that co-resident alone, so the invisible
    // remainder is genuinely in the namespace rather than simply absent.
    let users = corpus::co_resident_users(4);
    assert_eq!(users.len(), 4);
    for user in &users {
        let theirs = memory::types::ports::MemoryStore::recall(
            rig.store(),
            &corpus::reader_ctx("acme", user),
            &stage_one(CORPUS_DIM, user, "acme", 50),
        )
        .await
        .expect("recall as a co-resident");
        assert!(!theirs.is_empty(), "{user} owns part of the invisible remainder");
        for cand in &theirs {
            assert_eq!(&cand.fact.owner.user, user, "{} leaked to {user}", cand.fact_id);
        }
    }

    rig.discard().ok();
}

/// A number without its rig is not a measurement, it is a rumour. The report has to carry the engine
/// it ran on, that engine's own version, and every row's spread — and it must carry the store URL by
/// scheme only, because a remote URL can hold credentials in its authority.
#[tokio::test]
async fn report_records_the_rig_not_just_the_numbers() {
    // A run against a real embedded store, so `engine` and `engine_version` come from an engine
    // rather than from a constant.
    let dir = scratch_dir("report-rig");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("bench-report.json");

    let mut c = cfg(&format!("surrealkv://{}", dir.join("store").display()));
    c.report_path = path.clone();
    c.host_disk = "apfs on an internal ssd".into();
    // No leg enabled: an empty co-resident list and an empty fraction list skip both sweeps, which is
    // what makes this a test of the report *shell* rather than of a leg.
    c.co_residents = vec![];
    c.fractions = vec![];
    let harness = BenchHarness::connect(c).await.expect("connect the harness");

    let mut report = harness.run().await.expect("run writes a report");
    assert_eq!(report.engine, "surrealkv", "the engine is named, not inferred");
    assert_ne!(
        report.engine_version, "unknown",
        "an embedded engine answers its own version"
    );
    assert_eq!(report.store_url_scheme, "surrealkv");
    assert_eq!(report.host_disk.label, "apfs on an internal ssd");
    assert!(report.relevance.is_empty() && report.latency.is_empty() && report.co_residency.is_empty());
    assert!(report.ceiling.is_none());

    // A run with no leg enabled still writes a well-formed file naming the engine, not an empty one.
    let shell: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read the shell"))
            .expect("the shell parses");
    assert_eq!(shell["engine"], "surrealkv");
    assert_eq!(shell["relevance"].as_array().expect("relevance array").len(), 0);

    // Now with a row of each kind: every one carries its own spread.
    let timing = Timing {
        warmup_reps: 3,
        measure_reps: 9,
        p50_millis: 4.0,
        p95_millis: 7.0,
        p99_millis: 9.0,
        min_millis: 3.0,
        max_millis: 9.0,
        min_max_ratio: 3.0,
    };
    report.relevance.push(RelevanceLeg {
        co_residents: 10,
        visible: 1000,
        total: 2000,
        strategy: StrategyLabel::FilterFirstExact,
        queries: 20,
        overlap: 1.0,
        rank_correlation: 1.0,
        fill_ratio: 1.0,
        timing: timing.clone(),
        corpus_digest: "d".into(),
    });
    report.latency.push(LatencyLeg {
        visible_fraction: 0.5,
        visible: 1000,
        total: 2000,
        strategy: StrategyLabel::PostFilteredAnn,
        fill_ratio: 1.0,
        timing: timing.clone(),
        corpus_digest: "d".into(),
    });
    report.co_residency.push(CoResidencyLeg {
        namespace_total: 8000,
        visible: 1000,
        co_residents: 100,
        strategy: StrategyLabel::FilterFirstExact,
        fill_ratio: 1.0,
        timing,
        corpus_digest: "d".into(),
    });
    report.write(&path).expect("rewrite the report");

    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read the report"))
            .expect("the report parses");
    assert_eq!(v["engine"], "surrealkv");
    assert_eq!(v["engine_version"], report.engine_version.as_str());
    for leg in ["relevance", "latency", "co_residency"] {
        for row in v[leg].as_array().expect("leg rows") {
            assert!(
                row["timing"]["min_max_ratio"].as_f64().is_some(),
                "{leg} row carries no spread: {row}"
            );
            assert!(row["strategy"].is_string(), "{leg} row names no strategy");
            // No verdict field anywhere: whether a figure is acceptable is a user decision, and a
            // boolean in the report would hide it behind a number.
            for banned in ["pass", "passed", "ok", "verdict", "within_nfr"] {
                assert!(row.get(banned).is_none(), "{leg} row carries a verdict: {banned}");
            }
        }
    }

    // Scheme only, nowhere the full URL. The path this rig lives at is part of the store URL, so its
    // absence is what proves the rule holds.
    let raw = std::fs::read_to_string(&path).expect("read raw");
    assert!(
        !raw.contains("surrealkv://"),
        "the report must never carry the full store URL"
    );
    assert_eq!(v["store_url_scheme"], "surrealkv");

    let back: BenchReport = serde_json::from_str(&raw).expect("round trip");
    assert_eq!(back, report);
    assert_eq!(back.host_disk, HostDisk::describe("apfs on an internal ssd"));

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------------------------
// Plan 06 Phase 3 — the relevance leg (RFC 03 acceptance criterion 3).
//
// The leg opens its rig under `MemoryProfile::Individual`, which pairs the narrow owner read-filter
// with the `fact_owner_user` index hint. That is the filter-first arm RFC 03 proposes, and it puts
// the ANN arm behind the same filter so the two differ in strategy alone.
// ---------------------------------------------------------------------------------------------

/// A CI-sized config for the relevance leg. Small enough that a corpus rebuild per co-resident point
/// stays inside a test's patience, large enough that the two strategies can disagree.
fn relevance_cfg(rig_dir: &std::path::Path, report_path: &std::path::Path) -> BenchConfig {
    let mut c = cfg("mem://");
    c.report_path = report_path.to_path_buf();
    c.rig_dir = rig_dir.to_path_buf();
    c.embed_dim = CORPUS_DIM;
    c.queries = 3;
    c.co_residents = vec![1, 4];
    c.fractions = vec![];
    c.visible = 20;
    c.total = 100;
    c.stage1_k = 10;
    c.warmup_reps = 1;
    c.measure_reps = 1;
    c
}

/// Both strategies must be measured against **one** store holding **one** corpus, or the comparison
/// measures the data instead of the strategy. The `corpus_digest` on every row is the evidence, and
/// each co-resident point must appear exactly once per strategy.
#[tokio::test]
async fn relevance_leg_runs_both_strategies_on_one_corpus() {
    let dir = scratch_dir("relevance-one-corpus");
    let report_path = dir.with_extension("json");
    let c = relevance_cfg(&dir, &report_path);

    // A rig per point, as the leg itself does: reusing one store across points measures how many
    // rewrites the index has absorbed rather than the co-resident count.
    let mut rows = Vec::new();
    for n in [1u32, 4] {
        let point_dir = dir.join(format!("coresidents-{n:07}"));
        let rig = Rig::open_surrealkv(&point_dir, CORPUS_DIM, MemoryProfile::Individual)
            .await
            .expect("open rig");
        rows.extend(
            memory::bench::relevance::measure_point(&rig, &c, "acme", c.total, c.visible, n)
                .await
                .expect("measure a co-resident point"),
        );
        rig.discard().expect("discard the point's rig");
    }

    assert_eq!(rows.len(), 4, "two strategies at each of two points");
    for n in [1u32, 4] {
        let at: Vec<&RelevanceLeg> = rows.iter().filter(|r| r.co_residents == n).collect();
        assert_eq!(at.len(), 2, "co-resident point {n} must appear once per strategy");
        // One store, one corpus: the two strategies at a point share a digest.
        assert_eq!(
            at[0].corpus_digest, at[1].corpus_digest,
            "the two strategies at n={n} measured different corpora"
        );
        let labels: std::collections::BTreeSet<String> =
            at.iter().map(|r| r.strategy.as_str().to_string()).collect();
        assert_eq!(labels.len(), 2, "both strategies must be present at n={n}");
        for r in &at {
            assert_eq!(r.visible, c.visible);
            assert_eq!(r.total, c.total);
            assert_eq!(r.queries, c.queries);
            assert!(r.timing.measure_reps >= 1);
        }
    }

    // Different co-resident counts are different corpora — otherwise the sweep is not sweeping.
    let d1 = rows.iter().find(|r| r.co_residents == 1).unwrap();
    let d4 = rows.iter().find(|r| r.co_residents == 4).unwrap();
    assert_ne!(
        d1.corpus_digest, d4.corpus_digest,
        "the co-resident count must change the corpus"
    );

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&report_path).ok();
}

/// The test that proves the leg can see the defect it exists to look for. A leg that reports a fill
/// ratio of 1.0 everywhere has not been shown to measure anything.
///
/// At a deliberately low visible fraction the HNSW walk over-fetches a fixed multiple of `stage1_k`
/// and the scope filter then discards most of what it found, so the caller's rows are crowded out —
/// `RISK-013`'s failure mode. Filter-first filters before it caps and so cannot starve.
#[tokio::test]
async fn starvation_is_detected_when_it_is_present() {
    let dir = scratch_dir("relevance-starvation");
    let report_path = dir.with_extension("json");
    let rig = Rig::open_surrealkv(&dir, CORPUS_DIM, MemoryProfile::Individual)
        .await
        .expect("open rig");

    let mut c = relevance_cfg(&dir, &report_path);
    // 2% visible: the ANN's 4x over-fetch of stage1_k=10 walks 40 neighbours, of which under one is
    // the caller's on average. Filter-first still finds all ten it is asked for.
    c.visible = 20;
    c.total = 1000;
    c.stage1_k = 10;

    let rows = memory::bench::relevance::measure_point(&rig, &c, "acme", c.total, c.visible, 4)
        .await
        .expect("measure the starved point");

    let ann = rows
        .iter()
        .find(|r| r.strategy == StrategyLabel::PostFilteredAnn)
        .expect("an ANN row");
    let ff = rows
        .iter()
        .find(|r| r.strategy == StrategyLabel::FilterFirstExact)
        .expect("a filter-first row");

    assert_eq!(
        ff.fill_ratio, 1.0,
        "filter-first filters before it caps and cannot starve, got {}",
        ff.fill_ratio
    );
    assert!(
        ann.fill_ratio < 1.0,
        "the post-filtered arm must be seen to starve at a 2% visible fraction, got {}",
        ann.fill_ratio
    );
    // Relevance moves with it: the rows the index never surfaced are rows the caller did not get.
    assert!(
        ann.overlap < ff.overlap,
        "starvation must show up as lost relevance too: ann {} vs filter-first {}",
        ann.overlap,
        ff.overlap
    );
    assert_eq!(ff.overlap, 1.0, "the exact arm is the reference and is 1.0 by construction");

    rig.discard().ok();
    std::fs::remove_file(&report_path).ok();
}

/// The report carries the measures and no pass/fail field. Whether a relevance delta is acceptable is
/// a user decision, and a boolean would hide it behind a number.
///
/// Doubles as the phase's behavioural check: a CI-sized run at `MEMORY_BENCH_CORESIDENTS=1,4` writes
/// a report whose relevance rows carry a non-empty overlap measure for both strategies at both points.
#[tokio::test]
async fn relevance_leg_reports_without_a_verdict() {
    let dir = scratch_dir("relevance-no-verdict");
    let report_path = dir.with_extension("json");

    let mut c = relevance_cfg(&dir, &report_path);
    c.store_url = format!("surrealkv://{}", dir.with_extension("harness").display());
    let harness = BenchHarness::connect(c.clone()).await.expect("connect");
    let report = harness.run().await.expect("a run writes a report");

    assert_eq!(report.relevance.len(), 4, "two strategies at each of two points");
    for row in &report.relevance {
        assert!(
            row.overlap > 0.0,
            "{} at n={} carries no overlap measure",
            row.strategy,
            row.co_residents
        );
        assert!((0.0..=1.0).contains(&row.fill_ratio));
        assert!((-1.0..=1.0).contains(&row.rank_correlation));
    }

    // No verdict anywhere in the serialised leg — not on the row, not on the report.
    let raw = std::fs::read_to_string(&report_path).expect("read the report");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("the report parses");
    for row in v["relevance"].as_array().expect("relevance rows") {
        for banned in ["pass", "passed", "ok", "verdict", "starved", "within_nfr", "equivalent"] {
            assert!(
                row.get(banned).is_none(),
                "a relevance row carries a verdict field: {banned}"
            );
        }
        assert!(row["overlap"].as_f64().is_some());
        assert!(row["fill_ratio"].as_f64().is_some());
        assert!(row["timing"]["min_max_ratio"].as_f64().is_some());
    }
    assert!(v.get("relevance_passed").is_none());
    assert!(v.get("verdict").is_none());

    std::fs::remove_dir_all(dir.with_extension("harness")).ok();
    std::fs::remove_file(&report_path).ok();
}

// ---------------------------------------------------------------------------------------------
// Plan 06 Phase 4 — the absolute-latency leg (RFC 03 acceptance criterion 4).
// ---------------------------------------------------------------------------------------------

/// A CI-sized config for the latency leg.
fn latency_cfg(rig_dir: &std::path::Path, report_path: &std::path::Path) -> BenchConfig {
    let mut c = cfg("mem://");
    c.report_path = report_path.to_path_buf();
    c.rig_dir = rig_dir.to_path_buf();
    c.embed_dim = CORPUS_DIM;
    c.queries = 3;
    c.co_residents = vec![];
    c.fractions = vec![1.0, 0.25];
    c.visible = 40;
    c.stage1_k = 10;
    c.warmup_reps = 2;
    c.measure_reps = 2;
    c
}

/// Warm-up has to be *doing* something, not merely being recorded as a number: the first query
/// against a cold index pays for loading it, and folding that into a percentile is what produced the
/// 22x spread FU-024 was raised on.
///
/// Two halves, because the property and the plan's proposed method came apart. The **deterministic**
/// half proves the mechanism: a workload whose first calls are slow and whose later ones are fast,
/// measured with and without warm-up, must show the slow calls inside the reduction in one case and
/// outside it in the other. The **real-rig** half proves the leg carries the configured value onto
/// every row rather than a default.
///
/// The plan asked for the mechanism to be shown on a real store by comparing spreads. It cannot be:
/// building the corpus writes every embedding through the index immediately before the queries run,
/// so by the time the first query is issued the index is already warm and there is no cold-start cost
/// left to observe. Measured at a 40-fact corpus the unwarmed spread came out *below* the warmed one
/// (1.32 against 1.61) — noise, in both directions. A test that can fail either way has not shown
/// anything.
#[tokio::test]
async fn latency_leg_warms_before_it_measures() {
    // The mechanism, on a workload whose cold cost is known rather than hoped for.
    let slow_first = |warmup: u32| async move {
        let n = std::sync::atomic::AtomicU32::new(0);
        Timing::measure(warmup, 8, || async {
            let i = n.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let millis = if i < 4 { 20 } else { 1 };
            tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        })
        .await
    };
    let unwarmed = slow_first(0).await;
    let warmed = slow_first(4).await;

    assert_eq!(unwarmed.warmup_reps, 0);
    assert_eq!(warmed.warmup_reps, 4);
    assert_eq!(unwarmed.measure_reps, warmed.measure_reps, "same measured work");
    assert!(
        unwarmed.max_millis > 15.0,
        "an unwarmed reduction must contain the cold calls, max was {}",
        unwarmed.max_millis
    );
    assert!(
        warmed.max_millis < 15.0,
        "a warmed reduction must exclude the cold calls, max was {}",
        warmed.max_millis
    );
    assert!(
        unwarmed.min_max_ratio > warmed.min_max_ratio * 2.0,
        "the spread must differ with warm-up: unwarmed {} vs warmed {}",
        unwarmed.min_max_ratio,
        warmed.min_max_ratio
    );

    // The leg carries the configured value onto every row, so a reader knows what a percentile
    // excludes rather than assuming a default.
    let dir = scratch_dir("latency-warmup");
    let report_path = dir.with_extension("json");
    let base = latency_cfg(&dir, &report_path);
    for (i, warmup) in [0u32, 5].into_iter().enumerate() {
        let mut c = base.clone();
        c.warmup_reps = warmup;
        let point_dir = dir.join(format!("warmup-{i}"));
        let rig = Rig::open_surrealkv(&point_dir, CORPUS_DIM, MemoryProfile::Individual)
            .await
            .expect("open rig");
        let rows = memory::bench::latency::measure_point(&rig, &c, "acme", c.visible, 1.0)
            .await
            .expect("measure a fraction");
        rig.discard().expect("discard");
        assert_eq!(rows.len(), 2);
        for r in &rows {
            assert_eq!(
                r.timing.warmup_reps, warmup,
                "{} recorded {} warm-up reps, configured {warmup}",
                r.strategy, r.timing.warmup_reps
            );
            assert!(r.timing.measure_reps >= 1);
        }
    }

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&report_path).ok();
}

/// Criterion 4 asks for the crossover to be "measured and recorded". A grid of a handful of fractions
/// cannot locate it more finely than the two samples either side, so the report carries the bracketing
/// pair and the grid — and a run that never crossed carries `null` bounds with the grid intact, rather
/// than an invented point.
#[tokio::test]
async fn crossover_is_an_interval_and_names_its_grid() {
    let dir = scratch_dir("latency-crossover");
    let report_path = dir.with_extension("json");
    let mut c = latency_cfg(&dir, &report_path);
    c.store_url = format!("surrealkv://{}", dir.with_extension("harness").display());

    let harness = BenchHarness::connect(c.clone()).await.expect("connect");
    let report = harness.run().await.expect("a run writes a report");

    let x = report.crossover.as_ref().expect("the latency leg records a crossover");
    assert_eq!(
        x.grid.len(),
        c.fractions.len(),
        "the grid must name every fraction sampled, got {:?}",
        x.grid
    );
    assert!(
        x.grid.windows(2).all(|w| w[0] > w[1]),
        "the grid is recorded descending, got {:?}",
        x.grid
    );
    // Bounds are either both present (a crossing was bracketed) or both absent (none was found).
    // A single bound would be an interval with one end invented.
    assert_eq!(
        x.upper_fraction.is_some(),
        x.lower_fraction.is_some(),
        "a crossover must have both bounds or neither: {x:?}"
    );
    if x.transitions == 0 {
        assert!(x.upper_fraction.is_none(), "no crossing must record no bounds");
    } else {
        let (u, l) = (x.upper_fraction.unwrap(), x.lower_fraction.unwrap());
        assert!(u > l, "the bracket must be an interval, got {l}..{u}");
        assert!(x.grid.contains(&u) && x.grid.contains(&l), "bounds must be grid points");
    }

    // The grid survives serialisation with the bounds, whichever they are.
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).expect("read"))
            .expect("parse");
    assert_eq!(v["crossover"]["grid"].as_array().expect("grid").len(), c.fractions.len());
    assert!(v["crossover"]["transitions"].as_u64().is_some());

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(dir.with_extension("harness")).ok();
    std::fs::remove_file(&report_path).ok();
}

/// Every row reports the instrument's own instability beside its percentiles. A median-only report is
/// exactly what let a 22x spread go unnoticed.
///
/// Doubles as the phase's behavioural check: a CI-sized run reports the ANN arm's p95 at the pinned
/// visible set against a named engine, with the NFR-P3 threshold recorded as context and no verdict.
#[tokio::test]
async fn latency_rows_carry_the_spread() {
    let dir = scratch_dir("latency-spread");
    let report_path = dir.with_extension("json");
    let mut c = latency_cfg(&dir, &report_path);
    c.store_url = format!("surrealkv://{}", dir.with_extension("harness").display());
    c.fractions = vec![1.0, 0.5];

    let harness = BenchHarness::connect(c.clone()).await.expect("connect");
    let report = harness.run().await.expect("a run writes a report");

    assert_eq!(report.latency.len(), 4, "two strategies at each of two fractions");
    assert_eq!(report.engine, "surrealkv", "the engine is named");
    assert_ne!(report.engine_version, "unknown");
    assert_eq!(report.nfr_p3_millis, 50.0, "the threshold travels as context");

    for r in &report.latency {
        assert_eq!(r.visible, c.visible, "the visible set is pinned across fractions");
        assert!(
            r.timing.min_max_ratio >= 1.0,
            "{} at {} carries no spread",
            r.strategy,
            r.visible_fraction
        );
        assert!(
            r.timing.p99_millis >= r.timing.p95_millis
                && r.timing.p95_millis >= r.timing.p50_millis,
            "percentiles out of order at {}: p50 {} p95 {} p99 {}",
            r.visible_fraction,
            r.timing.p50_millis,
            r.timing.p95_millis,
            r.timing.p99_millis
        );
        assert!(r.timing.max_millis >= r.timing.min_millis);
        assert!(r.timing.measure_reps >= 1);
        assert!(
            (0.0..=1.0).contains(&r.fill_ratio),
            "{} at {} has no usable fill ratio: {}",
            r.strategy,
            r.visible_fraction,
            r.fill_ratio
        );
    }

    // A timing without a fill ratio can be read exactly backwards, so the ratio is on the row.
    for row in serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(&report_path).expect("read"),
    )
    .expect("parse")["latency"]
        .as_array()
        .expect("latency rows")
    {
        assert!(row["fill_ratio"].as_f64().is_some(), "a latency row carries no fill ratio");
    }

    // The behavioural check: the ANN arm's p95 at the pinned visible set, against a named engine.
    let ann = report
        .latency
        .iter()
        .find(|r| r.strategy == StrategyLabel::PostFilteredAnn)
        .expect("an ANN row");
    assert!(ann.timing.p95_millis > 0.0, "the ANN arm reports a p95");

    // Context, not verdict: the threshold is on the report and nothing says whether a row met it.
    let raw = std::fs::read_to_string(&report_path).expect("read the report");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("parse");
    assert_eq!(v["nfr_p3_millis"], 50.0);
    for row in v["latency"].as_array().expect("latency rows") {
        assert!(row["timing"]["min_max_ratio"].as_f64().is_some());
        for banned in ["pass", "passed", "ok", "verdict", "within_nfr", "meets_nfr_p3", "breach"] {
            assert!(row.get(banned).is_none(), "a latency row carries a verdict: {banned}");
        }
    }
    assert!(v.get("nfr_p3_met").is_none());
    assert!(v.get("verdict").is_none());

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(dir.with_extension("harness")).ok();
    std::fs::remove_file(&report_path).ok();
}

// ---------------------------------------------------------------------------------------------
// Plan 06 Phase 5 — the co-residency leg: per-user latency as the namespace grows underneath a
// pinned visible set.
//
// The distinction from the latency leg is what earns this leg its own phase. There the fraction
// moves and the namespace size follows from it, so the two are confounded; here only the namespace
// grows, which is what makes "does filter-first's cost track the visible set while the index walk's
// tracks the namespace" a question the rows can answer.
// ---------------------------------------------------------------------------------------------

/// A CI-sized config for the co-residency leg. The default `MEMORY_BENCH_SIZES` reaches 100,000
/// facts and is operator-only (FU-008): the corpus builder makes two round trips per fact, so the
/// list is where the size lives rather than the leg carrying a second, quieter definition of how big
/// it is willing to get.
fn coresidency_cfg(rig_dir: &std::path::Path, report_path: &std::path::Path) -> BenchConfig {
    let mut c = cfg("mem://");
    c.report_path = report_path.to_path_buf();
    c.rig_dir = rig_dir.to_path_buf();
    c.embed_dim = CORPUS_DIM;
    c.queries = 3;
    // The other two legs off, so what this run measures is this leg alone.
    c.co_residents = vec![];
    c.fractions = vec![];
    c.visible = 20;
    c.sizes = vec![40, 160];
    c.stage1_k = 10;
    c.warmup_reps = 1;
    c.measure_reps = 1;
    c
}

/// The invariant the whole leg is built on: the caller's own corpus is identical at every namespace
/// size, so a latency difference between two sizes cannot be attributed to it having grown.
///
/// Asserted on the rows rather than on the configuration, because it is `corpus::build` that decides
/// how many facts the caller actually ends up owning — it clamps `visible` to `total`, and a size
/// below the pinned visible set would silently shrink the caller's corpus and break the invariant
/// while every configuration value still read correctly.
///
/// The co-resident count is *not* held constant, and that is deliberate: it is derived from the size
/// so co-residents carry about as much memory as the caller, which is the shape a consumer
/// deployment has and the cardinality `fact_owner_user` is selective against.
#[tokio::test]
async fn co_residency_leg_holds_the_visible_set_constant() {
    let dir = scratch_dir("coresidency-pinned");
    let report_path = dir.with_extension("json");
    let mut c = coresidency_cfg(&dir, &report_path);
    c.store_url = format!("surrealkv://{}", dir.with_extension("harness").display());

    let harness = BenchHarness::connect(c.clone()).await.expect("connect");
    let report = harness.run().await.expect("a run writes a report");

    assert!(
        !report.co_residency.is_empty(),
        "the leg produced no rows at all"
    );
    for r in &report.co_residency {
        assert_eq!(
            r.visible, c.visible,
            "the visible set moved at namespace_total {}: {} against a pinned {}",
            r.namespace_total, r.visible, c.visible
        );
        assert!(
            r.namespace_total >= r.visible,
            "a namespace of {} cannot hold {} visible facts",
            r.namespace_total,
            r.visible
        );
    }

    // The namespace is what varies, and it varies over exactly the configured sizes.
    assert_eq!(coresidency::sizes_measured(&report.co_residency), c.sizes);

    // Co-residents grow with the namespace while the caller's set does not — the two axes this leg
    // exists to separate, visibly separated on the rows.
    let small = report
        .co_residency
        .iter()
        .find(|r| r.namespace_total == 40)
        .expect("a row at the smaller size");
    let large = report
        .co_residency
        .iter()
        .find(|r| r.namespace_total == 160)
        .expect("a row at the larger size");
    assert!(
        large.co_residents > small.co_residents,
        "co-residents must grow with the namespace: {} at 160 against {} at 40",
        large.co_residents,
        small.co_residents
    );
    assert_ne!(
        small.corpus_digest, large.corpus_digest,
        "two sizes measured the same corpus, so the sweep axis did not move"
    );

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(dir.with_extension("harness")).ok();
    std::fs::remove_file(&report_path).ok();
}

/// Every size carries a row for each strategy, and which one was faster at each is nameable from the
/// rows. A leg that measured one arm at a size has not compared anything there.
///
/// Doubles as the phase's behavioural check: a run at a CI-sized `sizes` list writes co-residency
/// rows for both strategies at both sizes, with every row's spread beside its percentiles and no
/// verdict field anywhere.
#[tokio::test]
async fn co_residency_leg_records_both_strategies_per_size() {
    let dir = scratch_dir("coresidency-strategies");
    let report_path = dir.with_extension("json");
    let mut c = coresidency_cfg(&dir, &report_path);
    c.store_url = format!("surrealkv://{}", dir.with_extension("harness").display());

    let harness = BenchHarness::connect(c.clone()).await.expect("connect");
    let report = harness.run().await.expect("a run writes a report");

    assert_eq!(
        report.co_residency.len(),
        c.sizes.len() * 2,
        "two strategies at each of {} sizes",
        c.sizes.len()
    );
    assert_eq!(report.engine, "surrealkv", "the engine is named");
    assert_ne!(report.engine_version, "unknown");

    for &size in &c.sizes {
        for strategy in [
            StrategyLabel::FilterFirstExact,
            StrategyLabel::PostFilteredAnn,
        ] {
            let rows: Vec<&CoResidencyLeg> = report
                .co_residency
                .iter()
                .filter(|r| r.namespace_total == size && r.strategy == strategy)
                .collect();
            assert_eq!(
                rows.len(),
                1,
                "expected exactly one {strategy} row at size {size}, got {}",
                rows.len()
            );
            let r = rows[0];
            assert!(
                r.timing.min_max_ratio >= 1.0,
                "{strategy} at {size} carries no spread"
            );
            assert!(
                r.timing.p99_millis >= r.timing.p95_millis
                    && r.timing.p95_millis >= r.timing.p50_millis,
                "percentiles out of order for {strategy} at {size}"
            );
            assert!(r.timing.measure_reps >= 1);
            // The visible fraction falls as the namespace grows, which is the regime in which the
            // index arm gets faster by returning almost nothing. Without this on the row, a reader
            // would take that for the scaling result the leg exists to produce (FU-012).
            assert!(
                (0.0..=1.0).contains(&r.fill_ratio),
                "{strategy} at {size} has no usable fill ratio: {}",
                r.fill_ratio
            );
        }
        // Both arms present at this size, so the faster one is nameable. It names the smaller
        // number; it does not judge it, which is why it is a function over the rows rather than a
        // field on them.
        let winner = coresidency::faster_at(&report.co_residency, size)
            .unwrap_or_else(|| panic!("no strategy named faster at size {size}"));
        assert!(matches!(
            winner,
            StrategyLabel::FilterFirstExact | StrategyLabel::PostFilteredAnn
        ));
    }

    // Both strategies at a size measured **one** corpus, or the comparison measured the data rather
    // than the strategy.
    for &size in &c.sizes {
        let digests: std::collections::BTreeSet<&str> = report
            .co_residency
            .iter()
            .filter(|r| r.namespace_total == size)
            .map(|r| r.corpus_digest.as_str())
            .collect();
        assert_eq!(
            digests.len(),
            1,
            "the two strategies at size {size} measured {} different corpora",
            digests.len()
        );
    }

    // The behavioural check reads the file, not the in-memory value: the rows have to survive to
    // disk, and no row may carry a verdict on what it measured.
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).expect("read the report"))
            .expect("parse the report");
    let rows = v["co_residency"].as_array().expect("co_residency rows");
    assert_eq!(rows.len(), c.sizes.len() * 2);
    for row in rows {
        assert!(row["timing"]["min_max_ratio"].as_f64().is_some());
        assert!(row["strategy"].is_string());
        assert!(
            row["fill_ratio"].as_f64().is_some(),
            "a co-residency row carries no fill ratio"
        );
        assert_eq!(row["visible"].as_u64(), Some(c.visible as u64));
        for banned in ["pass", "passed", "ok", "verdict", "within_nfr", "winner", "breach"] {
            assert!(
                row.get(banned).is_none(),
                "a co-residency row carries a verdict: {banned}"
            );
        }
    }

    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(dir.with_extension("harness")).ok();
    std::fs::remove_file(&report_path).ok();
}
