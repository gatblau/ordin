### SPEC: Tenancy Benchmark Harness
**File:** `src/bench` | **Package:** `memory::bench` | **Phase:** 7 | **Dependencies:** C1 (Memory Store)

> **Mode:** greenfield
> **derivedFromHld:** 0.12.0
> **Origin:** RFC 02-consumer-tenancy-profile, pass 1 (benchmark) — acceptance criteria 1, 2, 3, 7, 8, 11.

#### Purpose

The Tenancy Benchmark Harness measures what one tenant namespace costs and what a tenant sweep costs, at the tenant populations the consumer tenancy profile targets (10,000 and 50,000 namespaces, SA-TENPOP-01). It exists because every scale figure `memory` holds on record was measured *within* one namespace (FU-007: ANN p95 9.46 ms, stage-1 15.60 ms at 25k facts/namespace) and none across many namespaces, so OQ-VOLUMES cannot be pinned and the consumer profile cannot be sized before it is built. The harness answers four questions with numbers rather than argument: what a freshly-provisioned zero-fact namespace costs in objects, bytes and wall-clock (SA-FOOTPRINT-01); how the maintenance scheduler's per-tick cost decomposes at population (SA-SWEEP-COST-01); whether a namespace ceiling exists below 50,000 (OQ-NSCEILING); and whether namespace-per-tenant or database-per-tenant organises the same objects more cheaply (OQ-NS-VS-DB, the FU-006 mechanism). It is a measurement instrument, not a feature: it runs against a disposable store it provisions and drops itself, and it adds no code to any request path.

#### Approach

The harness is a **separate binary** (`memory-bench`) built from the same crate, driving C1's public `MemoryStore` trait and `Migrator` directly rather than through the HTTP or MCP edges (SA-BENCH-01). Driving the public trait keeps the measured figure equal to what the shipped code pays: the harness calls the same `Migrator::migrate_up` that `ensure_and_use` calls on every store operation, so the numbers transfer without a correction factor. Two alternatives were rejected: a Criterion micro-benchmark inside the existing test suite (rejected — provisioning 50,000 namespaces takes minutes to hours and drops a multi-gigabyte store, which no unit-test harness should do on every `cargo test`); and a subcommand on the `memory` binary (rejected — it would put population-scale DDL behind the same binary an operator runs in production, and would grow the shipped binary that NFR-MA3 keeps single, contradicting SA-BENCH-01). Timing is collected as raw per-operation samples and reduced to percentiles once at report time, so a slow tail is visible rather than averaged away — the FU-007 spike already saw p99 outliers of 78–135 ms behind a healthy p95, and an aggregate mean would have hidden them.

#### Shared Context

Types, traits, config keys, and env vars this spec depends on, duplicated from Phase 2C / 2D so this section is implementable alone.

##### Scope types (from 2C.3)

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ScopeRef { pub tenant: String, pub team: Option<String>, pub user: String }

#[derive(Clone)]
pub struct ScopeContext {
    pub tenant: String,
    pub teams: Vec<String>,
    pub user: String,
    pub token_jti: String,
    pub allowed_ops: OpSet,
    pub correlation_id: String,
}

#[derive(Clone, Copy)]
pub struct OpSet { pub read: bool, pub write: bool, pub forget: bool }
```

The harness constructs the **maintenance `ScopeContext`** shape when it replays a scheduler tick, identically to C7: `teams = []`, `user = ""`, `token_jti = "maintenance"`, `allowed_ops = { read: true, write: true, forget: true }`, fresh `correlation_id`. C1 recognises this shape via `is_maintenance_scope(ctx)` and widens `fact_read_filter(ctx)` to `"true"`. The harness relies on that behaviour only to make its replayed probe cost the same as the real one; it reads no fact content.

##### Trait surface consumed (from 2C.6, owned by C1) — exact signatures

```rust
#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn ensure_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>;
    // Refuses unless the namespace holds exactly one tenant (ADR-018); the harness provisions one
    // tenant per namespace, so teardown's drop is permitted.
    async fn drop_tenant_namespace(&self, tenant: &str) -> Result<(), StoreError>;
    async fn list_tenants(&self) -> Result<Vec<String>, StoreError>;
    async fn scan_recent_episodes(&self, ctx: &ScopeContext, since: DateTime<Utc>, limit: u32)
        -> Result<Vec<Fact>, StoreError>;
    // ...full surface owned by the C1 spec; the four above are all C11 calls.
}
```

`StoreError` is owned by the C1 spec and referenced here by name. Its variants used by this component are `StoreError::Timeout`, `StoreError::Unavailable`, `StoreError::Validation`, and `StoreError::Internal`.

##### Migrator surface consumed (owned by C1)

```rust
impl<'a> Migrator<'a> {
    pub fn new(db: &'a Surreal<Any>, embed_dim: u32) -> Self;
    pub async fn current_version(&self, tenant: &str) -> Result<u32, StoreError>;
    pub async fn migrate_up(&self, tenant: &str) -> Result<u32, StoreError>;
}

/// The database name used inside every tenant namespace (ADR-011).
pub const DB_NAME: &str = "recall";
```

`validate_tenant(tenant) -> Result<(), StoreError>` is the single gate every tenant identifier passes before interpolation into DDL (sql-safety layer rule). The harness generates identifiers and must call it explicitly; it does not rely on a caller having done so.

##### Config & env vars (from 2D, owner C11)

| Variable | Type | Default | Required | Description |
|---|---|---|---|---|
| `MEMORY_BENCH_STORE_URL` | url | _(unset)_ | **yes** | Endpoint of the disposable store. The binary exits non-zero before opening any connection when unset (SA-BENCH-ISOLATION-01). |
| `MEMORY_BENCH_POPULATION` | u32 list | `10000,50000` | no | Comma-separated tenant populations, ascending. |
| `MEMORY_BENCH_TENANT_PREFIX` | string | `bench` | no | Prefix for generated tenant ids. |
| `MEMORY_BENCH_CONCURRENCY` | u32 | `16` | no | Maximum concurrent provisioning tasks. |
| `MEMORY_BENCH_ISOLATION` | enum | `namespace` | no | `namespace` or `database` (OQ-NS-VS-DB). |
| `MEMORY_BENCH_REPORT_PATH` | path | `./bench-report.json` | no | Destination for the run report. |
| `MEMORY_BENCH_SWEEP_SAMPLES` | u32 | `10` | no | Consecutive simulated ticks timed per population. |
| `MEMORY_BENCH_RETAIN` | bool | `false` | no | Retain the population on exit instead of dropping it. |
| `MEMORY_EMBED_DIM` | u32 | `1024` | no | Embedding dimension substituted into the HNSW index DDL; the harness passes it to `Migrator::new` so the provisioned schema matches a real deployment (SA-EMBED-01). |
| `MEMORY_BENCH_WARMUP` | u32 | `3` | no | Repetitions run and discarded before a leg measures. The first query against a cold index pays for building it; folding that into a percentile is what produced the 22× spread. |
| `MEMORY_BENCH_REPS` | u32 | `9` | no | Repetitions each leg's percentiles are computed over. |
| `MEMORY_BENCH_HOST_DISK` | string | `unspecified` | no | The operator's description of the storage the rig runs on, copied verbatim into `host_disk.label`. Operator-supplied because a process cannot tell an NVMe device from a SATA SSD from a network volume, and an absolute latency figure is dominated by that difference (RISK-003); deriving a plausible value would put a fabricated fact beside a measured one. |
| `MEMORY_BENCH_FRACTIONS` | f64 list | `1.0,0.5,0.25,0.125,0.05,0.01` | no | Visible fractions the latency leg sweeps, descending. The namespace is sized to each with the visible set pinned, so the fraction is the only thing that moves. Entries at or below zero size no namespace and are dropped. |
| `MEMORY_BENCH_RIG_DIR` | path | `<temp>/memory-bench-rig` | no | Parent directory for the measurement rigs. Each sweep point gets its own child directory: SurrealKV holds an exclusive directory lock for the process lifetime, so rigs are opened sequentially and never share a path. A directory of its own rather than the path inside `MEMORY_BENCH_STORE_URL`, which the harness already holds open for its `INFO FOR ROOT` guard. |
| `MEMORY_BENCH_QUERIES` | u32 | `20` | no | Size of the fixed query set each leg averages its relevance measures over, and the number of distinct queries its timing samples cycle through. |
| `MEMORY_BENCH_CORESIDENTS` | u32 list | `1,10,100,1000` | no | Co-resident user counts the relevance leg sweeps, ascending. Unparseable entries are dropped rather than defaulting the whole list, so one typo costs a point and not the run's shape. |
| `MEMORY_BENCH_VISIBLE` | u32 | `1000` | no | Facts the measured caller owns — one consumer's memory. Pinned across every point of every leg, so a difference between points is never the caller's own corpus having grown. |
| `MEMORY_BENCH_TOTAL` | u32 | `20000` | no | Facts in the namespace for the relevance leg, held fixed while the co-resident count varies. That is what separates *how many users share the namespace* from *how large the namespace is*; the latter is the co-residency leg's variable. |
| `MEMORY_BENCH_SIZES` | u32 list | `2000,8000,20000,100000` | no | Namespace sizes the co-residency leg sweeps, ascending. Its own variable, and the one thing that separates it from the latency leg: there the *fraction* moves and the size follows from it, which conflates the cost of a larger visible set with the cost of a larger namespace. Here the visible set is pinned at `MEMORY_BENCH_VISIBLE` and only the namespace grows. The largest default size is operator-only — the corpus builder makes two round trips per fact, so a CI run sets a smaller list rather than the leg carrying a second, quieter definition of how big it will get. |
| `MEMORY_BENCH_STAGE1_K` | u16 | `50` | no | Candidates each stage-1 query asks for, and the ceiling in the fill ratio's denominator. Declared here rather than read from `MEMORY_STAGE1_K`: it carries the same default, but the harness reads no service configuration key that could tie it to a live deployment's settings (SA-BENCH-ISOLATION-01). |
| `MEMORY_LOG_LEVEL` | enum | `info` | no | Structured-log verbosity. |

**Sweep lists distinguish empty from unset.** `MEMORY_BENCH_CORESIDENTS`, `MEMORY_BENCH_FRACTIONS`, `MEMORY_BENCH_SIZES` and `MEMORY_BENCH_POPULATION` take their default only when the variable is **unset**; a variable that is set to an empty value yields an empty list and skips that leg. The two are different intentions, and folding them together runs a leg the operator asked to skip — against a 20,000-fact corpus, four times over, which is how it was found. Every other key keeps the "empty means unset" rule, where the distinction carries no such meaning.

#### Change note — plan 06, the legs are re-pointed

The three safety gates, the report's scheme-only URL rule and the ceiling handling are unchanged and
remain this spec's core. **The measured legs are not.** As originally specified this component
provisions 10,000 and 50,000 namespaces, which measures a topology RFC 03 moves away from
(`docs/wip/rfc/03-scalability-first-architecture.md`). The legs that decide the design are instead:

1. **Relevance across co-resident users** — filter-first vs post-filtered on one corpus and a fixed
   query set, with starvation measured as a fill ratio, as co-resident user count rises 1 → 1,000
   (RFC 03 acceptance criterion 3).
2. **Absolute stage-1 vector latency across visible fractions** — against NFR-P3 (≤ 50 ms) at a
   1,000-fact visible set, with the crossover recorded as an interval and its sampling grid (RFC 03
   acceptance criterion 4).
3. **Per-user latency as co-residency rises** — visible set pinned, namespace grown underneath it.

Two consequences for this component. The measurement rig is an **embedded SurrealKV store at a real
path, not a container and not `mem://`**: NFR-P3 is a claim about the in-process embedded engine, and
both a network hop and an in-memory engine measure something else. And every timing carries
`min_max_ratio` alongside its percentiles, because the defect that forced the re-pointing was a 22×
spread between two runs of one configuration that a median-only report did not show.

The population, footprint, sweep and database-shape legs are withdrawn. `IsolationShape::Database`
stays declared — it is configuration, not a leg — but nothing measures it until FU-006 is reopened.
Their methods (`provision`, `footprint`, `sweep`) and their result types (`ProvisionOutcome`,
`PopulationResult`, `NamespaceFootprint`, `SweepMeasurement`, `LatencyMeasurement`) are removed below
rather than left standing: they were members and producers of a report shape that no longer exists,
and a spec carrying two report shapes at once describes neither. `CeilingHit` stays — an engine
refusal part-way through a population is the same event whichever leg was populating it.

**One engine constraint the legs must respect:** SurrealKV takes an exclusive directory lock and the
embedded engine holds it for the process lifetime, so each rig needs its own directory and rigs cannot
be opened concurrently over one path.

#### Public Interface

```rust
pub struct BenchHarness {
    db: Surreal<Any>,
    cfg: BenchConfig,
}

#[derive(Clone)]
pub struct BenchConfig {
    pub store_url: String,          // MEMORY_BENCH_STORE_URL
    pub populations: Vec<u32>,      // MEMORY_BENCH_POPULATION
    pub tenant_prefix: String,      // MEMORY_BENCH_TENANT_PREFIX
    pub concurrency: u32,           // MEMORY_BENCH_CONCURRENCY
    pub isolation: IsolationShape,  // MEMORY_BENCH_ISOLATION
    pub report_path: PathBuf,       // MEMORY_BENCH_REPORT_PATH
    pub sweep_samples: u32,         // MEMORY_BENCH_SWEEP_SAMPLES
    pub retain: bool,               // MEMORY_BENCH_RETAIN
    pub embed_dim: u32,             // MEMORY_EMBED_DIM
    pub warmup_reps: u32,           // MEMORY_BENCH_WARMUP
    pub measure_reps: u32,          // MEMORY_BENCH_REPS
    pub host_disk: String,          // MEMORY_BENCH_HOST_DISK
    pub rig_dir: PathBuf,           // MEMORY_BENCH_RIG_DIR
    pub queries: u32,               // MEMORY_BENCH_QUERIES
    pub co_residents: Vec<u32>,     // MEMORY_BENCH_CORESIDENTS
    pub visible: u32,               // MEMORY_BENCH_VISIBLE
    pub total: u32,                 // MEMORY_BENCH_TOTAL
    pub stage1_k: u16,              // MEMORY_BENCH_STAGE1_K
    pub fractions: Vec<f64>,        // MEMORY_BENCH_FRACTIONS
    pub sizes: Vec<u32>,            // MEMORY_BENCH_SIZES
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationShape { Namespace, Database }

impl BenchHarness {
    /// Open the disposable store named by `cfg.store_url`. Returns `BenchError::StoreUrlUnset`
    /// when the URL is empty — the connection is never opened in that case.
    pub async fn connect(cfg: BenchConfig) -> Result<Self, BenchError>;

    /// Full run: the relevance, visible-fraction latency and co-residency legs, each against an
    /// embedded SurrealKV rig holding one shared corpus. **Writes the report to `cfg.report_path`
    /// whether or not any leg produced a row** — a run that measured nothing and a run whose numbers
    /// never reached the disk are different events, and an absent file cannot tell them apart.
    pub async fn run(&self) -> Result<BenchReport, BenchError>;

    /// Drop every tenant this harness created. Never touches a tenant whose id lacks
    /// `cfg.tenant_prefix`.
    pub async fn teardown(&self, count: u32) -> Result<u32, BenchError>;
}

/// The measurement rig: a real embedded SurrealKV store at a directory of its own, plus the timing
/// discipline that makes an absolute reading mean something. Deliberately not containerised —
/// NFR-P3 is a claim about the in-process engine, and a network hop measures something else.
pub struct Rig { /* store, dir, embed_dim */ }

impl Rig {
    /// Open a SurrealKV store at `dir` under an explicit profile. One rig per directory for the
    /// life of the process: the engine holds an exclusive directory lock and dropping the `Rig`
    /// does not release it, so a multi-size leg allocates a directory per rig and opens them
    /// sequentially.
    pub async fn open_surrealkv(dir: impl AsRef<Path>, embed_dim: u32, profile: MemoryProfile)
        -> Result<Self, BenchError>;
    pub fn store(&self) -> &Store;
    pub fn path(&self) -> &Path;
    pub fn embed_dim(&self) -> u32;
    /// Remove the rig's directory. Explicit rather than a `Drop` impl, so it cannot fire on a
    /// control-flow edge nobody wrote, and **refuses any directory that does not hold the engine's
    /// `LOCK` file** — the same discipline `teardown` applies to a tenant id, applied to a filesystem
    /// path, now that the path comes from configuration rather than being allocated by the caller
    /// (RISK-005). A content check rather than a name check: a naming convention can be satisfied by a
    /// directory that is not a store, and cannot be satisfied by one that is.
    pub fn discard(self) -> std::io::Result<()>;
}

/// The relevance leg (RFC 03 acceptance criterion 3). One point: build the corpus at `co_residents`,
/// then run **both** strategies over the fixed query set against that one store, exact arm first.
/// Returns one row per strategy. `rig` must be a freshly opened store — see Internal Logic.
pub async fn relevance::measure_point(rig: &Rig, cfg: &BenchConfig, tenant: &str,
                                      total: u32, visible: u32, co_residents: u32)
    -> Result<Vec<RelevanceLeg>, BenchError>;

/// The absolute-latency leg (RFC 03 acceptance criterion 4). One point: size the namespace to
/// `fraction` with the visible set pinned, then time **both** strategies over the fixed query set.
/// Returns one row per strategy. `rig` must be a freshly opened store, as above.
pub async fn latency::measure_point(rig: &Rig, cfg: &BenchConfig, tenant: &str,
                                    visible: u32, fraction: f64)
    -> Result<Vec<LatencyLeg>, BenchError>;
/// The namespace size that puts `visible` facts at `fraction` of it: `ceil(visible / fraction)`,
/// floored at `visible` so a corpus never claims more visible facts than it holds.
pub fn latency::total_for(visible: u32, fraction: f64) -> u32;
/// One co-resident per visible-set-worth of remainder, so co-residents hold about as much memory as
/// the measured caller — `fact_owner_user`'s selectivity is a function of the distinct owner count,
/// and a single co-resident owning the whole remainder would give it a cardinality no deployment has.
/// Shared with the co-residency leg, which derives its count the same way.
pub fn latency::co_residents_for(visible: u32, total: u32) -> u32;
/// Derive the crossover from a completed set of latency rows, decided on p50. A property of the grid
/// as a whole, so it is computed once from every row rather than accumulated as the sweep runs.
pub fn latency::crossover_from(rows: &[LatencyLeg]) -> Crossover;

/// The co-residency leg. One point: build a corpus of `namespace_total` facts with the visible set
/// pinned at `cfg.visible`, then time **both** strategies over the fixed query set against that one
/// store. Returns one row per strategy. `rig` must be a freshly opened store, as above.
pub async fn coresidency::measure_point(rig: &Rig, cfg: &BenchConfig, tenant: &str,
                                        namespace_total: u32)
    -> Result<Vec<CoResidencyLeg>, BenchError>;
/// Which strategy was faster at a size, on p50 — it **names the smaller number, it does not judge
/// it**, which is why it is a function over the rows rather than a field on them. `None` when the
/// size was not measured for both strategies: a comparison needs two arms.
pub fn coresidency::faster_at(rows: &[CoResidencyLeg], namespace_total: u32) -> Option<StrategyLabel>;
/// The sizes present in a completed leg, ascending — taken from the rows, so a leg that stopped
/// early reports where it actually looked rather than where it was asked to look.
pub fn coresidency::sizes_measured(rows: &[CoResidencyLeg]) -> Vec<u32>;

/// The corpus every leg measures against: one namespace of `total` user-private facts, `visible` of
/// them owned by `corpus::OWNER` and the remainder dealt round-robin across `co_residents` distinct
/// owner ids. Deterministic by construction, so a rerun measures the same corpus and a difference
/// between two runs is attributable to the strategy rather than to the data.
pub async fn corpus::build(rig: &Rig, tenant: &str, total: u32, visible: u32, co_residents: u32)
    -> Result<Corpus, BenchError>;
pub fn corpus::co_resident_users(n: u32) -> Vec<String>;
pub fn corpus::query_vector(embed_dim: u32) -> Vec<f32>;
/// `count` deterministic queries placed evenly through the corpus's embedding span, so each lands in
/// a different part of the ranking. All at the origin would make a fixed query set of twenty
/// identical searches, which measures one search twenty times.
pub fn corpus::query_set(embed_dim: u32, count: u32, total: u32) -> Vec<Vec<f32>>;
/// The fact's embedding: the query vector fanned out on one component by `index / total`, so the
/// corpus spans the same angular arc at any size. A **fixed** step separates ranks only at the size it
/// was chosen for — at 60 facts a `1e-6` step moves the cosine distance by under `2e-9`, the engine
/// scores the embedding array in `f32`, every distance collapses to `0.0`, and a rank correlation
/// computed over those ties measures the engine's tie-breaking rather than the strategy.
pub fn corpus::embedding(embed_dim: u32, index: u32, total: u32) -> Vec<f32>;
pub fn corpus::writer_ctx(tenant: &str) -> ScopeContext;
pub fn corpus::reader_ctx(tenant: &str, user: &str) -> ScopeContext;
/// The fact id at an index, and the stage-1 query every leg issues — one definition rather than one
/// per leg, so two legs cannot drift into measuring different queries (RISK-001's shape, applied to
/// the query rather than the seam).
pub fn corpus::fact_id(index: u32) -> String;
pub fn corpus::stage_one(qv: &[f32], ctx: &ScopeContext, stage1_k: u16) -> StageOneQuery;

/// Conveniences the report's own shape does not provide. Each **names the larger or smaller number;
/// none judges it** — which is why they are functions over the rows rather than fields on them.
pub fn relevance::better_overlap(rows: &[RelevanceLeg]) -> Option<StrategyLabel>;
pub fn relevance::digest_of(rows: &[RelevanceLeg]) -> Option<&str>;
```

##### Types owned by this spec

Two properties are load-bearing rather than decorative. **Every report names its rig**: `engine`,
`engine_version` and `host_disk` at the top, and a `Timing` carrying `min_max_ratio` on every row.
The defect that forced the re-pointing was a 22× spread between two runs of one nominal
configuration (FU-024), which a median-only row can reproduce while looking steady — and RFC 03 open
question 4c may move the embedded backend from SurrealKV to RocksDB, so a figure quoted without its
engine outlives the engine it was true of (RISK-002). And **no row carries a verdict**: the report
records measures and the thresholds they are read against, never whether a relevance delta or a
latency figure is acceptable. That is a user decision, and a boolean would hide it behind a number.

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct BenchReport {
    pub engine: String,                  // `surrealkv` / `mem` for an embedded store; `remote-<scheme>`
                                         // for a server whose backing engine the client cannot see
    pub engine_version: String,          // asked of the engine, never assumed from the linked crate;
                                         // `unknown` when the endpoint would not answer
    pub host_disk: HostDisk,
    pub embed_dim: u32,
    pub store_url_scheme: String,        // scheme only; the full URL is never written to the report
    pub relevance: Vec<RelevanceLeg>,
    pub latency: Vec<LatencyLeg>,
    pub crossover: Option<Crossover>,    // None until the latency leg runs
    pub nfr_p3_millis: f64,              // NFR-P3's stage-1 budget, recorded **as context** so the
                                         // threshold sits beside the measurement without the report
                                         // deciding anything; there is deliberately no field saying
                                         // whether a row met it
    pub co_residency: Vec<CoResidencyLeg>,
    pub ceiling: Option<CeilingHit>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StrategyLabel { PostFilteredAnn, FilterFirstExact }

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct HostDisk {
    pub os: String,                      // std::env::consts::OS
    pub arch: String,                    // std::env::consts::ARCH
    pub label: String,                   // MEMORY_BENCH_HOST_DISK verbatim, or `unspecified`
}

/// One timing sample set, reduced. `min_max_ratio` is the instrument's own instability, reported
/// rather than averaged away.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Timing {
    pub warmup_reps: u32,
    pub measure_reps: u32,
    pub p50_millis: f64,
    pub p95_millis: f64,
    pub p99_millis: f64,
    pub min_millis: f64,
    pub max_millis: f64,
    pub min_max_ratio: f64,              // max / min
}

/// RFC 03 acceptance criterion 3 — relevance across co-resident users.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct RelevanceLeg {
    pub co_residents: u32,
    pub visible: u32,
    pub total: u32,
    pub strategy: StrategyLabel,
    pub queries: u32,                    // fixed query set size this point averaged over
    pub overlap: f64,                    // mean |returned ∩ baseline| / |baseline|, [0,1]
    pub rank_correlation: f64,           // mean rank correlation against the baseline, [-1,1]
    pub fill_ratio: f64,                 // returned_visible / min(stage1_k, visible) — the starvation
                                         // measure, a ratio to what was available and not a raw count
    pub timing: Timing,
    pub corpus_digest: String,           // so two rows can be shown to have measured one corpus
}

/// RFC 03 acceptance criterion 4 — absolute stage-1 vector latency across visible fractions.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct LatencyLeg {
    pub visible_fraction: f64,           // visible / total
    pub visible: u32,
    pub total: u32,
    pub strategy: StrategyLabel,
    pub fill_ratio: f64,                 // returned / min(stage1_k, visible), as the relevance leg
                                         // measures it — on a latency row because a timing without it
                                         // reads exactly backwards; see Internal Logic
    pub timing: Timing,
    pub corpus_digest: String,
}

/// Where the two strategies swap places, as an interval on the sampling grid — never a point estimate
/// the grid cannot support.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Crossover {
    pub upper_fraction: Option<f64>,     // at and above it the index path was faster; None if no
                                         // adjacent pair on the grid changed winner
    pub lower_fraction: Option<f64>,     // at and below it filter-first was faster
    pub grid: Vec<f64>,                  // the fractions sampled, descending; recorded even when the
                                         // bounds are None, so a run that never crossed still says
                                         // where it looked
    pub transitions: u32,                // adjacent pairs that changed winner; more than one means the
                                         // bounds span several crossings rather than bracketing one
}

/// Per-user latency as co-residency rises: the visible set pinned while the namespace grows.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct CoResidencyLeg {
    pub namespace_total: u32,
    pub visible: u32,                    // identical at every point, which is the leg's whole design
    pub co_residents: u32,
    pub strategy: StrategyLabel,
    pub fill_ratio: f64,                 // returned / min(stage1_k, visible). Pinning the visible set
                                         // while the namespace grows *lowers* the visible fraction at
                                         // every step, which is the regime in which the post-filtered
                                         // arm's latency falls because it finds and returns almost
                                         // nothing; without this a reader would record that as the
                                         // scaling result the leg exists to produce
    pub timing: Timing,
    pub corpus_digest: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct CeilingHit {
    pub failed_at_index: u32,            // 0-based index of the first item that failed
    pub engine_error: String,            // the StoreError rendered verbatim, untruncated
}

#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    #[error("MEMORY_BENCH_STORE_URL is unset; memory-bench refuses to run against an inherited store configuration")]
    StoreUrlUnset,
    #[error("generated tenant id rejected by validate_tenant: {0}")]
    InvalidTenantId(String),
    #[error("refusing to run: {existing} already exists and matches MEMORY_BENCH_TENANT_PREFIX")]
    PrefixCollision { existing: String },
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("report could not be written to {path}: {source}")]
    ReportWrite { path: PathBuf, source: std::io::Error },
}
```

##### Example

Run against a disposable embedded SurrealKV store, naming the storage it sits on:

```
MEMORY_BENCH_STORE_URL=surrealkv:///tmp/bench-store \
MEMORY_BENCH_HOST_DISK="apfs on an internal nvme ssd" \
memory-bench
```

Report excerpt (`./bench-report.json`) — values illustrative of the shape, not measured figures:

```json
{
  "engine": "surrealkv",
  "engine_version": "3.1.5",
  "host_disk": { "os": "macos", "arch": "aarch64", "label": "apfs on an internal nvme ssd" },
  "embed_dim": 1024,
  "store_url_scheme": "surrealkv",
  "relevance": [
    {
      "co_residents": 1000,
      "visible": 1000,
      "total": 100000,
      "strategy": "filter-first-exact",
      "queries": 20,
      "overlap": 1.0,
      "rank_correlation": 1.0,
      "fill_ratio": 1.0,
      "timing": {
        "warmup_reps": 3, "measure_reps": 9,
        "p50_millis": 18.4, "p95_millis": 24.1, "p99_millis": 26.0,
        "min_millis": 17.9, "max_millis": 26.0, "min_max_ratio": 1.45
      },
      "corpus_digest": "9f2c…"
    }
  ],
  "latency": [],
  "co_residency": [],
  "ceiling": null
}
```

The `fill_ratio` figure is the one acceptance criterion 3 turns on. It is a ratio to what was
*available* rather than a raw count, because the failure mode RISK-013 names is the post-filter
discarding the caller's rows after the index walk — which shows up as this collapsing while the raw
returned count stays plausible.

An empty leg array is a run that measured nothing, not a run that failed: the report is written
either way, so an operator can tell the two apart.

#### Internal Logic

Everything in this section describes code that exists, with one exception that is fenced off at the
end. `connect`, the pre-flight prefix guard and `teardown` are the harness's safety layer; the
relevance, visible-fraction latency and co-residency legs are the measurements. The `provision`,
`sweep`, `footprint` and single-tenant-latency steps of the **withdrawn** population legs follow
`teardown` under their own heading, retained as the record of what they did and marked as
unimplemented, because a procedure sitting between two live ones reads as a requirement whatever the
preamble says. Their step numbers are unchanged: `phase-6-audit.md` cites step 7 by number.

##### `connect`

1. Read `MEMORY_BENCH_STORE_URL`. When empty or unset, return `BenchError::StoreUrlUnset` **before** any connection is opened and before any other env var is read (SA-BENCH-ISOLATION-01). Log `bench.refused.store_url_unset`. This is the only guard between a mistyped invocation and tens of thousands of dropped namespaces, so it is first.
2. Open the connection with `surrealdb::engine::any::connect(cfg.store_url)`, the same entry point `Store::connect` uses, so embedded (`surrealkv://`, `mem://`) and remote (`ws://`, `http(s)://`) endpoints are both reachable from one code path (FU-009). On failure return `BenchError::Store(StoreError::Unavailable)`. Log span `bench.connect` with field `scheme` (scheme only — the URL may carry credentials and is never logged in full).
3. When the endpoint is remote and both `MEMORY_STORE_REMOTE_USER` and `MEMORY_STORE_REMOTE_PASS` are set, call `db.signin(Root{ username, password })` (FU-019). This is what lets acceptance criterion 8 run the remote leg against a secured endpoint; without it only an unauthenticated remote server is reachable.

##### relevance leg (RFC 03 acceptance criterion 3)

1. For each count in `MEMORY_BENCH_CORESIDENTS`, ascending, open a rig at `MEMORY_BENCH_RIG_DIR/coresidents-<n>` under **`MemoryProfile::Individual`**. The profile decides the *read filter* and the strategy is passed separately, so this pairs the narrow owner equality with the `fact_owner_user` index hint — the filter-first arm RFC 03 proposes — and puts the ANN arm behind the same filter, so the two differ in strategy alone. Under the collective profile filter-first runs unhinted, which measures a different query.
2. **A fresh rig per point, opened and closed in sequence.** Rebuilding the corpus in place across points is measurably wrong: fact ids and embeddings are a function of index alone, so a rebuild rewrites only ownership and looks like the cheap way to sweep — but sweeping `1,10` and then `10,1` both put the higher post-filtered overlap on whichever point ran *first*, while the exact arm sat at 1.0 in every position. The approximate arm degrades with the number of rewrites the HNSW index has absorbed, so the sweep axis was measuring rebuild count rather than co-residency. Sequential rather than concurrent because SurrealKV holds an exclusive directory lock for the process lifetime.
3. Build the corpus: `MEMORY_BENCH_TOTAL` user-private facts, `MEMORY_BENCH_VISIBLE` of them the caller's, spread **evenly through the index range** by Bresenham rather than taken from the front. Embeddings fan out monotonically by `index / total`, so index order is rank order from the query and consecutive ranks stay separable in `f32` at any corpus size — giving the caller the first `visible` indices would make their facts precisely the nearest neighbours, the HNSW walk would return them at any visible fraction, and the fill ratio would read 1.0 at every point. A leg that reports 1.0 everywhere has not been shown to measure anything. The invisible remainder is dealt round-robin by its own ordinal across the `n` co-resident ids.
4. Run the exact arm (`FilterFirstExact`) over the fixed query set first and keep its returned ids per query as the **reference**. Filter-first filters before it caps, so it scores every visible row and returns the true top-k of what the caller can read — it *is* the ground truth for this corpus and cannot be graded against an approximation of itself. Grading both arms against it turns "at least equal to the HNSW path" into a number; the exact arm's own row is the 1.0 reference the other is read against, and a reference that is not 1.0 would mean the baseline is unstable, which is worth seeing.
5. Per strategy, record `overlap` = mean `|returned ∩ reference| / |reference|`; `rank_correlation` = mean Spearman ρ over the ids present in **both**, re-ranked within the shared subset so ids the other side missed do not leave gaps that read as disagreement, and `0.0` when fewer than two are shared because no ordering exists to correlate; and `fill_ratio` = mean `returned / min(stage1_k, visible)`, clamped to `[0,1]`. The denominator is what was *available*, not what was asked for: a caller who owns three facts and asks for ten has not been starved by receiving three.
6. Time each strategy with `Timing::measure`, one sample per query per repetition (`MEMORY_BENCH_REPS × MEMORY_BENCH_QUERIES` samples), cycling through the query set so no single query's neighbourhood dominates the distribution. The percentiles then describe a *query*, which is what NFR-P3 is a claim about.
7. Discard each point's rig immediately after measuring it, unless `MEMORY_BENCH_RETAIN`. A discard failure is logged and non-fatal — the numbers are already taken, and a scratch directory that outlives a run is an operator's tidy-up rather than a lost measurement.
8. **No verdict is written.** Whether a relevance delta is acceptable is a user decision, and a boolean in the report would hide it behind a number.

##### absolute-latency leg (RFC 03 acceptance criterion 4)

1. For each fraction in `MEMORY_BENCH_FRACTIONS`, descending, open a rig at `MEMORY_BENCH_RIG_DIR/fraction-<i>` under `MemoryProfile::Individual`, indexed rather than named by the fraction — `0.125` is not a directory name anyone wants to debug, and two fractions that round to the same string would collide on one path. A fresh rig per point, for the reason the relevance leg states.
2. Size the namespace to the fraction with the **visible set pinned** at `MEMORY_BENCH_VISIBLE`: `total = ceil(visible / fraction)`, floored at `visible`. An absolute claim about a 1,000-fact filtered set has to be measured at 1,000 facts at every fraction, or the number moves with the caller's own corpus rather than with the thing under test. The row records the *achieved* fraction, since the ceiling rounds.
3. Spread the invisible remainder across `(total - visible) / visible` co-resident users — co-residents holding about as much memory as the measured caller, which is the shape a consumer deployment has. This matters to the measurement: `fact_owner_user` is the index the filter-first arm stands on, and its selectivity is a function of how many distinct owners the namespace holds. One co-resident owning ninety-nine thousand facts would give that index a cardinality no deployment would.
4. Per strategy, run the fixed query set **once untimed** to record the fill ratio, then time it with `Rig::time_query_set`. The untimed pass costs one query set against a corpus that took minutes to build, and without it a reader cannot tell a strategy that got faster from one that stopped finding anything: the post-filtered arm's latency *falls* as the visible fraction falls, because it finds and returns almost nothing. Measured at a 5% fraction, its p50 was 2.92 ms against filter-first's 4.54 ms — with a fill ratio of 0.10 against 1.00. A crossover derived from timings alone records the point at which the index path "wins" while it is in fact starving.
5. Failures inside the timed section are **counted, not screened out beforehand**. `Timing::measure` discards its closure's output, errors included, so a leg whose queries all failed would report a fast set of percentiles for a query that never ran. A checked pass before the timing would catch that and would also warm the index, silently making `MEMORY_BENCH_WARMUP=0` not cold and taking the one thing an operator sets it to see.
6. Derive the crossover **once, from every row the leg produced**, rather than accumulating it as the sweep runs: it is a property of the grid as a whole, and a partial grid brackets the wrong pair. The winner at each fraction is decided on **p50**, not p95 — at the default nine repetitions a nearest-rank p95 *is* the slowest sample, so a crossover derived from it would be decided by one unlucky observation per point. NFR-P3's own p95 stays on every row; it is the threshold's statistic, not the comparison's.
7. A fraction measured for only one strategy has no winner: a comparison needs two arms, and treating a lone row as a win would invent a transition out of a missing measurement.
8. **No verdict against NFR-P3.** The threshold travels on the report as `nfr_p3_millis`. Whether a figure taken on this rig, on this operator's disk, clears a deployment budget is a judgement about the rig as much as about the number (RISK-003).

##### co-residency leg (per-user latency as co-residency rises)

1. For each size in `MEMORY_BENCH_SIZES`, ascending, open a rig at `MEMORY_BENCH_RIG_DIR/size-<i>` under `MemoryProfile::Individual`, indexed rather than named by the size so two configured sizes that clamp to one corpus cannot collide on a path. A fresh rig per point, for the reason the relevance leg states.
2. Build the corpus at `namespace_total = <size>` with the visible set **pinned** at `MEMORY_BENCH_VISIBLE`. Both legs pin the visible set; what differs is which variable is the grid. There the fraction is chosen and the size follows from it, so the sweep is spaced by fraction and stops where the fraction grid stops; here the size is chosen and the fraction follows from it, so the sweep is spaced by namespace size and reaches sizes no fraction grid asks for. That is what makes *"does filter-first's cost track the visible set while the index walk's tracks the namespace"* answerable across the range Stage 2 cares about. It is a difference of sampling, not of kind, and the fill ratio is required on both legs' rows because of it.
3. Derive the co-resident count from the size exactly as the latency leg derives it, `(total - visible) / visible`, for the same `fact_owner_user` selectivity reason. Derived rather than configured because it is not this leg's variable; a count held fixed while the namespace grew would give that index a cardinality no deployment has.
4. Per strategy, run the fixed query set **once untimed** to record the fill ratio, then time it with `Rig::time_query_set` — warm-up and repetitions as configured, exact arm first. The fill ratio is as load-bearing here as on a latency row and for the same reason: pinning the visible set while the namespace grows is arithmetically a falling visible fraction (1,000 of 2,000 is 50%, 1,000 of 100,000 is 1%), and that is the regime in which the post-filtered arm's latency falls because it finds and returns almost nothing. A row carrying only a timing would let the index path be recorded as overtaking filter-first at the largest namespace — the scaling result this leg exists to produce, arrived at by starvation.
5. Discard each point's rig immediately after measuring it unless `MEMORY_BENCH_RETAIN`; a discard failure is logged and non-fatal. A point that fails stops the leg and keeps the sizes already measured.
6. **Which strategy was faster at a size is a function over the rows, not a field on them** (`coresidency::faster_at`, on p50 for the reason the crossover uses p50), and it is never read without the fill ratios on the same rows. The report records measures and never a verdict; a `winner` on the row would read as one while hiding both the margin and the fill ratio that decide whether it means anything. The `bench.coresidency.faster` log line carries the winner's own fill ratio for that reason.

##### pre-flight prefix guard

3b. Before provisioning anything, call `MemoryStore::list_tenants()` once and refuse to run when **any** existing id begins with `cfg.tenant_prefix`: return `BenchError::PrefixCollision { existing }` and exit 2 without creating or dropping a namespace (SA-BENCH-ISOLATION-02). This closes the one route by which teardown destroys data it did not create even when every other guard holds: `DEFINE NAMESPACE IF NOT EXISTS` adopts a colliding pre-existing namespace, after which teardown's prefix check passes and drops it. One `INFO FOR ROOT` at start-up makes that adoption impossible.

##### `teardown`

15. Enumerate the tenants the run created from its own generated id list — **not** from `list_tenants()`. Drop each with `MemoryStore::drop_tenant_namespace(tenant)` (or `REMOVE DATABASE` for the `database` shape). Refuse to drop any id that does not begin with `cfg.tenant_prefix`; such an id returns `BenchError::InvalidTenantId` and the teardown continues with the remainder, so one unexpected id cannot strand the rest of the cleanup. `drop_tenant_namespace` itself refuses unless the namespace holds exactly one tenant (ADR-018); the harness provisions one tenant per namespace, so this is a guard the harness passes rather than a constraint on it. A later co-resident shape for the harness would have to switch teardown to `erase_tenant`, whose predicate is bounded, and would lose the prefix check's protection as its outermost guard because the predicate provides it.
16. When `cfg.retain` is `true`, skip teardown entirely and log `bench.teardown.retained` with the population — the operator has chosen to keep the population for a follow-up run and owns dropping it.

##### Withdrawn population legs — the record of what they did, not a specification of what exists

**No code implements any of the four procedures below.** They are kept because they are the only
account of how the withdrawn legs worked, and because `phase-6-audit.md` cites step 7 by number.
`provision`, `sweep`, `footprint` and `single_tenant_latency` are absent from `src/bench`, and no
type in *Types owned by this spec* carries their output. RFC 03 withdrew the legs
(`docs/wip/rfc/03-scalability-first-architecture.md`); reviving one means re-deriving it against the
current report shape, not implementing the text below as written.

##### `provision`

4. Generate `count` tenant ids as `format!("{}{:07}", cfg.tenant_prefix, i)` for `i` in `0..count`. Call `validate_tenant(&id)` on every generated id; a rejection returns `BenchError::InvalidTenantId(id)` and aborts the run. The harness generates its own identifiers and interpolates them into DDL through `Migrator`, so it passes the same gate every other caller passes — the sql-safety layer rule allows no second path.
5. Drive provisioning with at most `cfg.concurrency` tasks in flight. Per tenant, time a single `Migrator::migrate_up(tenant)` call (which internally runs `select_namespace` → `DEFINE NAMESPACE IF NOT EXISTS` + `USE NS` + `DEFINE DATABASE IF NOT EXISTS` + the `schema_migrations` DDL, then `current_version`, then the pending migration bodies) and record the elapsed milliseconds against the tenant index. Possible errors: `StoreError::Timeout`, `StoreError::Unavailable`, `StoreError::Internal`.
6. On the **first** provisioning failure, stop issuing new tasks, let in-flight tasks settle, and record `CeilingHit { failed_at_index, engine_error }` with the `StoreError` rendered verbatim and untruncated. Return `Ok(ProvisionOutcome)` rather than `Err` — **a ceiling is the answer to OQ-NSCEILING, not a harness failure**, and a run that hits one must still write its report. Log `bench.provision.ceiling` with the index and the error.
7. When `cfg.isolation == Database`, provision one database per tenant inside a single namespace instead. `DB_NAME` is a compile-time `const` in C1 (`src/store/migrate.rs`), so `Migrator` cannot be redirected to a per-tenant database name; the harness issues `DEFINE DATABASE IF NOT EXISTS <tenant>` against the fixed benchmark namespace itself and then applies the same migration bodies `Migrator::render_up` produces. `render_up` is private today (`fn render_up`, `src/store/migrate.rs:167`); **this work widens it to `pub(crate)`** so `src/bench` reuses it rather than re-reading the migration text (SA-BENCH-RENDER-01). That is the only change this component requires of C1, and it is recorded in the C1 spec as well as here. This leg measures object *organisation*, and it is the only path in this component that issues DDL outside `Migrator`; the generated identifier has already passed `validate_tenant` at step 4.

##### `sweep`

8. Time the three parts of a scheduler tick separately (SA-SWEEP-COST-01) rather than timing one whole tick. A whole tick at 50,000 tenants runs for longer than `POLL_INTERVAL`, so consecutive real ticks overlap and a wall-clock figure would measure queueing rather than cost. The three parts are:
   - **(a)** one `MemoryStore::list_tenants()` call, timed whole, recording the number of ids returned. This issues `INFO FOR ROOT` and returns every defined namespace in a single response, so its cost and its response size both grow with population.
   - **(b)** one `tenant_is_due`-equivalent probe against a sampled tenant: build the maintenance `ScopeContext` and call `scan_recent_episodes(ctx, since = now - 300s, limit = 1)`.
   - **(c)** one namespace preamble against a sampled tenant: `Migrator::migrate_up(tenant)` on an already-migrated namespace, which is the idempotent no-op path every store call pays through `ensure_and_use`.
9. Repeat (b) and (c) across `cfg.sweep_samples` distinct sampled tenants drawn evenly across the provisioned range, so a cold namespace and a warm one both contribute. Reduce each series to p50 and p95.
10. Compute `projected_tick_millis = list_tenants_p50 + population × (due_probe_p50 + ns_preamble_p50)`. Record it as a projection. Log `bench.sweep.measured` with all three parts and the projection.

##### `footprint` and byte attribution

11. Derive the **expected** object inventory by counting `DEFINE TABLE` / `DEFINE INDEX` / `DEFINE FIELD` / `DEFINE ANALYZER` statements in the migration set the harness just applied — the same `include_str!("../../migrations/0001_init.up.surql")` constant `Migrator` holds. For migration version 1 that is **10 tables, 19 indexes, 80 fields, 1 analyzer — `object_count` 110**, verified against `migrations/0001_init.up.surql` (the HNSW vector index is `fact_hnsw`, line 46). Then issue `INFO FOR DB` against one provisioned tenant as a **cross-check** and compare. When the two agree, report the figure. When they disagree, report the migration-derived figure and log `bench.footprint.divergent` with both. When the `INFO FOR DB` response cannot be parsed into object counts at all, report the migration-derived figure and log `bench.footprint.info_shape_unrecognised` (SA-FOOTPRINT-02). The migration set is the authority because it is verifiable in this repository; `INFO FOR DB`'s response shape is read nowhere in the codebase, so the harness must not depend on it.
12. Attribute steady-state bytes by difference: record total store size before provisioning and after, and divide by `provisioned` (SA-FOOTPRINT-01). For an embedded endpoint the size is the on-disk directory total for `MEMORY_BENCH_STORE_URL`'s path; for a remote endpoint the harness records `steady_state_bytes_per_ns = 0` and logs `bench.footprint.bytes_unavailable`, because it cannot read the server's disk. Acceptance criterion 1's byte figure therefore comes from the embedded leg; the remote leg supplies objects and wall-clock only. This limitation is stated rather than worked around.

##### single-tenant latency (acceptance criterion 7)

13. After the population is provisioned and still present, select one tenant, write a corpus of the FU-007 size (25,000 facts) into it, and issue `cfg.sweep_samples × 20` recall operations against C1's retrieval surface, recording ANN, stage-1, and end-to-end durations. Reduce to p95. The measurement's purpose is to show NFR-P2 (≤ 200 ms) and NFR-P3 (≤ 50 ms) still hold for one tenant **while** the full population exists alongside — so it runs before teardown, never after.
14. Record the p95 figures in the report without a pass/fail verdict. The harness measures; whether a breach is acceptable is a user decision, and encoding a threshold judgement in a measurement instrument would hide a marginal result behind a boolean.

#### Data Model

**N/A — this component defines no tables of its own.** It provisions the C1-owned schema unchanged (`migrations/0001_init.up.surql`, version 1, applied through `Migrator`) into namespaces it creates, writes its corpus through the C1 write surface, and writes its output to a JSON file rather than to the store. It reads no object counts back through `INFO FOR DB`: that was the withdrawn footprint leg's cross-check, and `INFO FOR ROOT` in the pre-flight guard is the only schema-introspection call left. It adds no `DEFINE TABLE`, `DEFINE FIELD`, `DEFINE INDEX`, or `DEFINE ANALYZER` statement to the migration set, and it introduces no migration.

**Rollback considerations (sql-safety layer rule).** The harness is destructive by design and by construction: `teardown` issues `REMOVE NAMESPACE` (or `REMOVE DATABASE`) for every tenant it created, which removes every object and every row inside. There is no rollback for a dropped namespace, so the safeguards are placed ahead of the destruction rather than behind it: (1) `MEMORY_BENCH_STORE_URL` is required and never inherited from `MEMORY_STORE_PATH` or `MEMORY_STORE_REMOTE_URL`, so the harness cannot pick up a live deployment's configuration (SA-BENCH-ISOLATION-01); (2) teardown enumerates only ids the run itself generated, never `list_tenants()` output, so a namespace the harness did not create is never a drop candidate; (3) every id is prefix-checked against `cfg.tenant_prefix` immediately before the drop. The `DEFINE NAMESPACE IF NOT EXISTS` in the provisioning path is non-destructive: a pre-existing namespace at a colliding id is adopted rather than recreated — and is then excluded from teardown only if its id fails the prefix check, so an operator who sets `MEMORY_BENCH_TENANT_PREFIX` to a live tenant's prefix would have that tenant dropped. That residual is recorded in *Gaps*.

#### Error Table

`memory-bench` is a command-line binary with no HTTP surface. The `Status` column records the **process exit code**; the `Code` column is the `BenchError` variant. No error in this table is ever rendered into an HTTP response, because this component serves none.

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| `MEMORY_BENCH_STORE_URL` unset or empty | 2 | `StoreUrlUnset` | stderr: `MEMORY_BENCH_STORE_URL is unset; memory-bench refuses to run against an inherited store configuration` |
| A generated or supplied tenant id fails `validate_tenant` | 2 | `InvalidTenantId` | stderr: `generated tenant id rejected by validate_tenant: <id>` |
| An existing namespace already matches `MEMORY_BENCH_TENANT_PREFIX` | 2 | `PrefixCollision` | stderr: `refusing to run: <id> already exists and matches MEMORY_BENCH_TENANT_PREFIX` |
| Store unreachable at connect, or during provisioning | 1 | `Store(Unavailable)` | stderr: `store error: memory store unavailable` |
| A store operation exceeded its deadline | 1 | `Store(Timeout)` | stderr: `store error: memory store timeout` |
| Provisioning failed before reaching the requested population (namespace ceiling) | 0 | _(none)_ | Report written with `ceiling: { failed_at_index, engine_error }`; **exit 0** — a ceiling is a measured result, not a run failure (OQ-NSCEILING) |
| Report file could not be written to `MEMORY_BENCH_REPORT_PATH` | 1 | `ReportWrite` | stderr: `report could not be written to <path>: <io error>` |
| Teardown encountered an id lacking the configured prefix | 0 | `InvalidTenantId` (logged, not fatal) | Report written; `bench.teardown.skipped` logged per skipped id; remaining ids still dropped |

#### Acceptance Criteria (Gherkin)

```gherkin
Feature: Tenancy Benchmark Harness

  Scenario: Happy path — the three legs measure and the report names its rig
    Given MEMORY_BENCH_STORE_URL points at a disposable empty store
    And MEMORY_BENCH_VISIBLE is "1000"
    When memory-bench runs to completion
    Then the report records engine, engine_version and host_disk
    And the store URL appears by scheme only, never in full
    And every row carries p50, p95, p99 and min_max_ratio
    And no row carries a pass/fail verdict against NFR-P3
    And every rig directory the run created is removed before the process exits

  Scenario: Relevance — the exact arm is the baseline and starvation is visible
    Given MEMORY_BENCH_CORESIDENTS is "1,10"
    And the visible fraction is low enough that the index walk over-fetches
    When the relevance leg runs
    Then both strategies are measured against one store holding one corpus
    And every row at a co-resident count carries the same corpus_digest
    And the filter-first row records overlap 1.0 as the baseline
    And the post-filtered row records a fill ratio below 1.0

  Scenario: Latency — the crossover is an interval on its grid, never a point
    Given MEMORY_BENCH_FRACTIONS is "1.0,0.5,0.25,0.125,0.05"
    And MEMORY_BENCH_VISIBLE pins the visible set at every fraction
    When the absolute-latency leg runs
    Then the report records the fractions sampled as the crossover grid
    And a grid on which the winner changed records both bracketing bounds
    And a grid on which it never changed records null bounds with the grid intact
    And every latency row carries a fill ratio beside its timing

  Scenario: Co-residency — the visible set is identical at every namespace size
    Given MEMORY_BENCH_SIZES is "2000,8000"
    And MEMORY_BENCH_VISIBLE is "1000"
    When the co-residency leg runs
    Then every row records visible 1000 whatever the namespace size
    And each size carries one row per strategy
    And each size's two rows share one corpus_digest
    And the co-resident count rises with the namespace while the visible set does not

  Scenario: Edge case — an emptied sweep list skips its leg rather than defaulting
    Given MEMORY_BENCH_SIZES is set to an empty value
    When memory-bench runs
    Then the co-residency leg is skipped
    And the report is still written with an empty co_residency array
    And the process exits 0

  Scenario: Edge case — a retryable write conflict does not abort a corpus build
    Given the store raises "Transaction conflict: … can be retried" on one corpus write
    When the corpus builder issues that write
    Then it retries up to five times with exponential backoff
    And the build continues rather than discarding the corpus already written

  Scenario: Error path — a non-retryable store failure stops the leg at its failing point
    Given the store fails a write that is not a retryable conflict
    When a leg runs
    Then the leg stops at the failing point and measures no further point
    And the process exits 1 with the store error on stderr
    And the report file holds the shell written before the leg started, naming the engine
    # The points measured before the failure survive on the in-memory report but do not reach the
    # file: `run` propagates the error before its final write. Recorded as FU-016 in
    # `docs/wip/plan/06-re-pointed-retrieval-benchmark.md`, not asserted here as though it were fixed.

  Scenario: Error path — an unset store URL refuses to run
    Given MEMORY_BENCH_STORE_URL is unset
    And MEMORY_STORE_PATH points at a store holding real tenant data
    When memory-bench starts
    Then no store connection is opened
    And it exits 2 with code StoreUrlUnset
    And no namespace is created or dropped

  Scenario: Error path — teardown never drops a namespace the run did not create
    Given a population was provisioned with prefix "bench"
    And a namespace "acme" exists in the same store
    When teardown runs
    Then only ids beginning with "bench" are dropped
    And "acme" is never a drop candidate because teardown reads the generated id list, not list_tenants
```

#### Performance, Security, Observability

- **Performance targets:** This component has no latency budget of its own — it is the instrument, not the subject, and no request path waits on it. Two bounds keep the instrument from distorting the measurement: concurrent provisioning tasks are capped at `MEMORY_BENCH_CONCURRENCY` (default 16) so the recorded per-tenant duration reflects store cost rather than harness contention; and timing samples are held as raw per-operation values and reduced to percentiles once at report time, so the memory ceiling is `population × 8 bytes` for the provisioning series (≈ 400 KB at 50,000) plus `sweep_samples × 3 × 8 bytes`. The figures the harness *measures* are governed by the NFRs it reports against: NFR-P2 (recall p95 ≤ 200 ms) and NFR-P3 (ANN ≤ 50 ms), both measured for a single tenant with the full population present.
- **Security:** The harness holds no end-user credentials and validates no OIDC token — it takes no inbound request and exposes no listener, so there is no entry point to authenticate. Its security surface is entirely outbound and destructive: every tenant identifier passes `validate_tenant` before interpolation into DDL (identifiers cannot be parameter-bound, so this gate is the only defence, and the harness must not bypass it); `MEMORY_BENCH_STORE_URL` is mandatory and never inherited, so a live deployment's configuration cannot be picked up implicitly; teardown drops only ids the run generated and prefix-checks each one immediately before the drop. Remote store credentials (`MEMORY_STORE_REMOTE_USER`/`MEMORY_STORE_REMOTE_PASS`) are read from env only; the store URL is logged and reported by **scheme only**, never in full, because a remote URL may carry credentials in its authority. No fact content is read, logged, or written to the report — the latency leg records durations, never results.
- **Observability:** Structured logs on the `memory-bench` target: `bench.connect{scheme,isolation}`, `bench.refused.prefix_collision`, `bench.ready`, `bench.corpus.progress{written,total}`, `bench.corpus.built{total,visible,co_residents,digest}`, `bench.corpus.write_conflict_retry{attempt,delay_millis}`, `bench.relevance.point{co_residents,strategy,overlap,fill_ratio,p50_millis}`, `bench.relevance.skipped`, `bench.latency.point{visible_fraction,strategy,visible,total,p50_millis,p95_millis,min_max_ratio,fill_ratio}`, `bench.latency.skipped`, `bench.coresidency.point{namespace_total,visible,co_residents,strategy,p50_millis,p95_millis,min_max_ratio}`, `bench.coresidency.faster{namespace_total,winner}`, `bench.coresidency.skipped`, `bench.rig.retained`, `bench.rig.discard_failed{error}`, `bench.engine_version.unknown{error}`, `bench.report.written{engine,engine_version,relevance_rows,latency_rows,co_residency_rows}`, `bench.teardown.done{dropped}`, `bench.teardown.retained{population}`, `bench.teardown.skipped{tenant}`, `bench.done{engine,engine_version}`. The withdrawn population, footprint and sweep legs took their own events (`bench.provision.*`, `bench.sweep.*`, `bench.footprint.*`) with them. The run report at `MEMORY_BENCH_REPORT_PATH` is the durable artefact; logs carry progress, the report carries results. No metrics are exported and no trace spans are emitted: the harness is a short-lived batch binary with no scrape endpoint, and adding one would mean shipping a listener in a tool whose whole purpose is to avoid touching the deployment.

#### Gaps

None. *(All three gaps raised in the initial draft were closed within this pass. Detail of how each was closed:)*

- **`INFO FOR DB` response shape — dependency inverted.** The initial draft counted objects from the `INFO FOR DB` response, whose key names are not verifiable in this repository (`INFO FOR DB` appears nowhere in `src/` or `tests/`). Internal Logic step 11 now derives the object inventory from the migration set — verified as 10 tables, 19 indexes, 80 fields, 1 analyzer against `migrations/0001_init.up.surql` — and uses `INFO FOR DB` only as a cross-check that logs a divergence. The unverifiable identifier is no longer on the critical path (SA-FOOTPRINT-02).
- **`Migrator::render_up` visibility — verified and pinned.** `render_up` is private (`fn render_up`, `src/store/migrate.rs:167`). Internal Logic step 7 pins the resolution: this work widens it to `pub(crate)` so `src/bench` reuses it. This is a change to a C1-owned file, documented on both sides — the C1 spec carries the matching note (SA-BENCH-RENDER-01).
- **Prefix collision on teardown — closed by a pre-flight guard.** The residual hazard (a pre-existing namespace adopted by `DEFINE NAMESPACE IF NOT EXISTS`, then dropped by a teardown whose prefix check passes) is closed by Internal Logic step 3b: the harness calls `list_tenants()` once before provisioning and refuses to run when any existing id matches `cfg.tenant_prefix`, exiting 2 with `PrefixCollision` before creating or dropping anything (SA-BENCH-ISOLATION-02).

---

#### Amendment January 2026 — Per-collection relevance leg (ADR-020)

> Source: HLD revision 0.11.0, ADR-020; ordin's ADR-110 acceptance runs (fairness, exact match, filtered recall and budget)). Counterpart: `memory-store.md` (amendment January 2026), whose steps 9′ and 10′ this leg measures. Every rule below adds to the sections above.

##### Purpose (addition)

A fourth leg, the **collection leg**, measures what rank fusion is for: whether a small collection's best fact reaches the stage-1 candidates beside a large collection, whether an exact rare term keeps its place, whether a collection filter returns a full page, and how long stage-1 takes with up to 4 typed sub-queries over 3 collections. Each run also records the **baseline**: the same corpus and queries through the as-built merge (fact-id cap, no collections), so the report shows the before and after side by side. It also runs a per-collection relevance **fixture** when one is supplied, reporting precision at 5, recall at 5 and mean reciprocal rank per collection.

##### Approach (addition)

Synthetic embeddings, as the other legs use, make the fairness case exact: the corpus is built so that collection B's answer fact is B's nearest neighbour to the query yet below the 50th-nearest fact of collection A by raw cosine, which is the case the as-built cap fails. The leg stays on C1 (`MemoryStore::recall`) with no model provider: the stage-1 candidates are what the fairness and filtered runs are about, and the order the exact-term run is read against is the fused order — the final order whenever the reranker is unavailable (C6 step 5′). The reranked final order and the whole-path p95 with the bundled models are measured by ordin's Memory Service performance scenario (`docs/spec/ordin-desktop/components/MemoryService.md`, amendment January 2026), which has the models; this leg reports the stage-1 share of the budget run. Rejected: *calling live providers from the harness* — a provider's latency would enter a figure that is a claim about the in-process engine (the reason the rig is not containerised).

Assumptions (Phase 1 1A):

- **SA-BENCH-COLL-01** The fairness corpus is collection `tk:a` with `MEMORY_BENCH_COLLECTION_SIZES[0]` facts and `tk:b` with `[1]` facts (defaults 20,000 and 200), each `tk:b` answer fact placed by construction between the 60th and 200th nearest overall; 20 fixed queries (`MEMORY_BENCH_QUERIES`).
- **SA-BENCH-COLL-02** The fixture format is the generic form of ordin's `ordin-team-checks.json`: `{"format":1,"checks":[{"query":"…","type":"lex"|"vec"|null,"expect":["fact:<uuidv7>", …]}]}`, at most 100 checks, `expect` 1–10 memory fact ids, file ≤ 256 KiB. `type: null` runs a plain query. One file per collection, named `<collection>.checks.json` in `MEMORY_BENCH_FIXTURE_DIR`, with the collection's facts in `<collection>.facts.jsonl` (one `Fact` per line, embedding in field `embedding`).

##### Shared Context (additions)

| Variable | Type | Default | Required | Description |
|---|---|---|---|---|
| `MEMORY_BENCH_COLLECTION_SIZES` | u32 list | `20000,200` | no | Sizes of the two fairness collections, large first (SA-BENCH-COLL-01). |
| `MEMORY_BENCH_COLLECTION_LOAD` | u32 | `3` | no | Collections of `MEMORY_BENCH_COLLECTION_SIZES[0]` facts each for the timing run (the budget run). |
| `MEMORY_BENCH_FIXTURE_DIR` | path | _(unset)_ | no | Directory of per-collection fixtures (SA-BENCH-COLL-02); unset skips the fixture run and the report says so. |

`StageOneQuery`, `StageOneSearch`, `Candidate.fused_score` / `fused_norm` and `RecallFilters.collections` are as `memory-store.md` (amendment January 2026) defines them.

##### Public Interface (additions)

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct CollectionLegReport {
    pub fairness: FairnessRow,                 // fairness run
    pub exact_term: ExactTermRow,              // exact-term run
    pub filtered: FilteredRow,                 // filtered run
    pub stage1_timing: Timing,                 // budget run, stage-1 share: 4 sub-queries, MEMORY_BENCH_COLLECTION_LOAD collections
    pub fixture: Option<Vec<FixtureRow>>,      // None = MEMORY_BENCH_FIXTURE_DIR unset
}
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct FairnessRow { pub queries: u32, pub answer_in_candidates: u32, pub baseline_answer_in_candidates: u32 }
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct ExactTermRow { pub queries: u32, pub in_top3: u32, pub baseline_in_top3: u32 }
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct FilteredRow { pub requested: u32, pub returned_min: u32, pub returned_mean: f64, pub baseline_returned_min: u32 }
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct FixtureRow { pub collection: String, pub checks: u32, pub precision_at_5: f64, pub recall_at_5: f64, pub mrr: f64 }
```

`BenchReport` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub collections: Option<CollectionLegReport>`. `BenchError` gains `FixtureUnreadable { path: String, reason: String }`.

##### Internal Logic (additions)

**collection leg**

1. Open a rig at `MEMORY_BENCH_RIG_DIR/collections` under `MemoryProfile::Individual`, one team scope, facts team-shared.
2. **Fairness.** Build SA-BENCH-COLL-01's corpus. For each of the 20 queries, run `recall` with `stage1_k = MEMORY_BENCH_STAGE1_K` and no collection filter; count queries whose `tk:b` answer fact is among the candidates. Run the same queries through the **baseline** — the as-built merge, reproduced in the harness as a private function that merges the three signals into a map keyed by fact id and takes the first `stage1_k` keys — and record its count beside it. ordin's fairness run reads the result against ≥ 19 of 20 (95 %).
3. **Exact term.** Add 20 facts to `tk:a`, each the only holder of one rare token (`zq-` followed by four digits); for each, run a plain query containing that token and a `lex` sub-query of it; count those whose holder is within the first 3 candidates by `fused_score`. Record the baseline count by the baseline's stage-1 order. The criterion is "not lower than the baseline".
4. **Filtered.** For each query, run `recall` with `filters.collections = ["tk:b"]`, `stage1_k = 50`; record the minimum and mean number returned, and the baseline (unfiltered top-K, then keep `tk:b` facts). The criterion is `returned_min = min(50, |tk:b matches|)`.
5. **Timing (stage-1 share of the budget run).** A fresh rig with `MEMORY_BENCH_COLLECTION_LOAD` collections of `MEMORY_BENCH_COLLECTION_SIZES[0]` facts; queries with 4 sub-queries (2 `lex`, 2 `vec`, synthetic vectors), timed with `Timing::measure` over `MEMORY_BENCH_REPS × MEMORY_BENCH_QUERIES` samples after `MEMORY_BENCH_WARMUP`. Read against the ANN sub-budget of ≤ 50 ms p95.
6. **Fixture.** When `MEMORY_BENCH_FIXTURE_DIR` is set, load each `<collection>.facts.jsonl` into the collection it names, then run each check (plain or typed as `type` says) restricted to nothing; per collection compute precision at 5 (expected ids among the first 5 candidates by `fused_score` ÷ 5), recall at 5 (expected ids among the first 5 ÷ expected ids) and mean reciprocal rank (mean of 1 ÷ rank of the first expected id, 0 when absent from the candidates). A file that does not parse or breaks SA-BENCH-COLL-02 → `BenchError::FixtureUnreadable`, exit 2, nothing measured for any collection.
7. Discard the rigs unless `MEMORY_BENCH_RETAIN`. **No verdict is written** (as every leg).

##### Data Model

N/A — disposable rigs only; the report is JSON on disk.

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| A fixture file does not parse or breaks SA-BENCH-COLL-02 | 2 | `FixtureUnreadable` | stderr: `fixture <path> could not be read: <reason>` |
| `MEMORY_BENCH_COLLECTION_SIZES` with fewer than two sizes, or the second not smaller than the first | 2 | `FixtureUnreadable` | stderr: `MEMORY_BENCH_COLLECTION_SIZES needs a larger size then a smaller one` |

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Tenancy Benchmark Harness — collection leg

  Scenario: Happy path — fairness against the baseline
    Given the default collection sizes 20000,200 and 20 queries
    When the collection leg runs
    Then the report's fairness row has queries 20 and both answer_in_candidates and baseline_answer_in_candidates recorded
    And no field of the row is a verdict

  Scenario: Edge — a filtered recall is not starved
    When the collection leg runs
    Then filtered.returned_min equals 50

  Scenario: Edge — no fixture directory
    Given MEMORY_BENCH_FIXTURE_DIR unset
    When the collection leg runs
    Then the report's collections.fixture is null and every other row is present

  Scenario: Happy path — a fixture reports per collection
    Given a fixture directory with "tk:b.checks.json" of 10 checks and "tk:b.facts.jsonl"
    When the collection leg runs
    Then collections.fixture holds one row for "tk:b" with checks 10 and precision_at_5, recall_at_5 and mrr in [0,1]

  Scenario: Error — an unreadable fixture
    Given "tk:b.checks.json" holding {"format":2}
    When the collection leg runs
    Then memory-bench exits 2 with FixtureUnreadable naming the file
```

##### Performance, Security, Observability (additions)

- **Performance:** the leg's own wall-clock is bounded by the rig build (20,200 facts, then `3 × 20,000`) and is not a target; the stage-1 timing row is the measurement.
- **Security:** fixtures are data read from a directory the operator names; ids and queries are never executed; the harness keeps its disposable-store guard (SA-BENCH-ISOLATION-01).
- **Observability:** log events `bench.collections.fairness`, `bench.collections.exact_term`, `bench.collections.filtered`, `bench.collections.timing`, `bench.collections.fixture` with the row's numbers.

##### Gaps

None.

---

#### Amendment January 2026 — Graph leg (ADR-021)

> Source: HLD revision 0.12.0, ADR-021; ordin's ADR-111 acceptance runs (criterion 5 budget, criteria 3 and 4 correctness). Counterparts: `memory-store.md` (step 9″, `GraphStore`) and `retrieval-engine.md` (step 6′, the whole-path budget), each amendment January 2026. Every rule below adds to the sections above.

##### Purpose (addition)

A fifth leg, the **graph leg**, measures what link expansion costs and checks that it finds what it should. It times stage-1 with the graph and link lists on a corpus of 10,000 facts, 20,000 relationships and 2,000 entities, once with `MEMORY_GRAPH_HOPS = 1` (the figure ordin's budget reads against) and once with 2 (reported only), on SurrealDB and on the SQL storage over SQLite; and it runs two correctness fixtures — "who supplies packaging?" over 1,000 facts with no matching word, and the A–B–C–D chain with a hop limit of 1.

##### Approach (addition)

As the collection leg, the graph leg stays on C1 with synthetic embeddings and no model provider: the graph and link lists are stage-1 work, and the reranked whole-path p95 with models is measured by ordin's Memory Service performance scenario. The SQL storage is built into the harness behind the crate feature `sql`, so both backends run the same corpus from one binary. Rejected: *a real-world graph sample* — none exists without personal data; *measuring only SurrealDB* — ordin's desktop runs the SQL storage, which is where the budget must hold.

Assumption (Phase 1 1A):

- **SA-BENCH-GRAPH-01** Corpus `MEMORY_BENCH_GRAPH_SHAPE` = facts, relationships, entities (defaults 10,000, 20,000, 2,000): each fact names 1–3 entities chosen with a Zipf distribution (s = 1.1) so a few entities are well connected; each relationship joins two distinct entities with one of 40 kinds; every relationship is asserted by one fact naming both endpoints. 200 queries each naming 1–3 entities by canonical name or alias. All facts team-shared in one team scope; names backfilled before timing.

##### Shared Context (additions)

| Variable | Type | Default | Required | Description |
|---|---|---|---|---|
| `MEMORY_BENCH_GRAPH_SHAPE` | u32 list | `10000,20000,2000` | no | Facts, relationships, entities of the graph leg (SA-BENCH-GRAPH-01). Three positive values, else exit 2. |

`StageOneQuery.graph_hops` / `graph_fanout` / `graph_cap` / `phrase_texts`, `Candidate.graph_hop`, `GraphStore` and `SqlStorage` (ordin change M-2, feature `sql`) are as `memory-store.md` and ordin's Memory Service spec define them.

##### Public Interface (additions)

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct GraphLegReport {
    pub backends: Vec<GraphBackendRow>,        // one per backend: "surrealdb", "sqlite"
    pub supplies_fixture: GraphFixtureRow,     // criterion 3 shape
    pub hop_limit_fixture: GraphFixtureRow,    // criterion 4 shape
}
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct GraphBackendRow { pub backend: String, pub hops1: Timing, pub hops2: Timing, pub graph_off: Timing, pub reached_mean: f64 }
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct GraphFixtureRow { pub backend: String, pub expected_found: bool, pub forbidden_found: bool, pub rank_of_expected: Option<u32> }
```

`BenchReport` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub graph: Option<GraphLegReport>`.

##### Internal Logic (additions)

**graph leg**

1. For each backend (SurrealDB rig at `MEMORY_BENCH_RIG_DIR/graph`; SQLite file at `MEMORY_BENCH_RIG_DIR/graph.sqlite` with `SqlStorage::migrations(SqlDialect::Sqlite)` applied), build SA-BENCH-GRAPH-01's corpus through `put_entity`, `put_fact`, `put_relationship` and `link_fact`, then run `backfill_entity_names` until it returns 0.
2. **Timing.** For each of the 200 queries, build a `StageOneQuery` with `phrase_texts = [query]`, a synthetic query vector, the query's keyword terms and `stage1_k = MEMORY_BENCH_STAGE1_K`; time `recall` with `graph_hops = 1`, with `graph_hops = 2`, and with `graph_hops = 0` and no graph list (the baseline), each over `MEMORY_BENCH_REPS` repetitions after `MEMORY_BENCH_WARMUP`. Record `reached_mean` = the mean number of candidates with `graph_hop ≥ 1` at hops 1.
3. **Supplies fixture.** A fresh rig of 1,000 facts with no fact text containing "supplies", entities Borealis and packaging, and fact F "Borealis provides our cartons" naming both and asserting the link Borealis–supplies–packaging; run the query "who supplies packaging?" with no entity filter; `expected_found` = F among the candidates; `rank_of_expected` = F's rank by `fused_score`.
4. **Hop-limit fixture.** A fresh rig with links A–B, B–C, C–D asserted by fAB, fBC, fCD, and facts fB, fC, fD each naming one entity and sharing no word with the query; query "A" with `graph_hops = 1`; `expected_found` = fB among the candidates; `forbidden_found` = fC or fD among them.
5. Discard the rigs unless `MEMORY_BENCH_RETAIN`. **No verdict is written**: ordin reads `hops1` p95 (stage-1 share) against its budget, and the fixtures against `expected_found = true`, `forbidden_found = false`, `rank_of_expected ≤ 5`.

##### Data Model

N/A — disposable rigs only.

##### Error Table (rows added)

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| `MEMORY_BENCH_GRAPH_SHAPE` without three positive values | 2 | `FixtureUnreadable` | stderr: `MEMORY_BENCH_GRAPH_SHAPE needs facts,relationships,entities` |
| The binary was built without feature `sql` | 2 | `FixtureUnreadable` | stderr: `graph leg needs the sql feature for the SQLite backend`; the SurrealDB row is still measured |

##### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Tenancy Benchmark Harness — graph leg

  Scenario: Happy path — both backends are timed
    Given the default graph shape and a build with feature sql
    When the graph leg runs
    Then graph.backends holds rows "surrealdb" and "sqlite", each with hops1, hops2 and graph_off timings
    And no field of the report is a verdict

  Scenario: Happy path — the supplies fixture (criterion 3)
    When the graph leg runs
    Then supplies_fixture.expected_found is true and rank_of_expected is at most 5 on each backend

  Scenario: Edge — the hop limit fixture (criterion 4)
    When the graph leg runs
    Then hop_limit_fixture.expected_found is true and forbidden_found is false on each backend

  Scenario: Error — a malformed shape
    Given MEMORY_BENCH_GRAPH_SHAPE "10000,20000"
    When the graph leg runs
    Then memory-bench exits 2 with FixtureUnreadable naming MEMORY_BENCH_GRAPH_SHAPE
```

##### Performance, Security, Observability (additions)

- **Performance:** the leg's wall-clock is bounded by two corpus builds of 32,000 records and is not a target; the timing rows are the measurement.
- **Security:** synthetic data only; the disposable-store guard applies (SA-BENCH-ISOLATION-01); the SQLite file lives under the rig directory and is deleted with it.
- **Observability:** log events `bench.graph.timing`, `bench.graph.fixture` with the rows' numbers.

##### Gaps

None.
