//! The organisation profile — restated from `OrganisationProfile.md` (SA-RECON-02), with its
//! amendments of January 2026 (ADR-102, the optional Slack keys) and January 2026 (ADR-110
//! `team_knowledge`, ADR-113 `knowledge_agents`).
//!
//! `profile.get_document` returns the whole document, so every section is restated field for field
//! with the owner's `serde` attributes and defaults. `ModelLevel`, `ExternalActionKind`, `VoiceTone`,
//! `VoiceRegister`, `SummariseWhen` and `AskBefore` are 2C types from `ordin-types`.

use std::collections::BTreeMap;

pub use ordin_types::domain::{AskBefore, SummariseWhen};
use ordin_types::domain::{ExternalActionKind, VoiceRegister, VoiceTone};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::wire::ModelLevel;
use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

// ---- the commands' own shapes ----

/// `profile.status`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProfileStatusInput {}

/// `profile.refresh`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProfileRefreshInput {}

/// `profile.get_document`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProfileGetDocumentInput {}

/// `profile.get_document`: the settings in force (shown in Diagnostics, 04:412).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ProfileGetDocumentResult {
    /// `"verified"` or `"defaults"`.
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_version: Option<String>,
    pub document: OrganisationProfileDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    /// A newer verified profile is now active.
    Activated { profile_version: String },
    /// The address served the active profile again (HTTP 304, or the same digest).
    Unchanged,
    /// The fetched profile failed a check; the last verified one is kept.
    Refused { code: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ProfileStatus {
    pub state: ProfileState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<ProfileFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProfileState {
    /// Agent profile.
    NotUsed,
    /// No profile address is known.
    NoAddress,
    /// An address is known and no verified profile has been activated yet.
    NeverFetched,
    /// A verified profile is active and the last check succeeded within 3 hours.
    Active,
    /// A verified profile is active; no check has succeeded for 3 hours.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ProfileFailure {
    pub code: String,
    pub at: Timestamp,
}

// ---- default helpers (the owner's private functions; each returns the value its name states) ----

fn yes() -> bool {
    true
}
fn two() -> u32 {
    2
}
fn three() -> u32 {
    3
}
fn four() -> u32 {
    4
}
fn ten() -> u32 {
    10
}
fn twenty() -> u32 {
    20
}
fn thirty() -> u32 {
    30
}
fn sixty() -> u32 {
    60
}
fn balanced() -> PlanMode {
    PlanMode::Balanced
}
fn heavy() -> AskBefore {
    AskBefore::Heavy
}
fn when_nearly_full() -> SummariseWhen {
    SummariseWhen::WhenNearlyFull
}
fn no_pause() -> DailyPause {
    DailyPause::NoPause
}
fn tls() -> MailSecurity {
    MailSecurity::Tls
}
fn slack_only() -> Vec<ChatChannel> {
    vec![ChatChannel::Slack]
}
fn en_gb() -> String {
    "en-GB".to_owned()
}
fn gitlab_com() -> String {
    "gitlab.com".to_owned()
}
fn empty_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}
fn all_sign_in_methods() -> Vec<MailSignInMethod> {
    vec![
        MailSignInMethod::SingleSignOn,
        MailSignInMethod::AppPassword,
        MailSignInMethod::MailPassword,
    ]
}
fn inbox_and_sent() -> Vec<String> {
    vec!["Inbox".to_owned(), "Sent".to_owned()]
}
fn default_harnesses() -> Vec<HarnessEntry> {
    vec![HarnessEntry {
        name: HarnessName::ClaudeCode,
        tested_versions: Vec::new(),
    }]
}

// ---- the document ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OrganisationProfileDocument {
    pub format_version: u32,
    /// `MAJOR.MINOR.PATCH`, each part 0–999,999,999, compared numerically.
    pub profile_version: String,
    #[serde(default)]
    pub agent_catalogue: AgentCatalogueSection,
    #[serde(default)]
    pub remote_agents: Vec<RemoteAgentEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slack: Option<SlackSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confluence: Option<ConfluenceSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microsoft: Option<MicrosoftSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub google: Option<GoogleSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_provider: Option<IdentityProviderSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gitlab: Option<GitlabSection>,
    #[serde(default = "default_harnesses")]
    pub harnesses: Vec<HarnessEntry>,
    #[serde(default)]
    pub usage: UsageSection,
    #[serde(default)]
    pub voice: VoiceSection,
    #[serde(default)]
    pub limits: LimitsSection,
    #[serde(default)]
    pub history: HistorySection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<EmailSection>,
    #[serde(default)]
    pub channels: ChannelsSection,
    #[serde(default)]
    pub telegram: TelegramSection,
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub nats: NatsSection,
    #[serde(default)]
    pub craft: CraftSection,
    #[serde(default)]
    pub language_servers: Vec<LanguageServerEntry>,
    #[serde(default)]
    pub engineering: EngineeringSection,
    #[serde(default)]
    pub workflows: WorkflowsSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brand: Option<BrandSection>,
    #[serde(default)]
    pub locales: LocalesSection,
    #[serde(default)]
    pub help: HelpSection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updates: Option<UpdatesSection>,
    #[serde(default)]
    pub sandbox: SandboxSection,
    /// Amended January 2026 (ADR-110).
    #[serde(default)]
    pub team_knowledge: TeamKnowledgeSection,
    /// Amended January 2026 (ADR-113).
    #[serde(default)]
    pub knowledge_agents: KnowledgeAgentsSection,
}

// ---- agent_catalogue ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentCatalogueSection {
    #[serde(default)]
    pub repositories: Vec<AgentRepositoryEntry>,
    #[serde(default = "yes")]
    pub person_may_add: bool,
    /// Executable-trust policy, typed and applied by the Checker; a JSON object of at most 262,144 bytes.
    #[serde(default = "empty_object")]
    pub trust_policy: serde_json::Value,
    /// Default: `help.destination`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub it_approval_destination: Option<HelpDestination>,
}

impl Default for AgentCatalogueSection {
    fn default() -> Self {
        AgentCatalogueSection {
            repositories: Vec::new(),
            person_may_add: yes(),
            trust_policy: empty_object(),
            it_approval_destination: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentRepositoryEntry {
    /// Stable key that workflow files format 3 cite as `profile_key`.
    pub key: String,
    pub display_name: String,
    pub host: String,
    pub repository: String,
    pub protected_branch: String,
    pub format: RepositoryFormat,
    #[serde(default)]
    pub signed_merges_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryFormat {
    Ordin,
    Apm,
}

// ---- remote_agents ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteAgentEntry {
    pub name: String,
    pub route: RemoteAgentRoute,
    /// `hosted_agent`: the agent's `https` base address. `slack`: the agent's Slack account name.
    pub address: String,
    /// Required for `hosted_agent`; absent for `slack`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<RemoteIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentRoute {
    HostedAgent,
    Slack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteIdentity {
    pub issuer: String,
    pub client_id: String,
    pub audience: String,
}

// ---- slack, confluence, microsoft, google, identity_provider ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackSection {
    /// The Slack Thread Adapter's workspace and dedicated channel; optional since ADR-102.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedicated_channel: Option<String>,
    /// The Slack app IT approves, PKCE-enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default = "yes")]
    pub tools_enabled: bool,
    /// Only these channel ids may be linked or named by a lifecycle stage; `None` = any channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_channels: Option<Vec<String>>,
    #[serde(default = "yes")]
    pub rules_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ConfluenceSection {
    pub site: String,
    pub oauth_client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_exchange_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MicrosoftSection {
    pub tenant_id: String,
    pub client_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GoogleSection {
    pub client_id: String,
    /// The non-confidential secret of a Google Desktop OAuth client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentityProviderSection {
    pub issuer: String,
    pub client_id: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

// ---- harnesses ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HarnessEntry {
    pub name: HarnessName,
    #[serde(default)]
    pub tested_versions: Vec<String>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum HarnessName {
    ClaudeCode,
    Copilot,
}

// ---- usage ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageSection {
    #[serde(default = "balanced")]
    pub plan_mode: PlanMode,
    /// Missing kinds allow all four levels.
    #[serde(default)]
    pub levels_allowed: BTreeMap<WorkKind, Vec<ModelLevel>>,
    /// harness → level → model name. Missing entries take the defaults.
    #[serde(default)]
    pub level_models: BTreeMap<HarnessName, BTreeMap<ModelLevel, String>>,
    #[serde(default = "two")]
    pub agents_at_once: u32,
    #[serde(default = "three")]
    pub agents_at_once_max: u32,
    #[serde(default)]
    pub defaults: UsageDefaults,
    /// `None` = the person may change every usage setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person_may_change: Option<Vec<UsageSettingName>>,
}

impl Default for UsageSection {
    fn default() -> Self {
        UsageSection {
            plan_mode: balanced(),
            levels_allowed: BTreeMap::new(),
            level_models: BTreeMap::new(),
            agents_at_once: two(),
            agents_at_once_max: three(),
            defaults: UsageDefaults::default(),
            person_may_change: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanMode {
    Save,
    Balanced,
    Best,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Talking,
    Coordinating,
    Routine,
    Documents,
    Code,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageDefaults {
    #[serde(default = "yes")]
    pub answer_without_model: bool,
    #[serde(default = "yes")]
    pub reuse_answers: bool,
    #[serde(default = "yes")]
    pub parts_only: bool,
    #[serde(default = "when_nearly_full")]
    pub summarise: SummariseWhen,
    #[serde(default = "yes")]
    pub continue_from_summary: bool,
    #[serde(default = "heavy")]
    pub ask_before: AskBefore,
    /// `None` = "true in workspaces with code".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_usage: Option<bool>,
    #[serde(default = "no_pause")]
    pub daily_pause: DailyPause,
}

impl Default for UsageDefaults {
    fn default() -> Self {
        UsageDefaults {
            answer_without_model: yes(),
            reuse_answers: yes(),
            parts_only: yes(),
            summarise: when_nearly_full(),
            continue_from_summary: yes(),
            ask_before: heavy(),
            show_usage: None,
            daily_pause: no_pause(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum DailyPause {
    #[serde(rename = "none")]
    NoPause,
    #[serde(rename = "100")]
    After100,
    #[serde(rename = "250")]
    After250,
    #[serde(rename = "500")]
    After500,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum UsageSettingName {
    PlanMode,
    LevelsAllowed,
    AgentsAtOnce,
    AnswerWithoutModel,
    ReuseAnswers,
    PartsOnly,
    Summarise,
    ContinueFromSummary,
    AskBefore,
    ShowUsage,
    DailyPause,
}

// ---- voice ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VoiceSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<VoiceTone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register: Option<VoiceRegister>,
    #[serde(default)]
    pub terms_use: Vec<String>,
    #[serde(default)]
    pub terms_avoid: Vec<String>,
}

// ---- limits ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LimitsSection {
    #[serde(default)]
    pub crew: CrewLimits,
    #[serde(default)]
    pub run: RunLimitsSection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CrewLimits {
    /// 10, 1–30.
    #[serde(default = "ten")]
    pub rounds: u32,
    /// 60, 1–480.
    #[serde(default = "sixty")]
    pub wall_clock_minutes: u32,
    /// 4, 1–8.
    #[serde(default = "four")]
    pub members_at_once: u32,
    /// None, ≥ 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_budget: Option<u32>,
}

impl Default for CrewLimits {
    fn default() -> Self {
        CrewLimits {
            rounds: ten(),
            wall_clock_minutes: sixty(),
            members_at_once: four(),
            request_budget: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RunLimitsSection {
    /// 60, 1–480.
    #[serde(default = "sixty")]
    pub wall_clock_minutes: u32,
}

impl Default for RunLimitsSection {
    fn default() -> Self {
        RunLimitsSection {
            wall_clock_minutes: sixty(),
        }
    }
}

// ---- history ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HistorySection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_days: Option<u32>,
}

// ---- email ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EmailSection {
    pub system: MailSystem,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Port default 993.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imap: Option<MailServer>,
    /// Port default 465.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub smtp: Option<MailServer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_certificate: Option<String>,
    #[serde(default = "all_sign_in_methods")]
    pub sign_in_methods: Vec<MailSignInMethod>,
    #[serde(default = "inbox_and_sent")]
    pub people_link_folders: Vec<String>,
    #[serde(default)]
    pub server_saves_sent: bool,
    #[serde(default)]
    pub company_domains: Vec<String>,
    #[serde(default)]
    pub internal_only: bool,
    #[serde(default = "twenty")]
    pub max_recipients: u32,
    #[serde(default)]
    pub no_attach_places: Vec<String>,
    #[serde(default)]
    pub foot_line_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailSystem {
    CompanyServer,
    Microsoft365,
    GoogleWorkspace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailServer {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default = "tls")]
    pub security: MailSecurity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailSecurity {
    Tls,
    Starttls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailSignInMethod {
    SingleSignOn,
    AppPassword,
    MailPassword,
}

// ---- channels, telegram ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChannelsSection {
    /// Default `[slack]`.
    #[serde(default = "slack_only")]
    pub allowed: Vec<ChatChannel>,
}

impl Default for ChannelsSection {
    fn default() -> Self {
        ChannelsSection {
            allowed: slack_only(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannel {
    Telegram,
    Slack,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramSection {
    #[serde(default = "yes")]
    pub tools_enabled: bool,
    /// `None` = any chat a workspace links; Telegram chat ids are integers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_chats: Option<Vec<i64>>,
    #[serde(default)]
    pub foot_line_required: bool,
}

impl Default for TelegramSection {
    fn default() -> Self {
        TelegramSection {
            tools_enabled: yes(),
            allowed_chats: None,
            foot_line_required: false,
        }
    }
}

// ---- server ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ServerSection {
    #[serde(default)]
    pub external_actions_off: Vec<ExternalActionKind>,
    #[serde(default = "yes")]
    pub listening_allowed: bool,
    #[serde(default)]
    pub client_certificate_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_certificate_name: Option<String>,
    #[serde(default)]
    pub triggers: ServerTriggers,
}

impl Default for ServerSection {
    fn default() -> Self {
        ServerSection {
            external_actions_off: Vec::new(),
            listening_allowed: yes(),
            client_certificate_required: false,
            client_certificate_name: None,
            triggers: ServerTriggers::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ServerTriggers {
    /// 20, 1–200.
    #[serde(default = "twenty")]
    pub daily_runs: u32,
}

impl Default for ServerTriggers {
    fn default() -> Self {
        ServerTriggers {
            daily_runs: twenty(),
        }
    }
}

// ---- nats ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NatsSection {
    #[serde(default)]
    pub servers: Vec<String>,
    #[serde(default)]
    pub tls: NatsTls,
    #[serde(default)]
    pub subjects_allowed: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NatsTls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_certificate: Option<String>,
}

// ---- craft ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftSection {
    #[serde(default)]
    pub apps: Vec<CraftApp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_url: Option<String>,
    /// File extension (no dot) → app name.
    #[serde(default)]
    pub file_types: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftApp {
    /// The app's identifier (the `craft.file_types` values name it).
    pub app_id: String,
    pub version: String,
    /// The app's allowed list; `None` = every tool the app's MCP mode lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    pub platforms: BTreeMap<ProfilePlatform, CraftAsset>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftAsset {
    pub asset_url: String,
    pub sha256: Sha256Hex,
    pub file_name: String,
}

/// Platforms a pinned download names (craft apps and language servers).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
pub enum ProfilePlatform {
    #[serde(rename = "macos_arm64")]
    MacosArm64,
    #[serde(rename = "macos_x86_64")]
    MacosX86_64,
    #[serde(rename = "windows_x86_64")]
    WindowsX86_64,
    #[serde(rename = "linux_x86_64")]
    LinuxX86_64,
}

// ---- gitlab ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GitlabSection {
    /// `"gitlab.com"` when absent.
    #[serde(default = "gitlab_com")]
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

// ---- language_servers ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LanguageServerEntry {
    pub language: ServerLanguage,
    pub name: String,
    pub version: String,
    pub platforms: BTreeMap<ProfilePlatform, LanguageServerAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServerLanguage {
    Go,
    Rust,
    Java,
    Typescript,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LanguageServerAsset {
    pub asset_url: String,
    pub sha256: Sha256Hex,
    pub file_name: String,
    pub binary_path: String,
}

// ---- engineering ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EngineeringSection {
    #[serde(default)]
    pub github: GithubPolicy,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GithubPolicy {
    #[serde(default)]
    pub always_approved_kinds: Vec<String>,
}

// ---- workflows ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company_repository: Option<CompanyRepository>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompanyRepository {
    pub host: String,
    pub repository: String,
    pub folder: String,
}

// ---- brand ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandSection {
    pub brand_identifier: String,
    pub version: String,
    /// Locale tag → strings.
    pub strings: BTreeMap<String, BrandStringsEntry>,
    pub logos: ThemedAsset,
    pub icon_set: ThemedAsset,
    pub design_tokens: BTreeMap<String, String>,
    #[serde(default)]
    pub fonts: Vec<BrandAsset>,
    pub links: BrandLinks,
    /// Locale tag → legal text.
    pub legal_text: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandStringsEntry {
    pub product_name: String,
    pub coordinator_name: String,
    pub about_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ThemedAsset {
    pub light: BrandAsset,
    pub dark: BrandAsset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandAsset {
    pub url: String,
    pub sha256: Sha256Hex,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandLinks {
    pub help_url: String,
    pub support_url: String,
    pub privacy_url: String,
}

// ---- locales ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalesSection {
    /// Empty = every bundled locale.
    #[serde(default)]
    pub offered: Vec<String>,
    /// `"en-GB"` when absent.
    #[serde(default = "en_gb")]
    pub fallback: String,
}

impl Default for LocalesSection {
    fn default() -> Self {
        LocalesSection {
            offered: Vec::new(),
            fallback: en_gb(),
        }
    }
}

// ---- help ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HelpSection {
    /// `None` = "Send to IT" goes through the person's own mail app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<HelpDestination>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HelpDestination {
    pub kind: HelpDestinationKind,
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HelpDestinationKind {
    TicketUrl,
    Email,
    SlackChannel,
}

// ---- updates ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UpdatesSection {
    pub feed_url: String,
}

// ---- sandbox ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SandboxSection {
    /// 30, 1–240.
    #[serde(default = "thirty")]
    pub session_idle_minutes: u32,
}

impl Default for SandboxSection {
    fn default() -> Self {
        SandboxSection {
            session_idle_minutes: thirty(),
        }
    }
}

// ---- team_knowledge (amended January 2026, ADR-110) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeSection {
    #[serde(default)]
    pub read_folders: Vec<CompanyReadFolder>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompanyReadFolder {
    /// Stable key; the Team Knowledge Service records it as the binding's company origin.
    pub key: String,
    /// Shown as the folder's name, with *Set by your company*.
    pub display_name: String,
    pub host: String,
    /// "owner/name".
    pub repository: String,
    pub branch: String,
    /// Relative to the repository root; "" = the root.
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub applies_to: AppliesTo,
    /// Read only while the branch requires review before a change is merged.
    #[serde(default)]
    pub review_required: bool,
    /// Whether the person may stop reading it.
    #[serde(default)]
    pub person_may_stop: bool,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AppliesTo {
    /// Every workspace in Team Knowledge mode.
    #[default]
    TeamKnowledge,
    /// Also every workspace with an active `GitRepositoryFolder` link (a code repository).
    Code,
}

// ---- knowledge_agents (amended January 2026, ADR-113) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeAgentsSection {
    #[serde(default)]
    pub front_doors: Vec<CompanyFrontDoor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompanyFrontDoor {
    /// Stable key; the Knowledge Query Client records it as the connection's company origin.
    pub key: String,
    /// Shown as the front door's name, with *Set by your company*.
    pub display_name: String,
    /// Base address, https only.
    pub url: String,
    #[serde(default)]
    pub applies_to: AppliesTo,
}

impl CommandInput for ProfileStatusInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for ProfileRefreshInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for ProfileGetDocumentInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_defaults_from_a_minimal_document() {
        let doc: OrganisationProfileDocument = serde_json::from_value(serde_json::json!({
            "format_version": 1,
            "profile_version": "2026.10.3",
            "usage": {"plan_mode": "balanced", "agents_at_once_max": 2},
            "locales": {"offered": ["en-GB", "de-DE"], "fallback": "en-GB"},
            "help": {"destination": {"kind": "email", "address": "it-help@example.com"}},
            "team_knowledge": {"read_folders": [{"key": "a", "display_name": "A", "host": "github.com",
                "repository": "o/n", "branch": "main", "applies_to": "code"}]},
            "knowledge_agents": {"front_doors": [{"key": "k", "display_name": "K",
                "url": "https://k.example.org"}]}
        }))
        .unwrap();
        assert_eq!(doc.harnesses, default_harnesses());
        assert_eq!(doc.usage.agents_at_once, 2);
        assert_eq!(doc.usage.agents_at_once_max, 2);
        assert_eq!(doc.usage.defaults, UsageDefaults::default());
        assert_eq!(doc.channels.allowed, vec![ChatChannel::Slack]);
        assert!(doc.agent_catalogue.person_may_add);
        assert_eq!(doc.agent_catalogue.trust_policy, serde_json::json!({}));
        assert_eq!(doc.limits.crew, CrewLimits::default());
        assert_eq!(doc.limits.run.wall_clock_minutes, 60);
        assert_eq!(doc.server.triggers.daily_runs, 20);
        assert!(doc.server.listening_allowed);
        assert!(doc.telegram.tools_enabled);
        assert_eq!(doc.sandbox.session_idle_minutes, 30);
        assert_eq!(
            doc.team_knowledge.read_folders[0].applies_to,
            AppliesTo::Code
        );
        assert_eq!(
            doc.knowledge_agents.front_doors[0].applies_to,
            AppliesTo::TeamKnowledge
        );
        assert!(doc.slack.is_none());
        // Every section's `Default` matches the serde defaults of an empty section.
        let empty: LocalesSection = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(empty, LocalesSection::default());
        let empty: ServerSection = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(empty, ServerSection::default());
        let empty: TelegramSection = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(empty, TelegramSection::default());
        let empty: AgentCatalogueSection = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(empty, AgentCatalogueSection::default());
        let empty: UsageSection = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(empty, UsageSection::default());
        let empty: SandboxSection = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(empty, SandboxSection::default());
        let empty: ChannelsSection = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(empty, ChannelsSection::default());
        let mail: EmailSection =
            serde_json::from_value(serde_json::json!({"system": "company_server",
                "imap": {"host": "imap.example.com"}}))
            .unwrap();
        assert_eq!(mail.sign_in_methods.len(), 3);
        assert_eq!(mail.people_link_folders, vec!["Inbox", "Sent"]);
        assert_eq!(mail.max_recipients, 20);
        assert_eq!(mail.imap.unwrap().security, MailSecurity::Tls);
        let gitlab: GitlabSection = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(gitlab.host, "gitlab.com");
        let pause: DailyPause = serde_json::from_value(serde_json::json!("250")).unwrap();
        assert_eq!(pause, DailyPause::After250);
    }

    #[test]
    fn unknown_document_keys_are_refused() {
        assert!(
            serde_json::from_value::<OrganisationProfileDocument>(serde_json::json!({
                "format_version": 1, "profile_version": "1.0.0", "extra": true
            }))
            .is_err()
        );
    }

    #[test]
    fn refresh_outcome_examples() {
        // The owner's example replies.
        assert_eq!(
            serde_json::to_value(RefreshOutcome::Unchanged).unwrap(),
            "unchanged"
        );
        assert_eq!(
            serde_json::to_value(RefreshOutcome::Refused {
                code: "PROF_SIGNATURE_INVALID".into()
            })
            .unwrap(),
            serde_json::json!({"refused": {"code": "PROF_SIGNATURE_INVALID"}})
        );
    }

    #[test]
    fn empty_inputs() {
        assert!(ProfileStatusInput {}.check().is_ok());
        assert!(ProfileRefreshInput {}.check().is_ok());
        assert!(ProfileGetDocumentInput {}.check().is_ok());
        assert!(serde_json::from_value::<ProfileStatusInput>(serde_json::json!({"a": 1})).is_err());
    }
}
