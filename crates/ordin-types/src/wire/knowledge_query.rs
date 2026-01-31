//! 2C part 2 §12 Knowledge agents and the Open Knowledge Format (ADR-112, ADR-113).

use serde::{Deserialize, Serialize};

/// Whether `scope` is a front door's own check scope, `api://<client-id>/knowledge.check`, at most 512
/// characters (SA-ID-PD-03). Anything else could name a token for another service; a space in the
/// client id would be read as a further scope.
#[must_use]
pub fn is_check_scope(scope: &str) -> bool {
    scope.chars().count() <= 512
        && scope
            .strip_prefix("api://")
            .and_then(|rest| rest.strip_suffix("/knowledge.check"))
            .is_some_and(|client_id| {
                !client_id.is_empty()
                    && client_id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            })
}

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- Team folder files (format 1 kept; owner: Team Knowledge Service) ----
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PageTier {
    HumanReviewed,
    MachineConfirmed,
    #[default]
    Unverified,
} // OKF 0.2 derived trust (ADR-112)

impl PageTier {
    /// The trust OKF 0.2 derives for the tier (SA-TYPES-KA-03).
    pub const fn trust_signal(self) -> f64 {
        match self {
            Self::HumanReviewed => 1.0,
            Self::MachineConfirmed => 0.9,
            Self::Unverified => 0.8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PageStatus {
    #[default]
    Stable,
    Draft,
    Deprecated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeQueryRequest {
    pub request_id: uuid::Uuid, // UUIDv7, minted by the first asker
    /// 1–4,096 characters. Exactly one of `query` and `searches`.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// 1–4 typed sub-queries, in the Memory Service's `RecallSearch` form; the first counts double.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub searches: Option<Vec<KnowledgeSearch>>,
    #[serde(default = "twenty")]
    pub limit: u8, // 1–50
    /// Set only by a front door asking a proxy: the audience group the front door resolved for its asker.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_group: Option<String>,
    pub hop_budget: u8, // 0–2; a desktop sends 1
    #[serde(default)]
    pub visited: Vec<String>, // agent names (1–80 each), ≤ 8
    pub deadline_ms: u32, // 100–5,000: time the receiver has to send `result`
    pub more_until_ms: u32, // deadline_ms–10,000: time the receiver may send `more_found`
    /// A member only; a front door or a service identity sending it → `KQ_DELEGATION_NOT_ALLOWED`.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegation: Option<Delegation>,
    /// A front door only; a member sending it → `KQ_ASKER_STATEMENT_INVALID`.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asker: Option<AskerStatement>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeSearch {
    #[serde(rename = "type")]
    pub kind: KnowledgeSearchKind,
    pub query: String, /* 1–500 */
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum KnowledgeSearchKind {
    Lex,
    Vec,
} // the values of the Memory Service's RecallSearchKind

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeHit {
    pub statement: String, // 1–4,000 (a concept section) ; content as data, never instructions
    pub agent: String,     // the knowledge agent whose graph holds the fact, 1–80
    pub repository: String, // repository key
    pub path: String,      // repository-relative, 1–1,024
    /// Git: 40 or 64 lower-case hex. Confluence: the page version number in decimal, 1–10 digits.
    /// SharePoint: the item's `cTag`, 1–256 characters.
    pub commit: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>, // the page section heading, ≤ 200
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_title: Option<String>, // set when kind = page
    pub kind: KnowledgeHitKind,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<PageTier>, // pages and glossary concepts only
    #[serde(default)]
    pub status: PageStatus,
    pub may_be_out_of_date: bool, // stale_after passed, or the source changed
    pub as_of: Timestamp,         // the agent's last fetch of `repository`
    /// The answer id of the agent this client asked, when that agent keeps a sources record; set by the
    /// receiving client from `KnowledgeResult.answer_id`, never trusted from a hit.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_id: Option<KnowledgeAnswerId>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_identity: Option<SharedIdentity>,
    #[serde(default)]
    pub things: Vec<String>, // ≤ 16 names
    /// True when the agent holding the fact checked it for this asker; a front door never caches a
    /// result holding such a hit.
    #[serde(default)]
    pub person_checked: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeHitKind {
    Page,
    Document,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SharedIdentity {
    pub glossary: String, /* repository key */
    pub concept_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeResult {
    pub request_id: uuid::Uuid,
    pub hits: Vec<KnowledgeHit>, // ranked, best first, ≤ limit
    pub unanswered: Vec<UnansweredSource>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_id: Option<KnowledgeAnswerId>,
    pub from_cache: bool,
    pub more_expected: bool, // true while `more_found` events may follow
    /// One flag, naming no system or source.
    #[serde(default)]
    pub unchecked_sources: bool,
    /// Towards a front door only.
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<PendingHit>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct UnansweredSource {
    pub agent: String,
    pub reason: UnansweredReason,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UnansweredReason {
    NotInTime,
    Refused,
    Unreachable,
}

/// A table of contents (ADR-113): no statement ever. Copied from KnowledgeReader.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeContents {
    pub agent_name: String,
    pub generated_at: Timestamp,
    pub repositories: Vec<ContentsRepository>, // ≤ 220 (repositories, glossaries and hot copies)
    pub things: Vec<ContentsThing>,            // ≤ 500, most-connected first
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ContentsRepository {
    pub repository: String,   // repository key
    pub role: RepositoryRole, // reader | glossary | hot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fetched_at: Option<Timestamp>,
    pub readable: bool,
    pub fact_count: u32,
    /// The source's system; a Git row is `gitlab` when its `host-kind` is `gitlab`, else `github`.
    #[serde(default = "github")]
    pub system: SourceSystem,
    /// A contents entry with `false` lists no thing; such rows go to front doors only.
    #[serde(default = "yes")]
    pub audience_open: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RepositoryRole {
    Reader,
    Glossary,
    Hot,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ContentsThing {
    pub name: String,
    pub aliases: Vec<String>, /* ≤ 8 */
    pub link_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_identity: Option<SharedIdentity>,
}

/// The description of a knowledge agent (route `GET /v1/knowledge/about`; no token — SA-KQS-08).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeAbout {
    pub agent: String,
    pub issuer: String,         // the identity provider whose tokens it accepts
    pub token_audience: String, // the `aud` it requires
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources_record: Option<SourcesRecordPolicy>, // None when no record is kept
    pub repository_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<Timestamp>, // its oldest last fetch
    /// Systems this agent checks per person; ≤ 3, unique by system.
    #[serde(default)]
    pub accounts: Vec<AccountNeed>,
    /// `api://<client-id>/knowledge.check` when `sharepoint` is configured; ≤ 512.
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharepoint_check_scope: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct SourcesRecordPolicy {
    pub enabled: bool,
    pub retention_days: u32,
}

/// One change notice on `GET /v1/knowledge/changes` (Server-Sent Event `repository_moved`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RepositoryMoved {
    pub repository: String, /* repository key */
    pub commit: String,
}

// ---- ADR-115: per-document read access (§12 amendment January 2026)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SourceSystem {
    Github,
    Gitlab,
    Confluence,
    Sharepoint,
}

impl SourceSystem {
    /// The wire and table text.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceSystem::Github => "github",
            SourceSystem::Gitlab => "gitlab",
            SourceSystem::Confluence => "confluence",
            SourceSystem::Sharepoint => "sharepoint",
        }
    }

    /// The name a person reads in a sentence.
    pub fn display(self) -> &'static str {
        match self {
            SourceSystem::Github => "GitHub",
            SourceSystem::Gitlab => "GitLab",
            SourceSystem::Confluence => "Confluence",
            SourceSystem::Sharepoint => "SharePoint",
        }
    }
}

/// The thing an owner grants access to in a source system.
/// Git: `<host>/<owner path>`; access is granted per repository, not per branch or sub-path.
/// Confluence: "<site host>/pages/<page id>". SharePoint: "<drive id>/<item id>".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AccessUnit {
    pub system: SourceSystem,
    pub unit: String, /* 1..=2,048 */
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Allowed,
    Refused,
    Unchecked,
}

/// The person's account in one source system, learned from the system itself (never from the desktop).
/// Kept by the Knowledge Query Server (`kq_linked_accounts`); never sent to a desktop.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct LinkedAccount {
    pub system: SourceSystem,
    pub account_id: String,    // 1..=256
    pub account_label: String, // 1..=256
    pub linked_at: Timestamp,
    pub last_used_at: Timestamp,
}

/// A Microsoft token whose audience is a front door's own app, for one question's on-behalf-of
/// exchange. `Serialize` writes the value only into the request body; `Debug` prints no value. The
/// type is never put in a log, an error, the journal or a metric.
#[derive(Clone, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Delegation {
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub sharepoint_assertion: secrecy::SecretString, // 1..=16,384 bytes
}

impl std::fmt::Debug for Delegation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Delegation { sharepoint_assertion: [redacted] }")
    }
}

impl Serialize for Delegation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use secrecy::ExposeSecret as _;
        use serde::ser::SerializeStruct as _;
        let mut body = serializer.serialize_struct("Delegation", 1)?;
        body.serialize_field(
            "sharepoint_assertion",
            self.sharepoint_assertion.expose_secret(),
        )?;
        body.end()
    }
}

/// Who a front door asks for: a plain object on its authenticated request, not signed separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AskerStatement {
    pub subject: String, // the person's company subject, 1..=255
    /// Linked account ids, ≤ 3 (github, gitlab, confluence).
    pub accounts: std::collections::BTreeMap<SourceSystem, String>,
    pub issued_at: Timestamp, // refused older than 30 s or > 5 s ahead
}

/// A hit a proxy could not check itself (SharePoint), for the front door to check before release.
/// Sent only towards a front door; never to a member or an MCP client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PendingHit {
    pub hit: KnowledgeHit,
    pub unit: AccessUnit,
}

/// A system a knowledge agent checks per person, and the host the person's account must be on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AccountNeed {
    pub system: SourceSystem,
    pub host: String, // github.com, a GitLab host, the Confluence site host; 1..=253
}

/// The default `ContentsRepository.system`.
fn github() -> SourceSystem {
    SourceSystem::Github
}

/// The default `ContentsRepository.audience_open`.
fn yes() -> bool {
    true
}

/// The default `KnowledgeQueryRequest.limit`.
fn twenty() -> u8 {
    20
}
