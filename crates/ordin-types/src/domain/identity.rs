//! 2C part 1 § Identity and settings.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- BrandPack ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct BrandPack {
    pub id: BrandPackId,
    pub brand_identifier: String,
    pub version: String,
    pub source: BrandPackSource,
    pub strings: Vec<BrandStrings>,
    pub logo_light_path: String,
    pub logo_dark_path: String,
    pub icon_set_light_path: String,
    pub icon_set_dark_path: String,
    /// Token name → value, as the pack declares them.
    pub design_tokens: std::collections::BTreeMap<String, String>,
    pub font_file_paths: Vec<String>,
    pub help_url: String,
    pub support_url: String,
    pub privacy_url: String,
    pub state: BrandPackState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal_reason: Option<String>,
    pub received_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct BrandStrings {
    pub locale: String,
    pub product_name: String,
    pub coordinator_name: String,
    pub about_text: String,
    pub legal_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BrandPackSource {
    Build,
    OrganisationProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum BrandPackState {
    Received,
    ContrastChecked,
    Active,
    Refused,
}

// ---- LocalePreference ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct LocalePreference {
    pub locale: String,
    pub source: LocaleSource,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum LocaleSource {
    OperatingSystem,
    Person,
}

// ---- HarnessAttachment ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct HarnessAttachment {
    pub id: HarnessAttachmentId,
    pub harness: HarnessKind,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub tested: bool,
    pub signed_in: bool,
    /// As the harness reports it; free text, never a credential.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign_in_method: Option<String>,
    pub state: HarnessAttachmentState,
    pub last_checked_at: Timestamp,
}

/// Defined once, here (`ordin_types::domain::identity`); part 2 §4 (`wire::hook`) re-exports it
/// (amended January 2026, decision 80: C-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind {
    ClaudeCode,
    CopilotCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum HarnessAttachmentState {
    Found,
    Checked,
    Ready,
    NeedsSignIn,
    NeedsUpdate,
}

// ---- OrganisationProfile ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct OrganisationProfile {
    pub id: OrganisationProfileId,
    pub profile_version: String,
    /// Detached signature as delivered, base64.
    pub signature: String,
    pub document_digest: Sha256Hex,
    pub cached_path: String,
    pub state: OrganisationProfileState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal_reason: Option<String>,
    pub fetched_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum OrganisationProfileState {
    Fetched,
    Verified,
    Active,
    Replaced,
    FailedVerification,
}

// ---- Credential ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Credential {
    pub id: CredentialId,
    pub kind: CredentialKind,
    pub account_label: String,
    /// Key of the OS keychain item that holds the secret; the secret never enters the database.
    pub keychain_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
    pub state: CredentialState,
    pub created_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// `Gitlab` (amended January 2026, decision 80: C-10, D9): Identity and Credentials holds the GitLab sign-in,
/// so the Project Service can push to GitLab remotes over HTTPS with a keychain token.
/// `Jira` (amended January 2026, ADR-106): one Jira Cloud credential per person, by the person's own API token
/// or by Atlassian OAuth; desktop only (IdentityAndCredentials.md amendment, `JiraAccess`).
pub enum CredentialKind {
    Github,
    Gitlab,
    Oidc,
    Slack,
    Confluence,
    Microsoft,
    Google,
    MailServerSingleSignOn,
    MailServerPassword,
    MailServerAppPassword,
    Jira,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
    Active,
    Revoked,
}

// ---- UsageSettings ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct UsageSettings {
    pub choice: Setting<UsageChoice>,
    /// Kind of work → model level; both named in /spec from ADR-062's catalogue.
    pub model_level_per_kind: Setting<std::collections::BTreeMap<String, String>>,
    /// Three values (amended January 2026, decision 80: C-18).
    pub thinking: Setting<ThinkingLevel>,
    pub answer_simple_requests_without_model: Setting<bool>,
    pub reuse_answers: Setting<bool>,
    pub give_agents_only_needed_parts: Setting<bool>,
    /// Three choices (amended January 2026, decision 80: C-18).
    pub summarise_long_conversations: Setting<SummariseWhen>,
    pub continue_from_summary_after_break: Setting<bool>,
    /// When Ordin asks before starting estimated work: three choices (amended January 2026, decision 80: C-18).
    pub ask_before_estimated_requests: Setting<AskBefore>,
    pub agents_at_same_time: Setting<u32>,
    pub show_usage_on_each_run: Setting<bool>,
    /// Daily pause threshold in model requests; `None` = no daily pause (assumed unit).
    pub daily_pause_requests: Setting<Option<u32>>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UsageChoice {
    SaveMyPlan,
    Balanced,
    BestResults,
    Custom,
}

// `ThinkingLevel { Low, Automatic, High }` is defined once, in part 2 (§2, `ordin_types::wire::run`).
pub use crate::wire::run::ThinkingLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SummariseWhen {
    Early,
    WhenNearlyFull,
    OnlyWhenAsked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AskBefore {
    Heavy,
    MediumOrHeavy,
    Never,
}

// ---- VoiceSettings ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct VoiceSettings {
    pub tone: Setting<VoiceTone>,
    pub register: Setting<VoiceRegister>,
    pub length: Setting<VoiceLength>,
    pub name_to_use: Setting<Option<String>>,
    pub notes: Setting<Option<String>>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VoiceTone {
    Friendly,
    Neutral,
    Warm,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VoiceRegister {
    Everyday,
    Formal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VoiceLength {
    Brief,
    Shortest,
}

// ---- ServerProfile ----
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ServerProfile {
    pub id: ServerProfileId,
    pub profile: ServerProfileKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<ServerShape>,
    pub chosen_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServerProfileKind {
    Personal,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ServerShape {
    Single,
    Scaled,
}
