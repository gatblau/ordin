//! Knowledge pages — restated from `KnowledgeService.md` (SA-RECON-02), with its amendments of
//! January 2026 (ADR-103 sharing fields, ADR-104 page links and suggestions) and January 2026 (ADR-110 read
//! pages and `knowledge.list_read_pages`, ADR-112 the page type, description, trust and status).
//!
//! `KnowledgePage`, `KnowledgeAuthor`, `KnowledgeSource`, `KnowledgePageState`, `CopyDestination`,
//! `PageTier` and `PageStatus` are 2C types from `ordin-types`; `AdoptionMark` is the Memory Service's.

use ordin_types::time::Timestamp;
use ordin_types::{
    AgentInstanceId, KnowledgePageId, KnowledgeSuggestionId, PublishedCopyId, TeamFolderId,
    WorkspaceId,
};
use serde::{Deserialize, Serialize};

use super::memory::AdoptionMark;
use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{
    CopyDestination, KnowledgeAuthor, KnowledgePage, KnowledgePageState, KnowledgeSource,
};
pub use ordin_types::wire::knowledge_query::{PageStatus, PageTier};

/// The page types the Service API accepts for a new page (ADR-112, `okf::PAGE_TYPES`).
pub const PAGE_TYPES: [&str; 5] = ["Explanation", "How-to", "Decision", "Reference", "Glossary"];

/// The owner's validation rows (2C `KnowledgePage`; ADR-104; ADR-112).
const TITLE_MAX_CHARS: usize = 200;
const BODY_MAX_BYTES: usize = 1024 * 1024;
const TAG_MAX_CHARS: usize = 64;
const TAGS_MAX: usize = 32;
const ALIAS_MAX_CHARS: usize = 200;
const ALIASES_MAX: usize = 16;
const TYPE_MAX_CHARS: usize = 64;
const DESCRIPTION_MAX_CHARS: usize = 200;

// ---- views ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PageView {
    pub page: KnowledgePage,
    /// One entry per source with a newer version not yet left (empty when current).
    pub changes: Vec<SourceChange>,
    pub pending_update: bool,
    pub relative_path: String,
    // Amended January 2026 (ADR-103).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_at: Option<Timestamp>,
    /// A `place` source names a place that is not on this computer.
    pub cites_other_computer: bool,
    // Amended January 2026 (ADR-104).
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    /// Aliases kept in the file but not usable as names: another page holds that name.
    pub unregistered_aliases: Vec<String>,
    /// The title in the file when another page holds it; the page keeps its previous title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_refused: Option<String>,
    pub links_out: Vec<PageLinkView>,
    pub links_in: Vec<PageRef>,
    pub links_truncated: u32,
    pub suggestions: Vec<SuggestionView>,
    // Amended January 2026 (ADR-110).
    /// Set for a read page: the folder's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_description: Option<String>,
    /// Serialised whenever `folder` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_by_company: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<TeamFolderId>,
    /// True for a read page.
    pub read_only: bool,
    /// Set for an own page adopted from a read folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adopted_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adoption_mark: Option<AdoptionMark>,
    // Amended January 2026 (ADR-112).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub trust: PageTier,
    /// Some only for draft and deprecated (stable is not shown).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PageStatus>,
    pub past_stale_after: bool,
    /// Verifiers as written, newest first, at most 20 ("Checked by …").
    pub verified: Vec<VerifiedView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SourceChange {
    pub origin: String,
    pub location: String,
    pub version_read: String,
    pub newer_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PageListing {
    pub this_workspace: Vec<PageSummary>,
    pub from_agents: Vec<AgentReferencePage>,
    /// Amended January 2026 (ADR-104).
    pub not_adopted: Vec<NotAdoptedFile>,
    /// Read pages, ordered by folder name then title (amended January 2026, ADR-110).
    pub from_other_teams: Vec<PageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PageSummary {
    pub workspace_id: WorkspaceId,
    pub id: KnowledgePageId,
    pub title: String,
    pub state: KnowledgePageState,
    pub written_by: KnowledgeAuthor,
    pub last_changed_at: Timestamp,
    // Amended January 2026 (ADR-103).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_at: Option<Timestamp>,
    pub cites_other_computer: bool,
    // Amended January 2026 (ADR-104).
    pub tags: Vec<String>,
    // Amended January 2026 (ADR-110).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set_by_company: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<TeamFolderId>,
    pub read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adopted_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adoption_mark: Option<AdoptionMark>,
    // Amended January 2026 (ADR-112).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub trust: PageTier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PageStatus>,
    pub past_stale_after: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AgentReferencePage {
    pub agent_instance_id: AgentInstanceId,
    /// The agent's display name only, never its version (ADR-057, decision 56).
    pub agent_name: String,
    pub relative_path: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PageRef {
    pub page_id: KnowledgePageId,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PageLinkView {
    /// None = no page of the workspace has that name ("No page called …").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<PageRef>,
    /// The target as written in the page, without `#section` and `|shown`.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    Link,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateReason {
    Names,
    Text,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SuggestionView {
    pub id: KnowledgeSuggestionId,
    pub kind: SuggestionKind,
    pub other: PageRef,
    /// Link: the words that would become the link, as they appear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_text: Option<String>,
    /// Link: up to 60 characters either side of the words, plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<DuplicateReason>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct NotAdoptedFile {
    pub relative_path: String,
    pub reason: NotAdoptedReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotAdoptedReason {
    PageLimit,
    TooLarge,
    HeaderInvalid,
    IdInOtherWorkspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct VerifiedView {
    pub by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

// ---- inputs and results ----

/// `knowledge.list`: `filter` is read as words (owner §S).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeListInput {
    pub workspace_id: WorkspaceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// `knowledge.get`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeGetInput {
    pub page_id: KnowledgePageId,
}

/// `knowledge.read_agent_page`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeReadAgentPageInput {
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeReadAgentPageResult {
    pub markdown: String,
}

/// `knowledge.save_edit`; absent `tags`, `aliases`, `type` and `description` leave the page's own
/// (ADR-104, ADR-112), so an input written before those amendments stays valid.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeSaveEditInput {
    pub page_id: KnowledgePageId,
    pub title: String,
    pub body_markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    /// One of the page types, or the value already in the file.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `knowledge.write`: the person's new page, no sources; `type` defaults to `Explanation`, an absent
/// `description` is the body's first sentence (ADR-112).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeWriteInput {
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub body_markdown: String,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub page_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeWriteResult {
    pub page_id: KnowledgePageId,
}

/// `knowledge.use_update` ("Use the update").
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeUseUpdateInput {
    pub page_id: KnowledgePageId,
}

/// `knowledge.keep_page` ("Keep the page as it is").
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeKeepPageInput {
    pub page_id: KnowledgePageId,
}

/// `knowledge.leave_it` ("Leave it").
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeLeaveItInput {
    pub page_id: KnowledgePageId,
}

/// `knowledge.remove` (to the system's bin).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeRemoveInput {
    pub page_id: KnowledgePageId,
}

/// `knowledge.undo_remove`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeUndoRemoveInput {
    pub page_id: KnowledgePageId,
}

/// `knowledge.copy_to_workspace`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeCopyToWorkspaceInput {
    pub page_id: KnowledgePageId,
    pub to_workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeCopyToWorkspaceResult {
    pub page_id: KnowledgePageId,
}

/// `knowledge.publish_to_confluence`.
// The owner's shape (KnowledgeService.md *Service API commands served*), which the catalogue row
// restates (ServiceApi.md, amended January 2026).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgePublishToConfluenceInput {
    pub page_id: KnowledgePageId,
    pub destination: CopyDestination,
    pub keep_up_to_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgePublishToConfluenceResult {
    pub published_copy_id: PublishedCopyId,
}

/// `knowledge.accept_suggestion` (ADR-104).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeAcceptSuggestionInput {
    pub page_id: KnowledgePageId,
    pub suggestion_id: KnowledgeSuggestionId,
}

/// `knowledge.dismiss_suggestion` (ADR-104).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeDismissSuggestionInput {
    pub page_id: KnowledgePageId,
    pub suggestion_id: KnowledgeSuggestionId,
}

/// `knowledge.list_read_pages` (ADR-110).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeListReadPagesInput {
    pub workspace_id: WorkspaceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<TeamFolderId>,
}

/// At most 2,000 per folder, ordered by folder name, then title.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KnowledgeListReadPagesResult {
    pub items: Vec<PageSummary>,
}

// ---- checks ----

/// Title 1–200 characters and body at most 1 MiB of UTF-8 (2C `KnowledgePage`).
fn check_title_and_body(title: &str, body: &str) -> Result<(), InputRejection> {
    rules::text(title, 1, TITLE_MAX_CHARS, "/title")?;
    rules::bytes(body, 0, BODY_MAX_BYTES, "/body_markdown")
}

/// A description: 1–200 characters on one line (ADR-112).
fn check_description(description: &Option<String>) -> Result<(), InputRejection> {
    if let Some(d) = description {
        rules::text(d, 1, DESCRIPTION_MAX_CHARS, "/description")?;
        if d.contains('\n') {
            return Err(rules::reject("/description"));
        }
    }
    Ok(())
}

impl CommandInput for KnowledgeListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeGetInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeReadAgentPageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeSaveEditInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_title_and_body(&self.title, &self.body_markdown)?;
        if let Some(tags) = &self.tags {
            // ADR-104: 1–64 characters after a leading `#` is dropped, no white space; ≤ 32 a page.
            rules::count(tags, 0, TAGS_MAX, "/tags")?;
            for (i, tag) in tags.iter().enumerate() {
                let bare = tag.strip_prefix('#').unwrap_or(tag);
                let at = format!("/tags/{i}");
                rules::text(bare, 1, TAG_MAX_CHARS, &at)?;
                if bare.contains(char::is_whitespace) {
                    return Err(rules::reject(&at));
                }
            }
        }
        if let Some(aliases) = &self.aliases {
            // ADR-104: 1–200 characters; ≤ 16 a page.
            rules::count(aliases, 0, ALIASES_MAX, "/aliases")?;
            for (i, alias) in aliases.iter().enumerate() {
                rules::text(alias, 1, ALIAS_MAX_CHARS, &format!("/aliases/{i}"))?;
            }
        }
        // The value already in the file is accepted, so only the length is checked here (ADR-112).
        rules::opt_text(&self.page_type, 1, TYPE_MAX_CHARS, "/type")?;
        check_description(&self.description)
    }
}

impl CommandInput for KnowledgeWriteInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_title_and_body(&self.title, &self.body_markdown)?;
        if let Some(t) = &self.page_type {
            if !PAGE_TYPES.contains(&t.as_str()) {
                return Err(rules::reject("/type"));
            }
        }
        check_description(&self.description)
    }
}

impl CommandInput for KnowledgeUseUpdateInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeKeepPageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeLeaveItInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeRemoveInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeUndoRemoveInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeCopyToWorkspaceInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgePublishToConfluenceInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeAcceptSuggestionInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeDismissSuggestionInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for KnowledgeListReadPagesInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn save_edit(extra: serde_json::Value) -> KnowledgeSaveEditInput {
        let mut v = json!({
            "page_id": KnowledgePageId::new().to_string(),
            "title": "Supplier rates",
            "body_markdown": "Rates are reviewed each quarter."
        });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        serde_json::from_value(v).expect("valid save_edit")
    }

    #[test]
    fn save_edit_without_the_new_fields_is_valid() {
        let mut input = save_edit(json!({}));
        assert!(input.check().is_ok());
        assert!(input.tags.is_none() && input.page_type.is_none());
    }

    #[test]
    fn save_edit_rules() {
        let cases = [
            (json!({"title": ""}), "/input/title"),
            (json!({"title": "t".repeat(201)}), "/input/title"),
            (
                json!({"body_markdown": "b".repeat(BODY_MAX_BYTES + 1)}),
                "/input/body_markdown",
            ),
            (json!({"tags": vec!["a"; 33]}), "/input/tags"),
            (json!({"tags": ["#"]}), "/input/tags/0"),
            (json!({"tags": ["ok", "two words"]}), "/input/tags/1"),
            (json!({"tags": ["x".repeat(65)]}), "/input/tags/0"),
            (json!({"aliases": vec!["a"; 17]}), "/input/aliases"),
            (json!({"aliases": [""]}), "/input/aliases/0"),
            (json!({"type": ""}), "/input/type"),
            (json!({"description": ""}), "/input/description"),
            (json!({"description": "two\nlines"}), "/input/description"),
        ];
        for (extra, at) in cases {
            let err = save_edit(extra.clone()).check().unwrap_err();
            assert_eq!(
                (err.code, err.at.as_str()),
                ("SVC_INPUT_INVALID", at),
                "{extra}"
            );
        }
        let mut ok = save_edit(json!({"tags": ["#suppliers"], "aliases": ["Rate rules"],
            "type": "Runbook", "description": "How rates are set."}));
        assert!(ok.check().is_ok());
    }

    #[test]
    fn write_rules() {
        let mut input = KnowledgeWriteInput {
            workspace_id: WorkspaceId::new(),
            title: "Rates".into(),
            body_markdown: String::new(),
            page_type: Some("How-to".into()),
            description: None,
        };
        assert!(input.check().is_ok());
        input.page_type = Some("Runbook".into());
        assert_eq!(input.check().unwrap_err().at, "/input/type");
        input.page_type = None;
        input.title = String::new();
        assert_eq!(input.check().unwrap_err().at, "/input/title");
    }

    #[test]
    fn suggestion_id_must_be_a_uuid() {
        let raw = json!({"page_id": KnowledgePageId::new().to_string(), "suggestion_id": "abc"});
        assert!(serde_json::from_value::<KnowledgeAcceptSuggestionInput>(raw).is_err());
    }

    #[test]
    fn id_only_inputs_pass() {
        let page_id = KnowledgePageId::new();
        let ws = WorkspaceId::new();
        let suggestion_id = KnowledgeSuggestionId::new();
        assert!(KnowledgeListInput {
            workspace_id: ws,
            filter: None
        }
        .check()
        .is_ok());
        assert!(KnowledgeGetInput { page_id }.check().is_ok());
        assert!(KnowledgeReadAgentPageInput {
            workspace_id: ws,
            agent_instance_id: AgentInstanceId::new(),
            relative_path: "knowledge/a.md".into()
        }
        .check()
        .is_ok());
        assert!(KnowledgeUseUpdateInput { page_id }.check().is_ok());
        assert!(KnowledgeKeepPageInput { page_id }.check().is_ok());
        assert!(KnowledgeLeaveItInput { page_id }.check().is_ok());
        assert!(KnowledgeRemoveInput { page_id }.check().is_ok());
        assert!(KnowledgeUndoRemoveInput { page_id }.check().is_ok());
        assert!(KnowledgeCopyToWorkspaceInput {
            page_id,
            to_workspace_id: ws
        }
        .check()
        .is_ok());
        assert!(KnowledgePublishToConfluenceInput {
            page_id,
            destination: CopyDestination::ConfluencePage {
                space: "OPS".into(),
                parent_page_id: "1".into()
            },
            keep_up_to_date: true
        }
        .check()
        .is_ok());
        assert!(KnowledgeAcceptSuggestionInput {
            page_id,
            suggestion_id
        }
        .check()
        .is_ok());
        assert!(KnowledgeDismissSuggestionInput {
            page_id,
            suggestion_id
        }
        .check()
        .is_ok());
        assert!(KnowledgeListReadPagesInput {
            workspace_id: ws,
            folder_id: None
        }
        .check()
        .is_ok());
    }

    #[test]
    fn summary_type_key() {
        let summary = PageSummary {
            workspace_id: WorkspaceId::new(),
            id: KnowledgePageId::new(),
            title: "Supplier rates".into(),
            state: KnowledgePageState::Current,
            written_by: KnowledgeAuthor::Ordin,
            last_changed_at: Timestamp::now(),
            shared_by: None,
            shared_at: None,
            cites_other_computer: false,
            tags: vec!["suppliers".into()],
            folder: None,
            folder_description: None,
            set_by_company: None,
            folder_id: None,
            read_only: false,
            adopted_from: None,
            adoption_mark: None,
            page_type: Some("Explanation".into()),
            description: None,
            trust: PageTier::HumanReviewed,
            status: None,
            past_stale_after: false,
        };
        let v = serde_json::to_value(summary).unwrap();
        assert_eq!(v["type"], "Explanation");
        assert_eq!(v["trust"], "human_reviewed");
        assert!(v.get("status").is_none() && v.get("folder").is_none());
    }
}
