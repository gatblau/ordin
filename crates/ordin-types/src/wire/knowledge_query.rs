//! 2C part 2 §12 Knowledge agents and the Open Knowledge Format (ADR-112, ADR-113).

use serde::{Deserialize, Serialize};

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
    pub commit: String,    // 40 or 64 lower-case hex
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

/// The default `KnowledgeQueryRequest.limit`.
fn twenty() -> u8 {
    20
}
