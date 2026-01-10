//! The measurement rig — a real embedded SurrealKV store plus the timing discipline that makes an
//! absolute latency reading mean something.
//!
//! This exists because of a specific failure. The in-repo benchmarks ran against an in-process
//! `mem://` engine and measured the same nominal configuration at **5.76 ms in one leg and 126.59 ms
//! in another** — a 22× spread attributable to index warmth. Only the within-leg comparisons were
//! sound, so Stage 1's own gate ("stage-1 latency inside NFR-P3 at 1,000 visible facts") could not be
//! judged at all, because NFR-P3 is an absolute claim about the SurrealKV-backed deployment (FU-024).
//!
//! Two things follow, and both are structural rather than advisory:
//!
//! * **The rig is SurrealKV at a real path, not `mem://`.** It is the configured default backend and
//!   the engine the binary links, so a number taken here is a number about the deployed store.
//! * **[`Timing`] reports its own spread.** Every measurement carries `min_max_ratio` alongside the
//!   percentiles. A harness that reports only a median can reproduce the 22× spread and show a
//!   perfectly steady-looking figure while doing it; this one cannot hide it.
//!
//! The rig is deliberately **not** containerised, unlike the harness's safety-gate tests. NFR-P3 is a
//! claim about the in-process embedded engine, and putting a network hop in front of it would measure
//! something else.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::bench::corpus;
use crate::bench::BenchError;
use crate::store::{Store, VectorStrategy};
use crate::types::placement::MemoryProfile;
use crate::types::scope::ScopeContext;

/// One timing sample set, reduced. Carried into the report verbatim.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Timing {
    /// Repetitions run and discarded before measurement began.
    pub warmup_reps: u32,
    /// Repetitions the percentiles below are computed over.
    pub measure_reps: u32,
    pub p50_millis: f64,
    pub p95_millis: f64,
    pub p99_millis: f64,
    pub min_millis: f64,
    pub max_millis: f64,
    /// `max / min`. The instrument's own instability, reported rather than averaged away — this is
    /// the number that was 22 on the rig FU-024 rejects. `1.0` when a single rep was run.
    pub min_max_ratio: f64,
}

impl Timing {
    /// Run `f` `warmup_reps` times discarding the results, then `measure_reps` times recording each
    /// duration, and reduce.
    ///
    /// The warm-up is not politeness: the first query against a cold index pays for building it, and
    /// including that cost in a percentile is what produced a 22× spread between two runs of the same
    /// configuration. `measure_reps` is clamped to at least 1, so a caller cannot ask for a reduction
    /// over nothing and receive zeros that read as a fast result.
    pub async fn measure<F, Fut, T>(warmup_reps: u32, measure_reps: u32, mut f: F) -> Self
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        for _ in 0..warmup_reps {
            let _ = f().await;
        }
        let reps = measure_reps.max(1);
        let mut samples = Vec::with_capacity(reps as usize);
        for _ in 0..reps {
            let t = Instant::now();
            let _ = f().await;
            samples.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let min = samples[0];
        let max = samples[samples.len() - 1];
        Self {
            warmup_reps,
            measure_reps: reps,
            p50_millis: percentile(&samples, 0.50),
            p95_millis: percentile(&samples, 0.95),
            p99_millis: percentile(&samples, 0.99),
            min_millis: min,
            max_millis: max,
            // A sub-microsecond floor on **both** sides. On the denominator it keeps a zero minimum
            // from making the ratio infinite. On the numerator it is what makes the degenerate case
            // report the spread it actually has: when every sample rounds to exactly 0.0 — a
            // workload below the timer's resolution — flooring the denominator alone yields
            // `0.0 / 1e-6 = 0.0`, and a spread of zero reads as an impossibly stable measurement in
            // the one field that exists to make instability visible. Flooring both yields
            // `1e-6 / 1e-6 = 1.0`, which is what a single sample means. No real measurement moves:
            // above the floor the two terms are the raw samples.
            min_max_ratio: max.max(1e-6) / min.max(1e-6),
        }
    }
}

/// Nearest-rank percentile over an ascending sample set. `q` in `[0,1]`.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// A store opened for measurement, and the directory it lives in.
pub struct Rig {
    store: Store,
    dir: PathBuf,
    embed_dim: u32,
}

impl Rig {
    /// Open a SurrealKV-backed store at `dir` under an explicit profile.
    ///
    /// The store is a real one on disk, which is the property that distinguishes this from
    /// `Store::new_in_memory_with_profile` and the reason the rig exists — an absolute latency claim
    /// measured on `mem://` is not a claim about the deployed engine.
    ///
    /// **One rig per directory, for the life of the process.** SurrealKV takes an exclusive file lock
    /// on the directory and the embedded engine holds it until the process exits — dropping the `Rig`
    /// does not release it. A second open of a held path fails with `LOCK is already locked by
    /// another process` rather than blocking. A leg that measures several corpus sizes must therefore
    /// give each rig **its own directory**; reusing one path across sizes will not work.
    pub async fn open_surrealkv(
        dir: impl AsRef<Path>,
        embed_dim: u32,
        profile: MemoryProfile,
    ) -> Result<Self, BenchError> {
        let dir = dir.as_ref().to_path_buf();
        let store = Store::open(
            &format!("surrealkv://{}", dir.display()),
            embed_dim,
            profile,
        )
        .await?;
        Ok(Self {
            store,
            dir,
            embed_dim,
        })
    }

    /// The store under measurement.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Where the store's files live.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// The embedding dimension the store was provisioned at.
    ///
    /// The corpus builder needs it and must not be told it separately: `set_fact_embedding` rejects
    /// a vector whose length differs from the store's dimension, so a corpus built at a dimension the
    /// rig was not opened at fails at the first write rather than measuring the wrong index.
    pub fn embed_dim(&self) -> u32 {
        self.embed_dim
    }

    /// Time one strategy over a fixed query set, one sample per query per repetition.
    ///
    /// The plan's `Rig::timed`. One sample per *query* rather than per pass over the set, because
    /// NFR-P3 is a claim about a query; and the counter walks the set so no single query's
    /// neighbourhood dominates the distribution.
    ///
    /// **Failures are counted inside the timed section, not screened out before it.**
    /// [`Timing::measure`] discards its closure's output, errors included, so a leg whose queries all
    /// failed would report a beautifully fast set of percentiles for a query that never ran. A
    /// checked pass *before* the timing would catch that — and would also warm the index, which would
    /// silently make `MEMORY_BENCH_WARMUP=0` not cold and take the one thing an operator sets it to
    /// see. So the closure records failures instead, and a non-zero count is re-run afterwards to
    /// surface the real error.
    ///
    /// `pub(crate)`, not `pub`: it names `VectorStrategy`, which is `pub(crate)` (rule C12), and
    /// widening a store internal so a timing helper can be called from outside the crate would be the
    /// worse trade.
    pub(crate) async fn time_query_set(
        &self,
        ctx: &ScopeContext,
        queries: &[Vec<f32>],
        stage1_k: u16,
        strategy: VectorStrategy,
        warmup_reps: u32,
        measure_reps: u32,
    ) -> Result<Timing, BenchError> {
        if queries.is_empty() {
            return Ok(Timing::default());
        }

        let next = std::cell::Cell::new(0usize);
        let failures = std::cell::Cell::new(0u32);
        let timing = Timing::measure(
            warmup_reps,
            measure_reps.max(1) * queries.len() as u32,
            || {
                let qv = &queries[next.get() % queries.len()];
                next.set(next.get() + 1);
                let q = corpus::stage_one(qv, ctx, stage1_k);
                let failures = &failures;
                async move {
                    let r = self.store.recall_vector_only(ctx, &q, strategy).await;
                    if r.is_err() {
                        failures.set(failures.get() + 1);
                    }
                    r
                }
            },
        )
        .await;

        if failures.get() > 0 {
            // Re-run one query outside the timing to recover the error itself: the counter proves a
            // failure happened, and the operator needs to know which.
            self.store
                .recall_vector_only(ctx, &corpus::stage_one(&queries[0], ctx, stage1_k), strategy)
                .await
                .map_err(BenchError::Store)?;
            return Err(BenchError::Store(crate::types::ports::StoreError::Internal(
                format!(
                    "{} of {} timed queries failed but a repeat succeeded; the timing is not \
                     trustworthy",
                    failures.get(),
                    timing.measure_reps
                ),
            )));
        }
        Ok(timing)
    }

    /// Remove the rig's directory.
    ///
    /// Explicit rather than a `Drop` impl. The harness's whole discipline is that it destroys only
    /// what it was told to destroy, and a directory that deletes itself when a value goes out of scope
    /// is the opposite of that — a caller who passed a path they cared about would lose it to a
    /// control-flow edge they never wrote.
    ///
    /// **Refuses any directory that does not hold the engine's `LOCK` file** (RISK-005). This is a
    /// recursive delete of a caller-supplied path, and from plan 06 Phase 3 the path comes from
    /// configuration (`MEMORY_BENCH_RIG_DIR`) rather than being allocated by the caller — which is the
    /// condition RISK-005 named as the point at which the guard becomes necessary. It is the same
    /// discipline `teardown` applies to a tenant id, applied to a filesystem path: check what you are
    /// about to destroy immediately before destroying it. A content check rather than a name check,
    /// because a naming convention can be satisfied by a directory that is not a store, and cannot be
    /// satisfied by one that is.
    pub fn discard(self) -> std::io::Result<()> {
        if !self.dir.join("LOCK").exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "refusing to remove {}: no SurrealKV LOCK file, so it is not a rig directory",
                    self.dir.display()
                ),
            ));
        }
        std::fs::remove_dir_all(&self.dir)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::store::VectorStrategy;
    use crate::types::domain::{Fact, MemoryClass, Visibility};
    use crate::types::ports::{MemoryStore, RecallFilters, StageOneQuery};
    use crate::types::scope::{OpSet, ScopeContext, ScopeRef};

    const DIM: u32 = 8;

    fn ctx(user: &str) -> ScopeContext {
        ScopeContext {
            tenant: "acme".into(),
            teams: vec![],
            user: user.into(),
            token_jti: "jti".into(),
            allowed_ops: OpSet {
                read: true,
                write: true,
                forget: true,
            },
            client: None,
            fact_authority: None,
            correlation_id: "c-rig".into(),
        }
    }

    fn fact(id: &str, user: &str) -> Fact {
        Fact {
            id: id.into(),
            content: serde_json::json!({"subject": "s", "predicate": "p", "object": "o"}),
            entities: vec!["entity:e1".into()],
            source_id: None,
            memory_class: MemoryClass::Semantic,
            fact_class: None,
            visibility: Visibility::UserPrivate,
            owner: ScopeRef {
                tenant: "acme".into(),
                team: None,
                user: user.into(),
            },
            valid_from: Utc::now(),
            valid_to: None,
            ingested_at: Utc::now(),
            confidence: 0.9,
            salience: 0.7,
            stability: 1.0,
            pii_review: false,
            supersedes: None,
            superseded_by: None,
            derived_from: vec![],
            last_recalled_at: None,
        }
    }

    /// The seam must run the same query production runs. It is the same function underneath —
    /// `recall` and `recall_vector_only` both call `Store::vector_leg` — and this asserts that stays
    /// true through the public path, because a benchmark whose query has drifted reports numbers that
    /// are worse than missing.
    ///
    /// In-crate rather than in `tests/bench_harness.rs`: `recall_vector_only` is `pub(crate)` (rule
    /// C12), and widening a measurement seam to `pub` purely so an external test could see it would
    /// be a worse trade than running this one under `cargo test --lib`.
    #[tokio::test]
    async fn vector_only_seam_agrees_with_the_public_recall_path() {
        for profile in [MemoryProfile::Collective, MemoryProfile::Individual] {
            let s = Store::new_in_memory_with_profile(DIM, profile)
                .await
                .unwrap();
            let c = ctx("u-me");
            // A high visible fraction, so neither strategy starves and the two are comparable.
            for i in 0..20 {
                let id = format!("fact:s{i}");
                s.put_fact(&fact(&id, "u-me")).await.unwrap();
                let mut v = vec![0.0_f32; DIM as usize];
                v[0] = 1.0;
                v[1] = i as f32 * 0.01;
                s.set_fact_embedding(&c, &id, &v, "m1").await.unwrap();
            }

            let mut qv = vec![0.0_f32; DIM as usize];
            qv[0] = 1.0;
            let q = StageOneQuery {
                query_vector: qv,
                // No keyword terms and no entity filter, so the only signal contributing to `memory`
                // is the vector leg and the comparison is like for like.
                keyword_terms: vec![],
                filters: RecallFilters::default(),
                scope: c.clone(),
                stage1_k: 10,
            };

            let seam = s
                .recall_vector_only(&c, &q, VectorStrategy::for_profile(profile))
                .await
                .unwrap();
            let public = s.recall(&c, &q).await.unwrap();

            let seam_ids: std::collections::BTreeSet<String> =
                seam.iter().map(|(fid, _)| format!("fact:{fid}")).collect();
            let public_ids: std::collections::BTreeSet<String> =
                public.iter().map(|c| c.fact_id.clone()).collect();
            assert!(!seam_ids.is_empty(), "{profile}: the seam returned nothing");
            assert_eq!(
                seam_ids, public_ids,
                "{profile}: the seam and the public path must select the same facts"
            );

            // `memory` folds its candidates through a map keyed by id, so its output is id-ordered
            // and cannot be compared for rank. The seam's own ordering is the measurable one, and it
            // must be by descending similarity — a latency leg that timed an unordered query would
            // not be timing a retrieval.
            for w in seam.windows(2) {
                assert!(
                    w[0].1 >= w[1].1,
                    "{profile}: seam results must be ordered best-first"
                );
            }
            // The scores the public path reports for those facts are the seam's scores.
            for cand in &public {
                let fid = cand.fact_id.trim_start_matches("fact:");
                let (_, sim) = seam.iter().find(|(f, _)| f == fid).expect("seam has it");
                assert!(
                    (cand.semantic_score - sim).abs() < 1e-9,
                    "{profile}: {fid} scored {} publicly and {sim} through the seam",
                    cand.semantic_score
                );
            }
        }
    }

    /// A caller asking for zero measured reps gets one, not a reduction over nothing — zeros in a
    /// latency report read as a fast result.
    ///
    /// The `min_max_ratio` assertion caught a real defect and did so intermittently, which is worse
    /// than not catching it: the floor was applied to the denominator alone, so a no-op closure whose
    /// elapsed time rounded to exactly `0.0` reduced to `0.0 / 1e-6 = 0.0` — a reported spread of
    /// zero, in the one field that exists to make instability visible. It reproduced about once in
    /// twenty-five runs, because the sample only sometimes rounds to zero. Flooring both terms makes
    /// the single-rep case exact rather than probable: with one sample `max == min`, so the ratio is
    /// `1.0` whatever the magnitude.
    #[tokio::test]
    async fn a_zero_rep_request_still_measures_once() {
        let t = Timing::measure(0, 0, || async {}).await;
        assert_eq!(t.measure_reps, 1);
        assert_eq!(t.min_max_ratio, 1.0);

        // The same invariant at a magnitude well above the floor, so the assertion above is shown to
        // be about "one sample" rather than about "a sample too small to measure".
        let slow = Timing::measure(0, 1, || async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        })
        .await;
        assert_eq!(slow.measure_reps, 1);
        assert_eq!(slow.min_max_ratio, 1.0);
        assert!(slow.max_millis >= 5.0);
    }

    /// `discard` is a recursive delete of a configured path, so it checks what it is about to
    /// destroy. A directory that is not a store survives (RISK-005).
    #[tokio::test]
    async fn discard_refuses_a_directory_that_is_not_a_rig() {
        let dir = std::env::temp_dir().join(format!("memory-not-a-rig-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        std::fs::write(dir.join("something-precious.txt"), b"keep me").expect("write");

        let rig = Rig {
            store: Store::new_in_memory_with_profile(DIM, MemoryProfile::Collective)
                .await
                .unwrap(),
            dir: dir.clone(),
            embed_dim: DIM,
        };
        let err = rig.discard().expect_err("a non-rig directory must be refused");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(
            dir.join("something-precious.txt").exists(),
            "the refused directory must be untouched"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let s = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert_eq!(percentile(&s, 0.50), 5.0);
        assert_eq!(percentile(&s, 0.95), 10.0);
        assert_eq!(percentile(&s, 0.99), 10.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
        assert_eq!(percentile(&[42.0], 0.99), 42.0);
    }
}
