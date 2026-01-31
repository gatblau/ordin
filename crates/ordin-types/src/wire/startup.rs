//! 2C part 2 §6 Start-up report items.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- StartUpReportItem ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartUpSubject {
    Run {
        run_id: RunId,
    },
    CrewRun {
        crew_run_id: CrewRunId,
    },
    /// The start of a hosted agent's release, whose report the Agent Loader builds
    /// (amended January 2026, decision 80: C-29).
    AgentRelease {
        agent_release_id: AgentReleaseId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartUpReportItem {
    EnforcedRule {
        rule: RuleRef,
        plain: String,
    },
    RuleNotEnforced {
        rule: RuleRef,
        plain: String,
        reason: NotEnforcedReason,
    },
    UnavailablePart {
        part: String,
        plain: String,
        reason: UnavailableReason,
    },
    AvailableTool {
        tool: String,
        origin: ToolOrigin,
        plain: String,
    },
    VerbNotOffered {
        adapter: AdapterKind,
        verb: AdapterVerb,
        plain: String,
    },
    /// ADR-067: a package's MCP server offers MCP Apps interfaces, which are not drawn.
    OwnScreensNotShown {
        server: String,
        plain: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum NotEnforcedReason {
    /// ADR-007: chat triggers, recurring-duty gates, settings only a chat system edits.
    NotApplicableToLocalSession,
    /// The route cannot carry the rule (for example structured approval over Slack).
    RouteCannotCarry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// An executable part not trusted under the organisation's policy (04 — Agent instance).
    NotTrusted,
    MissingDependency,
    HarnessCannotRun,
}
