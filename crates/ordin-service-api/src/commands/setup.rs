//! Getting ready — restated from `SetupAssistant.md` (SA-RECON-02), with its amendment for
//! releases and updates (ADR-114: `UpdateOffer.state`, What's new and the personal server's own update).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

use super::rules;

pub use ordin_types::domain::identity::{HarnessAttachment, HarnessKind};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;

/// `UpdateOffer.version` and the `version` inputs: 1–64 characters.
const VERSION_MAX_CHARS: usize = 64;

// ---- reports and progress ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckTrigger {
    Start,
    Background,
    OnDemand,
    BeforeSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    CompanySettings,
    Harness,
    HarnessSignIn,
    Tokens,
    AgentRepositories,
    AgentFolders,
    Programs,
    CraftApps,
    // Another copy of ordin on this computer (SetupAssistant SET-UP-27), after `CraftApps`.
    // spec: ordin-desktop/components/ServiceApi#SVC-SC-1
    SecondInstall,
    // The first-start question, which assistant to use (SetupAssistant SET-CHO-3), after
    // `SecondInstall`.
    // spec: ordin-desktop/components/SetupAssistant#SET-CHO-3
    HarnessChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Passed,
    Repaired,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PersonAction {
    TryAgain,
    SignInAgain,
    SignIn,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CheckResult {
    pub check: CheckKind,
    pub outcome: CheckOutcome,
    /// Error code for Diagnostics; None when passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Message identifier (Localisation) of the one sentence shown; None when passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Values for the message's placeholders (program name, harness name, what is off).
    pub message_args: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<PersonAction>,
    /// For a refused program: the program the sentence names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_program: Option<RefusedProgram>,
    pub checked_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RefusedProgram {
    pub name: String,
    pub publisher: String,
    pub sha256: Sha256Hex,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SetupReport {
    /// None for the guided sequence.
    pub trigger: Option<CheckTrigger>,
    pub results: Vec<CheckResult>,
    /// A harness is ready.
    pub ready: bool,
    pub started_at: Timestamp,
    pub ended_at: Timestamp,
}

/// The payload of the `setup_progress` event and part of `setup.status`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SetupProgress {
    pub running: bool,
    pub steps: Vec<ProgressStep>,
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ProgressStep {
    pub check: CheckKind,
    pub state: StepState,
    pub message_id: String,
    /// Set when the step has failed: why, in the failed result's words (SetupAssistant SET-R1-3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<StepProblem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct StepProblem {
    pub message_id: String,
    #[serde(default)]
    pub message_args: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Waiting,
    Running,
    Done,
    NeedsYou,
    Failed,
}

// ---- updates (ADR-114) ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct UpdateOffer {
    /// Semantic version, 1–64 characters.
    pub version: String,
    /// The feed's release notes, at most 4,000 characters (cut beyond).
    pub notes: String,
    /// Added January 2026 (ADR-114).
    pub state: UpdateState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
// spec: ordin-desktop/components/ServiceApi#SVC-SCR-6
pub enum UpdateState {
    Downloading,
    Ready,
    /// A whole-computer install: an administrator installs the version (SET-UP-25).
    AdministratorNeeded,
}

/// The release notes of the version now running, kept when its download was verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WhatsNew {
    /// The running version.
    pub version: String,
    /// At most 4,000 characters.
    pub notes: String,
    /// True once `setup.whats_new_seen` was called for this version.
    pub seen: bool,
}

/// A newer version a personal server's feed offers (`setup.server_update`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ServerUpdate {
    pub version: String,
    /// At most 4,000 characters.
    pub notes: String,
    /// False when this server cannot replace itself (in a container, or not run by its user unit).
    pub self_update: bool,
}

// ---- mechanical results ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SetupStatusResult {
    pub needs_guided: bool,
    pub progress: SetupProgress,
    pub attachments: Vec<HarnessAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SetupUpdateOfferResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<UpdateOffer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SetupCheckUpdateResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<UpdateOffer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SetupWhatsNewResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whats_new: Option<WhatsNew>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SetupServerUpdateResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<ServerUpdate>,
}

/// `{ accepted: true }`: the update runs in the background after the reply.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SetupUpdateServerResult {
    pub accepted: bool,
}

// ---- mechanical inputs ----

macro_rules! empty_inputs {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(rename_all = "snake_case", deny_unknown_fields)]
        pub struct $name {}

        impl CommandInput for $name {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

empty_inputs!(
    SetupStatusInput,
    SetupRunGuidedInput,
    SetupCheckNowInput,
    SetupUpdateOfferInput,
    SetupInstallUpdateInput,
    SetupCheckUpdateInput,
    SetupWhatsNewInput,
    SetupServerUpdateInput,
);

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SetupSignInHarnessInput {
    pub harness: HarnessKind,
}

/// `setup.choose_harness`: the person's answer to the first-start question.
// spec: ordin-desktop/components/ServiceApi#SVC-CHO-1
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChooseHarness {
    pub harness: HarnessKind,
}

/// `restarting` is true when the program starts again to use the chosen harness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ChooseHarnessResult {
    pub restarting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SetupWhatsNewSeenInput {
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SetupUpdateServerInput {
    pub version: String,
}

// ---- checks ----

/// Semantic-version text of 1–64 characters: `MAJOR.MINOR.PATCH`, then an optional `-pre-release`
/// and `+build` of dot-separated `[0-9A-Za-z-]` identifiers.
fn check_version(value: &str, at: &str) -> Result<(), InputRejection> {
    rules::text(value, 1, VERSION_MAX_CHARS, at)?;
    let (rest, build) = match value.split_once('+') {
        Some((r, b)) => (r, Some(b)),
        None => (value, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (rest, None),
    };
    let numbers: Vec<&str> = core.split('.').collect();
    let core_ok = numbers.len() == 3
        && numbers
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    let ident_ok = |part: Option<&str>| {
        part.is_none_or(|p| {
            p.split('.').all(|id| {
                !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        })
    };
    if !(core_ok && ident_ok(pre) && ident_ok(build)) {
        return Err(rules::reject(at));
    }
    Ok(())
}

impl CommandInput for SetupSignInHarnessInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for ChooseHarness {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for SetupWhatsNewSeenInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_version(&self.version, "/version")
    }
}

impl CommandInput for SetupUpdateServerInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_version(&self.version, "/version")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        for good in [
            "1.4.0",
            "10.0.12",
            "1.0.0-beta.2",
            "1.0.0+build.7",
            "1.0.0-rc-1+x",
        ] {
            assert!(check_version(good, "/version").is_ok(), "{good}");
        }
        for bad in [
            "",
            "1.4",
            "1.4.0.1",
            "v1.4.0",
            "1.4.x",
            "1.0.0-",
            "1.0.0-a..b",
            &"1".repeat(65),
        ] {
            let rejection = check_version(bad, "/version").unwrap_err();
            assert_eq!(
                (rejection.code, rejection.at.as_str()),
                ("SVC_INPUT_INVALID", "/input/version"),
                "{bad}"
            );
        }
        assert!(SetupWhatsNewSeenInput {
            version: "1.4.0".into()
        }
        .check()
        .is_ok());
        assert!(SetupUpdateServerInput {
            version: "nope".into()
        }
        .check()
        .is_err());
    }

    #[test]
    fn trivial_inputs_and_shapes() {
        assert!(SetupStatusInput {}.check().is_ok());
        assert!(SetupInstallUpdateInput {}.check().is_ok());
        let mut harness: SetupSignInHarnessInput =
            serde_json::from_str(r#"{"harness":"copilot_cli"}"#).unwrap();
        assert!(harness.check().is_ok());
        assert!(serde_json::from_str::<SetupStatusInput>(r#"{"x":1}"#).is_err());
        let offer = SetupUpdateOfferResult {
            offer: Some(UpdateOffer {
                version: "1.5.0".into(),
                notes: "Faster.".into(),
                state: UpdateState::Ready,
            }),
        };
        assert_eq!(
            serde_json::to_value(&offer).unwrap()["offer"]["state"],
            "ready"
        );
        let none = serde_json::to_value(SetupWhatsNewResult { whats_new: None }).unwrap();
        assert_eq!(none, serde_json::json!({}));
    }
}
