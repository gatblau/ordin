//! Getting ready and help — restated from `SetupAssistant.md` (SA-RECON-02), with its amendment for
//! releases and updates (ADR-114: `UpdateOffer.state`, What's new and the personal server's own update).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

use super::identity::v7_text_id;
use super::rules;

pub use ordin_types::domain::identity::{HarnessAttachment, HarnessKind};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;

v7_text_id!(
    /// A help request kept for sending (UUIDv7, *chosen here* by the owner).
    HelpPreviewId
);

/// `HelpRequestInput.what_doing`: 1–500 characters.
const WHAT_DOING_MAX_CHARS: usize = 500;
/// `HelpRequestInput.failure_sentence`: 1–300 characters.
const FAILURE_SENTENCE_MAX_CHARS: usize = 300;
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
    SendToIt,
    AskItForClaudeAccess,
    AskItForCopilotAccess,
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
    /// For a refused program: what Send to IT carries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_program: Option<RefusedProgram>,
    pub checked_at: Timestamp,
}

/// Also an input part of `HelpRequestInput`, so unknown fields are refused.
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

// ---- help requests ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HelpRequestInput {
    /// 1–500 characters.
    pub what_doing: String,
    /// 1–300 characters, the sentence the person saw.
    pub failure_sentence: String,
    /// Diagnostics line id, "r-81" (UX-A70); `^r-[0-9]{1,12}$`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Error codes and diagnostic lines; redacted by the Setup Assistant.
    pub details: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_program: Option<RefusedProgram>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HelpRequestBody {
    /// 1.
    pub format: u32,
    pub what_doing: String,
    pub failure_sentence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// After redaction.
    pub details: Vec<String>,
    pub application_version: String,
    /// Harness and bundled program versions.
    pub sidecar_versions: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused_program: Option<RefusedProgram>,
    pub sent_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HelpRequestPreview {
    pub preview_id: HelpPreviewId,
    /// None → the person enters an email address.
    pub destination: Option<HelpDestinationView>,
    /// Exactly what will be sent.
    pub body: HelpRequestBody,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HelpDestinationView {
    pub kind: HelpDestinationKind,
    pub address: String,
}

/// The Organisation Profile's destination kinds (`ticket_url`, `email`, `slack_channel`), one type
/// with the profile's own.
pub use super::profile::HelpDestinationKind;

/// `setup.help_send`: the owner's `HelpRequestOutcome` without `record` (the program journals the
/// record; ServiceApi.md row).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetupHelpSendResult {
    /// Delivered to a ticket address or a Slack channel.
    Sent,
    /// The person's mail app opened pre-filled.
    OpenedInMailApp,
    /// Server with an email destination: the text for the chat to show; the person forwards it.
    TextForPerson { text: String },
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
pub enum UpdateState {
    Downloading,
    Ready,
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

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SetupHelpSendInput {
    pub preview_id: HelpPreviewId,
    /// Must match `^[^@\s]{1,64}@[^@\s]{1,253}$`.
    pub fallback_address: Option<String>,
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

/// `^r-[0-9]{1,12}$`.
fn is_reference(value: &str) -> bool {
    value.strip_prefix("r-").is_some_and(|digits| {
        (1..=12).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

/// `^[^@\s]{1,64}@[^@\s]{1,253}$`, lengths in characters.
fn is_fallback_address(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    let part_ok = |part: &str, max: usize| {
        let count = part.chars().count();
        (1..=max).contains(&count) && !part.chars().any(|c| c == '@' || c.is_whitespace())
    };
    part_ok(local, 64) && part_ok(domain, 253)
}

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

impl CommandInput for HelpRequestInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.what_doing, 1, WHAT_DOING_MAX_CHARS, "/what_doing")?;
        rules::text(
            &self.failure_sentence,
            1,
            FAILURE_SENTENCE_MAX_CHARS,
            "/failure_sentence",
        )?;
        if let Some(reference) = &self.reference {
            if !is_reference(reference) {
                return Err(rules::reject("/reference"));
            }
        }
        Ok(())
    }
}

impl CommandInput for SetupHelpSendInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        if let Some(address) = &self.fallback_address {
            if !is_fallback_address(address) {
                return Err(rules::reject("/fallback_address"));
            }
        }
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

    const PREVIEW: &str = "0192a1d0-2222-7333-8444-555566667777";

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
    fn help_request() {
        let input = |what: &str, failure: &str, reference: Option<&str>| HelpRequestInput {
            what_doing: what.into(),
            failure_sentence: failure.into(),
            reference: reference.map(str::to_owned),
            details: vec![],
            refused_program: None,
        };
        assert!(input("Comparing quotes", "It stopped.", Some("r-81"))
            .check()
            .is_ok());
        assert!(input("Comparing quotes", "It stopped.", None)
            .check()
            .is_ok());
        for (what, failure, reference, at) in [
            ("", "x", None, "/input/what_doing"),
            (&"w".repeat(501) as &str, "x", None, "/input/what_doing"),
            ("x", "", None, "/input/failure_sentence"),
            ("x", &"f".repeat(301), None, "/input/failure_sentence"),
            ("x", "x", Some("r-"), "/input/reference"),
            ("x", "x", Some("81"), "/input/reference"),
            ("x", "x", Some("r-1234567890123"), "/input/reference"),
            ("x", "x", Some("r-8a"), "/input/reference"),
        ] {
            let rejection = input(what, failure, reference).check().unwrap_err();
            assert_eq!(rejection.at, at);
        }
    }

    #[test]
    fn help_send_address() {
        let input = |address: Option<&str>| SetupHelpSendInput {
            preview_id: HelpPreviewId::parse(PREVIEW).unwrap(),
            fallback_address: address.map(str::to_owned),
        };
        assert!(input(None).check().is_ok());
        assert!(input(Some("it@acme.com")).check().is_ok());
        for bad in [
            "it.acme.com",
            "@acme.com",
            "it@",
            "i t@acme.com",
            "it@acme@x",
            &format!("{}@a", "x".repeat(65)),
        ] {
            assert_eq!(
                input(Some(bad)).check().unwrap_err().at,
                "/input/fallback_address",
                "{bad}"
            );
        }
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
        let sent =
            serde_json::to_value(SetupHelpSendResult::TextForPerson { text: "t".into() }).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({"kind": "text_for_person", "text": "t"})
        );
    }
}
