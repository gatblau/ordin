//! The Rules Engine (RulesEngine.md): ordin's one fixed-code, deterministic evaluator of what an
//! agent may do. It decides at the start of a run, before each tool call and at the end, and each
//! decision is allow, deny or ask a person. It makes no model call.
//!
//! Sources, strongest first: the application's own policy (folder grants, the shell policy, the
//! approval policy for externally visible actions, the profile's email and server rules), the
//! package's declaration in the wrightrun control vocabulary, and the agent's own tool policy,
//! which can only narrow. Every decision is journalled with the rule that produced it, and an *ask*
//! becomes an approval through the Approval Service before the decision returns. Counters, locks
//! and per-run facts live in the Journal's datastore (module `rules`).
//!
//! Since January 2026 (ADR-107) the engine also runs the agent profile's content checks against
//! a checker the organisation supplies, through [`ContentChecker`].
//!
//! Confirmation items this crate is conditional on (Playbook §7.8), each built with the spec's
//! reading: C-26 and C-27 (built-in tool names, `policy::FILE_TOOLS`), C-78 (the reviewer check
//! applies only when the hook names the sub-agent), C-80 (approved-once records survive a move).
#![forbid(unsafe_code)]

mod address;
mod args;
mod content;
mod decide;
mod engine;
mod error;
mod eval;
mod facts;
pub mod host_forms;
mod load;
mod policy;
mod setting;
mod shell;
mod start;
mod store;
mod types;
mod vocab;

#[cfg(test)]
mod tests;

pub use engine::{duty_lock_key, RulesEngine};
pub use error::{RulesError, ALL_CODES};
pub use types::{
    Address, AdoptedRun, ApprovalNeed, ApprovalWait, CallContext, CardWording, CheckerUnreachable,
    CheckerVerdict, CommandOrigin, CommandRequest, ContentCheckOutcome, ContentCheckReply,
    ContentCheckRequest, ContentChecker, ControlLine, ControlSource, EmailCheck, EmailVerdict,
    EmailViolation, EndDecision, FactError, GitHostSetting, MetricsSink, PathAccessRequest,
    PlaceRead, PlaceReader, ProgramKind, Proposal, RulesConfig, RulesHost, RulesOutcome, RunKey,
    RunRestriction, RunStartDecision, RunStartFacts, StartOutcome, ToolClass,
};
pub use vocab::{
    AdditiveOutcome, AddressCondition, AgentToolPolicy, ArgumentForm, ArgumentOutcome,
    ArgumentRule, CallTarget, Check, CheckKind, CommandBound, Condition, ConfiguredControls,
    CountOf, DecidingOutcome, DecimalShare, Declaration, DeliveryTo, DutyGate, GateOutcome, Gates,
    GrantedPlace, HeartbeatForm, InstanceRuleSet, JudgedLoopGate, MatchMode, MessageRule, Point,
    StepGate, ToolDescription,
};

/// wrightrun A-18, SA-71.
pub const MAX_PATTERN_CHARS: usize = 200;
/// Text a pattern or keyword list is matched against.
pub const MAX_MATCH_CHARS: usize = 4000;
/// 2D-4 run lease; duty locks use the same figure.
pub const LEASE_SECONDS: u32 = 30;
pub const LEASE_RENEW_SECONDS: u32 = 10;
pub const PLACE_NO_RUN_SECONDS: u32 = 60;
/// SA-71.
pub const MAX_ARGUMENT_RULES: usize = 200;
/// LW-18.
pub const MAX_ARGUMENT_BYTES: usize = 1_048_576;
/// A run record older than a day is dropped from memory.
pub const RUN_FACTS_TTL_SECONDS: u64 = 86_400;
pub const BUNDLE_DIR_PREFIXES: [&str; 4] = ["/.apm/", "/knowledge/", "/acceptance/", "/brand/"];
pub const BUNDLE_FILES: [&str; 3] = [
    "/AGENTS.md",
    "/wrightrun-manifest.json",
    "/wrightrun-declaration.yaml",
];
pub const DUTY_ENDINGS: [&str; 5] = [
    "stop-present",
    "escalation-present",
    "nothing-to-do",
    "overlap",
    "cycle-limit",
];
