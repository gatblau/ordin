//! The Rules Engine's one error type and the codes it owns (RulesEngine.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (its `RULE_` rows), sorted.
///
/// Some are owned here but never returned as a `RulesError`: the tool-call codes
/// (`RULE_TOOL_NAME_INVALID`, `RULE_ARGUMENTS_TOO_LARGE`, `RULE_ORIGIN_INVALID`,
/// `RULE_SESSION_MISMATCH`, `RULE_JOURNAL_UNAVAILABLE`, `RULE_COMMAND_INVALID`,
/// `RULE_COMMAND_NOT_DECLARED`, `RULE_PATH_NOT_ALLOWED`, `RULE_CHECKER_UNREACHABLE`) travel as a
/// `deny` verdict or a content-check outcome and are written to the log; `RULE_POLICY_WIDENS`
/// cannot arise because an agent's policy is stored as deny and ask lists only (Internal Logic A.4);
/// `RULE_FAIL_CLOSED_NOT_DENY`, `RULE_ID_INVALID` and `RULE_REASON_INVALID` are registered for
/// 2C's own validation of a `ToolDecision` and have no row in the Error Table.
pub const ALL_CODES: &[&str] = &[
    "RULE_APPROVAL_UNAVAILABLE",
    "RULE_ARGUMENTS_TOO_LARGE",
    "RULE_BAD_PATTERN",
    "RULE_CHECKER_UNREACHABLE",
    "RULE_COMMAND_INVALID",
    "RULE_COMMAND_NOT_DECLARED",
    "RULE_FACT_UNAVAILABLE",
    "RULE_FAIL_CLOSED_NOT_DENY",
    "RULE_GATE_OVERRIDE",
    "RULE_ID_INVALID",
    "RULE_JOURNAL_UNAVAILABLE",
    "RULE_NOT_AN_ADDRESS",
    "RULE_ORIGIN_INVALID",
    "RULE_PATH_NOT_ALLOWED",
    "RULE_POLICY_INVALID",
    "RULE_POLICY_WIDENS",
    "RULE_REASON_INVALID",
    "RULE_RULESET_NOT_LOADED",
    "RULE_RUN_UNKNOWN",
    "RULE_SESSION_MISMATCH",
    "RULE_SETTING_RULE_NOT_ALLOWED",
    "RULE_STATE_UNAVAILABLE",
    "RULE_TOOL_NAME_INVALID",
    "RULE_TOO_MANY_RULES",
    "RULE_UNBOUND_TOOL",
    "RULE_UNKNOWN_ARGUMENT",
];

/// Codes carried by a `deny` verdict or a log line rather than by a `RulesError`.
pub(crate) mod codes {
    pub const TOOL_NAME_INVALID: &str = "RULE_TOOL_NAME_INVALID";
    pub const ARGUMENTS_TOO_LARGE: &str = "RULE_ARGUMENTS_TOO_LARGE";
    pub const ORIGIN_INVALID: &str = "RULE_ORIGIN_INVALID";
    pub const RUN_UNKNOWN: &str = "RULE_RUN_UNKNOWN";
    pub const SESSION_MISMATCH: &str = "RULE_SESSION_MISMATCH";
    pub const RULESET_NOT_LOADED: &str = "RULE_RULESET_NOT_LOADED";
    pub const JOURNAL_UNAVAILABLE: &str = "RULE_JOURNAL_UNAVAILABLE";
    pub const APPROVAL_UNAVAILABLE: &str = "RULE_APPROVAL_UNAVAILABLE";
    pub const STATE_UNAVAILABLE: &str = "RULE_STATE_UNAVAILABLE";
    pub const FACT_UNAVAILABLE: &str = "RULE_FACT_UNAVAILABLE";
    pub const COMMAND_INVALID: &str = "RULE_COMMAND_INVALID";
    pub const COMMAND_NOT_DECLARED: &str = "RULE_COMMAND_NOT_DECLARED";
    pub const PATH_NOT_ALLOWED: &str = "RULE_PATH_NOT_ALLOWED";
    pub const CHECKER_UNREACHABLE: &str = "RULE_CHECKER_UNREACHABLE";
    pub const UNBOUND_TOOL: &str = "RULE_UNBOUND_TOOL";
    pub const UNKNOWN_ARGUMENT: &str = "RULE_UNKNOWN_ARGUMENT";
    pub const BAD_PATTERN: &str = "RULE_BAD_PATTERN";
    pub const TOO_MANY_RULES: &str = "RULE_TOO_MANY_RULES";
    pub const GATE_OVERRIDE: &str = "RULE_GATE_OVERRIDE";
    pub const POLICY_INVALID: &str = "RULE_POLICY_INVALID";
}

/// Every failure of this crate. `Display` never carries an argument value or a message text.
#[derive(Debug, thiserror::Error)]
pub enum RulesError {
    #[error("{code}: {message}")]
    Load {
        code: &'static str,
        message: String,
        r#where: String,
    },
    #[error("fact unavailable")]
    FactUnavailable { control: String },
    #[error("control state unavailable")]
    State(#[source] sqlx::Error),
    #[error("rule set not loaded")]
    RuleSetNotLoaded,
    #[error("unknown run")]
    RunUnknown,
    #[error("not an address: {0}")]
    NotAnAddress(&'static str),
    #[error("setting rule not allowed: {0}")]
    SettingRuleNotAllowed(String),
    #[error("approval wait unavailable")]
    ApprovalUnavailable,
}

impl RulesError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            RulesError::Load { code, .. } => code,
            RulesError::FactUnavailable { .. } => codes::FACT_UNAVAILABLE,
            RulesError::State(_) => codes::STATE_UNAVAILABLE,
            RulesError::RuleSetNotLoaded => codes::RULESET_NOT_LOADED,
            RulesError::RunUnknown => codes::RUN_UNKNOWN,
            RulesError::NotAnAddress(_) => "RULE_NOT_AN_ADDRESS",
            RulesError::SettingRuleNotAllowed(_) => "RULE_SETTING_RULE_NOT_ALLOWED",
            RulesError::ApprovalUnavailable => codes::APPROVAL_UNAVAILABLE,
        }
    }

    /// A fact, the control state and the Approval Service may answer later (Error Table:
    /// `Err(retryable)`); every other error is a property of the input.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            RulesError::FactUnavailable { .. }
                | RulesError::State(_)
                | RulesError::ApprovalUnavailable
        )
    }

    /// A load refusal with its code, message and place.
    pub(crate) fn load(code: &'static str, message: String, place: impl Into<String>) -> Self {
        RulesError::Load {
            code,
            message,
            r#where: place.into(),
        }
    }
}

impl CodedError for RulesError {
    fn code(&self) -> &'static str {
        RulesError::code(self)
    }

    fn retryable(&self) -> bool {
        RulesError::retryable(self)
    }

    fn repair(&self) -> Option<RepairAction> {
        if self.retryable() {
            Some(RepairAction::TryAgain)
        } else {
            None
        }
    }
}

/// Any statement failure on the control-state tables is `RULE_STATE_UNAVAILABLE` (G.7).
impl From<sqlx::Error> for RulesError {
    fn from(error: sqlx::Error) -> Self {
        RulesError::State(error)
    }
}
