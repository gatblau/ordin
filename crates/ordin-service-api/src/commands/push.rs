//! Sending changes back — restated from `PushService.md` (SA-RECON-02).
//!
//! Pending pushes the person settles, and published copies. `PendingPush`, `PublishedCopy` and the copy
//! types are 2C types and are imported; the Push Service's own inputs carry `deny_unknown_fields`, as the
//! owner states.

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{
    CopyDestination, CopyFormat, CopyOriginal, PendingPush, PublishedCopy,
};
pub use ordin_types::{ApprovalId, PublishedCopyId};

/// A path relative to its place: at most 4,096 bytes (2C `FileVersion.path`).
const PLACE_PATH_MAX_BYTES: usize = 4096;

/// What the person keeps when a pushed document changed at its origin meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeldChoice {
    KeepMine,
    KeepOrdins,
    ApplyOrdinsToMine,
}

/// `push.choose_version`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChooseVersionInput {
    pub pending_push_id: PendingPushId,
    pub working_copy_item_id: WorkingCopyItemId,
    pub choice: HeldChoice,
}

/// `push.try_again`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TryAgainInput {
    pub pending_push_id: PendingPushId,
    pub working_copy_item_id: WorkingCopyItemId,
}

/// `push.publish_copy` (**Publish a copy…**).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PublishCopyRequest {
    pub workspace_id: WorkspaceId,
    pub original: CopyOriginal,
    /// The commit for an original in a repository place; otherwise the File Service version number as
    /// text ("4").
    pub original_version: String,
    pub original_name: String,
    /// `CopyOriginal::KnowledgePage` only: the page's file (place and path), supplied by the Knowledge
    /// Service from its page row (SA-CLOSE-01); `None` for `PlaceFile`.
    pub page_file: Option<(PlaceId, String)>,
    pub destination: CopyDestination,
    pub format: CopyFormat,
    pub followed_branch: Option<String>,
    pub keep_up_to_date: bool,
    /// "Use another name" sets it.
    pub file_name: String,
}

/// `push.copy_action`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CopyActionInput {
    pub published_copy_id: PublishedCopyId,
    pub action: CopyAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CopyAction {
    TryAgain,
    BringItIn,
    LeaveIt,
    ReplaceAtNextUpdate,
    StopKeepingUpToDate,
    KeepUpToDate,
    Remove,
}

/// `push.list_pending`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PushListPendingInput {
    pub workspace_id: WorkspaceId,
}

/// `push.update_copies`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PushUpdateCopiesInput {
    pub original: CopyOriginal,
    pub workspace_id: WorkspaceId,
}

/// A path relative to its place: 1–4,096 bytes, no leading `/`, no `..` segment.
fn place_path(value: &str, at: &str) -> Result<(), InputRejection> {
    rules::bytes(value, 1, PLACE_PATH_MAX_BYTES, at)?;
    if value.starts_with('/') || value.split(['/', '\\']).any(|segment| segment == "..") {
        return Err(rules::reject(at));
    }
    Ok(())
}

fn check_original(original: &CopyOriginal) -> Result<(), InputRejection> {
    match original {
        CopyOriginal::PlaceFile { path, .. } => place_path(path, "/original/path"),
        CopyOriginal::KnowledgePage { .. } => Ok(()),
    }
}

impl CommandInput for ChooseVersionInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for TryAgainInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for CopyActionInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for PushListPendingInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for PushUpdateCopiesInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_original(&self.original)
    }
}

impl CommandInput for PublishCopyRequest {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_original(&self.original)?;
        // Internal Logic F1: a knowledge page needs its file (`PUSH_REQUEST_INVALID` at the owner).
        if matches!(self.original, CopyOriginal::KnowledgePage { .. }) && self.page_file.is_none() {
            return Err(rules::reject("/page_file"));
        }
        if let Some((_, path)) = &self.page_file {
            place_path(path, "/page_file/1")?;
        }
        // 2C `PublishedCopy`: `exact_file` only with `confluence_attachment`
        // (`PUB_FORMAT_MAKER_MISMATCH`); `page` only with `confluence_page`
        // (`PUB_FORMAT_DESTINATION_MISMATCH`).
        let format_ok = match self.format {
            CopyFormat::ExactFile => {
                matches!(
                    self.destination,
                    CopyDestination::ConfluenceAttachment { .. }
                )
            }
            CopyFormat::Page => matches!(self.destination, CopyDestination::ConfluencePage { .. }),
            CopyFormat::Word | CopyFormat::Pdf | CopyFormat::Slides => true,
        };
        if !format_ok {
            return Err(rules::reject("/format"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        original: CopyOriginal,
        format: CopyFormat,
        destination: CopyDestination,
    ) -> PublishCopyRequest {
        PublishCopyRequest {
            workspace_id: WorkspaceId::new(),
            original,
            original_version: "4".into(),
            original_name: "rates.md".into(),
            page_file: None,
            destination,
            format,
            followed_branch: None,
            keep_up_to_date: true,
            file_name: "rates.docx".into(),
        }
    }

    fn place_file(path: &str) -> CopyOriginal {
        CopyOriginal::PlaceFile {
            place_id: PlaceId::new(),
            path: path.into(),
        }
    }

    fn page() -> CopyDestination {
        CopyDestination::ConfluencePage {
            space: "SUP".into(),
            parent_page_id: "123".into(),
        }
    }

    fn sharepoint() -> CopyDestination {
        CopyDestination::SharepointFile {
            site: "s".into(),
            library: "l".into(),
            folder: "f".into(),
        }
    }

    #[test]
    fn publish_copy_rules() {
        assert!(
            request(place_file("rates.md"), CopyFormat::Word, sharepoint())
                .check()
                .is_ok()
        );
        assert!(request(place_file("rates.md"), CopyFormat::Page, page())
            .check()
            .is_ok());
        let attachment = CopyDestination::ConfluenceAttachment {
            space: "SUP".into(),
            parent_page_id: "123".into(),
        };
        assert!(
            request(place_file("a.pdf"), CopyFormat::ExactFile, attachment)
                .check()
                .is_ok()
        );

        let knowledge = CopyOriginal::KnowledgePage {
            knowledge_page_id: KnowledgePageId::new(),
        };
        let mut with_file = request(knowledge.clone(), CopyFormat::Page, page());
        with_file.page_file = Some((PlaceId::new(), "knowledge/rates.md".into()));
        assert!(with_file.check().is_ok());

        let mut bad_file = with_file.clone();
        bad_file.page_file = Some((PlaceId::new(), "/abs".into()));
        let cases = [
            (
                request(knowledge, CopyFormat::Page, page()),
                "/input/page_file",
            ),
            (bad_file, "/input/page_file/1"),
            (
                request(place_file("a"), CopyFormat::Page, sharepoint()),
                "/input/format",
            ),
            (
                request(place_file("a"), CopyFormat::ExactFile, page()),
                "/input/format",
            ),
            (
                request(place_file("../a"), CopyFormat::Word, sharepoint()),
                "/input/original/path",
            ),
        ];
        for (mut input, at) in cases {
            let err = input.check().unwrap_err();
            assert_eq!((err.code, err.at.as_str()), ("SVC_INPUT_INVALID", at));
        }
    }

    #[test]
    fn other_inputs() {
        let ws = WorkspaceId::new();
        let mut update = PushUpdateCopiesInput {
            original: place_file(""),
            workspace_id: ws,
        };
        assert_eq!(update.check().unwrap_err().at, "/input/original/path");
        update.original = place_file("a.md");
        assert!(update.check().is_ok());
        assert!(PushListPendingInput { workspace_id: ws }.check().is_ok());
        let push = PendingPushId::new();
        let item = WorkingCopyItemId::new();
        assert!(ChooseVersionInput {
            pending_push_id: push,
            working_copy_item_id: item,
            choice: HeldChoice::KeepMine
        }
        .check()
        .is_ok());
        assert!(TryAgainInput {
            pending_push_id: push,
            working_copy_item_id: item
        }
        .check()
        .is_ok());
        let mut action: CopyActionInput = serde_json::from_value(serde_json::json!({
            "published_copy_id": PublishedCopyId::new(), "action": "replace_at_next_update"
        }))
        .unwrap();
        assert_eq!(action.action, CopyAction::ReplaceAtNextUpdate);
        assert!(action.check().is_ok());
    }
}
