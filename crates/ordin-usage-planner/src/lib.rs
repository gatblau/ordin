//! The Usage Planner (UsagePlanner.md): the policy module that spends the person's plan sparingly.
//! It chooses the model level for every step from the person's usage settings within the
//! organisation profile's frame, steps a step that fell short up one level once, estimates a
//! crew's or a large task's use before it starts, decides when an earlier answer may be shown again
//! and when a conversation should be summarised, and counts each run's use in daily counters. It
//! decides in fixed code with no model call, and it calls none of its callers.
//!
//! Tables `usage_settings`, `reused_answers`, `usage_daily` and `usage_work_history` live in the
//! Journal's datastore, registered as module `usage_planner` through [`UsagePlanner::migrations`].
#![forbid(unsafe_code)]

mod error;
mod log;
mod planner;
mod policy;
mod store;
mod types;

#[cfg(test)]
mod tests;

pub use error::{UsageError, ALL_CODES};
/// One owner: the Organisation Profile's `WorkKind`, re-exported so the Session Supervisor and the
/// Crew Orchestrator keep writing `ordin_usage_planner::WorkKind`.
pub use ordin_organisation_profile::WorkKind;
pub use planner::UsagePlanner;
pub use types::{
    AnswerToIndex, EstimateBand, EstimateBasis, EstimateRequest, LevelChoice, ReuseQuery, RunUsage,
    Today, UsageEstimate, UsageSettingsChange, WorkShape,
};
