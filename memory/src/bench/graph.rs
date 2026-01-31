//! The graph leg (tenancy-benchmark amendment January 2026, ADR-021): what link expansion costs at
//! stage-1, on SurrealDB and on the SQL storage over SQLite and (amendment January 2026) PostgreSQL,
//! and whether it finds what it should — the "who supplies packaging?" fixture (criterion 3) and the
//! A–B–C–D hop-limit fixture (criterion 4).
//!
//! As the collection leg, the leg stays on C1 (`MemoryStore::recall`) with synthetic embeddings and
//! no model provider. **No verdict is written**: ordin reads `hops1` p95 against its budget and the
//! fixtures against `expected_found`, `forbidden_found` and `rank_of_expected ≤ 5`.

use std::time::Instant;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::bench::corpus::with_conflict_retry;
use crate::bench::rig::{Rig, Timing};
use crate::bench::{BenchConfig, BenchError};
use crate::types::domain::{Entity, Fact, MemoryClass, Relationship, Visibility};
use crate::types::placement::MemoryProfile;
use crate::types::ports::{LinkRole, RecallFilters, StageOneQuery, StoragePort, StoreError};
use crate::types::scope::{OpSet, ScopeContext, ScopeRef};

/// The leg's one team scope; every fact is team-shared within it (SA-BENCH-GRAPH-01).
const TEAM: &str = "ws:bench";
const USER: &str = "u-bench";
const MODEL_VERSION: &str = "bench-graph-v1";
/// Kinds a relationship takes one of (SA-BENCH-GRAPH-01).
const KINDS: usize = 40;
/// Queries of the timing run (SA-BENCH-GRAPH-01).
const QUERIES: usize = 200;
/// The Zipf exponent of entity choice (SA-BENCH-GRAPH-01).
const ZIPF_S: f64 = 1.1;

/// `BenchReport.graph`.
// Each fixture is a list, one row per backend.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct GraphLegReport {
    /// One per backend: "surrealdb", "sqlite", "postgres".
    pub backends: Vec<GraphBackendRow>,
    /// Criterion 3 shape, one row per backend.
    pub supplies_fixture: Vec<GraphFixtureRow>,
    /// Criterion 4 shape, one row per backend.
    pub hop_limit_fixture: Vec<GraphFixtureRow>,
    /// The write leg (SA-BENCH-TRIPLE-01), one row per backend; it runs after the graph steps.
    #[serde(default)]
    pub write_leg: Vec<WriteLegRow>,
}

/// One backend's write-leg figures (tenancy-benchmark amendment ADR-022). A backend that could not be
/// built carries `error` and no figures; the run continues with the other backends.
// spec: components/tenancy-benchmark#SA-BENCH-TRIPLE-01
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct WriteLegRow {
    pub backend: String,
    pub lookup_p95_ms: Option<f64>,
    pub record_p95_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct GraphBackendRow {
    pub backend: String,
    pub hops1: Timing,
    pub hops2: Timing,
    pub graph_off: Timing,
    /// Mean candidates with `graph_hop ≥ 1` at hops 1.
    pub reached_mean: f64,
    /// Not in the amendment's row: the corpus actually built (facts, relationships, entities).
    #[serde(default)]
    pub shape: Vec<u32>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct GraphFixtureRow {
    pub backend: String,
    pub expected_found: bool,
    pub forbidden_found: bool,
    pub rank_of_expected: Option<u32>,
}

/// Validate `MEMORY_BENCH_GRAPH_SHAPE`: three positive values.
pub(crate) fn validate_shape(shape: &[u32]) -> Result<(u32, u32, u32), BenchError> {
    match shape {
        [f, r, e] if *f > 0 && *r > 0 && *e > 1 => Ok((*f, *r, *e)),
        _ => Err(BenchError::FixtureUnreadable {
            path: "MEMORY_BENCH_GRAPH_SHAPE".into(),
            reason: "MEMORY_BENCH_GRAPH_SHAPE needs facts,relationships,entities".into(),
        }),
    }
}

/// The leg's writing and reading scope (public for the SQL dialect probe of `storage_conformance`).
pub fn writer(tenant: &str) -> ScopeContext {
    ScopeContext {
        tenant: tenant.to_string(),
        teams: vec![TEAM.to_string()],
        user: USER.to_string(),
        token_jti: "bench".into(),
        allowed_ops: OpSet {
            read: true,
            write: true,
            forget: false,
        },
        client: None,
        fact_authority: None,
        correlation_id: "bench-graph".into(),
    }
}

fn owner(tenant: &str) -> ScopeRef {
    ScopeRef {
        tenant: tenant.to_string(),
        team: Some(TEAM.to_string()),
        user: USER.to_string(),
    }
}

fn unit(rng: &mut StdRng, dim: u32) -> Vec<f32> {
    let mut v: Vec<f32> = (0..dim).map(|_| rng.gen_range(-1.0f32..1.0)).collect();
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    v.iter_mut().for_each(|x| *x /= n);
    v
}

/// A Zipf(s) sampler over `0..n` by inverse CDF.
struct Zipf {
    cdf: Vec<f64>,
}

impl Zipf {
    fn new(n: u32, s: f64) -> Self {
        let mut acc = 0.0;
        let mut cdf = Vec::with_capacity(n as usize);
        for k in 1..=n {
            acc += 1.0 / f64::from(k).powf(s);
            cdf.push(acc);
        }
        let total = acc;
        cdf.iter_mut().for_each(|c| *c /= total);
        Self { cdf }
    }

    fn sample(&self, rng: &mut StdRng) -> usize {
        let u: f64 = rng.gen();
        self.cdf.partition_point(|c| *c < u).min(self.cdf.len() - 1)
    }
}

fn entity_id(i: usize) -> String {
    format!(
        "entity:{}",
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, format!("g-ent-{i}").as_bytes())
    )
}

fn fact_id(tag: &str, i: usize) -> String {
    format!(
        "fact:{}",
        uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!("g-{tag}-{i}").as_bytes()
        )
    )
}

fn rel_id(tag: &str, i: usize) -> String {
    format!(
        "relationship:{}",
        uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!("g-{tag}-rel-{i}").as_bytes()
        )
    )
}

fn base_fact(tenant: &str, id: String, text: String, entities: Vec<String>) -> Fact {
    let now = chrono::Utc::now();
    Fact {
        id,
        content: serde_json::json!({ "statement": text }),
        entities,
        source_id: None,
        memory_class: MemoryClass::Semantic,
        fact_class: None,
        visibility: Visibility::TeamShared,
        owner: owner(tenant),
        valid_from: now,
        valid_to: None,
        ingested_at: now,
        confidence: 0.9,
        salience: 0.5,
        stability: 1.0,
        pii_review: false,
        supersedes: None,
        superseded_by: None,
        derived_from: vec![],
        last_recalled_at: None,
        collection: String::new(),
        single_valued: false,
        changed: false,
        triple_key: None,
    }
}

fn relationship(tenant: &str, id: String, from: &str, kind: &str, to: &str) -> Relationship {
    let now = chrono::Utc::now();
    Relationship {
        owner_only: false,
        author: None,
        id,
        kind: kind.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        valid_from: now,
        valid_to: None,
        ingested_at: now,
        confidence: 0.8,
        source_id: None,
        owner: owner(tenant),
        collection: String::new(),
        replaces: None,
    }
}

async fn put_fact(
    s: &dyn StoragePort,
    ctx: &ScopeContext,
    f: &Fact,
    v: &[f32],
) -> Result<(), StoreError> {
    with_conflict_retry(|| s.put_fact(f)).await?;
    with_conflict_retry(|| s.set_fact_embedding(ctx, &f.id, v, MODEL_VERSION)).await
}

async fn backfill(s: &dyn StoragePort, tenant: &str) -> Result<(), StoreError> {
    while s.backfill_entity_names(tenant, 500).await? > 0 {}
    Ok(())
}

/// SA-BENCH-GRAPH-01's corpus; returns the 200 queries (each naming 1–3 entities by name or alias).
pub async fn build_corpus(
    s: &dyn StoragePort,
    tenant: &str,
    dim: u32,
    (facts, rels, entities): (u32, u32, u32),
) -> Result<Vec<String>, StoreError> {
    let ctx = writer(tenant);
    let mut rng = StdRng::seed_from_u64(0x6a21);
    let zipf = Zipf::new(entities, ZIPF_S);
    let started = Instant::now();
    for i in 0..entities as usize {
        let e = Entity {
            private_aliases: vec![],
            alias_folders: vec![],
            id: entity_id(i),
            canonical_name: format!("ent{i}"),
            aliases: vec![format!("alias{i} co")],
            owner: owner(tenant),
            collection: String::new(),
        };
        with_conflict_retry(|| s.put_entity(&e)).await?;
    }
    // Facts naming 1–3 Zipf-chosen distinct entities.
    let mut named: Vec<Vec<usize>> = Vec::with_capacity(facts as usize);
    for i in 0..facts as usize {
        let want = rng.gen_range(1..=3);
        let mut ents: Vec<usize> = Vec::new();
        while ents.len() < want {
            let e = zipf.sample(&mut rng);
            if !ents.contains(&e) {
                ents.push(e);
            }
        }
        let f = base_fact(
            tenant,
            fact_id("c", i),
            format!(
                "note {i} records {}",
                ents.iter()
                    .map(|e| format!("ent{e}"))
                    .collect::<Vec<_>>()
                    .join(" with ")
            ),
            ents.iter().map(|e| entity_id(*e)).collect(),
        );
        put_fact(s, &ctx, &f, &unit(&mut rng, dim)).await?;
        named.push(ents);
        if i % 2000 == 1999 {
            tracing::info!(target: "memory-bench", facts = i + 1, secs = started.elapsed().as_secs(), "bench.graph.corpus");
        }
    }
    // Each relationship joins two distinct entities a fact names, and is asserted by that fact.
    let pairs: Vec<(usize, usize, usize)> = named
        .iter()
        .enumerate()
        .flat_map(|(f, ents)| {
            let mut out = Vec::new();
            for a in 0..ents.len() {
                for b in a + 1..ents.len() {
                    out.push((f, ents[a], ents[b]));
                }
            }
            out
        })
        .collect();
    if !pairs.is_empty() {
        for j in 0..rels as usize {
            let (f, a, b) = pairs[j % pairs.len()];
            let kind = format!("kind{}", (j / pairs.len() * 7 + j) % KINDS);
            let r = relationship(tenant, rel_id("c", j), &entity_id(a), &kind, &entity_id(b));
            let asserting = fact_id("c", f);
            with_conflict_retry(|| s.put_relationship(&r)).await?;
            with_conflict_retry(|| s.link_fact(&ctx, &r.id, &asserting, LinkRole::Asserted))
                .await?;
        }
    }
    backfill(s, tenant).await?;
    tracing::info!(target: "memory-bench", secs = started.elapsed().as_secs(), "bench.graph.corpus_built");
    let queries = (0..QUERIES)
        .map(|_| {
            let n = rng.gen_range(1..=3);
            let parts: Vec<String> = (0..n)
                .map(|_| {
                    let e = zipf.sample(&mut rng);
                    if rng.gen_bool(0.5) {
                        format!("ent{e}")
                    } else {
                        format!("alias{e} co")
                    }
                })
                .collect();
            format!("what do we know about {}", parts.join(" and "))
        })
        .collect();
    Ok(queries)
}

/// The stage-1 query of one timing sample.
pub fn query(
    ctx: &ScopeContext,
    text: &str,
    v: Vec<f32>,
    hops: u8,
    k: u16,
    graph: bool,
) -> StageOneQuery {
    StageOneQuery {
        query_vector: v,
        keyword_terms: crate::retrieval::keyword_terms(text),
        searches: vec![],
        filters: RecallFilters::default(),
        scope: ctx.clone(),
        stage1_k: k,
        graph_hops: hops,
        graph_fanout: 32,
        graph_cap: 200,
        phrase_texts: if graph {
            vec![text.to_string()]
        } else {
            vec![]
        },
    }
}

/// The query vectors of the timing run, one per query, always the same for the same count.
pub fn query_vectors(count: usize, dim: u32) -> Vec<Vec<f32>> {
    let mut rng = StdRng::seed_from_u64(0x9e11);
    (0..count).map(|_| unit(&mut rng, dim)).collect()
}

/// Step 2: time every query `reps` times after `warmup` at hops 1, 2, and off.
pub async fn timing(
    s: &dyn StoragePort,
    tenant: &str,
    queries: &[String],
    cfg: &BenchConfig,
    backend: &str,
    shape: (u32, u32, u32),
) -> Result<GraphBackendRow, StoreError> {
    let ctx = writer(tenant);
    let vectors = query_vectors(queries.len(), cfg.embed_dim);
    let k = cfg.stage1_k.max(1);
    let mut rows: Vec<Timing> = Vec::new();
    let mut reached_total = 0usize;
    for (hops, graph) in [(1u8, true), (2, true), (0, false)] {
        for (t, v) in queries.iter().zip(&vectors).take(cfg.warmup_reps as usize) {
            s.recall(&ctx, &query(&ctx, t, v.clone(), hops, k, graph))
                .await?;
        }
        let mut samples = Vec::with_capacity(queries.len() * cfg.measure_reps.max(1) as usize);
        for _ in 0..cfg.measure_reps.max(1) {
            for (t, v) in queries.iter().zip(&vectors) {
                let q = query(&ctx, t, v.clone(), hops, k, graph);
                let started = Instant::now();
                let got = s.recall(&ctx, &q).await?;
                samples.push(started.elapsed().as_secs_f64() * 1000.0);
                if hops == 1 {
                    reached_total += got.iter().filter(|c| c.graph_hop.unwrap_or(0) >= 1).count();
                }
            }
        }
        let t = Timing::from_samples(cfg.warmup_reps, samples);
        tracing::info!(target: "memory-bench", backend, hops, graph, p50 = t.p50_millis, p95 = t.p95_millis, "bench.graph.timing");
        rows.push(t);
    }
    let samples_hops1 = queries.len() * cfg.measure_reps.max(1) as usize;
    let graph_off = rows.pop().unwrap_or_default();
    let hops2 = rows.pop().unwrap_or_default();
    let hops1 = rows.pop().unwrap_or_default();
    Ok(GraphBackendRow {
        backend: backend.to_string(),
        hops1,
        hops2,
        graph_off,
        reached_mean: reached_total as f64 / samples_hops1.max(1) as f64,
        shape: vec![shape.0, shape.1, shape.2],
    })
}

/// Step 3: 1,000 facts, none containing "supplies"; F names Borealis and packaging and asserts the
/// link; "who supplies packaging?" with no entity filter.
async fn supplies_fixture(
    s: &dyn StoragePort,
    tenant: &str,
    cfg: &BenchConfig,
    backend: &str,
) -> Result<GraphFixtureRow, StoreError> {
    let ctx = writer(tenant);
    let mut rng = StdRng::seed_from_u64(0x5ab1);
    let (b, p) = (entity_id(900_001), entity_id(900_002));
    for (id, name) in [(&b, "Borealis"), (&p, "packaging")] {
        let e = Entity {
            private_aliases: vec![],
            alias_folders: vec![],
            id: id.clone(),
            canonical_name: name.into(),
            aliases: vec![],
            owner: owner(tenant),
            collection: String::new(),
        };
        s.put_entity(&e).await?;
    }
    for i in 0..999 {
        let other = entity_id(800_000 + i % 50);
        if i < 50 {
            s.put_entity(&Entity {
                private_aliases: vec![],
                alias_folders: vec![],
                id: other.clone(),
                canonical_name: format!("filler {i}"),
                aliases: vec![],
                owner: owner(tenant),
                collection: String::new(),
            })
            .await?;
        }
        let f = base_fact(
            tenant,
            fact_id("s", i),
            format!("an ordinary note number {i} about work"),
            vec![other],
        );
        put_fact(s, &ctx, &f, &unit(&mut rng, cfg.embed_dim)).await?;
    }
    let target = base_fact(
        tenant,
        fact_id("s", 999),
        "Borealis provides our cartons".into(),
        vec![b.clone(), p.clone()],
    );
    put_fact(s, &ctx, &target, &unit(&mut rng, cfg.embed_dim)).await?;
    let r = relationship(tenant, rel_id("s", 0), &b, "supplies", &p);
    s.put_relationship(&r).await?;
    s.link_fact(&ctx, &r.id, &target.id, LinkRole::Asserted)
        .await?;
    backfill(s, tenant).await?;
    let text = "who supplies packaging?";
    let got = s
        .recall(
            &ctx,
            &query(
                &ctx,
                text,
                unit(&mut rng, cfg.embed_dim),
                1,
                cfg.stage1_k.max(1),
                true,
            ),
        )
        .await?;
    let mut by_fused: Vec<(&String, f64)> =
        got.iter().map(|c| (&c.fact_id, c.fused_score)).collect();
    by_fused.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let rank = by_fused
        .iter()
        .position(|(id, _)| **id == target.id)
        .map(|p| p as u32 + 1);
    let row = GraphFixtureRow {
        backend: backend.into(),
        expected_found: rank.is_some(),
        forbidden_found: false,
        rank_of_expected: rank,
    };
    tracing::info!(target: "memory-bench", backend, found = row.expected_found, rank = ?row.rank_of_expected, "bench.graph.fixture");
    Ok(row)
}

/// Step 4: links A–B, B–C, C–D asserted by fAB, fBC, fCD; fB, fC, fD each naming one entity and
/// sharing no word with the query "A"; hops 1.
// With a vector every one of the seven facts is a stage-1 candidate by the vector list alone, which
// would make `forbidden_found` true whatever the graph did. The fixture query is graph-only (no vector,
// no keyword terms); the supplies fixture keeps the timing run's vector and terms.
async fn hop_limit_fixture(
    s: &dyn StoragePort,
    tenant: &str,
    backend: &str,
) -> Result<GraphFixtureRow, StoreError> {
    let ctx = writer(tenant);
    let names = ["A", "B", "C", "D"];
    let ids: Vec<String> = (0..4).map(|i| entity_id(700_000 + i)).collect();
    for (id, n) in ids.iter().zip(names) {
        s.put_entity(&Entity {
            private_aliases: vec![],
            alias_folders: vec![],
            id: id.clone(),
            canonical_name: n.into(),
            aliases: vec![],
            owner: owner(tenant),
            collection: String::new(),
        })
        .await?;
    }
    for i in 0..3 {
        let f = base_fact(
            tenant,
            fact_id("h-link", i),
            format!("link {i}"),
            vec![ids[i].clone(), ids[i + 1].clone()],
        );
        s.put_fact(&f).await?;
        let r = relationship(tenant, rel_id("h", i), &ids[i], "next", &ids[i + 1]);
        s.put_relationship(&r).await?;
        s.link_fact(&ctx, &r.id, &f.id, LinkRole::Asserted).await?;
    }
    let single: Vec<String> = (1..4).map(|i| fact_id("h-one", i)).collect();
    for i in 1..4 {
        let f = base_fact(
            tenant,
            single[i - 1].clone(),
            format!("thing number {i} matters"),
            vec![ids[i].clone()],
        );
        s.put_fact(&f).await?;
    }
    backfill(s, tenant).await?;
    let got = s
        .recall(&ctx, &query(&ctx, "A", vec![], 1, 50, true))
        .await?;
    let has = |id: &String| got.iter().any(|c| &c.fact_id == id);
    let row = GraphFixtureRow {
        backend: backend.into(),
        expected_found: has(&single[0]),
        forbidden_found: has(&single[1]) || has(&single[2]),
        rank_of_expected: None,
    };
    tracing::info!(target: "memory-bench", backend, found = row.expected_found, forbidden = row.forbidden_found, "bench.graph.fixture");
    Ok(row)
}

/// Which of a backend's three stores a step runs on.
#[derive(Clone, Copy)]
enum Step {
    Timing,
    Supplies,
    Hops,
    /// SA-BENCH-TRIPLE-01, after the graph steps.
    Write,
}

/// Every step, in the order a run takes them.
const STEPS: [Step; 4] = [Step::Timing, Step::Supplies, Step::Hops, Step::Write];

impl Step {
    fn dir(self) -> &'static str {
        match self {
            Step::Timing => "graph",
            Step::Supplies => "graph-supplies",
            Step::Hops => "graph-hops",
            Step::Write => "graph-write",
        }
    }
}

/// Run `step` on `s`, recording its row in `report`.
async fn step_on(
    s: &dyn StoragePort,
    step: Step,
    cfg: &BenchConfig,
    tenant: &str,
    name: &str,
    shape: (u32, u32, u32),
    report: &mut GraphLegReport,
) -> Result<(), BenchError> {
    match step {
        Step::Timing => {
            let queries = build_corpus(s, tenant, cfg.embed_dim, shape).await?;
            report
                .backends
                .push(timing(s, tenant, &queries, cfg, name, shape).await?);
        }
        Step::Supplies => report
            .supplies_fixture
            .push(supplies_fixture(s, tenant, cfg, name).await?),
        Step::Hops => report
            .hop_limit_fixture
            .push(hop_limit_fixture(s, tenant, name).await?),
        // A backend that cannot be built fails its row, and the run continues with the others.
        // spec: components/tenancy-benchmark#SA-BENCH-TRIPLE-01
        Step::Write => {
            let row = match write_leg(s, tenant).await {
                Ok((lookup, record)) => WriteLegRow {
                    backend: name.to_string(),
                    lookup_p95_ms: Some(lookup),
                    record_p95_ms: Some(record),
                    error: None,
                },
                Err(e) => {
                    tracing::warn!(target: "memory-bench", backend = name, error = %e, "bench.write.failed");
                    WriteLegRow {
                        backend: name.to_string(),
                        lookup_p95_ms: None,
                        record_p95_ms: None,
                        error: Some(BenchError::Store(e).to_string()),
                    }
                }
            };
            report.write_leg.push(row);
        }
    }
    Ok(())
}

/// Write-leg corpus bounds (SA-BENCH-TRIPLE-01).
const WRITE_FACTS: usize = 20_000;
const WRITE_SUBJECTS: usize = 2_000;
const WRITE_PREDICATES: usize = 30;
const WRITE_SINGLE_VALUED: usize = 10;
const WRITE_CALLS: usize = 1_000;

/// SA-BENCH-TRIPLE-01: 20,000 live triple facts in one scope over 2,000 subjects and 30 predicates (the
/// first 10 written single-valued), built through `put_fact` with `TripleKey`s; then 1,000 timed
/// `live_same_subject_predicate(.., 64)` calls for random (subject, single-valued predicate) pairs and
/// 1,000 timed `record_conflict` calls on live pairs. Returns the two p95s in milliseconds.
// spec: components/tenancy-benchmark#SA-BENCH-TRIPLE-01
pub async fn write_leg(s: &dyn StoragePort, tenant: &str) -> Result<(f64, f64), StoreError> {
    let ctx = writer(tenant);
    let mut rng = StdRng::seed_from_u64(0x7a22);
    for i in 0..WRITE_SUBJECTS {
        let e = Entity {
            private_aliases: vec![],
            alias_folders: vec![],
            id: entity_id(100_000 + i),
            canonical_name: format!("Subject {i}"),
            aliases: vec![],
            owner: owner(tenant),
            collection: String::new(),
        };
        with_conflict_retry(|| s.put_entity(&e)).await?;
    }
    let mut ids: Vec<String> = Vec::with_capacity(WRITE_FACTS);
    for i in 0..WRITE_FACTS {
        let subject = i % WRITE_SUBJECTS;
        let predicate = (i / WRITE_SUBJECTS) % WRITE_PREDICATES;
        let id = fact_id("write", i);
        let mut f = base_fact(
            tenant,
            id.clone(),
            String::new(),
            vec![entity_id(100_000 + subject)],
        );
        f.content = serde_json::json!({
            "subject": format!("Subject {subject}"),
            "predicate": format!("p{predicate}"),
            "object": {"kind": "text", "value": format!("v{i}")},
        });
        f.single_valued = predicate < WRITE_SINGLE_VALUED;
        f.triple_key = Some(crate::types::domain::TripleKey {
            subject_entity: entity_id(100_000 + subject),
            predicate: format!("p{predicate}"),
            object_kind: crate::types::domain::ObjectKind::Text,
            object_value: format!("v{i}"),
            object_unit: None,
            object_entity: None,
            author: None,
        });
        with_conflict_retry(|| s.put_fact(&f)).await?;
        ids.push(id);
    }
    let mut lookups = Vec::with_capacity(WRITE_CALLS);
    for _ in 0..WRITE_CALLS {
        let subject = entity_id(100_000 + rng.gen_range(0..WRITE_SUBJECTS));
        let predicate = format!("p{}", rng.gen_range(0..WRITE_SINGLE_VALUED));
        let started = Instant::now();
        s.live_same_subject_predicate(&ctx, &owner(tenant), &subject, &predicate, 64)
            .await?;
        lookups.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let mut records = Vec::with_capacity(WRITE_CALLS);
    let now = chrono::Utc::now();
    for _ in 0..WRITE_CALLS {
        let a = rng.gen_range(0..WRITE_FACTS);
        let b = (a + 1 + rng.gen_range(0..WRITE_FACTS - 1)) % WRITE_FACTS;
        let started = Instant::now();
        s.record_conflict(&ctx, &ids[a], &ids[b], now).await?;
        records.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let lookup = Timing::from_samples(0, lookups).p95_millis;
    let record = Timing::from_samples(0, records).p95_millis;
    tracing::info!(target: "memory-bench", lookup_p95_ms = lookup, record_p95_ms = record, "bench.write.timing");
    Ok((lookup, record))
}

/// A build without feature `sql` cannot measure `sqlite` or `postgres`: the refusal for the first
/// one `backends` names, in that order, or `None` when it names neither.
// spec: components/tenancy-benchmark#SA-BENCH-GRAPH-03
#[cfg_attr(feature = "sql", allow(dead_code))]
fn sql_backend_unavailable(backends: &[String]) -> Option<BenchError> {
    let wants = |b: &str| backends.iter().any(|x| x == b);
    let name = if wants("sqlite") {
        "SQLite"
    } else if wants("postgres") {
        "postgres"
    } else {
        return None;
    };
    Some(BenchError::FixtureUnreadable {
        path: "MEMORY_BENCH_GRAPH_BACKENDS".into(),
        reason: format!("graph leg needs the sql feature for the {name} backend"),
    })
}

/// Run the leg: SurrealDB rigs under `cfg.rig_dir/graph*`, SQLite files `cfg.rig_dir/graph*.sqlite`,
/// PostgreSQL databases `<tenant>_graph*` on the server `MEMORY_BENCH_GRAPH_PG_URL` names, each step on
/// a fresh store, discarded unless `MEMORY_BENCH_RETAIN`.
pub(crate) async fn run(cfg: &BenchConfig, tenant: &str) -> Result<GraphLegReport, BenchError> {
    let shape = validate_shape(&cfg.graph_shape)?;
    let mut report = GraphLegReport {
        backends: vec![],
        supplies_fixture: vec![],
        hop_limit_fixture: vec![],
        write_leg: vec![],
    };
    let wants = |b: &str| cfg.graph_backends.iter().any(|x| x == b);
    // Refused before any backend is measured: this build cannot measure an SQL backend.
    // spec: components/tenancy-benchmark#SA-BENCH-GRAPH-03
    #[cfg(not(feature = "sql"))]
    if let Some(e) = sql_backend_unavailable(&cfg.graph_backends) {
        tracing::error!(target: "memory-bench", "bench.graph.sql_unavailable");
        return Err(e);
    }
    // Refused before any backend is measured, so a long SurrealDB run is never thrown away.
    // spec: components/tenancy-benchmark#SA-BENCH-GRAPH-02
    #[cfg(feature = "sql")]
    let pg_server = if wants("postgres") {
        let server = pg::server(cfg)?;
        pg::preflight(&server, tenant).await?;
        Some(server)
    } else {
        None
    };
    for step in STEPS {
        if !wants("surrealdb") {
            break;
        }
        let rig = Rig::open_surrealkv(
            cfg.rig_dir.join(step.dir()),
            cfg.embed_dim,
            MemoryProfile::Collective,
        )
        .await?;
        let r = step_on(
            rig.store(),
            step,
            cfg,
            tenant,
            "surrealdb",
            shape,
            &mut report,
        )
        .await;
        if cfg.retain {
            tracing::info!(target: "memory-bench", "bench.rig.retained");
        } else if let Err(e) = rig.discard() {
            tracing::warn!(target: "memory-bench", error = %e, "bench.rig.discard_failed");
        }
        r?;
    }
    #[cfg(feature = "sql")]
    for step in STEPS {
        if !wants("sqlite") {
            break;
        }
        let path = cfg.rig_dir.join(format!("{}.sqlite", step.dir()));
        let pool = sqlite_pool(&path).await?;
        let store = crate::store::sql::SqlStorage::new(
            crate::store::sql::SqlPool::Sqlite(pool.clone()),
            cfg.embed_dim,
        );
        let r = step_on(&store, step, cfg, tenant, "sqlite", shape, &mut report).await;
        pool.close().await;
        if !cfg.retain {
            remove_sqlite(&path);
        }
        r?;
    }
    // spec: components/tenancy-benchmark#SA-BENCH-GRAPH-02
    #[cfg(feature = "sql")]
    if let Some(server) = &pg_server {
        for step in STEPS {
            let name = pg::database(tenant, step);
            pg::create_database(server, &name).await?;
            let r = async {
                let pool = pg::open(server, &name).await?;
                let store = crate::store::sql::SqlStorage::new(
                    crate::store::sql::SqlPool::Postgres(pool.clone()),
                    cfg.embed_dim,
                );
                let r = step_on(&store, step, cfg, tenant, "postgres", shape, &mut report).await;
                pool.close().await;
                r
            }
            .await;
            // Only a database this run created reaches the drop.
            if cfg.retain {
                tracing::info!(target: "memory-bench", "bench.rig.retained");
            } else if let Err(e) = pg::drop_database(server, &name).await {
                tracing::warn!(target: "memory-bench", error = %e, "bench.rig.discard_failed");
            }
            r?;
        }
    }
    #[cfg(not(feature = "sql"))]
    {
        // The SurrealDB rows are measured; the leg then refuses (exit 2) for the SQLite row.
        tracing::warn!(target: "memory-bench", rows = report.backends.len(), "bench.graph.sqlite_unavailable");
        return Err(BenchError::FixtureUnreadable {
            path: "MEMORY_BENCH_GRAPH_SHAPE".into(),
            reason: "graph leg needs the sql feature for the SQLite backend".into(),
        });
    }
    #[allow(unreachable_code)]
    Ok(report)
}

#[cfg(feature = "sql")]
fn remove_sqlite(path: &std::path::Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut p = path.to_path_buf().into_os_string();
        p.push(suffix);
        let _ = std::fs::remove_file(p);
    }
}

/// A fresh SQLite file migrated with `SqlStorage::migrations(SqlDialect::Sqlite)`.
#[cfg(feature = "sql")]
async fn sqlite_pool(path: &std::path::Path) -> Result<sqlx::SqlitePool, BenchError> {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::str::FromStr;
    let io = |e: sqlx::Error| BenchError::Store(StoreError::Unavailable(e.to_string()));
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    remove_sqlite(path);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .map_err(io)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(10))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .map_err(io)?;
    for (_, sql) in crate::store::sql::SqlStorage::migrations(crate::store::sql::SqlDialect::Sqlite)
    {
        sqlx::raw_sql(sqlx::AssertSqlSafe(*sql))
            .execute(&pool)
            .await
            .map_err(io)?;
    }
    Ok(pool)
}

/// The `postgres` backend's databases on the disposable server (SA-BENCH-GRAPH-02). The server URL
/// may carry a password, so no error or log line here repeats it.
#[cfg(feature = "sql")]
mod pg {
    use std::str::FromStr;

    use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
    use sqlx::{AssertSqlSafe, Connection, PgConnection};

    use super::Step;
    use crate::bench::{BenchConfig, BenchError};
    use crate::store::sql::{SqlDialect, SqlStorage};
    use crate::types::ports::StoreError;

    fn io(e: sqlx::Error) -> BenchError {
        BenchError::Store(StoreError::Unavailable(e.to_string()))
    }

    /// The server `MEMORY_BENCH_GRAPH_PG_URL` names; unset is a refusal (exit 2).
    pub(super) fn server(cfg: &BenchConfig) -> Result<PgConnectOptions, BenchError> {
        let url = cfg
            .graph_pg_url
            .as_deref()
            .ok_or_else(|| BenchError::FixtureUnreadable {
                path: "MEMORY_BENCH_GRAPH_PG_URL".into(),
                reason: "MEMORY_BENCH_GRAPH_PG_URL is unset; the postgres graph backend needs a \
                         disposable PostgreSQL server"
                    .into(),
            })?;
        // The parser's message may quote the URL; a fixed one is reported instead.
        PgConnectOptions::from_str(url).map_err(|_| {
            BenchError::Store(StoreError::Unavailable(
                "MEMORY_BENCH_GRAPH_PG_URL is not a PostgreSQL connection URL".into(),
            ))
        })
    }

    /// `<tenant>_<step dir>`: the tenant id passed `validate_tenant`, so the name holds only ASCII
    /// letters, digits, `_` and `-`, and is always double-quoted where it reaches SQL.
    pub(super) fn database(tenant: &str, step: Step) -> String {
        format!("{tenant}_{}", step.dir())
    }

    /// Refuse when any of the leg's databases already exists: the leg drops only what it created.
    pub(super) async fn preflight(
        server: &PgConnectOptions,
        tenant: &str,
    ) -> Result<(), BenchError> {
        let names: Vec<String> = super::STEPS
            .into_iter()
            .map(|s| database(tenant, s))
            .collect();
        let mut conn = PgConnection::connect_with(server).await.map_err(io)?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT datname FROM pg_database WHERE datname = ANY($1) ORDER BY datname LIMIT 1",
        )
        .bind(&names)
        .fetch_optional(&mut conn)
        .await
        .map_err(io)?;
        let _ = conn.close().await;
        match existing {
            Some(existing) => {
                tracing::error!(target: "memory-bench", "bench.refused.prefix_collision");
                Err(BenchError::PrefixCollision { existing })
            }
            None => Ok(()),
        }
    }

    async fn admin(server: &PgConnectOptions, statement: String) -> Result<(), BenchError> {
        let mut conn = PgConnection::connect_with(server).await.map_err(io)?;
        let r = sqlx::raw_sql(AssertSqlSafe(statement))
            .execute(&mut conn)
            .await
            .map_err(io);
        let _ = conn.close().await;
        r.map(|_| ())
    }

    pub(super) async fn create_database(
        server: &PgConnectOptions,
        name: &str,
    ) -> Result<(), BenchError> {
        admin(server, format!("CREATE DATABASE \"{name}\"")).await
    }

    pub(super) async fn drop_database(
        server: &PgConnectOptions,
        name: &str,
    ) -> Result<(), BenchError> {
        admin(
            server,
            format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"),
        )
        .await
    }

    /// A pool on the new database, migrated with `SqlStorage::migrations(SqlDialect::Postgres)`.
    pub(super) async fn open(server: &PgConnectOptions, name: &str) -> Result<PgPool, BenchError> {
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(server.clone().database(name))
            .await
            .map_err(io)?;
        for (_, sql) in SqlStorage::migrations(SqlDialect::Postgres) {
            sqlx::raw_sql(AssertSqlSafe(*sql))
                .execute(&pool)
                .await
                .map_err(io)?;
        }
        Ok(pool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shape_needs_three_positive_values() {
        assert!(validate_shape(&[10_000, 20_000, 2_000]).is_ok());
        for bad in [&[10_000, 20_000][..], &[0, 1, 2], &[]] {
            let e = validate_shape(bad).unwrap_err();
            assert!(e
                .to_string()
                .contains("MEMORY_BENCH_GRAPH_SHAPE needs facts,relationships,entities"));
            assert_eq!(e.exit_code(), 2);
        }
    }

    #[cfg(feature = "sql")]
    #[test]
    fn the_postgres_backend_needs_a_server_and_never_repeats_its_url() {
        let mut cfg = BenchConfig::from_env();
        cfg.graph_pg_url = None;
        let e = pg::server(&cfg).unwrap_err();
        assert!(e.to_string().contains(
            "MEMORY_BENCH_GRAPH_PG_URL is unset; the postgres graph backend needs a disposable PostgreSQL server"
        ));
        assert_eq!(e.exit_code(), 2);
        cfg.graph_pg_url = Some("postgres://bench:secret-word@host:not-a-port/db".into());
        let e = pg::server(&cfg).unwrap_err();
        assert!(!e.to_string().contains("secret-word"), "{e}");
        assert_eq!(e.exit_code(), 1);
        assert_eq!(
            pg::database("bench0000000", Step::Supplies),
            "bench0000000_graph-supplies"
        );
    }

    #[test]
    fn a_build_without_sql_refuses_an_sql_backend_before_measuring() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(sql_backend_unavailable(&names(&["surrealdb"])).is_none());
        assert!(sql_backend_unavailable(&[]).is_none());
        for (backends, expected) in [
            (&["surrealdb", "postgres"][..], "postgres"),
            (&["postgres"][..], "postgres"),
            (&["surrealdb", "sqlite"][..], "SQLite"),
            (&["postgres", "sqlite"][..], "SQLite"),
        ] {
            let e = sql_backend_unavailable(&names(backends)).expect("refused");
            assert_eq!(e.exit_code(), 2);
            assert!(
                e.to_string().contains(&format!(
                    "graph leg needs the sql feature for the {expected} backend"
                )),
                "{e}"
            );
        }
    }

    #[test]
    fn zipf_favours_the_first_entities() {
        let z = Zipf::new(2000, ZIPF_S);
        let mut rng = StdRng::seed_from_u64(1);
        let firsts = (0..10_000).filter(|_| z.sample(&mut rng) < 10).count();
        assert!(firsts > 2_000, "{firsts}");
    }
}
