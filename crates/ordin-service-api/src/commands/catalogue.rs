//! Agent repositories and the library — restated from `CatalogueService.md` (SA-RECON-02).
//!
//! The Catalogue Service owns these commands, their names and their types; the program converts
//! between `ordin_catalogue`'s types and these, and its contract test keeps the two forms equal.

use ordin_types::domain::agents::PartKind;
use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::agents::{
    AgentRepository, Lifecycle, Part, VocabularyTerm, WithdrawnListEntry,
};
pub use ordin_types::{AgentRepositoryId, LifecycleId, PartId};

/// The longest web address `ProbeRepositoryInput` accepts, in bytes.
const WEB_ADDRESS_MAX_BYTES: usize = 2048;
/// The longest library search text, in characters (`LibraryQuery.text`).
const LIBRARY_TEXT_MAX_CHARS: usize = 500;
/// A repository display name: 1–120 characters (the 2C display-name rule).
const DISPLAY_NAME_MAX_CHARS: usize = 120;

// ---- repositories ----

/// `catalogue.list_repositories` takes no input.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueListRepositoriesInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProbeRepositoryInput {
    /// At most 2,048 bytes.
    pub web_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RepositoryProbe {
    // SPEC-GAP-20: CatalogueService.md types `probe_id` as `uuid::Uuid`, which this crate cannot name (it
    // depends on no `uuid`); carried as its 36-character hyphenated text, the same wire form.
    pub probe_id: String,
    pub host: String,
    pub repository: String,
    pub suggested_name: String,
    pub format: ordin_types::domain::agents::AgentFormat,
    pub agent_count: u32,
    pub person_can_publish: bool,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AddRepositoryInput {
    /// The probe's id, a hyphenated UUID (SPEC-GAP-20).
    pub probe_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RenameRepositoryInput {
    pub id: AgentRepositoryId,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RepositorySummary {
    pub repository: AgentRepository,
    pub agent_count: u32,
    /// `source = profile`.
    pub set_by_company: bool,
    pub last_fetch: Option<FetchOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FetchOutcome {
    pub agent_repository_id: AgentRepositoryId,
    pub result: FetchResult,
    pub commit: Option<String>,
    pub at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FetchResult {
    Ingested {
        offered: u32,
        not_offered: u32,
    },
    Unchanged,
    /// ADR-084: not published until signed.
    MergeUnsigned,
    /// Fail closed, R-78.
    WithdrawnListUnreadable,
    CommitRefused {
        reason_code: String,
    },
    Unreachable,
    SignInNeeded,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueRemoveRepositoryInput {
    pub id: AgentRepositoryId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueFetchNowInput {
    pub id: Option<AgentRepositoryId>,
}

// ---- library ----

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct LibraryQuery {
    /// At most 500 characters.
    pub text: Option<String>,
    /// Empty = WorkspaceProfile only (the agent library).
    pub kinds: Vec<PartKind>,
    /// Filter "From:".
    pub repositories: Vec<AgentRepositoryId>,
    pub area: Option<String>,
    pub audience: Option<String>,
    pub publisher: Option<String>,
    /// Deprecated parts (curators).
    pub include_not_for_new: bool,
    /// 2C `Cursor` (C-15).
    pub cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LibraryEntry {
    pub part: PartView,
    /// The repository display name, or "ordin".
    pub from: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LibraryFacets {
    pub area: Vec<(String, u32)>,
    pub audience: Vec<(String, u32)>,
    pub publisher: Vec<(String, u32)>,
    pub repository: Vec<(AgentRepositoryId, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PartView {
    pub part: Part,
    /// One sentence, "what it does".
    pub summary: String,
    pub offered: bool,
    pub state_label: PartStateLabel,
    pub from: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartStateLabel {
    Current,
    NotForNewAgents,
    Withdrawn,
    NotOffered,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PartDocument {
    pub view: PartView,
    /// Agent Model fields by kind.
    pub body: serde_json::Value,
    /// Shown only in code workspaces.
    pub file_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct UsedBy {
    pub part_id: PartId,
    pub agents: Vec<PartView>,
    pub parts: Vec<PartView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MadeOf {
    pub agent: PartView,
    pub groups: Vec<(PartKind, Vec<PartView>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PartHistory {
    pub versions: Vec<PartVersionLine>,
    pub replaces: Vec<PartView>,
    pub replaced_by: Vec<PartView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PartVersionLine {
    pub version: String,
    pub commit: String,
    pub merged_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CataloguePartInput {
    pub id: PartId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueReadPartInput {
    pub id: PartId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueUsedByInput {
    pub id: PartId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueMadeOfInput {
    pub agent: PartId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CataloguePartHistoryInput {
    pub id: PartId,
}

// ---- curation reads ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HealthReport {
    pub agent_repository_id: AgentRepositoryId,
    pub computed_at: Timestamp,
    pub commit: String,
    pub lines: Vec<HealthLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthLine {
    UnusedPart {
        part_id: PartId,
    },
    LookAlike {
        part_id: PartId,
        similar_to: PartId,
    },
    SharedIdentifier {
        identifier: String,
        other_repository_id: AgentRepositoryId,
    },
    BrokenBorrowedReference {
        part_id: PartId,
        reference: String,
    },
    UsedWhileDeprecated {
        part_id: PartId,
        used_by: Vec<PartId>,
    },
    UsedWhileWithdrawn {
        part_id: PartId,
        used_by: Vec<PartId>,
    },
    UncitedTerm {
        vocabulary_term_id: VocabularyTermId,
    },
    /// A crucible journey whose seams loop (ADR-099 item 7).
    JourneyNotOffered {
        part_id: PartId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RepositoryLayout {
    pub kinds: Vec<KindLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KindLine {
    pub kind: PartKind,
    pub count: u32,
    pub directory: String,
    pub readme: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueHealthInput {
    pub id: AgentRepositoryId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueWithdrawnListInput {
    pub id: AgentRepositoryId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueVocabularyInput {
    pub id: AgentRepositoryId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueLayoutInput {
    pub id: AgentRepositoryId,
}

// ---- lifecycles ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueLifecyclesInput {
    pub id: Option<AgentRepositoryId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CatalogueLifecycleInput {
    pub id: LifecycleId,
}

// ---- checks ----

/// True for 36 characters of lowercase hexadecimal in the 8-4-4-4-12 hyphenated form (any version).
pub(crate) fn is_hyphenated_uuid(raw: &str) -> bool {
    raw.len() == 36
        && raw.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

/// Inputs whose only field is an identifier: the type itself is the check.
macro_rules! no_rules {
    ($($ty:ty),* $(,)?) => {$(
        impl CommandInput for $ty {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

no_rules!(
    CatalogueListRepositoriesInput,
    CatalogueRemoveRepositoryInput,
    CatalogueFetchNowInput,
    CataloguePartInput,
    CatalogueReadPartInput,
    CatalogueUsedByInput,
    CatalogueMadeOfInput,
    CataloguePartHistoryInput,
    CatalogueHealthInput,
    CatalogueWithdrawnListInput,
    CatalogueVocabularyInput,
    CatalogueLayoutInput,
    CatalogueLifecyclesInput,
    CatalogueLifecycleInput,
);

impl CommandInput for ProbeRepositoryInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::bytes(&self.web_address, 0, WEB_ADDRESS_MAX_BYTES, "/web_address")
    }
}

impl CommandInput for AddRepositoryInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        if !is_hyphenated_uuid(&self.probe_id) {
            return Err(rules::reject("/probe_id"));
        }
        rules::text(
            &self.display_name,
            1,
            DISPLAY_NAME_MAX_CHARS,
            "/display_name",
        )
    }
}

impl CommandInput for RenameRepositoryInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(
            &self.display_name,
            1,
            DISPLAY_NAME_MAX_CHARS,
            "/display_name",
        )
    }
}

impl CommandInput for LibraryQuery {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::opt_text(&self.text, 0, LIBRARY_TEXT_MAX_CHARS, "/text")?;
        rules::cursor(&self.cursor, "/cursor")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_inputs() {
        let mut probe = ProbeRepositoryInput {
            web_address: "https://github.com/northwind/finance-agents".into(),
        };
        assert!(probe.check().is_ok());
        let mut probe = ProbeRepositoryInput {
            web_address: "x".repeat(WEB_ADDRESS_MAX_BYTES + 1),
        };
        let refusal = probe.check().unwrap_err();
        assert_eq!(
            (refusal.code, refusal.at.as_str()),
            ("SVC_INPUT_INVALID", "/input/web_address")
        );
        let mut add = AddRepositoryInput {
            probe_id: "0192a2b0-1111-7aaa-8bbb-ccccdddd0001".into(),
            display_name: "Finance agents".into(),
        };
        assert!(add.check().is_ok());
        add.display_name = String::new();
        assert_eq!(add.check().unwrap_err().at, "/input/display_name");
        add.probe_id = "0192A2B0-1111-7aaa-8bbb-ccccdddd0001".into();
        assert_eq!(add.check().unwrap_err().at, "/input/probe_id");
        assert!(!is_hyphenated_uuid("0192a2b0x1111-7aaa-8bbb-ccccdddd0001"));
        let mut rename = RenameRepositoryInput {
            id: AgentRepositoryId::new(),
            display_name: "x".repeat(DISPLAY_NAME_MAX_CHARS + 1),
        };
        assert_eq!(rename.check().unwrap_err().at, "/input/display_name");
        rename.display_name = "Company agents".into();
        assert!(rename.check().is_ok());
    }

    #[test]
    fn library_query() {
        let mut query = LibraryQuery {
            text: Some("rates".into()),
            ..LibraryQuery::default()
        };
        assert!(query.check().is_ok());
        query.text = Some("x".repeat(LIBRARY_TEXT_MAX_CHARS + 1));
        assert_eq!(query.check().unwrap_err().at, "/input/text");
        query.text = None;
        query.cursor = Some(Cursor::encode(&"k".repeat(900)));
        assert_eq!(query.check().unwrap_err().code, "SRV_BAD_CURSOR");
    }

    #[test]
    fn identifier_inputs_have_no_rules() {
        assert!(CatalogueListRepositoriesInput {}.check().is_ok());
        assert!(CatalogueFetchNowInput { id: None }.check().is_ok());
        assert!(CatalogueLifecycleInput {
            id: LifecycleId::new()
        }
        .check()
        .is_ok());
        // Unknown fields are refused by the type.
        let extra = serde_json::json!({"id": null, "force": true});
        assert!(serde_json::from_value::<CatalogueFetchNowInput>(extra).is_err());
    }

    #[test]
    fn probe_example_shape() {
        let json = serde_json::json!({"probe_id":"0192a2b0-1111-7aaa-8bbb-ccccdddd0001","host":"github.com",
            "repository":"northwind/finance-agents","suggested_name":"finance-agents","format":"apm",
            "agent_count":12,"person_can_publish":false,"expires_at":"2026-01-10T10:25:00.000Z"});
        let probe: RepositoryProbe = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&probe).unwrap(), json);
        let outcome = FetchResult::Ingested {
            offered: 3,
            not_offered: 1,
        };
        assert_eq!(
            serde_json::to_value(&outcome).unwrap(),
            serde_json::json!({"kind":"ingested","offered":3,"not_offered":1})
        );
    }
}
