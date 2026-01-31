//! Inputs, results and notices of the Package Catalogue (PackageCatalogue.md *Public Interface*,
//! *Inputs and results*). Inputs that arrive from the Service API carry `deny_unknown_fields`.

use std::path::PathBuf;

use ordin_package_inspector::MayDoChange;
use ordin_resolver::model::{ChosenBy, PartKind, PendingPackChoice};
use ordin_types::domain::agents::AgentInstance;
use ordin_types::{AgentDraftId, AgentInstanceId, AgentRepositoryId, PartId, WorkspaceId};
use serde::{Deserialize, Serialize};

pub use ordin_catalogue::AgentRepositoryRef;
pub use ordin_types::domain::HarnessKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AddAgentInput {
    pub workspace_id: WorkspaceId,
    /// A WorkspaceProfile part offered by the Catalogue Service.
    pub agent: PartId,
    /// Filled by the program from the Project Service before the call (languages of the workspace's
    /// linked code).
    pub workspace_languages: Vec<String>,
    /// BCP 47, for example `en-GB`.
    pub interface_locale: String,
    /// From **Change**; empty on a first add.
    pub choices: Vec<PackChoice>,
}

/// A pack choice for one required pack rule of the agent (ADR-089 *Change*, decision 64).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PackChoice {
    /// 0-based index in the agent's `requiredPracticePacks` (the Resolver's `PackAnswer`).
    pub rule_index: u32,
    /// Pack part identifiers.
    pub packs: Vec<String>,
}

/// One pack or overlay a plan picked, for "Uses: ..." (derived from `ResolutionPlan.selections`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct UsesItem {
    pub kind: PartKind,
    pub identifier: String,
    /// The part's name from the Catalogue Service.
    pub label: String,
    pub chosen_by: ChosenBy,
    /// False for `MakerFixed`; true otherwise.
    pub changeable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AddPreview {
    pub agent_name: String,
    pub from: String,
    pub uses: Vec<UsesItem>,
    pub asks: Vec<PendingPackChoice>,
    pub available: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Availability {
    Available,
    PackMissing { pack_class_label: String },
    NeedsItApproval { parts: Vec<String> },
    Unsafe,
}

// The variants are the spec's; boxing `agent` would change the public interface callers match on.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AddOutcome {
    Ready {
        agent: InstalledAgent,
    },
    /// Message `agent-add-unsafe`, no action.
    Unsafe,
    /// Message `agent-add-needs-it`, no action.
    NeedsItApproval {
        parts: Vec<String>,
    },
    PackMissing {
        pack_class_label: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InstalledAgent {
    pub instance: AgentInstance,
    pub workspaces: Vec<WorkspaceId>,
    pub tasks: Vec<String>,
    pub uses: Vec<UsesItem>,
    /// The stopped reason is `instance.stopped_reason`.
    pub update: Option<PendingUpdate>,
}

/// The Package Inspector's plain-words differences between the installed and the candidate agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingUpdate {
    pub to_commit: String,
    pub may_do_changes: Vec<MayDoChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChangeChoiceInput {
    pub agent_instance_id: AgentInstanceId,
    pub choice: PackChoice,
}

/// One open pack question of an installed instance (decision 64; SA-CLOSE-04). Answered with
/// `record_answer` as `ChangeChoiceInput { agent_instance_id, choice: PackChoice { rule_index, packs } }`,
/// `packs` taken from `choices`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingChoice {
    pub agent_instance_id: AgentInstanceId,
    pub rule_index: u32,
    /// How many packs `PackChoice.packs` must hold.
    pub at_least: u32,
    /// The rule's class; "pack" when the rule names none.
    pub class_label: String,
    /// The rule's candidates, sorted by identifier.
    pub choices: Vec<PendingChoiceOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingChoiceOption {
    /// Pack part identifier; an element of `PackChoice.packs`.
    pub pack: String,
    /// The pack's name from the Catalogue Service; the identifier when the lookup fails.
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoveAgentInput {
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: Option<WorkspaceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMatch {
    Present {
        agent_instance_id: AgentInstanceId,
        display_name: String,
    },
    OtherVersion {
        agent_instance_id: AgentInstanceId,
        display_name: String,
        installed_version: String,
    },
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentFound {
    pub agent: PartId,
    pub agent_repository_id: AgentRepositoryId,
    pub identifier: String,
    pub name: String,
    /// One sentence, from the Catalogue Service's `PartView`.
    pub summary: String,
    /// Repository display name or "ordin".
    pub from: String,
    pub in_workspace: Option<AgentInstanceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentFolderReport {
    pub checked: u32,
    pub repaired: u32,
    pub failed: Vec<AgentFolderFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentFolderFailure {
    pub agent_name: String,
    /// `PKG_FOLDER_UNUSABLE` or `PKG_FOLDER_MISMATCH`.
    pub code: String,
}

/// The Knowledge Service's view of one agent of a workspace (KnowledgeService.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceAgent {
    pub agent_instance_id: AgentInstanceId,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct TrialInput {
    pub agent_draft_id: AgentDraftId,
    pub agent_repository_id: AgentRepositoryId,
    pub worktree: PathBuf,
    /// The agent's identifier.
    pub agent: String,
    pub workspace_id: WorkspaceId,
    pub workspace_languages: Vec<String>,
    pub interface_locale: String,
}

#[derive(Debug, Clone)]
pub enum AgentNotice {
    Added {
        agent_instance_id: AgentInstanceId,
        workspace_id: WorkspaceId,
    },
    UpdatedBetweenRuns {
        agent_instance_id: AgentInstanceId,
    },
    /// An entry in Needs you.
    UpdateNeedsChoice {
        agent_instance_id: AgentInstanceId,
    },
    /// Withdrawn or trust lost.
    Stopped {
        agent_instance_id: AgentInstanceId,
        reason: String,
    },
    /// A refused instance is ready again (trust regained, or a fixed version accepted; SA-CLOSE-02).
    Resumed {
        agent_instance_id: AgentInstanceId,
    },
    Removed {
        agent_instance_id: AgentInstanceId,
    },
}

/// One of the agents bundled with the application (ADR-027, ADR-083).
// spec: ordin-desktop/components/PackageCatalogue#PKG-REC-1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BundledAgent {
    Ordin,
    AgentMaker,
}

impl BundledAgent {
    /// The instance id, fixed for every installation, in the shape of `BUNDLED_REPOSITORY_ID`.
    pub const fn instance_id(self) -> ordin_types::AgentInstanceId {
        match self {
            BundledAgent::Ordin => {
                ordin_types::AgentInstanceId(uuid::uuid!("01900000-0000-7000-8000-000000000101"))
            }
            BundledAgent::AgentMaker => {
                ordin_types::AgentInstanceId(uuid::uuid!("01900000-0000-7000-8000-000000000102"))
            }
        }
    }

    /// The WorkspaceProfile identifier in the bundled catalogue (`BUNDLED_REPOSITORY_ID`).
    pub const fn identifier(self) -> Option<&'static str> {
        match self {
            // Ordin's own WorkspaceProfile identifier, supplied by the user (OrdinDesktop.md OQ-REC-6).
            BundledAgent::Ordin => Some("urn:ordin:default:workspaces:coordinator"),
            BundledAgent::AgentMaker => Some("urn:ordin:default:workspaces:advisor"),
        }
    }

    /// `ordin` or `agent_maker`, for logs.
    pub const fn log_name(self) -> &'static str {
        match self {
            BundledAgent::Ordin => "ordin",
            BundledAgent::AgentMaker => "agent_maker",
        }
    }
}

/// Whether a catalogue part is one of ordin's own two agents (Ordin, the agent maker): the bundled
/// catalogue's part with a `BundledAgent` identifier. They are never added to a workspace and never
/// offered as agents to add (PKG-OWN-1).
// spec: ordin-desktop/components/PackageCatalogue#PKG-OWN-1
pub fn is_own_agent(repository: ordin_types::AgentRepositoryId, identifier: &str) -> bool {
    repository == ordin_resolver::model::BUNDLED_REPOSITORY_ID
        && [BundledAgent::Ordin, BundledAgent::AgentMaker]
            .iter()
            .any(|a| a.identifier() == Some(identifier))
}

/// Whether the instance is one of the two bundled instances (PKG-REC-1: no column marks it).
pub(crate) fn is_bundled(id: ordin_types::AgentInstanceId) -> bool {
    id == BundledAgent::Ordin.instance_id() || id == BundledAgent::AgentMaker.instance_id()
}
