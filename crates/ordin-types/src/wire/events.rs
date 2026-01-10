//! The kind of a Service API event, named by emitters that cannot depend on `ordin-service-api`.

use serde::{Deserialize, Serialize};

/// The kind of a Service API event, as the Journal records it (2C wire *Names* item 7).
/// Modules that cannot depend on `ordin-service-api` name the kind of the event they write
/// to the Journal with this enum; `ordin-service-api` decodes the payload (ServiceApi spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RunStateChanged,
    RunProgress,
    PlanChanged,
    StepMarked,
    StartUpReportReady,
    ApprovalRequested,
    ApprovalDecided,
    CrewRoundChanged,
    BlockShown,
    Reset,
    ServerStateChanged,
    WorkspaceChanged,
    NeedsYouChanged,
    BlockAvailable,
    LocaleChanged,
    BrandChanged,
    SetupProgress,
    SignInChanged,
    LanguageChanged,        // amended January 2026, ADR-109
    KnowledgeAgentsChanged, // amended January 2026, ADR-113
    MoreFound,              // amended January 2026, ADR-113
}

impl EventKind {
    /// The snake_case wire name, equal to the SSE `event:` line and to the Journal's `kind` column.
    pub fn as_str(&self) -> &'static str {
        match self {
            EventKind::RunStateChanged => "run_state_changed",
            EventKind::RunProgress => "run_progress",
            EventKind::PlanChanged => "plan_changed",
            EventKind::StepMarked => "step_marked",
            EventKind::StartUpReportReady => "start_up_report_ready",
            EventKind::ApprovalRequested => "approval_requested",
            EventKind::ApprovalDecided => "approval_decided",
            EventKind::CrewRoundChanged => "crew_round_changed",
            EventKind::BlockShown => "block_shown",
            EventKind::Reset => "reset",
            EventKind::ServerStateChanged => "server_state_changed",
            EventKind::WorkspaceChanged => "workspace_changed",
            EventKind::NeedsYouChanged => "needs_you_changed",
            EventKind::BlockAvailable => "block_available",
            EventKind::LocaleChanged => "locale_changed",
            EventKind::BrandChanged => "brand_changed",
            EventKind::SetupProgress => "setup_progress",
            EventKind::SignInChanged => "sign_in_changed",
            EventKind::LanguageChanged => "language_changed",
            EventKind::KnowledgeAgentsChanged => "knowledge_agents_changed",
            EventKind::MoreFound => "more_found",
        }
    }

    /// The inverse of `as_str`; `None` for any other string.
    pub fn parse(raw: &str) -> Option<EventKind> {
        match raw {
            "run_state_changed" => Some(EventKind::RunStateChanged),
            "run_progress" => Some(EventKind::RunProgress),
            "plan_changed" => Some(EventKind::PlanChanged),
            "step_marked" => Some(EventKind::StepMarked),
            "start_up_report_ready" => Some(EventKind::StartUpReportReady),
            "approval_requested" => Some(EventKind::ApprovalRequested),
            "approval_decided" => Some(EventKind::ApprovalDecided),
            "crew_round_changed" => Some(EventKind::CrewRoundChanged),
            "block_shown" => Some(EventKind::BlockShown),
            "reset" => Some(EventKind::Reset),
            "server_state_changed" => Some(EventKind::ServerStateChanged),
            "workspace_changed" => Some(EventKind::WorkspaceChanged),
            "needs_you_changed" => Some(EventKind::NeedsYouChanged),
            "block_available" => Some(EventKind::BlockAvailable),
            "locale_changed" => Some(EventKind::LocaleChanged),
            "brand_changed" => Some(EventKind::BrandChanged),
            "setup_progress" => Some(EventKind::SetupProgress),
            "sign_in_changed" => Some(EventKind::SignInChanged),
            "language_changed" => Some(EventKind::LanguageChanged),
            "knowledge_agents_changed" => Some(EventKind::KnowledgeAgentsChanged),
            "more_found" => Some(EventKind::MoreFound),
            _ => None,
        }
    }
}
