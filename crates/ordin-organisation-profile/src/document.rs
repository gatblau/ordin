//! The organisation profile document (OrganisationProfile.md *The organisation profile document*, with
//! the amendments of ADR-102, ADR-110 and ADR-113).
//!
//! Every struct refuses unknown keys. A section that may be absent takes its type's `Default`, which
//! uses the same values as the per-key `default = "…"` helpers, so a consumer never repeats a default.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use ordin_types::domain::identity::{AskBefore, SummariseWhen, VoiceRegister, VoiceTone};
pub use ordin_types::domain::runs::ExternalActionKind;
pub use ordin_types::num::Sha256Hex;
pub use ordin_types::wire::run::ModelLevel;

/// Deserialises a unit-variant enum through `deserialize_str`, so that a parse failure inside a map
/// keyed by it reports the key in its dotted path (`craft.apps[0].platforms.macos_arm64.sha256`) rather
/// than `?`. The accepted names are exactly the serde names the `Serialize` derive writes.
macro_rules! deserialize_by_name {
    ($ty:ident { $($name:literal => $variant:ident),+ $(,)? }) => {
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                const NAMES: &[&str] = &[$($name),+];
                let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                match raw.as_ref() {
                    $($name => Ok($ty::$variant),)+
                    other => Err(serde::de::Error::unknown_variant(other, NAMES)),
                }
            }
        }
    };
}

// ---- default helpers (private; each returns the value its name states)

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
fn gitlab_com() -> String {
    "gitlab.com".to_owned()
}

/// The whole document. Top-level keys are 2D-2's sections, plus `profile_version`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    // spec: ordin-desktop/components/OrganisationProfile#SA-CLOSE-02
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
    // spec: ordin-desktop/components/OrganisationProfile#SA-CLOSE-03
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
    /// Amendment January 2026 (ADR-110).
    #[serde(default)]
    pub team_knowledge: TeamKnowledgeSection,
    /// Amendment January 2026 (ADR-113).
    #[serde(default)]
    pub knowledge_agents: KnowledgeAgentsSection,
}

// ---- agent_catalogue (08:32; ADR-082, ADR-086)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentCatalogueSection {
    #[serde(default)]
    pub repositories: Vec<AgentRepositoryEntry>,
    #[serde(default = "yes")]
    pub person_may_add: bool,
    /// Executable-trust policy over tool bindings and hooks; typed and applied by the Checker.
    /// Organisation Profile checks only that it is a JSON object of at most 262,144 bytes.
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryFormat {
    Ordin,
    Apm,
}

// ---- remote_agents (02:698; ADR-098 item 8)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAgentRoute {
    HostedAgent,
    Slack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoteIdentity {
    pub issuer: String,
    pub client_id: String,
    pub audience: String,
}

// ---- slack, confluence, microsoft, google, identity_provider
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackSection {
    /// The Slack Thread Adapter's workspace and dedicated channel (ADR-008). Optional (ADR-102): absent →
    /// the Slack Thread Adapter offers no Slack route, as when the whole section is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedicated_channel: Option<String>,
    /// The Slack app IT approves, PKCE-enabled (08:82, revision 0.49.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// IT can turn the Slack tools, linked channels, drafts and Ordin's reactions off (Slack Service);
    /// the person's own handover notices are not affected (`server.external_actions_off` stops those).
    #[serde(default = "yes")]
    pub tools_enabled: bool,
    /// Only these channel ids may be linked to a workspace or named by a lifecycle stage;
    /// `None` = any channel the person belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_channels: Option<Vec<String>>,
    /// IT can turn every "without asking" rule off; Slack posts then always ask.
    #[serde(default = "yes")]
    pub rules_allowed: bool,
}

impl Default for SlackSection {
    /// The values the Slack Service reads when the whole section is absent (ADR-102).
    fn default() -> Self {
        SlackSection {
            workspace: None,
            dedicated_channel: None,
            client_id: None,
            tools_enabled: yes(),
            allowed_channels: None,
            rules_allowed: yes(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ConfluenceSection {
    pub site: String,
    pub oauth_client_id: String,
    /// A company-hosted token exchange, used only if C-12 shows Atlassian 3LO needs a client secret (08:82).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_exchange_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MicrosoftSection {
    pub tenant_id: String,
    pub client_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GoogleSection {
    pub client_id: String,
    /// The non-confidential secret of a Google Desktop OAuth client (08:82, revision 0.49.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentityProviderSection {
    pub issuer: String,
    pub client_id: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

// ---- harnesses (02:707, :829) — default [{name: claude_code, tested_versions: []}]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HarnessEntry {
    pub name: HarnessName,
    #[serde(default)]
    pub tested_versions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessName {
    ClaudeCode,
    Copilot,
}

deserialize_by_name!(HarnessName { "claude_code" => ClaudeCode, "copilot" => Copilot });

// ---- usage (ADR-062; 08:71)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageSection {
    #[serde(default = "balanced")]
    pub plan_mode: PlanMode,
    /// Missing kinds allow all four levels.
    #[serde(default)]
    pub levels_allowed: BTreeMap<WorkKind, Vec<ModelLevel>>,
    /// harness → level → model name. Missing entries take the defaults of `level_model`.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanMode {
    Save,
    Balanced,
    Best,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Talking,
    Coordinating,
    Routine,
    Documents,
    Code,
}

deserialize_by_name!(WorkKind {
    "talking" => Talking,
    "coordinating" => Coordinating,
    "routine" => Routine,
    "documents" => Documents,
    "code" => Code,
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// `None` = "true in workspaces with code" (2D-2).
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

// ---- voice (ADR-063; 08:73)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

// ---- limits (07:44–49; 02:486)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LimitsSection {
    #[serde(default)]
    pub crew: CrewLimits,
    #[serde(default)]
    pub run: RunLimitsSection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

// ---- history (2D-2; 1D Configuration pass item 2)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HistorySection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_days: Option<u32>,
}

// ---- email (08:72; ADR-070, ADR-071)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EmailSection {
    pub system: MailSystem,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Port default 993 (applied by the Mail Service).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imap: Option<MailServer>,
    /// Port default 465 (applied by the Mail Service).
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailSystem {
    CompanyServer,
    Microsoft365,
    GoogleWorkspace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MailServer {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default = "tls")]
    pub security: MailSecurity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailSecurity {
    Tls,
    Starttls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailSignInMethod {
    SingleSignOn,
    AppPassword,
    MailPassword,
}

// ---- channels, telegram (A-40; ADR-075, ADR-077)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChannelsSection {
    /// `[slack]` by default.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannel {
    Telegram,
    Slack,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramSection {
    #[serde(default = "yes")]
    pub tools_enabled: bool,
    /// `None` = any chat a workspace links. Telegram chat ids are integers (SA-RECON-05).
    // spec: ordin-desktop/components/OrganisationProfile#SA-RECON-05
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

// ---- server (ADR-073, ADR-074, ADR-096 item 4)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

// ---- nats (ADR-074)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NatsSection {
    #[serde(default)]
    pub servers: Vec<String>,
    #[serde(default)]
    pub tls: NatsTls,
    #[serde(default)]
    pub subjects_allowed: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NatsTls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_certificate: Option<String>,
}

// ---- craft (02:700, :747)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

// spec: ordin-desktop/components/OrganisationProfile#SA-CLOSE-01
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftApp {
    /// The app's identifier (the `craft.file_types` values name it).
    pub app_id: String,
    pub version: String,
    /// The app's allowed list (ADR-017); `None` = every tool the app's MCP mode lists (SA-CLOSE-01).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    pub platforms: BTreeMap<ProfilePlatform, CraftAsset>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftAsset {
    pub asset_url: String,
    pub sha256: Sha256Hex,
    pub file_name: String,
}

/// Platforms a pinned download names (craft apps and language servers; K9, K12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
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

deserialize_by_name!(ProfilePlatform {
    "macos_arm64" => MacosArm64,
    "macos_x86_64" => MacosX86_64,
    "windows_x86_64" => WindowsX86_64,
    "linux_x86_64" => LinuxX86_64,
});

// ---- gitlab (D9; SA-CLOSE-02)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GitlabSection {
    /// `gitlab.com` when absent.
    #[serde(default = "gitlab_com")]
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

// ---- language_servers (ADR-055; SA-CLOSE-03)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LanguageServerEntry {
    pub language: ServerLanguage,
    pub name: String,
    pub version: String,
    pub platforms: BTreeMap<ProfilePlatform, LanguageServerAsset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerLanguage {
    Go,
    Rust,
    Java,
    Typescript,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LanguageServerAsset {
    pub asset_url: String,
    pub sha256: Sha256Hex,
    pub file_name: String,
    pub binary_path: String,
}

// ---- engineering (02:766; 04:139; SA-GH-01)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EngineeringSection {
    #[serde(default)]
    pub github: GithubPolicy,
    /// 2D-2's key, read by the Project Service (amended January 2026).
    #[serde(default)]
    pub coding_tasks: CodingTasksPolicy,
}

/// `engineering.coding_tasks` (2D-2; ADR-053). An absent section or key reads as the default, 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CodingTasksPolicy {
    #[serde(default = "three")]
    pub max_attempts: u32,
}

impl Default for CodingTasksPolicy {
    fn default() -> Self {
        CodingTasksPolicy {
            max_attempts: three(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct GithubPolicy {
    #[serde(default)]
    pub always_approved_kinds: Vec<String>,
}

// ---- workflows (08:20; ADR-066)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowsSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company_repository: Option<CompanyRepository>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompanyRepository {
    pub host: String,
    pub repository: String,
    pub folder: String,
}

// ---- brand (08:33; ADR-041). Brand fetches each asset, checks its hash and the contrast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandStringsEntry {
    pub product_name: String,
    pub coordinator_name: String,
    pub about_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ThemedAsset {
    pub light: BrandAsset,
    pub dark: BrandAsset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandAsset {
    pub url: String,
    pub sha256: Sha256Hex,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BrandLinks {
    pub help_url: String,
    pub support_url: String,
    pub privacy_url: String,
}

// ---- locales (ADR-042; 08:34)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LocalesSection {
    /// Empty = every bundled locale (Localisation intersects with its bundled catalogues).
    #[serde(default)]
    pub offered: Vec<String>,
    /// `en-GB` by default.
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

// ---- help (02:704; 08:14–15; OQ-HELP-REPLY decision (b))
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HelpSection {
    /// `None` = "Send to IT" goes through the person's own mail app (08:14; SA-RECON-06).
    // spec: ordin-desktop/components/OrganisationProfile#SA-RECON-06
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<HelpDestination>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HelpDestination {
    pub kind: HelpDestinationKind,
    pub address: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelpDestinationKind {
    TicketUrl,
    Email,
    SlackChannel,
}

// ---- updates (2D-2). A verified profile's feed replaces the installer's (D10; SA-RECON-03); this
// component only carries the value, the Setup Assistant owns the update check.
// spec: ordin-desktop/components/OrganisationProfile#SA-RECON-03
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UpdatesSection {
    pub feed_url: String,
}

// ---- sandbox (09:737; 04:403)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

// ---- team_knowledge (ADR-110; 2D-2 amended January 2026, decision 90)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TeamKnowledgeSection {
    #[serde(default)]
    pub read_folders: Vec<CompanyReadFolder>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompanyReadFolder {
    /// Stable key; the Team Knowledge Service records it as the binding's company origin.
    pub key: String,
    /// Shown as the folder's name, with *Set by your company*.
    pub display_name: String,
    pub host: String,
    /// "owner/name"
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
    /// Whether the person may stop reading it (SA-PROFR-03).
    // spec: ordin-desktop/components/OrganisationProfile#SA-PROFR-03
    #[serde(default)]
    pub person_may_stop: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppliesTo {
    /// Every workspace in Team Knowledge mode.
    #[default]
    TeamKnowledge,
    /// Also every workspace with an active `GitRepositoryFolder` link (a code repository).
    Code,
}

// ---- knowledge_agents (ADR-113; 2D-2 amended January 2026, decision 92)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeAgentsSection {
    #[serde(default)]
    pub front_doors: Vec<CompanyFrontDoor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompanyFrontDoor {
    /// Stable key; the Knowledge Query Client records it as the connection's company origin.
    pub key: String,
    /// Shown as the front door's name, with *Set by your company*.
    pub display_name: String,
    /// Base address, https only (SA-PROF-KA-02).
    pub url: String,
    /// The ADR-110 type, unchanged (SA-PROF-KA-03).
    // spec: ordin-desktop/components/OrganisationProfile#SA-PROF-KA-03
    #[serde(default)]
    pub applies_to: AppliesTo,
}

impl OrganisationProfileDocument {
    /// The model name a harness uses at a level, applying 2D-2's defaults:
    /// claude_code: smaller → "haiku", standard → "sonnet", most_capable → "opus";
    /// copilot: automatic → None (Copilot's own selection).
    ///
    // SPEC-GAP-2: the spec states no default for claude_code `automatic`, nor for copilot's other three
    // levels. Both return `None` here (the Usage Planner and the Copilot adapter then choose); an entry
    // in `usage.level_models` is always returned as written.
    pub fn level_model(&self, harness: HarnessName, level: ModelLevel) -> Option<String> {
        if let Some(model) = self
            .usage
            .level_models
            .get(&harness)
            .and_then(|levels| levels.get(&level))
        {
            return Some(model.clone());
        }
        let default = match (harness, level) {
            (HarnessName::ClaudeCode, ModelLevel::Smaller) => "haiku",
            (HarnessName::ClaudeCode, ModelLevel::Standard) => "sonnet",
            (HarnessName::ClaudeCode, ModelLevel::MostCapable) => "opus",
            _ => return None,
        };
        Some(default.to_owned())
    }

    /// `agent_catalogue.it_approval_destination`, else `help.destination`.
    pub fn it_approval_destination(&self) -> Option<&HelpDestination> {
        self.agent_catalogue
            .it_approval_destination
            .as_ref()
            .or(self.help.destination.as_ref())
    }

    /// Every section at the default 2D-2 gives it. `profile_version` is `0.0.0`.
    ///
    // SPEC-GAP-3: the spec gives the defaults document no `profile_version`; `0.0.0` (the lowest value
    // the version rule admits) is used, and `SnapshotSource::Defaults` tells a reader it is not a
    // verified profile.
    pub(crate) fn defaults() -> Self {
        OrganisationProfileDocument {
            format_version: crate::FORMAT_VERSION,
            profile_version: "0.0.0".to_owned(),
            agent_catalogue: AgentCatalogueSection::default(),
            remote_agents: Vec::new(),
            slack: None,
            confluence: None,
            microsoft: None,
            google: None,
            identity_provider: None,
            gitlab: None,
            harnesses: default_harnesses(),
            usage: UsageSection::default(),
            voice: VoiceSection::default(),
            limits: LimitsSection::default(),
            history: HistorySection::default(),
            email: None,
            channels: ChannelsSection::default(),
            telegram: TelegramSection::default(),
            server: ServerSection::default(),
            nats: NatsSection::default(),
            craft: CraftSection::default(),
            language_servers: Vec::new(),
            engineering: EngineeringSection::default(),
            workflows: WorkflowsSection::default(),
            brand: None,
            locales: LocalesSection::default(),
            help: HelpSection::default(),
            updates: None,
            sandbox: SandboxSection::default(),
            team_knowledge: TeamKnowledgeSection::default(),
            knowledge_agents: KnowledgeAgentsSection::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand-written `Deserialize` of the map-key enums reads exactly what `Serialize` writes.
    #[test]
    fn map_key_enums_round_trip_and_refuse_other_names() {
        for platform in [
            ProfilePlatform::MacosArm64,
            ProfilePlatform::MacosX86_64,
            ProfilePlatform::WindowsX86_64,
            ProfilePlatform::LinuxX86_64,
        ] {
            let text = serde_json::to_string(&platform).unwrap();
            assert_eq!(
                serde_json::from_str::<ProfilePlatform>(&text).unwrap(),
                platform
            );
        }
        for kind in [
            WorkKind::Talking,
            WorkKind::Coordinating,
            WorkKind::Routine,
            WorkKind::Documents,
            WorkKind::Code,
        ] {
            let text = serde_json::to_string(&kind).unwrap();
            assert_eq!(serde_json::from_str::<WorkKind>(&text).unwrap(), kind);
        }
        for harness in [HarnessName::ClaudeCode, HarnessName::Copilot] {
            let text = serde_json::to_string(&harness).unwrap();
            assert_eq!(serde_json::from_str::<HarnessName>(&text).unwrap(), harness);
        }
        assert!(serde_json::from_str::<ProfilePlatform>("\"linux_arm64\"").is_err());
        assert!(serde_json::from_str::<HarnessName>("\"copilot_cli\"").is_err());
    }

    #[test]
    fn level_model_defaults_and_overrides() {
        let mut doc = OrganisationProfileDocument::defaults();
        assert_eq!(
            doc.level_model(HarnessName::ClaudeCode, ModelLevel::Smaller)
                .as_deref(),
            Some("haiku")
        );
        assert_eq!(
            doc.level_model(HarnessName::ClaudeCode, ModelLevel::Standard)
                .as_deref(),
            Some("sonnet")
        );
        assert_eq!(
            doc.level_model(HarnessName::ClaudeCode, ModelLevel::MostCapable)
                .as_deref(),
            Some("opus")
        );
        assert_eq!(
            doc.level_model(HarnessName::Copilot, ModelLevel::Automatic),
            None
        );
        doc.usage.level_models.insert(
            HarnessName::Copilot,
            BTreeMap::from([(ModelLevel::Standard, "gpt-5".to_owned())]),
        );
        assert_eq!(
            doc.level_model(HarnessName::Copilot, ModelLevel::Standard)
                .as_deref(),
            Some("gpt-5")
        );
    }

    #[test]
    fn it_approval_destination_falls_back_to_help() {
        let mut doc = OrganisationProfileDocument::defaults();
        assert_eq!(doc.it_approval_destination(), None);
        let help = HelpDestination {
            kind: HelpDestinationKind::Email,
            address: "it@example.com".into(),
        };
        doc.help.destination = Some(help.clone());
        assert_eq!(doc.it_approval_destination(), Some(&help));
        let it = HelpDestination {
            kind: HelpDestinationKind::TicketUrl,
            address: "https://it.example.com/new".into(),
        };
        doc.agent_catalogue.it_approval_destination = Some(it.clone());
        assert_eq!(doc.it_approval_destination(), Some(&it));
    }

    /// An absent section takes its type's `Default`, which holds the same values as the per-key helpers.
    #[test]
    fn absent_sections_equal_their_defaults() {
        let parsed: OrganisationProfileDocument =
            serde_json::from_str(r#"{"format_version":1,"profile_version":"0.0.0"}"#).unwrap();
        assert_eq!(parsed, OrganisationProfileDocument::defaults());
        let empty_sections: OrganisationProfileDocument = serde_json::from_str(
            r#"{"format_version":1,"profile_version":"0.0.0","agent_catalogue":{},"usage":{"defaults":{}},
               "limits":{"crew":{},"run":{}},"channels":{},"telegram":{},"server":{"triggers":{}},
               "locales":{},"sandbox":{}}"#,
        )
        .unwrap();
        assert_eq!(empty_sections, OrganisationProfileDocument::defaults());
        let gitlab: GitlabSection = serde_json::from_str("{}").unwrap();
        assert_eq!(gitlab.host, "gitlab.com");
        let email: EmailSection = serde_json::from_str(r#"{"system":"microsoft365"}"#).unwrap();
        assert_eq!(email.sign_in_methods.len(), 3);
        assert_eq!(email.people_link_folders, ["Inbox", "Sent"]);
        assert_eq!(email.max_recipients, 20);
        let server: MailServer = serde_json::from_str(r#"{"host":"imap.example.com"}"#).unwrap();
        assert_eq!(server.security, MailSecurity::Tls);
        let pause: DailyPause = serde_json::from_str("\"250\"").unwrap();
        assert_eq!(pause, DailyPause::After250);
    }
}
