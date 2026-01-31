//! Home — restated from `HomeAndSearch.md` (SA-RECON-02).
//!
//! The *Home*, *Needs you*, changed-mark and **Run again** commands of the Home and Search module, with
//! the module's own result types; `ChangedMark` and `InputUsed` are 2C types and are imported.

use ordin_types::prelude::*;
use ordin_types::wire::ContentRef;
use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

pub use ordin_types::common::InputUsed;
pub use ordin_types::domain::ChangedMark;

/// Where a Home row, a search result or a notification opens.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenTarget {
    Workspace {
        workspace_id: WorkspaceId,
    },
    Document {
        workspace_id: WorkspaceId,
        place_id: PlaceId,
        path: String,
    },
    Run {
        workspace_id: WorkspaceId,
        run_id: RunId,
    },
    CrewRun {
        workspace_id: WorkspaceId,
        crew_run_id: CrewRunId,
    },
    Approval {
        workspace_id: WorkspaceId,
        approval_id: ApprovalId,
    },
    /// A question, form, crew plan or offer block waiting for the person (reconciliation row 182).
    Block {
        workspace_id: WorkspaceId,
        block_id: BlockId,
    },
    /// A hosted agent's approver card (SA-CLOSE-05); approve and reject go through `hosted.decide_card`.
    HostedApproval {
        remote_agent_id: RemoteAgentId,
        approval_id: ApprovalId,
    },
    /// A handover handed or returned to the person; the desktop's notifier reads this variant.
    Handover {
        workspace_id: Option<WorkspaceId>,
        handover_id: HandoverId,
    },
    KnowledgePage {
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
    },
    /// A screen by name: "home", "settings", "add_documents" …
    Screen {
        name: String,
    },
    /// An item from a registered search source.
    // SPEC-GAP-4: HomeAndSearch.md names this field `kind`, which is also the enum's tag; serde
    // refuses the collision (as ServiceApi.md SA-CLOSE-02 found for `credential_kind`). The Rust
    // field keeps the owner's name and is written `item_kind` on the wire until the owner renames it.
    Item {
        #[serde(rename = "item_kind")]
        kind: String,
        id: String,
        workspace_id: Option<WorkspaceId>,
    },
}

/// One line of Home or *Needs you*.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HomeRow {
    pub workspace_id: Option<WorkspaceId>,
    pub workspace_name: Option<String>,
    pub line: String,
    pub at: Timestamp,
    pub is_new: bool,
    pub opens: OpenTarget,
    pub run_again: bool,
}

/// Home: what waits for the person, what runs, and what changed since Home was last opened.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HomeView {
    pub waiting: Vec<HomeRow>,
    pub running: Vec<HomeRow>,
    pub since_last_looked: Vec<HomeRow>,
    pub new_count: u32,
}

/// The run or crew run whose inputs **Run again** lists.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunAgainSubject {
    Run { run_id: RunId },
    CrewRun { crew_run_id: CrewRunId },
}

/// One input of an earlier run, with what is there now.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RunAgainInput {
    pub input: InputUsed,
    pub newer_version: Option<String>,
    pub check: VersionCheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VersionCheck {
    Same,
    Newer,
    Gone,
    CannotCheck,
}

// ---- inputs ----

/// `home.view`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HomeViewInput {
    pub workspace_id: Option<WorkspaceId>,
}

/// `home.opened`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HomeOpenedInput {}

/// `home.workspace_opened`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HomeWorkspaceOpenedInput {
    pub workspace_id: WorkspaceId,
}

/// `home.changed_marks`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HomeChangedMarksInput {
    pub workspace_id: WorkspaceId,
}

/// `home.document_opened`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HomeDocumentOpenedInput {
    pub workspace_id: WorkspaceId,
    pub item: ContentRef,
}

/// `home.run_again_inputs`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HomeRunAgainInputsInput {
    pub subject: RunAgainSubject,
}

/// `home.needs_you`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HomeNeedsYouInput {
    pub workspace_id: Option<WorkspaceId>,
}

/// Inputs whose fields carry no rule beyond their types.
macro_rules! no_rules {
    ($($t:ty),* $(,)?) => {$(
        impl CommandInput for $t {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

no_rules!(
    HomeViewInput,
    HomeOpenedInput,
    HomeWorkspaceOpenedInput,
    HomeChangedMarksInput,
    HomeRunAgainInputsInput,
    HomeNeedsYouInput,
);

impl CommandInput for HomeDocumentOpenedInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        super::check_content_ref(&self.item, "/item")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_opened_checks_the_reference() {
        let mut ok = HomeDocumentOpenedInput {
            workspace_id: WorkspaceId::new(),
            item: ContentRef::Artefact {
                artefact_id: ArtefactId::new(),
            },
        };
        assert!(ok.check().is_ok());
        let mut bad = HomeDocumentOpenedInput {
            workspace_id: WorkspaceId::new(),
            item: ContentRef::PlaceFile {
                place_id: PlaceId::new(),
                relative_path: "../x".into(),
            },
        };
        let err = bad.check().unwrap_err();
        assert_eq!(err.code, "SVC_INPUT_INVALID");
        assert_eq!(err.at, "/input/item/relative_path");
    }

    #[test]
    fn inputs_without_rules_pass() {
        assert!(HomeViewInput { workspace_id: None }.check().is_ok());
        assert!(HomeOpenedInput {}.check().is_ok());
        assert!(HomeWorkspaceOpenedInput {
            workspace_id: WorkspaceId::new()
        }
        .check()
        .is_ok());
        assert!(HomeChangedMarksInput {
            workspace_id: WorkspaceId::new()
        }
        .check()
        .is_ok());
        assert!(HomeNeedsYouInput { workspace_id: None }.check().is_ok());
        let mut again = HomeRunAgainInputsInput {
            subject: RunAgainSubject::CrewRun {
                crew_run_id: CrewRunId::new(),
            },
        };
        assert!(again.check().is_ok());
    }

    #[test]
    fn subject_and_targets_have_the_owner_shape() {
        let subject: RunAgainSubject =
            serde_json::from_value(serde_json::json!({"kind": "run", "run_id": RunId::new()}))
                .unwrap();
        assert!(matches!(subject, RunAgainSubject::Run { .. }));
        let target = OpenTarget::Screen {
            name: "home".into(),
        };
        assert_eq!(
            serde_json::to_value(&target).unwrap(),
            serde_json::json!({"kind": "screen", "name": "home"})
        );
        let refused = serde_json::from_value::<HomeOpenedInput>(serde_json::json!({"x": 1}));
        assert!(refused.is_err());
    }
}
