//! The Sandbox's codes and the sentences the model reads (Sandbox.md *Error Table*).

use ordin_mcp_front::{ArgumentProblem, ToolFailure};

/// Every code this crate owns, sorted.
pub const ALL_CODES: &[&str] = &[
    "SBX_ARGS_INVALID",
    "SBX_AUDIT_KEY_UNAVAILABLE",
    "SBX_BUSY",
    "SBX_CALLER_NOT_SUPPORTED",
    "SBX_CAPABILITY_UNKNOWN",
    "SBX_CONFIG_INVALID",
    "SBX_CREDENTIAL_UNAVAILABLE",
    "SBX_ENVIRONMENT_INVALID",
    "SBX_ENVIRONMENT_UNSUPPORTED",
    "SBX_FILE_INVALID",
    "SBX_GUEST_DIGEST_MISMATCH",
    "SBX_GUEST_START_FAILED",
    "SBX_INPUTS_TOO_LARGE",
    "SBX_INPUT_NOT_FOUND",
    "SBX_INPUT_NOT_GRANTED",
    "SBX_INPUT_UNREADABLE",
    "SBX_INTERACTION_REFUSED",
    "SBX_INTERNAL",
    "SBX_KEY_MISSING",
    "SBX_MIXED_CREDENTIALS",
    "SBX_MODULE_HANDLE_INVALID",
    "SBX_OUTPUT_SAVE_FAILED",
    "SBX_POLICY_INVALID",
    "SBX_RULES_UNAVAILABLE",
    "SBX_RUN_BUDGET_EXHAUSTED",
    "SBX_RUN_TIMEOUT",
    "SBX_SESSIONS_FULL",
    "SBX_SESSION_BUSY",
    "SBX_SIGN_IN_NEEDED",
    "SBX_TOOL_UNKNOWN",
];

/// One refused or failed call, before it becomes a `ToolFailure`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

impl Refusal {
    fn of(code: &'static str, retryable: bool, message: impl Into<String>) -> Refusal {
        Refusal {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn into_failure(self) -> ToolFailure {
        ToolFailure {
            code: self.code.to_owned(),
            message: self.message,
            retryable: self.retryable,
        }
    }

    // spec: ordin-desktop/components/Sandbox#SBX_TOOL_UNKNOWN
    pub fn tool_unknown() -> Refusal {
        Refusal::of(
            "SBX_TOOL_UNKNOWN",
            false,
            "Use python_sandbox; that tool is not offered here.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_CALLER_NOT_SUPPORTED
    pub fn caller_not_supported() -> Refusal {
        Refusal::of(
            "SBX_CALLER_NOT_SUPPORTED",
            false,
            "The sandbox is not offered to hosted agents.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_ARGS_INVALID
    pub fn args_invalid(problem: &ArgumentProblem) -> Refusal {
        Refusal::of("SBX_ARGS_INVALID", false, problem.reason.clone())
    }

    // spec: ordin-desktop/components/Sandbox#SBX_ENVIRONMENT_UNSUPPORTED
    pub fn environment_unsupported() -> Refusal {
        Refusal::of(
            "SBX_ENVIRONMENT_UNSUPPORTED",
            false,
            "Environment variables are not available in this version; put the values in the code.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_SESSION_BUSY
    pub fn session_busy() -> Refusal {
        Refusal::of(
            "SBX_SESSION_BUSY",
            true,
            "The work folder is in use by another call; wait for it to finish.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_SESSIONS_FULL
    pub fn sessions_full() -> Refusal {
        Refusal::of(
            "SBX_SESSIONS_FULL",
            true,
            "Too many work folders are open; try again shortly or run without workSession.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_BUSY
    pub fn busy() -> Refusal {
        Refusal::of("SBX_BUSY", true, "The sandbox is busy; try again shortly.")
    }

    // spec: ordin-desktop/components/Sandbox#SBX_RULES_UNAVAILABLE
    pub fn rules_unavailable() -> Refusal {
        Refusal::of(
            "SBX_RULES_UNAVAILABLE",
            true,
            "The sandbox cannot check which systems this agent may use; try again.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_POLICY_INVALID
    pub fn policy_invalid() -> Refusal {
        Refusal::of(
            "SBX_POLICY_INVALID",
            false,
            "The sandbox could not prepare this agent's systems.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_CAPABILITY_UNKNOWN
    pub fn capability_unknown(name: &str, granted: &[String]) -> Refusal {
        let granted = if granted.is_empty() {
            "none".to_owned()
        } else {
            granted.join(", ")
        };
        Refusal::of(
            "SBX_CAPABILITY_UNKNOWN",
            false,
            format!("`{name}` is not a system this agent may call; granted: {granted}."),
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_MIXED_CREDENTIALS
    pub fn mixed_credentials() -> Refusal {
        Refusal::of(
            "SBX_MIXED_CREDENTIALS",
            false,
            "Call systems that use different sign-ins in separate runs.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_KEY_MISSING
    pub fn key_missing(key_name: &str) -> Refusal {
        Refusal::of(
            "SBX_KEY_MISSING",
            false,
            format!("The key `{key_name}` for this agent has not been entered."),
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_INPUT_NOT_GRANTED
    pub fn input_not_granted() -> Refusal {
        Refusal::of(
            "SBX_INPUT_NOT_GRANTED",
            false,
            "The agent may not read that place in this workspace; ask the person to grant it, or pass the file's content in files.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_INPUT_NOT_FOUND
    pub fn input_not_found(path: &str) -> Refusal {
        Refusal::of(
            "SBX_INPUT_NOT_FOUND",
            false,
            format!("`{path}` was not found in that place."),
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_INPUTS_TOO_LARGE
    pub fn inputs_too_large() -> Refusal {
        Refusal::of(
            "SBX_INPUTS_TOO_LARGE",
            false,
            "Inputs exceed the sandbox limits (1,000 files, 64 MiB, 16 MiB per file).",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_INPUT_UNREADABLE
    pub fn input_unreadable(path: &str) -> Refusal {
        Refusal::of(
            "SBX_INPUT_UNREADABLE",
            true,
            format!("`{path}` could not be read; try again."),
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_FILE_INVALID
    pub fn file_invalid(path: &str) -> Refusal {
        Refusal::of(
            "SBX_FILE_INVALID",
            false,
            format!("`{path}` is not a valid file entry: relative path, no `..`, base64 content."),
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_SIGN_IN_NEEDED
    pub fn sign_in_needed(system: &str) -> Refusal {
        Refusal::of(
            "SBX_SIGN_IN_NEEDED",
            false,
            format!("The person needs to sign in to {system} again before this can run."),
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_CREDENTIAL_UNAVAILABLE
    pub fn credential_unavailable() -> Refusal {
        Refusal::of(
            "SBX_CREDENTIAL_UNAVAILABLE",
            true,
            "The sign-in store is not available; try again.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_INTERACTION_REFUSED
    pub fn interaction_refused() -> Refusal {
        Refusal::of(
            "SBX_INTERACTION_REFUSED",
            false,
            "The sandbox could not get permission for that system.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_RUN_BUDGET_EXHAUSTED
    pub fn run_budget_exhausted() -> Refusal {
        Refusal::of(
            "SBX_RUN_BUDGET_EXHAUSTED",
            false,
            "This session has used its sandbox call allowance.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_RUN_TIMEOUT
    pub fn run_timeout() -> Refusal {
        Refusal::of(
            "SBX_RUN_TIMEOUT",
            false,
            "The code did not finish within 30 seconds.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_GUEST_START_FAILED
    pub fn guest_start_failed() -> Refusal {
        Refusal::of(
            "SBX_GUEST_START_FAILED",
            false,
            "The sandbox could not start the code.",
        )
    }

    // spec: ordin-desktop/components/Sandbox#SBX_INTERNAL
    pub fn internal() -> Refusal {
        Refusal::of("SBX_INTERNAL", true, "The sandbox failed; try again.")
    }

    // spec: ordin-desktop/components/Sandbox#SBX_OUTPUT_SAVE_FAILED
    pub fn output_save_failed() -> Refusal {
        Refusal::of(
            "SBX_OUTPUT_SAVE_FAILED",
            true,
            "The results could not be saved to the agent's folder; run it again.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_is_sorted_unique_and_registered_to_this_crate() {
        let mut sorted = ALL_CODES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, ALL_CODES);
        for code in ALL_CODES {
            let entry = ordin_types::error_registry::lookup(code)
                .unwrap_or_else(|| panic!("{code} is not in the registry"));
            assert_eq!(entry.owner_crate, "ordin-sandbox", "{code}");
        }
    }

    #[test]
    fn retryable_flags_match_the_registry() {
        let refusals = [
            Refusal::tool_unknown(),
            Refusal::caller_not_supported(),
            Refusal::environment_unsupported(),
            Refusal::session_busy(),
            Refusal::sessions_full(),
            Refusal::busy(),
            Refusal::rules_unavailable(),
            Refusal::policy_invalid(),
            Refusal::capability_unknown("crm", &[]),
            Refusal::mixed_credentials(),
            Refusal::key_missing("k"),
            Refusal::input_not_granted(),
            Refusal::input_not_found("a"),
            Refusal::inputs_too_large(),
            Refusal::input_unreadable("a"),
            Refusal::file_invalid("a"),
            Refusal::sign_in_needed("tickets"),
            Refusal::credential_unavailable(),
            Refusal::interaction_refused(),
            Refusal::run_budget_exhausted(),
            Refusal::run_timeout(),
            Refusal::guest_start_failed(),
            Refusal::internal(),
            Refusal::output_save_failed(),
        ];
        for r in refusals {
            let entry = ordin_types::error_registry::lookup(r.code).expect("registered");
            assert_eq!(entry.retryable, r.retryable, "{}", r.code);
            assert!(!r.message.is_empty() && r.message.len() <= 2000);
        }
    }

    #[test]
    fn capability_unknown_names_what_is_granted() {
        let r = Refusal::capability_unknown("crm", &["tickets".to_owned(), "repo".to_owned()]);
        assert_eq!(
            r.message,
            "`crm` is not a system this agent may call; granted: tickets, repo."
        );
    }
}
