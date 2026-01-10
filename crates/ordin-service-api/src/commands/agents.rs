//! Agents in a workspace — restated from `PackageCatalogue.md` (`agents.preview_add` to `agents.remove`)
//! and from `PackageInspector.md` (`agents.made_of`, `agents.installed_files`, `agents.part_page`;
//! ADR-093, SA-CLOSE-06) (SA-RECON-02).
//!
//! The screen never sends a path: the program fills the Package Inspector's `folder_path` and
//! `bundle_dir` from the instance before it calls `installed_files` or `part_page`.

use ordin_types::domain::agents::PartKind;
use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::agents::AgentInstance;

/// A pack or part identifier in a choice: 1–200 characters (2C `Part.identifier`).
const IDENTIFIER_MAX_CHARS: usize = 200;

// ---- Package Catalogue ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AddAgentInput {
    pub workspace_id: WorkspaceId,
    /// A WorkspaceProfile part offered by the Catalogue Service.
    pub agent: PartId,
    /// Filled by the program from the Project Service before the call (languages of the workspace's
    /// linked code).
    // SPEC-GAP-18: PackageCatalogue.md gives this field no `serde(default)`, yet says the screens do
    // not send it and the program fills it; with `deny_unknown_fields` and no default the screens'
    // input would be refused, so an absent list reads as empty.
    #[serde(default)]
    pub workspace_languages: Vec<String>,
    /// BCP 47, e.g. "en-GB".
    pub interface_locale: String,
    /// From **Change**; empty on a first add.
    pub choices: Vec<PackChoice>,
}

/// A pack choice for one required pack rule of the agent (ADR-089 *Change*, decision 64).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PackChoice {
    /// 0-based index in the agent's requiredPracticePacks (Resolver `PackAnswer`).
    pub rule_index: u32,
    /// Pack part identifiers.
    pub packs: Vec<String>,
}

/// One pack or overlay a plan picked, for "Uses: …".
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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

/// Who picked a pack or overlay (Agent Model, restated with its `serde` spelling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ChosenBy {
    WorkspaceCode,
    MakerFixed,
    PersonAnswer,
    PersonChange,
    AgentDefault,
    InterfaceLanguage,
}

/// A required pack rule nobody has answered yet (Agent Model, restated with its `serde` spelling).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingPackChoice {
    /// 0-based index of the rule in `requiredPracticePacks`.
    pub rule_index: u32,
    pub at_least: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Identifiers of the packs the rule allows, sorted; the question's choices (ADR-089).
    pub choices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AddPreview {
    pub agent_name: String,
    pub from: String,
    pub uses: Vec<UsesItem>,
    pub asks: Vec<PendingPackChoice>,
    pub available: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Availability {
    Available,
    PackMissing { pack_class_label: String },
    NeedsItApproval { parts: Vec<String> },
    Unsafe,
}

// The owner's shape is kept: boxing `Ready.agent` would change the restated type.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AddOutcome {
    Ready {
        agent: InstalledAgent,
    },
    /// Message `agent-add-unsafe`, action `send_to_it`.
    Unsafe,
    /// Message `agent-add-needs-it`, action Ask IT (`send_to_it`).
    NeedsItApproval {
        parts: Vec<String>,
    },
    PackMissing {
        pack_class_label: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InstalledAgent {
    pub instance: AgentInstance,
    pub workspaces: Vec<WorkspaceId>,
    pub tasks: Vec<String>,
    pub uses: Vec<UsesItem>,
    /// The stopped reason is `instance.stopped_reason`.
    pub update: Option<PendingUpdate>,
}

/// The Package Inspector's plain-words differences of an update.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PendingUpdate {
    pub to_commit: String,
    pub may_do_changes: Vec<MayDoChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MayDoChangeKind {
    ToolAdded,
    ToolRemoved,
    ReadAdded,
    ReadRemoved,
    ConnectionAdded,
    ConnectionRemoved,
    PolicyChanged,
    DeclarationChanged,
    AuthorityChanged,
}

/// One difference in plain words: the screen renders `message_id` with `name` as its argument.
// SPEC-GAP-19: PackageInspector.md types `message_id` (here and in `KindGroup`, `PartPage`) as
// `&'static str`, which cannot be read back from JSON; restated as `String`, the same wire form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct MayDoChange {
    pub kind: MayDoChangeKind,
    /// The tool or connection name, or the new authority in snake_case; empty for the digests.
    pub name: String,
    /// Fluent message id, `may-do-<kind in kebab-case>`, e.g. "may-do-connection-added".
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChangeChoiceInput {
    pub agent_instance_id: AgentInstanceId,
    pub choice: PackChoice,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RemoveAgentInput {
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: Option<WorkspaceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentsListInput {
    pub workspace_id: Option<WorkspaceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentsGetInput {
    pub agent_instance_id: AgentInstanceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentsAcceptUpdateInput {
    pub agent_instance_id: AgentInstanceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentsDeclineUpdateInput {
    pub agent_instance_id: AgentInstanceId,
}

// ---- Package Inspector ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    BusinessUser,
    LinksCode,
}

/// `agents.made_of` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MadeOfInput {
    pub agent_instance_id: AgentInstanceId,
    pub detail: Detail,
}

/// `agents.installed_files` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct InstalledFilesInput {
    pub agent_instance_id: AgentInstanceId,
}

/// `agents.part_page` input. `part_id` is a `PartLine` identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PartPageInput {
    pub agent_instance_id: AgentInstanceId,
    pub part_id: String,
    pub detail: Detail,
}

/// crucible's loading modes, serialised as crucible spells them (Agent Model).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LoadingMode {
    AlwaysOn,
    OnDemand,
    Event,
    ModeSwitch,
    RegisteredInvocation,
    Structural,
    None,
}

/// A part as one plan, bundle or lock file names it (Agent Model, restated with its `serde` spelling).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartRef {
    pub kind: PartKind,
    /// `metadata.id`.
    pub id: String,
    pub name: String,
    pub version: String,
    /// `sha256:<hex>`.
    pub content_hash: String,
    /// `<host>/<owner>/<name>`, or `bundled`.
    pub repository: String,
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<LoadingMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartState {
    InUse,
    NotForNewAgents,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PartLine {
    pub part: PartRef,
    /// `metadata.name`, else the slug.
    pub name: String,
    /// `metadata.description`, first sentence, at most 300 characters.
    pub what_it_does: String,
    pub state: PartState,
    /// "From: …" display name; None when the part is from the agent's own repository.
    pub from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct KindGroup {
    /// Fluent message id of the kind in plain words, e.g. "part-kind-ability" (SPEC-GAP-19).
    pub plain_kind_message_id: String,
    pub count: u32,
    pub parts: Vec<PartLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InstalledMadeOf {
    pub display_name: String,
    pub what_it_does: String,
    /// Command names, for "Its tasks: …".
    pub tasks: Vec<String>,
    pub part_count: u32,
    pub groups: Vec<KindGroup>,
    /// LinksCode only: (repository, commit) for "Pinned by its lock file: [repository] at [commit]".
    pub pinned_by: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct InstalledFileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PartPage {
    pub line: PartLine,
    /// SPEC-GAP-19.
    pub plain_kind_message_id: String,
    /// The part's Markdown body, or its YAML manifest text for YAML-only kinds; data, never obeyed.
    pub text: String,
    /// LinksCode only: the part's primary file path inside the bundle, for **Show the file**.
    pub file: Option<String>,
}

// ---- checks ----

/// One pack choice: identifiers of 1–200 characters, no repeats.
fn check_choice(choice: &PackChoice, at: &str) -> Result<(), InputRejection> {
    for (i, pack) in choice.packs.iter().enumerate() {
        rules::text(pack, 1, IDENTIFIER_MAX_CHARS, &format!("{at}/packs/{i}"))?;
    }
    rules::unique(&choice.packs, &format!("{at}/packs"))
}

impl CommandInput for AddAgentInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::locale(&self.interface_locale, "/interface_locale")?;
        for (i, choice) in self.choices.iter().enumerate() {
            check_choice(choice, &format!("/choices/{i}"))?;
        }
        Ok(())
    }
}

impl CommandInput for ChangeChoiceInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_choice(&self.choice, "/choice")
    }
}

impl CommandInput for PartPageInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.part_id, 1, IDENTIFIER_MAX_CHARS, "/part_id")
    }
}

/// Inputs whose fields are identifiers and closed enums: the type itself is the check.
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
    RemoveAgentInput,
    AgentsListInput,
    AgentsGetInput,
    AgentsAcceptUpdateInput,
    AgentsDeclineUpdateInput,
    MadeOfInput,
    InstalledFilesInput,
);

#[cfg(test)]
mod tests {
    use super::*;

    fn add_input() -> AddAgentInput {
        serde_json::from_value(serde_json::json!({
            "workspace_id": "0192a0f1-1111-7222-8333-444455556666",
            "agent": "0192a2c0-0000-7000-8000-0000000000aa",
            "interface_locale": "en-GB",
            "choices": [{"rule_index": 0, "packs": ["urn:ordin:acme:practice-packs:go"]}]
        }))
        .expect("the screens' input, without workspace_languages")
    }

    #[test]
    fn add_agent_input() {
        let mut input = add_input();
        assert!(input.workspace_languages.is_empty());
        assert!(input.check().is_ok());
        input.interface_locale = "x".into();
        assert_eq!(input.check().unwrap_err().at, "/input/interface_locale");
        let mut input = add_input();
        input.choices[0].packs.push(String::new());
        assert_eq!(input.check().unwrap_err().at, "/input/choices/0/packs/1");
        let mut input = add_input();
        let pack = input.choices[0].packs[0].clone();
        input.choices[0].packs.push(pack);
        assert_eq!(input.check().unwrap_err().at, "/input/choices/0/packs/1");
    }

    #[test]
    fn change_choice_and_part_page() {
        let mut change = ChangeChoiceInput {
            agent_instance_id: AgentInstanceId::new(),
            choice: PackChoice {
                rule_index: 1,
                packs: vec!["a".into()],
            },
        };
        assert!(change.check().is_ok());
        change.choice.packs = vec!["x".repeat(IDENTIFIER_MAX_CHARS + 1)];
        let refusal = change.check().unwrap_err();
        assert_eq!(
            (refusal.code, refusal.at.as_str()),
            ("SVC_INPUT_INVALID", "/input/choice/packs/0")
        );
        let mut page = PartPageInput {
            agent_instance_id: AgentInstanceId::new(),
            part_id: String::new(),
            detail: Detail::LinksCode,
        };
        assert_eq!(page.check().unwrap_err().at, "/input/part_id");
        page.part_id = "urn:ordin:default:commands:lifecycle:apply".into();
        assert!(page.check().is_ok());
        assert!(AgentsListInput { workspace_id: None }.check().is_ok());
        assert!(MadeOfInput {
            agent_instance_id: AgentInstanceId::new(),
            detail: Detail::BusinessUser
        }
        .check()
        .is_ok());
    }

    #[test]
    fn owner_spellings() {
        assert_eq!(
            serde_json::to_value(ChosenBy::MakerFixed).unwrap(),
            "maker-fixed"
        );
        let pending = PendingPackChoice {
            rule_index: 0,
            at_least: 1,
            class: None,
            choices: vec![],
        };
        assert_eq!(
            serde_json::to_value(&pending).unwrap(),
            serde_json::json!({"ruleIndex": 0, "atLeast": 1, "choices": []})
        );
        assert_eq!(
            serde_json::to_value(Availability::PackMissing {
                pack_class_label: "language".into()
            })
            .unwrap(),
            serde_json::json!({"kind": "pack_missing", "pack_class_label": "language"})
        );
        assert!(serde_json::from_value::<MadeOfInput>(serde_json::json!({
            "agent_instance_id": "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff", "detail": "links_code", "path": "/x"
        }))
        .is_err());
    }
}
