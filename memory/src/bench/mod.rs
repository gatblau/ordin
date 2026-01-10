//! C11 — Tenancy Benchmark Harness. A disposable, operator-invoked instrument that measures what a
//! large tenant population costs: the per-namespace footprint, the scheduler sweep, and one tenant's
//! read latency while the population exists alongside it.
//!
//! This module is the harness's **safety layer and configuration**, and it is deliberately built and
//! tested before any code exists that can provision or drop a namespace. The harness is destructive by
//! construction — teardown issues `REMOVE NAMESPACE`, which has no rollback — so the three guards below
//! stand between a mistyped invocation and tens of thousands of dropped namespaces:
//!
//! 1. **`MEMORY_BENCH_STORE_URL` is mandatory and never inherited** from `MEMORY_STORE_PATH` or
//!    `MEMORY_STORE_REMOTE_URL`, so the harness cannot pick up a live deployment's configuration and
//!    opens no connection at all when it is unset (SA-BENCH-ISOLATION-01).
//! 2. **A prefix collision refuses the run** before anything is created: one `INFO FOR ROOT` at
//!    start-up, and any pre-existing namespace matching the configured prefix aborts with exit 2
//!    (SA-BENCH-ISOLATION-02). Without it, `DEFINE NAMESPACE IF NOT EXISTS` would *adopt* a colliding
//!    live namespace, after which teardown's prefix check passes and drops it.
//! 3. **Teardown enumerates only the ids the run generated**, never `list_tenants()` output, and
//!    prefix-checks each one immediately before the drop, so a namespace the harness did not create is
//!    never a drop candidate.
//!
//! The harness takes no inbound request, exposes no listener, and holds no end-user credentials. Its
//! whole security surface is outbound and destructive. Every tenant identifier it generates passes
//! `validate_tenant` before it can reach DDL — identifiers cannot be parameter-bound, so that gate is
//! the only defence and is never bypassed (sql-safety layer rule).

pub mod coresidency;
pub mod corpus;
pub mod latency;
pub mod relevance;
pub mod report;
pub mod rig;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

use crate::bench::report::{engine_from_scheme, BenchReport, HostDisk};
use crate::bench::rig::Rig;
use crate::types::placement::MemoryProfile;
use crate::store::migrate::{map_db_err, validate_tenant, Migrator};
use crate::types::ports::StoreError;

/// Which physical shape the harness provisions a tenant as (OQ-NS-VS-DB).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationShape {
    /// One namespace per tenant — the ADR-011 shape a real deployment uses.
    #[default]
    Namespace,
    /// One database per tenant inside a single namespace — the alternative FU-006 asks about.
    Database,
}

impl IsolationShape {
    /// Parse the configured value. An unknown value is rejected rather than defaulted, so a typo
    /// fails start-up instead of silently measuring the other shape.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "namespace" => Some(Self::Namespace),
            "database" => Some(Self::Database),
            _ => None,
        }
    }

    /// The configured spelling, for logs and the report.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Namespace => "namespace",
            Self::Database => "database",
        }
    }
}

impl std::fmt::Display for IsolationShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything the harness reads from the environment (C11 §Config & env vars).
#[derive(Clone, Debug)]
pub struct BenchConfig {
    /// `MEMORY_BENCH_STORE_URL` — the disposable store. Never inherited from the service's own store
    /// configuration; an empty value is a refusal, not a default.
    pub store_url: String,
    /// `MEMORY_BENCH_POPULATION` — tenant populations, ascending, so the first leg sizes the next.
    pub populations: Vec<u32>,
    /// `MEMORY_BENCH_TENANT_PREFIX` — prefix for generated tenant ids, and the teardown guard.
    pub tenant_prefix: String,
    /// `MEMORY_BENCH_CONCURRENCY` — maximum concurrent provisioning tasks.
    pub concurrency: u32,
    /// `MEMORY_BENCH_ISOLATION` — `namespace` or `database`.
    pub isolation: IsolationShape,
    /// `MEMORY_BENCH_REPORT_PATH` — destination for the run report.
    pub report_path: PathBuf,
    /// `MEMORY_BENCH_SWEEP_SAMPLES` — simulated ticks timed per population.
    pub sweep_samples: u32,
    /// `MEMORY_BENCH_RETAIN` — keep the population on exit instead of dropping it.
    pub retain: bool,
    /// `MEMORY_EMBED_DIM` — substituted into the HNSW index DDL, so the provisioned schema matches a
    /// real deployment's (SA-EMBED-01).
    pub embed_dim: u32,
    /// `MEMORY_BENCH_WARMUP` — repetitions run and discarded before a leg starts measuring. Not
    /// politeness: the first query against a cold index pays for building it, and folding that into a
    /// percentile is what produced the 22x spread FU-024 was raised on.
    pub warmup_reps: u32,
    /// `MEMORY_BENCH_REPS` — repetitions each leg's percentiles are computed over.
    pub measure_reps: u32,
    /// `MEMORY_BENCH_HOST_DISK` — the operator's description of the storage the rig runs on, copied
    /// verbatim into the report. Operator-supplied because the process cannot tell an NVMe device
    /// from a network volume, and an absolute latency figure is dominated by that difference
    /// (RISK-003).
    pub host_disk: String,
    /// `MEMORY_BENCH_QUERIES` — size of the fixed query set each leg averages its measures over.
    pub queries: u32,
    /// `MEMORY_BENCH_CORESIDENTS` — the co-resident user counts the relevance leg sweeps, ascending.
    pub co_residents: Vec<u32>,
    /// `MEMORY_BENCH_VISIBLE` — facts the measured caller owns. Pinned across every point of every
    /// leg at one consumer's memory, so a difference between points is never the caller's own corpus
    /// having grown.
    pub visible: u32,
    /// `MEMORY_BENCH_TOTAL` — facts in the namespace for the relevance leg. Held fixed while the
    /// co-resident count varies, which is what isolates *how many users share the namespace* from
    /// *how large the namespace is* — the latter is the co-residency leg's variable, not this one's.
    pub total: u32,
    /// `MEMORY_BENCH_FRACTIONS` — visible fractions the latency leg sweeps, descending. The namespace
    /// is sized to each one with the visible set pinned, so the fraction is the only thing that moves.
    pub fractions: Vec<f64>,
    /// `MEMORY_BENCH_RIG_DIR` — where the measurement rig's SurrealKV store lives.
    ///
    /// A directory of its own, and **not** the path inside `store_url`: the harness already holds that
    /// endpoint open for its `INFO FOR ROOT` guard, and SurrealKV's exclusive directory lock would
    /// refuse a second open of it. Configured rather than derived from `store_url`, so the recursive
    /// delete in `Rig::discard` never receives a path computed from something else — and `discard`
    /// checks for the engine's `LOCK` file before removing anything either way (RISK-005).
    pub rig_dir: PathBuf,
    /// `MEMORY_BENCH_SIZES` — namespace sizes the co-residency leg sweeps, ascending.
    ///
    /// The co-residency leg's own variable, and the one thing that separates it from the latency
    /// leg: there the *fraction* moves and the namespace size follows from it, which conflates the
    /// cost of a larger visible set with the cost of a larger namespace. Here the visible set is
    /// pinned at `visible` and the namespace grows underneath it, so a difference between two points
    /// is attributable to co-residency alone.
    pub sizes: Vec<u32>,
    /// `MEMORY_BENCH_STAGE1_K` — candidates each stage-1 query asks for, and the numerator's ceiling
    /// in the fill ratio.
    ///
    /// Declared here rather than read from `MEMORY_STAGE1_K`. It carries the same default, but the
    /// harness reads no service configuration key that could tie it to a live deployment's settings
    /// (SA-BENCH-ISOLATION-01); an operator who wants to mirror a deployment sets it explicitly.
    pub stage1_k: u16,
}

/// Read `key` from the environment, falling back to `default` when unset or empty.
fn env_or(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

/// Parse `key` as a `u32`, falling back to `default` when unset, empty, or unparseable. A malformed
/// number is not fatal here: none of these values can widen the destructive surface — the prefix and
/// the store URL can, and both are strings validated on their own path.
fn env_u32(key: &str, default: u32) -> u32 {
    env_or(key, &default.to_string()).parse().unwrap_or(default)
}

/// The raw text of a sweep list: `key` when it is **set**, `default` only when it is **unset**.
///
/// Deliberately not [`env_or`], which folds empty into unset. For a sweep list the two are different
/// intentions: an operator who sets `MEMORY_BENCH_CORESIDENTS=` is asking for that leg not to run, and
/// falling back to a four-point default there runs a leg they asked to skip — against a 20,000-fact
/// corpus, four times over, which is how this was found.
fn env_list_raw(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Parse `key` as a comma-separated `u32` list. Unparseable entries are dropped rather than defaulting
/// the whole list, so one typo in a sweep list costs that point and not the run's shape.
fn env_u32_list(key: &str, default: &str) -> Vec<u32> {
    env_list_raw(key, default)
        .split(',')
        .filter_map(|p| p.trim().parse::<u32>().ok())
        .collect()
}

/// Parse `key` as a comma-separated `f64` list, on the same terms as [`env_u32_list`]. Non-positive
/// and non-finite entries are dropped as well as unparseable ones: a fraction of zero or below sizes
/// no namespace, and a point that cannot be built is not a point that was measured.
fn env_f64_list(key: &str, default: &str) -> Vec<f64> {
    env_list_raw(key, default)
        .split(',')
        .filter_map(|p| p.trim().parse::<f64>().ok())
        .filter(|f| f.is_finite() && *f > 0.0)
        .collect()
}

impl BenchConfig {
    /// Load from the environment. **`MEMORY_BENCH_STORE_URL` is read on its own** and is deliberately
    /// not defaulted from `MEMORY_STORE_PATH` or `MEMORY_STORE_REMOTE_URL`: inheriting the service's
    /// store configuration is precisely the accident SA-BENCH-ISOLATION-01 exists to prevent, and an
    /// unset value must reach `connect` so it can refuse.
    pub fn from_env() -> Self {
        Self {
            store_url: std::env::var("MEMORY_BENCH_STORE_URL").unwrap_or_default(),
            populations: env_u32_list("MEMORY_BENCH_POPULATION", "10000,50000"),
            tenant_prefix: env_or("MEMORY_BENCH_TENANT_PREFIX", "bench"),
            concurrency: env_u32("MEMORY_BENCH_CONCURRENCY", 16),
            isolation: IsolationShape::parse(&env_or("MEMORY_BENCH_ISOLATION", "namespace"))
                .unwrap_or_default(),
            report_path: PathBuf::from(env_or("MEMORY_BENCH_REPORT_PATH", "./bench-report.json")),
            sweep_samples: env_u32("MEMORY_BENCH_SWEEP_SAMPLES", 10),
            retain: env_or("MEMORY_BENCH_RETAIN", "false") == "true",
            embed_dim: env_u32("MEMORY_EMBED_DIM", 1024),
            warmup_reps: env_u32("MEMORY_BENCH_WARMUP", 3),
            measure_reps: env_u32("MEMORY_BENCH_REPS", 9),
            host_disk: env_or("MEMORY_BENCH_HOST_DISK", "unspecified"),
            queries: env_u32("MEMORY_BENCH_QUERIES", 20),
            co_residents: env_u32_list("MEMORY_BENCH_CORESIDENTS", "1,10,100,1000"),
            visible: env_u32("MEMORY_BENCH_VISIBLE", 1000),
            total: env_u32("MEMORY_BENCH_TOTAL", 20000),
            fractions: env_f64_list("MEMORY_BENCH_FRACTIONS", "1.0,0.5,0.25,0.125,0.05,0.01"),
            rig_dir: PathBuf::from(env_or(
                "MEMORY_BENCH_RIG_DIR",
                &std::env::temp_dir().join("memory-bench-rig").to_string_lossy(),
            )),
            sizes: env_u32_list("MEMORY_BENCH_SIZES", "2000,8000,20000,100000"),
            stage1_k: env_u32("MEMORY_BENCH_STAGE1_K", 50).min(u16::MAX as u32) as u16,
        }
    }

    /// The scheme of `store_url`, which is all the report ever records — a remote URL may carry
    /// credentials in its authority, so the full value is never written to a file or a log.
    pub fn store_url_scheme(&self) -> String {
        self.store_url
            .split_once("://")
            .map(|(s, _)| s.to_string())
            .unwrap_or_default()
    }
}

/// Every failure mode of the harness (C11 Error Table). The `Display` text is what reaches stderr;
/// [`BenchError::exit_code`] is what reaches the shell.
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
    ReportWrite {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl BenchError {
    /// The process exit code for this failure (C11 Error Table `Status` column). A **refusal** exits
    /// 2 and a store or IO failure exits 1, so an operator's wrapper script can tell "I stopped you
    /// on purpose" from "the store broke". A measured namespace ceiling is neither — it exits 0 and
    /// is reported, because a ceiling is the answer this instrument exists to find.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::StoreUrlUnset | Self::InvalidTenantId(_) | Self::PrefixCollision { .. } => 2,
            Self::Store(_) | Self::ReportWrite { .. } => 1,
        }
    }
}

/// The harness: a connection to a disposable store plus the configuration that governs it.
pub struct BenchHarness {
    db: Surreal<Any>,
    cfg: BenchConfig,
}

impl BenchHarness {
    /// Open the disposable store named by `cfg.store_url`.
    ///
    /// Refuses with [`BenchError::StoreUrlUnset`] when the URL is empty, **before** opening any
    /// connection — the refusal has to precede the connection, or a run that inherited a live
    /// deployment's configuration would already be attached to it by the time anyone noticed.
    ///
    /// Signs in as root when the endpoint is remote and both `MEMORY_STORE_REMOTE_USER` and
    /// `MEMORY_STORE_REMOTE_PASS` are set, matching `Store::connect`'s conditional signin (FU-019).
    /// An embedded endpoint and an unauthenticated remote one both skip it.
    pub async fn connect(cfg: BenchConfig) -> Result<Self, BenchError> {
        if cfg.store_url.trim().is_empty() {
            return Err(BenchError::StoreUrlUnset);
        }
        let db = surrealdb::engine::any::connect(cfg.store_url.clone())
            .await
            .map_err(map_db_err)?;
        let remote = cfg.store_url.starts_with("ws") || cfg.store_url.starts_with("http");
        if remote {
            if let (Ok(user), Ok(pass)) = (
                std::env::var("MEMORY_STORE_REMOTE_USER"),
                std::env::var("MEMORY_STORE_REMOTE_PASS"),
            ) {
                if !user.is_empty() && !pass.is_empty() {
                    db.signin(surrealdb::opt::auth::Root {
                        username: user,
                        password: pass,
                    })
                    .await
                    .map_err(map_db_err)?;
                }
            }
        }
        tracing::info!(
            target: "memory-bench",
            scheme = %cfg.store_url_scheme(),
            isolation = %cfg.isolation,
            "bench.connect"
        );
        Ok(Self { db, cfg })
    }

    /// The configuration this harness was opened with.
    pub fn config(&self) -> &BenchConfig {
        &self.cfg
    }

    /// Run the measurement legs and write the report to `cfg.report_path`.
    ///
    /// **The report is written whether or not any leg produced a row.** A run that measured nothing
    /// and a run whose numbers never reached the disk are different events, and an empty or absent
    /// file cannot tell an operator which one happened — so the shell naming the engine, the engine's
    /// own version and the host is written either way, and the legs fill it in.
    ///
    /// The legs run in the order they were built, which is also the order they depend on each other
    /// for meaning: relevance first, because a latency win on a strategy that returns the wrong rows
    /// is not a win; then latency across visible fractions; then the namespace growing underneath a
    /// pinned visible set. Each is skipped when its own sweep list is empty, so an operator can run
    /// one leg without running the other two.
    pub async fn run(&self) -> Result<BenchReport, BenchError> {
        let scheme = self.cfg.store_url_scheme();
        let mut report = BenchReport::new(
            engine_from_scheme(&scheme),
            self.engine_version().await,
            HostDisk::describe(self.cfg.host_disk.clone()),
            self.cfg.embed_dim,
            scheme,
        );
        // The shell goes to disk before any leg runs, so a run that dies mid-leg still leaves a file
        // naming the engine it died on rather than nothing at all. It is rewritten with the rows below.
        report.write(&self.cfg.report_path)?;

        self.relevance_leg(&mut report).await?;
        self.latency_leg(&mut report).await?;
        self.co_residency_leg(&mut report).await?;

        report.write(&self.cfg.report_path)?;
        tracing::info!(
            target: "memory-bench",
            engine = %report.engine,
            engine_version = %report.engine_version,
            relevance_rows = report.relevance.len(),
            latency_rows = report.latency.len(),
            co_residency_rows = report.co_residency.len(),
            "bench.report.written"
        );
        Ok(report)
    }

    /// The relevance leg (RFC 03 acceptance criterion 3): both strategies over a fixed query set, at
    /// each configured co-resident user count, against one measurement rig.
    ///
    /// The rig opens under [`MemoryProfile::Individual`]. The profile decides the *read filter*, and
    /// the strategy is passed separately — so this pairs the narrow owner equality with the
    /// `fact_owner_user` index hint, which is the filter-first arm RFC 03 actually proposes, and it
    /// puts the ANN arm behind the same filter so the two differ in strategy alone. Under the
    /// collective profile filter-first would run unhinted, which measures a different query.
    ///
    /// The corpus namespace is a `generate_ids` tenant, so it passes `validate_tenant` at the one
    /// place ids are minted rather than at the point it reaches DDL.
    async fn relevance_leg(&self, report: &mut BenchReport) -> Result<(), BenchError> {
        if self.cfg.co_residents.is_empty() {
            tracing::info!(target: "memory-bench", "bench.relevance.skipped");
            return Ok(());
        }
        let tenant = self
            .generate_ids(1)?
            .pop()
            .ok_or_else(|| BenchError::InvalidTenantId(self.cfg.tenant_prefix.clone()))?;

        let mut result = Ok(());
        for &n in &self.cfg.co_residents {
            // A rig of its own per point, opened and closed in sequence. Rebuilding the corpus in
            // place across points is measurably wrong: the approximate arm degrades with the number
            // of rewrites the HNSW index has absorbed, so sweeping `1,10` and `10,1` both showed the
            // *first* point winning regardless of its co-resident count. The exact arm was unmoved,
            // which is what identified the index state rather than the corpus as the cause. Sequential
            // and not concurrent because SurrealKV holds an exclusive directory lock (FU-005).
            let dir = self.cfg.rig_dir.join(format!("coresidents-{n:07}"));
            let rig = match Rig::open_surrealkv(&dir, self.cfg.embed_dim, MemoryProfile::Individual)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };

            let point = crate::bench::relevance::measure_point(
                &rig,
                &self.cfg,
                &tenant,
                self.cfg.total,
                self.cfg.visible,
                n,
            )
            .await;

            if self.cfg.retain {
                tracing::info!(target: "memory-bench", co_residents = n, "bench.rig.retained");
            } else if let Err(e) = rig.discard() {
                // Not fatal: the numbers are already taken, and a scratch directory that outlives the
                // run is an operator's tidy-up rather than a lost measurement.
                tracing::warn!(target: "memory-bench", error = %e, "bench.rig.discard_failed");
            }

            match point {
                Ok(rows) => report.relevance.extend(rows),
                Err(e) => {
                    // Stop at the first failure but keep the points already measured: a leg that
                    // discarded four good rows because the fifth failed would have thrown away the
                    // expensive part of the run.
                    result = Err(e);
                    break;
                }
            }
        }

        // The parent holds only the per-point directories, so a non-recursive remove tidies up when
        // they have all gone and leaves anything unexpected exactly where it is.
        if !self.cfg.retain {
            std::fs::remove_dir(&self.cfg.rig_dir).ok();
        }
        result
    }

    /// The absolute-latency leg (RFC 03 acceptance criterion 4): both strategies over the fixed query
    /// set at each configured visible fraction, with the visible set pinned at one consumer's memory.
    ///
    /// A rig per fraction, opened and closed in sequence — each fraction is a different corpus size,
    /// and reusing one store would measure how many rewrites the index had absorbed (see the relevance
    /// leg) as well as colliding with SurrealKV's exclusive directory lock (FU-005).
    ///
    /// The crossover is derived once, from every row the leg produced, rather than accumulated as the
    /// sweep runs: it is a property of the grid as a whole, and a partial grid would bracket the wrong
    /// pair. A leg that stopped early still records the crossover of the points it reached, with the
    /// grid naming exactly those points.
    async fn latency_leg(&self, report: &mut BenchReport) -> Result<(), BenchError> {
        if self.cfg.fractions.is_empty() {
            tracing::info!(target: "memory-bench", "bench.latency.skipped");
            return Ok(());
        }
        let tenant = self
            .generate_ids(1)?
            .pop()
            .ok_or_else(|| BenchError::InvalidTenantId(self.cfg.tenant_prefix.clone()))?;

        let mut result = Ok(());
        let mut rows: Vec<crate::bench::report::LatencyLeg> = Vec::new();
        for (i, &fraction) in self.cfg.fractions.iter().enumerate() {
            // Indexed rather than named by the fraction: `0.125` is not a directory name anyone wants
            // to debug, and two fractions that round to the same string would collide on one path.
            let dir = self.cfg.rig_dir.join(format!("fraction-{i:03}"));
            let rig = match Rig::open_surrealkv(&dir, self.cfg.embed_dim, MemoryProfile::Individual)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };

            let point = crate::bench::latency::measure_point(
                &rig,
                &self.cfg,
                &tenant,
                self.cfg.visible,
                fraction,
            )
            .await;

            if self.cfg.retain {
                tracing::info!(target: "memory-bench", fraction, "bench.rig.retained");
            } else if let Err(e) = rig.discard() {
                tracing::warn!(target: "memory-bench", error = %e, "bench.rig.discard_failed");
            }

            match point {
                Ok(measured) => rows.extend(measured),
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }

        report.crossover = Some(crate::bench::latency::crossover_from(&rows));
        report.latency.extend(rows);
        if !self.cfg.retain {
            std::fs::remove_dir(&self.cfg.rig_dir).ok();
        }
        result
    }

    /// The co-residency leg: both strategies over the fixed query set at each configured namespace
    /// size, with the visible set pinned at one consumer's memory.
    ///
    /// **The distinction from the latency leg is the whole point of running both.** There the
    /// fraction moves and the namespace size follows from it, so a difference between two points
    /// could be the larger namespace or the smaller fraction and nothing separates them. Here only
    /// the namespace grows, which is what makes "does filter-first's cost track the visible set while
    /// the index walk's tracks the namespace" a question the rows can answer.
    ///
    /// A rig per size, opened and closed in sequence, for the reasons the two legs above state: a
    /// rebuilt corpus measures the rewrites the index absorbed rather than the swept variable, and
    /// SurrealKV holds an exclusive directory lock for the process lifetime (FU-005).
    ///
    /// **The largest configured sizes are operator-only.** The default list reaches 100,000 facts,
    /// which is well beyond a CI budget at the corpus builder's two round trips per fact (FU-008);
    /// the list is configuration, so a CI run sets a smaller one rather than the leg carrying a
    /// second, quieter definition of how big it is willing to get.
    async fn co_residency_leg(&self, report: &mut BenchReport) -> Result<(), BenchError> {
        if self.cfg.sizes.is_empty() {
            tracing::info!(target: "memory-bench", "bench.coresidency.skipped");
            return Ok(());
        }
        let tenant = self
            .generate_ids(1)?
            .pop()
            .ok_or_else(|| BenchError::InvalidTenantId(self.cfg.tenant_prefix.clone()))?;

        let mut result = Ok(());
        for (i, &size) in self.cfg.sizes.iter().enumerate() {
            // Indexed rather than named by the size, matching the latency leg: two configured sizes
            // that clamp to the same corpus would otherwise collide on one path.
            let dir = self.cfg.rig_dir.join(format!("size-{i:03}"));
            let rig = match Rig::open_surrealkv(&dir, self.cfg.embed_dim, MemoryProfile::Individual)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };

            let point =
                crate::bench::coresidency::measure_point(&rig, &self.cfg, &tenant, size).await;

            if self.cfg.retain {
                tracing::info!(target: "memory-bench", size, "bench.rig.retained");
            } else if let Err(e) = rig.discard() {
                tracing::warn!(target: "memory-bench", error = %e, "bench.rig.discard_failed");
            }

            match point {
                Ok(rows) => report.co_residency.extend(rows),
                Err(e) => {
                    // The sizes already measured stay on `report`, as in the two legs above. Whether
                    // they reach the disk is `run`'s error path, which propagates before the final
                    // write — so on a failure they survive in memory and are lost to the file
                    // (FU-016).
                    result = Err(e);
                    break;
                }
            }
        }

        if !self.cfg.retain {
            std::fs::remove_dir(&self.cfg.rig_dir).ok();
        }
        result
    }

    /// The engine's own version, asked of the engine rather than assumed from the linked crate.
    ///
    /// Non-fatal: an endpoint that will not answer the probe yields `unknown`, because a run that
    /// took its measurements should not be discarded over a label. It is never filled in from the
    /// crate version — the two agree for an embedded store and need not for a remote one, and a
    /// figure quoted against an engine version nobody checked is the failure RISK-002 describes.
    async fn engine_version(&self) -> String {
        match self.db.version().await {
            Ok(v) => v.to_string(),
            Err(e) => {
                tracing::warn!(target: "memory-bench", error = %e, "bench.engine_version.unknown");
                "unknown".to_string()
            }
        }
    }

    /// A `Migrator` bound to this harness's connection, at the configured embedding dimension.
    pub fn migrator(&self) -> Migrator<'_> {
        Migrator::new(&self.db, self.cfg.embed_dim)
    }

    /// The tenant ids this run owns, in creation order: `{prefix}{i:07}`.
    ///
    /// Deterministic from the configuration alone, which is what lets teardown enumerate the run's own
    /// ids without consulting the store. Every id passes `validate_tenant` here, at the one place they
    /// are minted, rather than at each of the sites that later interpolates one into DDL.
    pub fn generate_ids(&self, count: u32) -> Result<Vec<String>, BenchError> {
        let mut ids = Vec::with_capacity(count as usize);
        for i in 0..count {
            let id = format!("{}{:07}", self.cfg.tenant_prefix, i);
            validate_tenant(&id).map_err(|_| BenchError::InvalidTenantId(id.clone()))?;
            ids.push(id);
        }
        Ok(ids)
    }

    /// Refuse the run when any namespace already matching `cfg.tenant_prefix` exists
    /// (SA-BENCH-ISOLATION-02). One `INFO FOR ROOT`, before anything is created or dropped.
    ///
    /// This closes the one route by which teardown destroys data it did not create even when every
    /// other guard holds: `DEFINE NAMESPACE IF NOT EXISTS` adopts a colliding pre-existing namespace,
    /// and teardown's prefix check then passes on it.
    pub async fn preflight(&self) -> Result<(), BenchError> {
        for existing in self.list_namespaces().await? {
            if existing.starts_with(&self.cfg.tenant_prefix) {
                tracing::error!(target: "memory-bench", "bench.refused.prefix_collision");
                return Err(BenchError::PrefixCollision { existing });
            }
        }
        Ok(())
    }

    /// Drop every namespace this run generated ids for, and nothing else. Returns how many were
    /// dropped.
    ///
    /// Three properties matter and each is load-bearing. The candidate list comes from
    /// [`generate_ids`](Self::generate_ids), **not** from the store, so a namespace the harness did not
    /// create is never considered. Every id is prefix-checked immediately before its drop, so an id
    /// that somehow lacks the prefix is skipped rather than dropped — logged and non-fatal, because one
    /// unexpected id must not strand the rest of the cleanup. And `cfg.retain` skips the whole thing,
    /// for an operator who wants the population for a follow-up run.
    pub async fn teardown(&self, count: u32) -> Result<u32, BenchError> {
        if self.cfg.retain {
            tracing::info!(target: "memory-bench", population = count, "bench.teardown.retained");
            return Ok(0);
        }
        let mut dropped = 0u32;
        for tenant in self.generate_ids(count)? {
            if !tenant.starts_with(&self.cfg.tenant_prefix) {
                tracing::warn!(target: "memory-bench", "bench.teardown.skipped");
                continue;
            }
            // `validate_tenant` has already passed in `generate_ids`; it is the only gate on this
            // interpolation, so it runs before the id is minted rather than being assumed here.
            self.db
                .query(format!("REMOVE NAMESPACE IF EXISTS {tenant}"))
                .await
                .map_err(map_db_err)?
                .check()
                .map_err(map_db_err)?;
            dropped += 1;
        }
        tracing::info!(target: "memory-bench", dropped, "bench.teardown.done");
        Ok(dropped)
    }

    /// Namespace ids currently defined at the root, via `INFO FOR ROOT`.
    ///
    /// The harness's own read of the root, rather than `MemoryStore::list_tenants`, because it holds a
    /// bare connection and not a `Store` — C11 provisions schemas, it does not serve memory.
    pub async fn list_namespaces(&self) -> Result<Vec<String>, BenchError> {
        let mut resp = self.db.query("INFO FOR ROOT").await.map_err(map_db_err)?;
        // `INFO FOR ROOT` is root-level and carries no `USE` prefix, so its rows are at slot 0.
        let info: Vec<serde_json::Value> = resp
            .take(0)
            .map_err(|e| StoreError::Internal(format!("list namespaces: {e}")))?;
        Ok(info
            .first()
            .and_then(|v| v.get("namespaces"))
            .and_then(|v| v.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(url: &str) -> BenchConfig {
        BenchConfig {
            store_url: url.into(),
            populations: vec![200],
            tenant_prefix: "bench".into(),
            concurrency: 4,
            isolation: IsolationShape::Namespace,
            report_path: PathBuf::from("./bench-report.json"),
            sweep_samples: 2,
            retain: false,
            embed_dim: 8,
            warmup_reps: 1,
            measure_reps: 3,
            host_disk: "unspecified".into(),
            queries: 2,
            co_residents: vec![1, 2],
            visible: 10,
            total: 40,
            fractions: vec![1.0, 0.5],
            sizes: vec![40, 80],
            rig_dir: PathBuf::from("/tmp/memory-bench-rig-unit"),
            stage1_k: 10,
        }
    }

    /// An explicitly emptied sweep list means "do not run this leg", and is not the same thing as an
    /// unset one. Folding the two together runs a leg the operator asked to skip.
    #[test]
    fn an_emptied_sweep_list_is_empty_and_an_unset_one_is_the_default() {
        let key = "MEMORY_BENCH_TEST_SWEEP_LIST";
        std::env::remove_var(key);
        assert_eq!(env_u32_list(key, "1,10"), vec![1, 10], "unset takes the default");
        std::env::set_var(key, "");
        assert!(env_u32_list(key, "1,10").is_empty(), "emptied means no points");
        std::env::set_var(key, "2, 4 ,nonsense,8");
        assert_eq!(
            env_u32_list(key, "1,10"),
            vec![2, 4, 8],
            "one unparseable entry costs that point, not the list"
        );
        std::env::set_var(key, "1.0,0.5,-1,0,abc");
        assert_eq!(
            env_f64_list(key, "1.0"),
            vec![1.0, 0.5],
            "a fraction at or below zero sizes no namespace and is not a point"
        );
        std::env::remove_var(key);
    }

    #[test]
    fn isolation_shape_accepts_exactly_two_values_and_rejects_anything_else() {
        assert_eq!(
            IsolationShape::parse("namespace"),
            Some(IsolationShape::Namespace)
        );
        assert_eq!(
            IsolationShape::parse("database"),
            Some(IsolationShape::Database)
        );
        assert_eq!(IsolationShape::parse("Namespace"), None);
        assert_eq!(IsolationShape::parse("ns"), None);
        assert_eq!(IsolationShape::parse(""), None);
    }

    /// The report records the scheme and never the authority, because a remote URL may carry
    /// credentials there.
    #[test]
    fn only_the_scheme_is_recoverable_from_a_store_url() {
        assert_eq!(cfg("ws://user:pass@host:8000").store_url_scheme(), "ws");
        assert_eq!(
            cfg("surrealkv:///tmp/bench-store").store_url_scheme(),
            "surrealkv"
        );
        assert_eq!(cfg("").store_url_scheme(), "");
    }

    /// Refusals exit 2 and failures exit 1, so an operator's wrapper can tell them apart.
    #[test]
    fn refusals_and_failures_carry_different_exit_codes() {
        assert_eq!(BenchError::StoreUrlUnset.exit_code(), 2);
        assert_eq!(BenchError::InvalidTenantId("x".into()).exit_code(), 2);
        assert_eq!(
            BenchError::PrefixCollision {
                existing: "bench0000001".into()
            }
            .exit_code(),
            2
        );
        assert_eq!(BenchError::Store(StoreError::Timeout).exit_code(), 1);
    }

    /// The refusal must precede the connection: `connect` on an unset URL opens nothing, so a run that
    /// would have inherited a live deployment's configuration is never attached to it.
    #[tokio::test]
    async fn connect_refuses_an_unset_store_url_without_opening_anything() {
        for url in ["", "   "] {
            match BenchHarness::connect(cfg(url)).await {
                Err(BenchError::StoreUrlUnset) => {}
                other => panic!("expected StoreUrlUnset for {url:?}, got {:?}", other.err()),
            }
        }
    }

    #[tokio::test]
    async fn generated_ids_are_ordered_prefixed_and_valid_tenants() {
        let h = BenchHarness::connect(cfg("mem://")).await.unwrap();
        let ids = h.generate_ids(3).unwrap();
        assert_eq!(ids, vec!["bench0000000", "bench0000001", "bench0000002"]);
        for id in &ids {
            validate_tenant(id).expect("every generated id passes the DDL gate");
        }
    }

    /// A prefix that cannot be a legal namespace identifier is refused at id generation, before any
    /// DDL could receive it — `validate_tenant` is the only gate on that interpolation.
    #[tokio::test]
    async fn an_illegal_prefix_is_refused_at_id_generation() {
        let mut c = cfg("mem://");
        c.tenant_prefix = "bench; REMOVE NAMESPACE acme --".into();
        let h = BenchHarness::connect(c).await.unwrap();
        match h.generate_ids(1) {
            Err(BenchError::InvalidTenantId(id)) => assert!(id.starts_with("bench;")),
            other => panic!("expected InvalidTenantId, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn retain_skips_teardown_entirely() {
        let mut c = cfg("mem://");
        c.retain = true;
        let h = BenchHarness::connect(c).await.unwrap();
        assert_eq!(h.teardown(5).await.unwrap(), 0);
    }
}
