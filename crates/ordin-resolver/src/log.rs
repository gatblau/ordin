//! The log events of ResolverAndCompiler.md *Observability* (SA-OBS-01). Each event carries counts,
//! identifiers and timings only: never part content, file paths, or credential values.

use std::time::Duration;

use ordin_agent_model::ResolutionPlan;

use crate::error::ResolveError;

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

// spec: ordin-desktop/components/ResolverAndCompiler#SA-OBS-01
pub(crate) fn resolved(plan: &ResolutionPlan, duration: Duration) {
    tracing::info!(
        root_id = plan.root.id.as_str(),
        root_version = plan.root.version.as_str(),
        plan_hash = plan.plan_hash.as_str(),
        parts = count(plan.resolved.len()),
        packs = count(plan.practice_packs.len()),
        overlays = count(plan.overlay_order.len()),
        pending_choices = count(plan.selections.pending_choices.len()),
        warnings = count(plan.warnings.len()),
        duration_ms = millis(duration),
        "resolver.resolved"
    );
}

pub(crate) fn resolve_failed(root_id: &str, error: &ResolveError) {
    tracing::warn!(
        root_id = root_id,
        error_code = error.code(),
        retryable = error.retryable(),
        "resolver.resolve_failed"
    );
}

pub(crate) fn compiled(
    plan_hash: &str,
    parts: usize,
    bytes: u64,
    reused: bool,
    duration: Duration,
) {
    tracing::info!(
        plan_hash = plan_hash,
        parts = count(parts),
        bytes = bytes,
        reused = reused,
        duration_ms = millis(duration),
        "resolver.compiled"
    );
}

pub(crate) fn digest_mismatch(part_id: &str, repository_label: &str) {
    tracing::error!(
        part_id = part_id,
        repository_label = repository_label,
        "resolver.digest_mismatch"
    );
}

pub(crate) fn constituent_deprecated(part_id: &str) {
    tracing::info!(part_id = part_id, "resolver.constituent_deprecated");
}

pub(crate) fn pack_candidate_missing(part_id: &str) {
    tracing::warn!(part_id = part_id, "resolver.pack_candidate_missing");
}

pub(crate) fn pruned(removed: u32, kept: u32, bytes_freed: u64) {
    tracing::info!(
        removed = removed,
        kept = kept,
        bytes_freed = bytes_freed,
        "resolver.pruned"
    );
}

/// An error, because it means the program's start-up order is wrong.
pub(crate) fn source_not_bound(operation: &str) {
    tracing::error!(operation = operation, "resolver.source_not_bound");
}

pub(crate) fn instructions_compiled(plan_hash: &str, parts: usize, bytes: usize) {
    tracing::info!(
        plan_hash = plan_hash,
        parts = count(parts),
        bytes = count(bytes),
        "resolver.instructions_compiled"
    );
}
