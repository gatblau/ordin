//! The public types of the crate (KnowledgeQueryClient.md *Public Interface*, *Shared Context*, and
//! the ADR-115 amendment). The wire shapes live in `ordin_types::wire::knowledge_query`.

use std::sync::Arc;

use ordin_identity::SecretString;
use ordin_types::common::SetBy;
use ordin_types::domain::agent_profile::{KnowledgeProxy, ServiceIdentity};
use ordin_types::time::Timestamp;
pub use ordin_types::wire::knowledge_query::{
    AccessUnit, AccountNeed, AskerStatement, Delegation, PendingHit, SourceSystem,
};
use ordin_types::wire::knowledge_query::{
    ContentsRepository, KnowledgeAbout, KnowledgeContents, KnowledgeHit, KnowledgeSearch,
    RepositoryMoved, SourcesRecordPolicy, UnansweredSource,
};
use ordin_types::{KnowledgeConnectionId, WorkspaceId};
use serde::{Deserialize, Serialize};

// ---- constants (2D-4)

/// ADR-113 (decision 92).
pub const FIRST_DEADLINE_MS: u32 = 1_000;
/// ADR-113.
pub const MORE_FOUND_UNTIL_MS: u32 = 5_000;
/// ADR-113: the backup request starts when the primary has sent no `result` within this time.
pub const HEDGE_DELAY_MS: u64 = 300;
/// ADR-113.
pub const DEFAULT_HOP_BUDGET: u8 = 1;
/// Chosen in the spec (SA-KQC-03).
// spec: ordin-desktop/components/KnowledgeQueryClient#SA-KQC-03
pub const MAX_HOP_BUDGET: u8 = 2;
/// ADR-113.
pub const MAX_VISITED: usize = 8;
/// Chosen in the spec: subtracted from a deadline before it is sent on.
pub const NETWORK_ALLOWANCE_MS: u64 = 50;
/// ADR-113.
pub const MAX_CONNECTIONS_PER_WORKSPACE: usize = 8;
/// 2D-4 *Stage-1 rank fusion*.
pub const RRF_K: f64 = 60.0;
/// ADR-113 (open connections).
pub const IDLE_TIMEOUT_SECONDS: u64 = 90;
/// Chosen in the spec (SA-KQC-05).
// spec: ordin-desktop/components/KnowledgeQueryClient#SA-KQC-05
pub const CONTENTS_REFRESH_MINUTES: u64 = 60;
/// Chosen in the spec.
pub const ABOUT_TIMEOUT_SECONDS: u64 = 5;
/// Chosen in the spec; the last value repeats.
pub const CHANGE_STREAM_BACKOFF_SECONDS: [u64; 7] = [1, 2, 4, 8, 16, 32, 60];
/// ADR-113 (limit at most 50).
pub const MAX_HITS_PER_SOURCE: usize = 50;

// ---- the events of a question's stream

/// A `more_found` event.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MoreFoundEvent {
    pub hits: Vec<KnowledgeHit>,
    #[serde(default)]
    pub unanswered: Vec<UnansweredSource>,
    #[serde(default)]
    pub unchecked_sources: bool,
    #[serde(default)]
    pub pending: Vec<PendingHit>,
}

/// A `done` event.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DoneEvent {
    #[serde(default)]
    pub unanswered: Vec<UnansweredSource>,
    #[serde(default)]
    pub unchecked_sources: bool,
}

// ---- construction

/// What the program passes to `KnowledgeQueryClient::new`.
pub struct KnowledgeClientDeps {
    pub journal: Arc<ordin_journal::Journal>,
    /// Some on D and P (the person's sign-in); None on A.
    pub identity: Option<Arc<ordin_identity::IdentityService>>,
    /// The program's shared HTTP client (reqwest 0.13.5, rustls); this crate derives per-agent clients.
    pub http: reqwest::Client,
    pub mode: KnowledgeClientMode,
}

pub enum KnowledgeClientMode {
    /// Desktop and personal server: the person's company sign-in.
    Person { sign_in: CompanySignIn },
    /// Agent profile (a front door asking its proxies): the agent's own service identity.
    /// `service_token` is None only when `proxies` is empty; `new` refuses proxies with no service
    /// token (`KQ_CONFIG_INVALID`).
    Agent {
        proxies: Vec<KnowledgeProxy>,
        service_token: Option<ServiceTokenSettings>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanySignIn {
    pub issuer: String,
    pub client_id: String,
}

/// `ServiceIdentity` with its secret resolved by the program from `client_secret_env` (2D-1d).
///
/// The spec names `secrecy::SecretString`; this crate's manifest does not list `secrecy`, so the
/// workspace's own `ordin_identity::SecretString` (same contract: never prints, zeroised) is used.
pub struct ServiceTokenSettings {
    pub identity: ServiceIdentity,
    pub client_secret: SecretString,
}

/// ADR-113: a company-named front door of the organisation profile (copied, no crate edge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyFrontDoor {
    pub key: String,
    pub display_name: String,
    pub url: String,
    pub applies_to: AppliesTo,
}

/// As `team_knowledge.read_folders[]` uses it (copied, no crate edge).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppliesTo {
    #[default]
    TeamKnowledge,
    Code,
}

// ---- asking

pub enum AskTarget {
    /// Every live connection of the workspace (D, P).
    Workspace(WorkspaceId),
    /// The front door's chosen proxies, each with an optional backup covering the same repositories (A).
    Proxies(Vec<ProxyAsk>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyAsk {
    /// Proxy name.
    pub primary: String,
    pub backup: Option<String>,
}

pub struct KnowledgeAsk {
    pub target: AskTarget,
    pub query: Option<String>,
    pub searches: Option<Vec<KnowledgeSearch>>,
    /// 1-50.
    pub limit: u8,
    /// A only.
    pub access_group: Option<String>,
    /// Budget the receivers get.
    pub hop_budget: u8,
    /// Agents already visited (on A, this agent included).
    pub visited: Vec<String>,
    pub first_deadline: tokio::time::Instant,
    pub more_until: tokio::time::Instant,
    pub request_id: uuid::Uuid,
    /// ADR-115, A only; None on D and P.
    pub asker: Option<AskerStatement>,
}

#[derive(Debug, Clone)]
pub struct KnowledgeBatch {
    /// Merged, de-duplicated, grouped, at most `limit`.
    pub hits: Vec<KnowledgeHit>,
    /// Agents that sent a result.
    pub answered: Vec<String>,
    pub unanswered: Vec<UnansweredSource>,
    /// (agent, repository key, commit) of every repository the merged hits came from.
    pub relied_on: Vec<(String, String, String)>,
    pub final_batch: bool,
    /// ADR-115: the OR of every answering agent's flag.
    pub unchecked_sources: bool,
    /// ADR-115, A only; never merged into `hits`.
    pub pending: Vec<(String, PendingHit)>,
}

/// The answer of one ask: the first batch by the first deadline, later batches as More found.
pub struct KnowledgeAnswerStream {
    pub(crate) rx: tokio::sync::mpsc::Receiver<KnowledgeBatch>,
    pub(crate) done: bool,
    /// Dropping the stream cancels every open task.
    pub(crate) _guard: tokio_util::sync::DropGuard,
}

impl KnowledgeAnswerStream {
    /// Resolves at the first deadline at the latest.
    pub async fn first(&mut self) -> KnowledgeBatch {
        match self.rx.recv().await {
            Some(batch) => {
                if batch.final_batch {
                    self.done = true;
                }
                batch
            }
            None => {
                self.done = true;
                KnowledgeBatch::empty_final()
            }
        }
    }

    /// The next batch, re-merged with everything already returned; None after the final batch.
    pub async fn more(&mut self) -> Option<KnowledgeBatch> {
        if self.done {
            return None;
        }
        match self.rx.recv().await {
            Some(batch) => {
                if batch.final_batch {
                    self.done = true;
                }
                Some(batch)
            }
            None => {
                self.done = true;
                None
            }
        }
    }
}

impl KnowledgeBatch {
    pub(crate) fn empty_final() -> KnowledgeBatch {
        KnowledgeBatch {
            hits: Vec::new(),
            answered: Vec::new(),
            unanswered: Vec::new(),
            relied_on: Vec::new(),
            final_batch: true,
            unchecked_sources: false,
            pending: Vec::new(),
        }
    }
}

/// Implemented by the Knowledge Query Server.
pub trait ChangeListener: Send + Sync {
    fn repository_moved(&self, proxy: &str, moved: RepositoryMoved);
    fn contents_refreshed(&self, proxy: &str, contents: KnowledgeContents);
    fn proxy_unreachable(&self, proxy: &str);
}

// ---- connections

/// The Service API view (2C wire).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeConnectionView {
    pub connection_id: KnowledgeConnectionId,
    pub name: String,
    pub url: String,
    pub added_by: SetBy,
    /// None when the agent keeps no record.
    pub sources_record: Option<SourcesRecordPolicy>,
    pub last_answered_at: Option<Timestamp>,
}

/// What the Memory Service reads (no network).
#[derive(Debug, Clone)]
pub struct KnowledgeConnection {
    pub id: KnowledgeConnectionId,
    pub name: String,
    pub url: String,
    pub added_by: SetBy,
    pub state: ConnectionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Ready,
    SignInNeeded,
    NotInAudience,
    Unreachable,
}

impl ConnectionState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ConnectionState::Ready => "ready",
            ConnectionState::SignInNeeded => "sign_in_needed",
            ConnectionState::NotInAudience => "not_in_audience",
            ConnectionState::Unreachable => "unreachable",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<ConnectionState> {
        match raw {
            "ready" => Some(ConnectionState::Ready),
            "sign_in_needed" => Some(ConnectionState::SignInNeeded),
            "not_in_audience" => Some(ConnectionState::NotInAudience),
            "unreachable" => Some(ConnectionState::Unreachable),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeConnectionDetails {
    pub connection: KnowledgeConnectionView,
    /// Read fresh by `details`; None when the read failed.
    pub about: Option<KnowledgeAbout>,
    pub state: ConnectionState,
    pub repositories: Vec<ContentsRepository>,
    pub as_of: Option<Timestamp>,
    /// One plain sentence.
    pub last_problem: Option<String>,
    /// ADR-115: one per `about.accounts` entry.
    pub accounts: Vec<ConnectionAccount>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ConnectionAccount {
    pub system: SourceSystem,
    pub state: AccountLinkState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountLinkState {
    Linked,
    NotSignedIn,
    Refused,
}

impl AccountLinkState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            AccountLinkState::Linked => "linked",
            AccountLinkState::NotSignedIn => "not_signed_in",
            AccountLinkState::Refused => "refused",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<AccountLinkState> {
        match raw {
            "linked" => Some(AccountLinkState::Linked),
            "not_signed_in" => Some(AccountLinkState::NotSignedIn),
            "refused" => Some(AccountLinkState::Refused),
            _ => None,
        }
    }
}
