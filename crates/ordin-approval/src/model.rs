//! The Approval Service's own public types (ApprovalService.md *Public Interface*). The 2C types
//! it carries are used from `ordin-types`, never redefined.

use ordin_types::domain::runs::{Approval, ApprovalState, ApprovalSubject};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{
    AffectedItem, ApprovalAsker, ApprovalCard, ApprovalContent, RejectStyle,
};
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{ApprovalId, BackendId, WorkspaceId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct ApprovalConfig {
    /// This installation's backend identifier (2C BackendId), copied into every card.
    pub backend: BackendId,
    /// Shown as `decided_on` for a press made on this computer's own screens ("Asha's laptop").
    pub local_computer_name: String,
    /// Agent profile only; `None` on the desktop and the personal server.
    pub approvers: Option<ApproverPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApproverPolicy {
    /// Identity-provider subject identifiers of individual approvers.
    pub users: Vec<String>,
    /// Identity-provider group names; a member of any listed group is an approver.
    pub groups: Vec<String>,
    /// ADR-097 item 4: whether the asker may approve their own run's actions that stay inside the company.
    pub asker_may_approve_internal: bool,
}

/// What an asking component hands over. Every caller builds this struct as defined here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ApprovalRequest {
    pub workspace_id: WorkspaceId,
    pub asked_by: ApprovalAsker,
    pub question: String,
    pub verb: String,
    pub where_line: String,
    pub affects: Vec<AffectedItem>,
    pub changes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ApprovalContent>,
    pub reject_style: RejectStyle,
    /// Whether a **See …** button is offered; true only when `content` is present.
    pub show_details: bool,
    /// The proposal as the Rules Engine stated it; stored as the `Approval` entity's fields.
    pub proposal: ActionProposal,
    /// Agent profile: who asked (identity-provider subject, or `<connection>/<user>` for chat).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asker: Option<String>,
    /// Agent profile: the action leaves the company. Ignored outside the agent profile.
    pub leaves_company: bool,
}

/// Where a press came from. The program, the Remote API, the Channel Gateway and the Caller API fill it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PressOrigin {
    /// The screens of the computer this core runs on.
    LocalScreen,
    /// A paired desktop through the Remote API (personal server).
    PairedDesktop { computer_name: String },
    /// A chat button (Telegram or Slack) from the paired private chat.
    PairedChat { channel: ChatChannel },
    /// Agent profile: an approver identified by the identity provider or the chat link.
    Approver {
        subject: String,
        groups: Vec<String>,
        via: ApproverVia,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannel {
    Telegram,
    Slack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApproverVia {
    Chat,
    Desktop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalOutcome {
    pub approval_id: ApprovalId,
    pub state: ApprovalState,
    pub decided_on: String,
}

/// The stored record with its card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalRecord {
    pub card: ApprovalCard,
    pub proposal: ActionProposal,
    pub state: ApprovalState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<Timestamp>,
}

impl ApprovalRecord {
    /// The 2C `Approval` entity where the asker is a run or a crew round; `None` for an
    /// `ApprovalAsker::Person` card, which has no `ApprovalSubject`.
    ///
    /// 2C's `Approval` holds `asked_by` and an optional `subject`, so a person's card could also be
    /// rendered; this method gives `None` for one.
    pub fn to_domain(&self) -> Option<Approval> {
        let subject = match &self.card.asked_by {
            ApprovalAsker::Run { run_id } => ApprovalSubject::Run { run_id: *run_id },
            ApprovalAsker::CrewRound { crew_run_id, round } => ApprovalSubject::CrewRound {
                crew_run_id: *crew_run_id,
                round_number: *round,
            },
            ApprovalAsker::Person => return None,
        };
        Some(Approval {
            id: self.card.approval_id,
            asked_by: self.card.asked_by.clone(),
            subject: Some(subject),
            action: self.proposal.action.clone(),
            destination: self.proposal.destination.clone(),
            affected_records: self
                .proposal
                .affects
                .iter()
                .map(crate::store::affected_label)
                .collect(),
            consequence: self.proposal.consequence.clone(),
            state: self.state,
            decided_by: self.decided_by.clone(),
            decided_at: self.decided_at,
            created_at: self.card.created_at,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    Decided(ApprovalState),
    DeadlinePassed,
}

/// The identifiers a record needs again after a decision (kept private to the crate).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ids {
    pub approval: ApprovalId,
    pub workspace: WorkspaceId,
}
