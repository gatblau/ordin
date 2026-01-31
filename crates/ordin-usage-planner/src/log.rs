//! The component's log events (UsagePlanner.md *Observability*; Logging.md). Each event is one
//! `tracing` call whose message is the event name, under the target `ordin_usage_planner`. Fields are
//! strings or numbers: the line formatter writes a field recorded through `Display` or `Debug` as
//! `<debug>`. No event carries the request text of a reused answer, an input location or model
//! output.

use crate::UsagePlanner;

const TARGET: &str = "ordin_usage_planner";

// SPEC-GAP-9: the spec names the field `level` for `usage_level_chosen` and `usage_model_not_accepted`,
// but `level` is a reserved key of the log line (Logging.md R-2) and the formatter drops it; the field
// is written as `model_level`.
pub(crate) fn level_chosen(kind: &str, harness: &str, model_level: &str) {
    tracing::debug!(
        target: TARGET,
        component = UsagePlanner::MODULE,
        kind,
        harness,
        model_level,
        "usage_level_chosen"
    );
}

pub(crate) fn stepped_up(kind: &str, from: &str, to: &str) {
    tracing::info!(
        target: TARGET,
        component = UsagePlanner::MODULE,
        kind,
        from,
        to,
        "usage_stepped_up"
    );
}

pub(crate) fn settings_changed(fields: &str) {
    tracing::info!(
        target: TARGET,
        component = UsagePlanner::MODULE,
        fields,
        "usage_settings_changed"
    );
}

pub(crate) fn daily_pause_reached(turns_today: u32, limit: u32) {
    tracing::info!(
        target: TARGET,
        component = UsagePlanner::MODULE,
        turns_today,
        limit,
        "usage_daily_pause_reached"
    );
}

pub(crate) fn answer_reused(reused_answer_id: &str) {
    tracing::info!(
        target: TARGET,
        component = UsagePlanner::MODULE,
        reused_answer_id,
        "usage_answer_reused"
    );
}

pub(crate) fn levels_empty(kind: &str) {
    tracing::warn!(
        target: TARGET,
        component = UsagePlanner::MODULE,
        kind,
        "usage_levels_empty"
    );
}

pub(crate) fn model_not_accepted(harness: &str, model_level: &str) {
    tracing::warn!(
        target: TARGET,
        component = UsagePlanner::MODULE,
        harness,
        model_level,
        "usage_model_not_accepted"
    );
}

/// G.2: a newer version of an input skipped a row. The spec logs it and names no event.
pub(crate) fn reuse_input_newer() {
    // SPEC-GAP-3: G.2 says `REUSE_INPUT_NEWER` "is logged" but the Observability list has no event
    // for it; it is logged under the event name `usage_reuse_input_newer` with its code.
    tracing::debug!(
        target: TARGET,
        component = UsagePlanner::MODULE,
        code = "REUSE_INPUT_NEWER",
        "usage_reuse_input_newer"
    );
}

/// A counter, history or journal write that failed after the caller's work was done.
pub(crate) fn journal_failed(code: &str, kind: &str) {
    tracing::warn!(
        target: TARGET,
        component = UsagePlanner::MODULE,
        code,
        kind,
        "usage_journal_failed"
    );
}
