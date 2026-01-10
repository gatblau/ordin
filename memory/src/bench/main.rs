//! `memory-bench` binary entry point (C11) — the tenancy benchmark harness.
//!
//! A short-lived, operator-invoked batch binary. It takes no inbound request, binds no listener, and
//! exports no metrics: its output is a JSON report at `MEMORY_BENCH_REPORT_PATH` plus structured logs.
//! It is **not** a third service — neither shipped binary gains code from it (NFR-MA3).
//!
//! Exit codes come from the C11 error table and are the operator's contract: **2** is a refusal (the
//! harness stopped you on purpose — no store URL, a prefix collision, an illegal tenant id), **1** is a
//! store or IO failure, and **0** is a completed run. A namespace ceiling is a completed run: it exits
//! 0 with the ceiling in the report, because finding one is what this instrument is for.
//!
//! Only `main` may panic on unrecoverable bootstrap (X1 / C5 rule); library code never panics.

use std::process::ExitCode;

use memory::bench::{BenchConfig, BenchHarness};

#[tokio::main]
async fn main() -> ExitCode {
    // Logging is initialised from `MEMORY_LOG_LEVEL` alone. The harness deliberately does not load the
    // service's `Config`: that would read `MEMORY_STORE_PATH` / `MEMORY_STORE_REMOTE_URL` and is the
    // inheritance SA-BENCH-ISOLATION-01 forbids.
    let level = std::env::var("MEMORY_LOG_LEVEL").unwrap_or_else(|_| "info".into());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(level))
        .init();

    let cfg = BenchConfig::from_env();
    let populations = cfg.populations.clone();

    let harness = match BenchHarness::connect(cfg).await {
        Ok(h) => h,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(err.exit_code());
        }
    };

    // The pre-flight guard runs before anything is created, so a colliding namespace stops the run
    // rather than being adopted and later dropped.
    if let Err(err) = harness.preflight().await {
        eprintln!("{err}");
        return ExitCode::from(err.exit_code());
    }

    tracing::info!(
        target: "memory-bench",
        populations = ?populations,
        "bench.ready"
    );

    // `run` sequences the relevance, latency and co-residency legs, each skipped when its own sweep
    // list is empty, and writes the report either way — so a run with no leg enabled leaves a
    // well-formed file naming the engine and an empty leg set rather than nothing at all, which is
    // the difference between "measured nothing" and "the numbers never reached the disk".
    match harness.run().await {
        Ok(report) => {
            tracing::info!(
                target: "memory-bench",
                engine = %report.engine,
                engine_version = %report.engine_version,
                "bench.done"
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(err.exit_code())
        }
    }
}
