//! Handovers and lifecycles — restated from `HandoverService.md` (SA-RECON-02), with its amendments
//! of January 2026 (ADR-102: Slack notices and `handover.open_link`; ADR-106: Jira handovers).
//!
//! The handover itself, its receiver, events and state, lifecycles, stages and the Slack and Jira
//! additions to them are 2C types and are imported. Every input carries `deny_unknown_fields`
//! (HandoverService.md *Public Interface*: "Service API inputs carry `deny_unknown_fields`").

use ordin_types::domain::{Lifecycle, LifecycleStage, StageChoice, StageSlackChannel};
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{Handover, HandoverNoticeKind, HandoverState, WorkspaceStage};
pub use ordin_types::{
    ApprovalId, HandoverId, HandoverPlaceId, LifecycleId, SlackRuleId, WorkspaceId,
};

/// `HandoverDraftView.checks` holds the Checker's findings, which the Handover Service passes on.
pub use self::checker_finding::HandoverFinding;

/// One per draft; side-by-side handovers share it.
// SPEC-GAP-25: the owner types this as `DraftGroupId(pub uuid::Uuid)`; `uuid` is not a dependency of this crate,
// so the id is carried as its 36-character lowercase hyphenated text (any UUID version), checked on read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct DraftGroupId(pub String);

impl<'de> Deserialize<'de> for DraftGroupId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        if is_uuid_text(&raw) {
            Ok(DraftGroupId(raw))
        } else {
            Err(serde::de::Error::custom("not a lowercase hyphenated UUID"))
        }
    }
}

/// True for a 36-character lowercase hyphenated UUID of any version.
fn is_uuid_text(raw: &str) -> bool {
    raw.len() == 36
        && raw.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

/// Whether Slack can reach a person, restated from `SlackService.md` (`SlackReach`), which no
/// `slack.*` row names; `Found` carries no member id across the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlackReach {
    Found,
    NotFound,
    NotSignedIn,
    ScopeNeeded,
    NotAllowed,
}

// ---- sending ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OpenDraftInput {
    pub workspace_id: WorkspaceId,
    pub results: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UpdateDraftInput {
    pub draft: DraftGroupId,
    pub title: Option<String>,
    /// At a choice; the person chooses (ADR-099 item 3).
    pub next_stage: Option<String>,
    /// One per stage (side by side), else one.
    pub receivers: Option<Vec<StageReceiver>>,
    pub results: Option<Vec<String>>,
    pub note: Option<[NoteSectionEdit; 4]>,
    /// Outside a lifecycle: a place the workspace links.
    pub place: Option<HandoverPlaceId>,
    /// *Also tell them in Slack* (per draft group); None = unchanged (amended January 2026, ADR-102).
    pub tell_in_slack: Option<bool>,
    /// The issue a Jira handover rides on; None = unchanged (amended January 2026, ADR-106).
    pub jira_issue: Option<JiraIssueSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct StageReceiver {
    pub stage: Option<String>,
    pub person: PersonChoice,
}

/// A person offered or chosen as a receiver. Appears in inputs and results, so unknown fields are not
/// refused here; the inputs that carry it refuse theirs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PersonChoice {
    pub name: String,
    /// May be "" for a Jira choice whose address Jira hides (ADR-106).
    pub address: String,
    pub github_login: Option<String>,
    pub outside_company: bool,
    /// Filled by `find_people` for a choice with an email address (at most 5 per call) (ADR-102).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack: Option<SlackReach>,
    /// 1–128 of `[A-Za-z0-9:_-]`; required for a Jira place (ADR-106).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jira_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NoteSectionEdit {
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FindPeopleInput {
    pub draft: DraftGroupId,
    /// 1–100 characters.
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HandoverDraftView {
    pub draft: DraftGroupId,
    /// One per receiver.
    pub handovers: Vec<Handover>,
    /// When the stage leads to a choice.
    pub choices: Vec<StageChoice>,
    /// Ordin's recommendation and reason (P5).
    pub recommended: Option<(String, String)>,
    /// Who took the stage last.
    pub suggested_receiver: Option<PersonChoice>,
    pub checks: Vec<HandoverFinding>,
    pub can_hand_over: bool,
    /// One per handover of the draft group, in the order of `handovers` (ADR-102).
    pub slack: Vec<DraftSlackView>,
    /// Set only when the draft's place is a Jira place (ADR-106).
    pub jira: Option<DraftJiraView>,
}

/// The Slack line under one receiver of a draft (amended January 2026, ADR-102).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DraftSlackView {
    pub handover_id: HandoverId,
    /// The receiver, by address.
    pub reach: SlackReach,
    pub tell_in_slack: bool,
    /// The receiving stage's channel.
    pub stage_channel: Option<StageSlackChannel>,
    /// The person's rule for that channel, when on.
    pub stage_rule: Option<SlackRuleId>,
    /// The sentence the draft screen shows under the receiver (`handover-slack-reach-*`).
    pub line: String,
}

/// The issue a Jira handover rides on (amended January 2026, ADR-106).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JiraIssueSelection {
    /// One of the workspace's linked issues in the place's project;
    /// `^[A-Z][A-Z0-9_]{1,9}-[1-9][0-9]{0,9}$`.
    Linked { key: String },
    /// A new issue in the place's project, named after the handover's title; stated on the card.
    CreateNew,
}

/// The Jira part of a draft on a Jira place (amended January 2026, ADR-106).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DraftJiraView {
    /// The place's display name.
    pub project: String,
    /// The workspace's linked issues in that project, newest link first.
    pub options: Vec<JiraIssueOption>,
    pub selection: JiraIssueSelection,
    /// The line the draft shows (`handover-jira-rides-on` / `handover-jira-creates`).
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct JiraIssueOption {
    pub key: String,
    /// At most 255 characters.
    pub summary: String,
}

/// `handover.hand_over`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverHandOverInput {
    pub draft: DraftGroupId,
}

/// `handover.take_back`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverTakeBackInput {
    pub id: HandoverId,
}

/// `handover.remind`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverRemindInput {
    pub id: HandoverId,
}

/// `handover.change_and_hand_over_again`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverChangeAndHandOverAgainInput {
    pub id: HandoverId,
}

// ---- receiving ----

/// `handover.check_now`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverCheckNowInput {}

/// `handover.open`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverOpenInput {
    pub id: HandoverId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HandoverScreen {
    pub handover: Handover,
    /// Note lines that read as instructions (R-81).
    pub marked_lines: Vec<String>,
    /// "1 of 2 handed over" at a meeting point.
    pub arrived: Option<(u32, u32)>,
    pub still_under_way: Vec<String>,
    pub results: Vec<ResultLine>,
    pub actions: Vec<HandoverAction>,
    /// Slack messages about this handover: those this installation posted, and the record's
    /// `slack_messages` (amended January 2026, ADR-102).
    pub slack: Vec<SlackLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ResultLine {
    pub label: String,
    pub link: String,
    pub cannot_open: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HandoverAction {
    AcceptAndStart,
    ReturnTo,
    AskOrdin,
    Close,
}

/// One Slack message about a handover, as the handover screen shows it (ADR-102).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SlackLine {
    pub notice: HandoverNoticeKind,
    pub destination_line: String,
    pub state: SlackLineState,
    pub permalink: Option<String>,
}

/// The post record's state as the screen shows it (ADR-102).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlackLineState {
    Waiting,
    Posted,
    NotSent,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AcceptInput {
    pub id: HandoverId,
    pub into: AcceptTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AcceptTarget {
    NewWorkspace { name: String },
    Existing { workspace_id: WorkspaceId },
}

/// `handover.return_options`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverReturnOptionsInput {
    pub id: HandoverId,
}

/// A stage and person a handover may be returned to. Appears in a result and inside `ReturnInput`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ReturnOption {
    pub stage: String,
    pub person: PersonChoice,
    pub when: Option<String>,
    pub round: u32,
    pub max_rounds: Option<u32>,
    pub declared: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReturnInput {
    pub id: HandoverId,
    pub to: ReturnOption,
    /// 1–1,000 characters.
    pub reason: String,
    /// Whether the stage-channel message carries the reason; default true (ADR-102).
    #[serde(default = "yes")]
    pub reason_in_slack: bool,
}

fn yes() -> bool {
    true
}

// ---- lifecycles and stages ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FollowStageInput {
    pub workspace_id: WorkspaceId,
    pub lifecycle_id: LifecycleId,
    pub stage_name: String,
}

/// `handover.stop_following`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverStopFollowingInput {
    pub workspace: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct StartForStageInput {
    pub lifecycle_id: LifecycleId,
    pub stage_name: String,
    pub workspace_name: String,
}

/// `handover.stage_of`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverStageOfInput {
    pub workspace: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct StageView {
    pub lifecycle: Lifecycle,
    pub stage_name: String,
    pub next: Vec<String>,
    /// The OutputContract's sections.
    pub hands_on: Vec<String>,
}

/// `handover.lifecycle_view`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverLifecycleViewInput {
    pub id: LifecycleId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LifecycleView {
    pub lifecycle: Lifecycle,
    pub stages: Vec<StageLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct StageLine {
    pub stage: LifecycleStage,
    pub handovers: Vec<HandoverLine>,
    pub you_are_here: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct HandoverLine {
    pub id: HandoverId,
    pub title: String,
    pub other_person: String,
    pub state: HandoverState,
    pub round: u32,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    HandedToYou,
    HandedOver,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverFilter {
    pub workspace_id: Option<WorkspaceId>,
    pub open_only: bool,
}

/// `handover.list` — the catalogue row wraps the owner's `HandoverFilter` as `{ filter }`.
// HandoverService.md takes the same wrapper (amended January 2026).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverListInput {
    pub filter: HandoverFilter,
}

/// `handover.open_link` (ADR-102): a handover link opened from Slack, an email or a paste.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HandoverOpenLinkInput {
    /// 1–4,096 characters.
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkOpened {
    /// The handover is here; the program raises `ordin://open-target` for it.
    Opened { handover_id: HandoverId },
    /// The place is now followed; the record is not readable yet (the next check retries). Outcome
    /// `HAND_NOT_FOUND_YET`.
    NotFoundYet { place: String },
}

/// The Checker's finding as the draft view carries it, restated from `Checker.md`
/// (`HandoverCheckReport.findings`), which no catalogue row of the `checker` group names.
mod checker_finding {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    pub struct HandoverFinding {
        /// `results_readable`, `contract_sections` or `receiver_reachable`.
        pub code: String,
        pub passed: bool,
        /// The result link or contract identifier the finding concerns.
        pub item: Option<String>,
        pub missing_sections: Vec<String>,
    }
}

// ---- checks (HandoverService.md shared rule rows) ----

/// `Handover.results`: at most 50 links.
const MAX_RESULTS: usize = 50;
/// `NoteSection.body`: at most 4,000 characters.
const MAX_NOTE_CHARS: usize = 4000;

/// `HandoverReceiver.address` (RFC 5321, at most 254) unless a Jira choice leaves it empty;
/// `jira_account_id` 1–128 of `[A-Za-z0-9:_-]`.
fn check_person(person: &PersonChoice, at: &str) -> Result<(), InputRejection> {
    if let Some(id) = &person.jira_account_id {
        rules::text(id, 1, 128, &format!("{at}/jira_account_id"))?;
        if !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-'))
        {
            return Err(rules::reject(&format!("{at}/jira_account_id")));
        }
    }
    let address_may_be_empty = person.jira_account_id.is_some();
    if !(address_may_be_empty && person.address.is_empty()) {
        rules::email(&person.address, 3, 254, &format!("{at}/address"))?;
    }
    Ok(())
}

/// `^[A-Z][A-Z0-9_]{1,9}-[1-9][0-9]{0,9}$`.
fn is_jira_key(key: &str) -> bool {
    let Some((project, number)) = key.split_once('-') else {
        return false;
    };
    let mut p = project.chars();
    let project_ok = p.next().is_some_and(|c| c.is_ascii_uppercase())
        && (2..=10).contains(&project.len())
        && p.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    let number_ok = (1..=10).contains(&number.len())
        && number.starts_with(|c: char| ('1'..='9').contains(&c))
        && number.chars().all(|c| c.is_ascii_digit());
    project_ok && number_ok
}

impl CommandInput for OpenDraftInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::count(&self.results, 0, MAX_RESULTS, "/results")
    }
}

impl CommandInput for UpdateDraftInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::opt_text(&self.title, 1, 120, "/title")?;
        if let Some(receivers) = &self.receivers {
            for (i, receiver) in receivers.iter().enumerate() {
                check_person(&receiver.person, &format!("/receivers/{i}/person"))?;
            }
        }
        if let Some(results) = &self.results {
            rules::count(results, 0, MAX_RESULTS, "/results")?;
        }
        if let Some(note) = &self.note {
            for (i, section) in note.iter().enumerate() {
                rules::text(&section.body, 0, MAX_NOTE_CHARS, &format!("/note/{i}/body"))?;
            }
        }
        if let Some(JiraIssueSelection::Linked { key }) = &self.jira_issue {
            if !is_jira_key(key) {
                return Err(rules::reject("/jira_issue/key"));
            }
        }
        Ok(())
    }
}

impl CommandInput for FindPeopleInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.query, 1, 100, "/query")
    }
}

impl CommandInput for ReturnInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_person(&self.to.person, "/to/person")?;
        rules::text(&self.reason, 1, 1000, "/reason")
    }
}

impl CommandInput for HandoverOpenLinkInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.link, 1, 4096, "/link")
    }
}

/// Inputs with no stateless rule beyond their shape.
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
    HandoverHandOverInput,
    HandoverTakeBackInput,
    HandoverRemindInput,
    HandoverChangeAndHandOverAgainInput,
    HandoverCheckNowInput,
    HandoverOpenInput,
    AcceptInput,
    HandoverReturnOptionsInput,
    FollowStageInput,
    HandoverStopFollowingInput,
    StartForStageInput,
    HandoverStageOfInput,
    HandoverLifecycleViewInput,
    HandoverListInput,
);

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> DraftGroupId {
        DraftGroupId("00000000-0000-0000-0000-000000000000".into())
    }

    fn person(address: &str, jira: Option<&str>) -> PersonChoice {
        PersonChoice {
            name: "Sam Lee".into(),
            address: address.into(),
            github_login: None,
            outside_company: false,
            slack: None,
            jira_account_id: jira.map(Into::into),
        }
    }

    fn update() -> UpdateDraftInput {
        UpdateDraftInput {
            draft: draft(),
            title: Some("Northwind assessment".into()),
            next_stage: None,
            receivers: Some(vec![StageReceiver {
                stage: None,
                person: person("sam.lee@acme.co.uk", None),
            }]),
            results: Some(vec!["https://acme.sharepoint.com/a.docx".into()]),
            note: None,
            place: None,
            tell_in_slack: None,
            jira_issue: Some(JiraIssueSelection::Linked {
                key: "ABC-12".into(),
            }),
        }
    }

    fn at(r: Result<(), InputRejection>) -> String {
        let e = r.unwrap_err();
        assert_eq!(e.code, "SVC_INPUT_INVALID");
        e.at
    }

    /// One change to a valid draft input and the pointer it is refused at.
    type Case = (Box<dyn Fn(&mut UpdateDraftInput)>, &'static str);

    #[test]
    fn update_draft_rules() {
        assert!(update().check().is_ok());
        let cases: Vec<Case> = vec![
            (Box::new(|u| u.title = Some(String::new())), "/input/title"),
            (
                Box::new(|u| u.title = Some("x".repeat(121))),
                "/input/title",
            ),
            (
                Box::new(|u| {
                    u.receivers = Some(vec![StageReceiver {
                        stage: None,
                        person: person("nope", None),
                    }])
                }),
                "/input/receivers/0/person/address",
            ),
            (
                Box::new(|u| {
                    u.receivers = Some(vec![StageReceiver {
                        stage: None,
                        person: person("", Some("bad id!")),
                    }])
                }),
                "/input/receivers/0/person/jira_account_id",
            ),
            (
                Box::new(|u| u.results = Some(vec![String::new(); 51])),
                "/input/results",
            ),
            (
                Box::new(|u| {
                    u.note = Some([
                        NoteSectionEdit {
                            body: String::new(),
                        },
                        NoteSectionEdit {
                            body: "x".repeat(4001),
                        },
                        NoteSectionEdit {
                            body: String::new(),
                        },
                        NoteSectionEdit {
                            body: String::new(),
                        },
                    ])
                }),
                "/input/note/1/body",
            ),
            (
                Box::new(|u| {
                    u.jira_issue = Some(JiraIssueSelection::Linked {
                        key: "abc-1".into(),
                    })
                }),
                "/input/jira_issue/key",
            ),
        ];
        for (change, pointer) in cases {
            let mut input = update();
            change(&mut input);
            assert_eq!(at(input.check()), pointer);
        }
        // A Jira choice may have an empty address.
        let mut jira = update();
        jira.receivers = Some(vec![StageReceiver {
            stage: Some("Analysis".into()),
            person: person("", Some("712020:5c1e")),
        }]);
        jira.jira_issue = Some(JiraIssueSelection::CreateNew);
        assert!(jira.check().is_ok());
    }

    #[test]
    fn draft_group_ids() {
        assert!(serde_json::from_value::<DraftGroupId>(serde_json::json!(
            "0192a3d0-1111-7aaa-8bbb-cccc0000d001"
        ))
        .is_ok());
        for bad in [
            "0192A3D0-1111-7AAA-8BBB-CCCC0000D001",
            "nope",
            "0192a3d0x1111-7aaa-8bbb-cccc0000d001",
        ] {
            assert!(
                serde_json::from_value::<DraftGroupId>(serde_json::json!(bad)).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn jira_keys() {
        for good in ["AB-1", "ABC_1-1234567890", "A1-9"] {
            assert!(is_jira_key(good), "{good}");
        }
        for bad in [
            "A-1",
            "AB-0",
            "AB-",
            "ab-1",
            "ABCDEFGHIJK-1",
            "AB1",
            "AB-12345678901",
        ] {
            assert!(!is_jira_key(bad), "{bad}");
        }
    }

    #[test]
    fn other_inputs() {
        let mut open = OpenDraftInput {
            workspace_id: WorkspaceId::new(),
            results: vec![],
        };
        assert!(open.check().is_ok());
        open.results = vec![String::new(); 51];
        assert_eq!(at(open.check()), "/input/results");

        let mut find = FindPeopleInput {
            draft: draft(),
            query: String::new(),
        };
        assert_eq!(at(find.check()), "/input/query");
        find.query = "sam".into();
        assert!(find.check().is_ok());

        let option = ReturnOption {
            stage: "Analysis".into(),
            person: person("priya.shah@acme.co.uk", None),
            when: None,
            round: 1,
            max_rounds: Some(3),
            declared: true,
        };
        let mut ret = ReturnInput {
            id: HandoverId::new(),
            to: option,
            reason: String::new(),
            reason_in_slack: true,
        };
        assert_eq!(at(ret.check()), "/input/reason");
        ret.reason = "Payment terms are missing".into();
        assert!(ret.check().is_ok());
        ret.to.person.address = "x".into();
        assert_eq!(at(ret.check()), "/input/to/person/address");

        let mut link = HandoverOpenLinkInput {
            link: String::new(),
        };
        assert_eq!(at(link.check()), "/input/link");
        link.link = "ordin://handover/open?h=x&t=git&p=y".into();
        assert!(link.check().is_ok());

        assert!(HandoverCheckNowInput {}.check().is_ok());
        assert!(HandoverListInput {
            filter: HandoverFilter {
                workspace_id: None,
                open_only: true
            }
        }
        .check()
        .is_ok());
    }

    #[test]
    fn shapes() {
        // `reason_in_slack` defaults to true; unknown fields and selection kinds are refused.
        let id = HandoverId::new();
        let raw = serde_json::json!({
            "id": id, "reason": "r",
            "to": {"stage": "Analysis", "person": {"name": "P", "address": "p@a.co", "github_login": null,
                   "outside_company": false}, "when": null, "round": 1, "max_rounds": null, "declared": false}
        });
        let ret: ReturnInput = serde_json::from_value(raw).unwrap();
        assert!(ret.reason_in_slack);
        assert!(serde_json::from_value::<JiraIssueSelection>(
            serde_json::json!({"kind": "pick_any"})
        )
        .is_err());
        assert_eq!(
            serde_json::to_value(JiraIssueSelection::CreateNew).unwrap(),
            serde_json::json!({"kind": "create_new"})
        );
        assert_eq!(
            serde_json::to_value(LinkOpened::NotFoundYet {
                place: "Proc".into()
            })
            .unwrap(),
            serde_json::json!({"kind": "not_found_yet", "place": "Proc"})
        );
        assert!(
            serde_json::from_value::<HandoverCheckNowInput>(serde_json::json!({"x": 1})).is_err()
        );
        assert_eq!(
            serde_json::to_value(HandoverAction::AcceptAndStart).unwrap(),
            serde_json::json!("accept_and_start")
        );
        assert_eq!(
            serde_json::to_value(draft()).unwrap(),
            serde_json::json!("00000000-0000-0000-0000-000000000000")
        );
        let accept: AcceptInput = serde_json::from_value(serde_json::json!({
            "id": id, "into": {"kind": "new_workspace", "name": "Northwind"}
        }))
        .unwrap();
        assert!(matches!(accept.into, AcceptTarget::NewWorkspace { .. }));
    }
}
