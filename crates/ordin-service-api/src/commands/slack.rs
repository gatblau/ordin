//! Slack — restated from `SlackService.md` (SA-RECON-02; amendment January 2026, ADR-102).
//!
//! The owner's Service API types (`SlackStatus`, `OfferedSlackChannel`, `SlackLinkSummary`,
//! `SlackMessageOut`, `NewSlackRule`, `SlackRuleView`, `SlackScopeSet`) keep their names; the inline
//! inputs take the mechanical `Slack<Verb>Input` names. The 2C Slack entities are imported from
//! `ordin-types`; `SignInChallenge` is Identity's, restated in `commands::identity`.

use ordin_types::domain::{SlackRuleKind, SlackRuleScope};
use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

pub use super::identity::SignInChallenge;
pub use ordin_types::domain::{SlackLink, SlackPost, SlackRule};
pub use ordin_types::ApprovalId;

/// Channel and rule names: 1–80 characters, no `#` (`SLK_NAME_INVALID`).
const NAME_MAX_CHARS: usize = 80;
/// A rule's daily cap: 1–20 (`SLK_RULE_CAP_INVALID`); absent means the owner's default, 20.
const DAILY_CAP_MIN: u32 = 1;
const DAILY_CAP_MAX: u32 = 20;

// ---- named owner types ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SlackStatus {
    pub offered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_not_offered: Option<SlackNotOfferedReason>,
    pub signed_in: bool,
    /// The Slack workspace's name (`auth.test` `team`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    /// The signed-in person's Slack name (`auth.test` `user`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_name: Option<String>,
    /// `users:read` granted.
    pub names_allowed: bool,
    /// `users:read.email` and `im:write` granted.
    pub direct_messages_allowed: bool,
    /// True when a permission of the DirectMessages set is missing (the screen offers "Allow…").
    pub scope_needed: bool,
    /// False when `server.external_actions_off` names `slack_post`.
    pub posting_allowed: bool,
    pub rules_allowed: bool,
    /// The organisation profile names the Slack app (`slack.client_id`).
    pub profile_app: bool,
    /// The person connected their own Slack app (SA-SLK-15).
    pub own_app: bool,
    /// Present when the profile names no Slack app and none is connected: the manifest to create one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlackNotOfferedReason {
    ToolsTurnedOff,
    NoSlackApp,
    SignInNotOfferedHere,
    AppNotApproved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlackScopeSet {
    Base,
    Names,
    DirectMessages,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OfferedSlackChannel {
    pub channel_id: String,
    pub name: String,
    pub private: bool,
    pub allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SlackLinkSummary {
    pub link: SlackLink,
    pub new_messages: u32,
    pub kept_from: Timestamp,
    pub older_skipped: bool,
    pub reactions_paused_today: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SlackMessageOut {
    pub link_id: SlackLinkId,
    pub channel_name: String,
    pub ts: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_ts: Option<String>,
    pub sender_name: String,
    pub sent_at: Timestamp,
    /// The text as kept; content, never instructions (R-81, R-98).
    pub text: String,
    pub from_person: bool,
    /// True when the instruction detector matched.
    pub asks_agent_to_act: bool,
    /// Built without a call; opens the message in Slack. Never followed by an agent.
    pub permalink: String,
}

/// Sent by the screens only after the person's confirmation on the rule screen.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NewSlackRule {
    pub kind: SlackRuleKind,
    pub scope: SlackRuleScope,
    pub channel_id: String,
    pub channel_name: String,
    pub daily_cap: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SlackRuleView {
    pub rule: SlackRule,
    pub sent_today: u32,
}

// ---- inline inputs (mechanical names, SA-RECON-02) ----

/// `slack.status`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackStatusInput {}

/// `slack.sign_in`: `base`, `names` or `direct_messages`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackSignInInput {
    pub scopes: SlackScopeSet,
}

/// `slack.connect_own_app`: only when the profile names no Slack app.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackConnectOwnAppInput {
    pub client_id: String,
}

/// `slack.offered_channels`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackOfferedChannelsInput {
    pub ws: WorkspaceId,
}

/// `slack.link`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackLinkInput {
    pub ws: WorkspaceId,
    pub channel_id: String,
}

/// `slack.unlink`: deletes the link's kept messages and drafts.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackUnlinkInput {
    pub ws: WorkspaceId,
    pub link_id: SlackLinkId,
}

/// `slack.links`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackLinksInput {
    pub ws: WorkspaceId,
}

/// `slack.messages`: marks the page seen (SA-PAGE-01).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackMessagesInput {
    pub ws: WorkspaceId,
    pub link_id: SlackLinkId,
    pub cursor: Option<String>,
}

/// `slack.set_ordin_reads`: "Let Ordin look at new messages".
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackSetOrdinReadsInput {
    pub ws: WorkspaceId,
    pub link_id: SlackLinkId,
    pub on: bool,
}

/// `slack.rules`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackRulesInput {}

/// `slack.add_rule`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackAddRuleInput {
    pub rule: NewSlackRule,
}

/// `slack.turn_off_rule`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackTurnOffRuleInput {
    pub rule_id: SlackRuleId,
}

/// `slack.try_again`: a new card for a refused post.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackTryAgainInput {
    pub ws: WorkspaceId,
    pub post_id: SlackPostId,
}

/// `slack.post_record`: one post's record.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackPostRecordInput {
    pub post_id: SlackPostId,
}

// ---- checks ----

/// A channel id `^[CG][A-Z0-9]{8,20}$` (`SLK_CHANNEL_ID_INVALID`).
fn channel_id(value: &str, at: &str) -> Result<(), InputRejection> {
    let mut chars = value.chars();
    let ok = matches!(chars.next(), Some('C' | 'G'))
        && (9..=21).contains(&value.len())
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(rules::reject(at))
    }
}

/// A channel name: 1–80 characters, no `#` (`SLK_NAME_INVALID`).
fn channel_name(value: &str, at: &str) -> Result<(), InputRejection> {
    rules::text(value, 1, NAME_MAX_CHARS, at)?;
    if value.contains('#') {
        return Err(rules::reject(at));
    }
    Ok(())
}

impl CommandInput for SlackConnectOwnAppInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        // SA-SLK-15: the pasted client id matches `^[0-9]+\.[0-9]+$`.
        let ok = self.client_id.split_once('.').is_some_and(|(a, b)| {
            !a.is_empty()
                && !b.is_empty()
                && a.chars().all(|c| c.is_ascii_digit())
                && b.chars().all(|c| c.is_ascii_digit())
        });
        if ok {
            Ok(())
        } else {
            Err(rules::reject("/client_id"))
        }
    }
}

impl CommandInput for SlackLinkInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        channel_id(&self.channel_id, "/channel_id")
    }
}

impl CommandInput for SlackMessagesInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor_text(&self.cursor, "/cursor")
    }
}

impl CommandInput for SlackAddRuleInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        let rule = &self.rule;
        // `SLK_RULE_SCOPE_MISMATCH`: `HandoverNotices` ↔ `Stage`, `OrdinAnswers` ↔ `Link`.
        let scope_ok = matches!(
            (rule.kind, &rule.scope),
            (SlackRuleKind::HandoverNotices, SlackRuleScope::Stage { .. })
                | (SlackRuleKind::OrdinAnswers, SlackRuleScope::Link { .. })
        );
        if !scope_ok {
            return Err(rules::reject("/rule/scope"));
        }
        if let SlackRuleScope::Stage { stage_name, .. } = &rule.scope {
            rules::text(stage_name, 1, NAME_MAX_CHARS, "/rule/scope/stage_name")?;
        }
        channel_id(&rule.channel_id, "/rule/channel_id")?;
        channel_name(&rule.channel_name, "/rule/channel_name")?;
        rules::opt_range(
            rule.daily_cap,
            DAILY_CAP_MIN,
            DAILY_CAP_MAX,
            "/rule/daily_cap",
        )
    }
}

/// Inputs with no stateless rule beyond their types.
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
    SlackStatusInput,
    SlackSignInInput,
    SlackOfferedChannelsInput,
    SlackUnlinkInput,
    SlackLinksInput,
    SlackSetOrdinReadsInput,
    SlackRulesInput,
    SlackTurnOffRuleInput,
    SlackTryAgainInput,
    SlackPostRecordInput,
);

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> SlackAddRuleInput {
        SlackAddRuleInput {
            rule: NewSlackRule {
                kind: SlackRuleKind::HandoverNotices,
                scope: SlackRuleScope::Stage {
                    lifecycle_id: LifecycleId::new(),
                    stage_name: "Analysis".into(),
                },
                channel_id: "C04XYZ12AB".into(),
                channel_name: "design-reviews".into(),
                daily_cap: None,
            },
        }
    }

    #[test]
    fn add_rule_rules() {
        assert!(rule().check().is_ok());
        type Change = Box<dyn Fn(&mut NewSlackRule)>;
        let cases: Vec<(Change, &str)> = vec![
            (
                Box::new(|r| r.kind = SlackRuleKind::OrdinAnswers),
                "/input/rule/scope",
            ),
            (
                Box::new(|r| {
                    r.scope = SlackRuleScope::Stage {
                        lifecycle_id: LifecycleId::new(),
                        stage_name: String::new(),
                    }
                }),
                "/input/rule/scope/stage_name",
            ),
            (
                Box::new(|r| r.channel_id = "D04XYZ12AB".into()),
                "/input/rule/channel_id",
            ),
            (
                Box::new(|r| r.channel_id = "C04".into()),
                "/input/rule/channel_id",
            ),
            (
                Box::new(|r| r.channel_id = "C04xyz12ab".into()),
                "/input/rule/channel_id",
            ),
            (
                Box::new(|r| r.channel_name = "#design".into()),
                "/input/rule/channel_name",
            ),
            (
                Box::new(|r| r.channel_name = String::new()),
                "/input/rule/channel_name",
            ),
            (Box::new(|r| r.daily_cap = Some(0)), "/input/rule/daily_cap"),
            (
                Box::new(|r| r.daily_cap = Some(21)),
                "/input/rule/daily_cap",
            ),
        ];
        for (change, at) in cases {
            let mut input = rule();
            change(&mut input.rule);
            let refusal = input.check().unwrap_err();
            assert_eq!(
                (refusal.code, refusal.at.as_str()),
                ("SVC_INPUT_INVALID", at)
            );
        }
        let mut answers = rule();
        answers.rule.kind = SlackRuleKind::OrdinAnswers;
        answers.rule.scope = SlackRuleScope::Link {
            slack_link_id: SlackLinkId::new(),
        };
        answers.rule.daily_cap = Some(20);
        assert!(answers.check().is_ok());
    }

    // Scenario: Error path — an unknown field in slack.add_rule (the typed read refuses it).
    #[test]
    fn add_rule_refuses_unknown_field() {
        let raw = serde_json::json!({"rule": {
            "kind": "ordin_answers",
            "scope": {"kind": "link", "slack_link_id": SlackLinkId::new()},
            "channel_id": "C04XYZ12AB", "channel_name": "design", "post_now": true
        }});
        assert!(serde_json::from_value::<SlackAddRuleInput>(raw).is_err());
    }

    #[test]
    fn other_inputs() {
        let ws = WorkspaceId::new();
        let mut own = SlackConnectOwnAppInput {
            client_id: "123456.7890".into(),
        };
        assert!(own.check().is_ok());
        for bad in ["123456", "12a.34", ".34", "12."] {
            own.client_id = bad.into();
            assert_eq!(own.check().unwrap_err().at, "/input/client_id", "{bad}");
        }
        let mut link = SlackLinkInput {
            ws,
            channel_id: "G0123456789".into(),
        };
        assert!(link.check().is_ok());
        link.channel_id = "x".into();
        assert_eq!(link.check().unwrap_err().at, "/input/channel_id");
        let mut messages = SlackMessagesInput {
            ws,
            link_id: SlackLinkId::new(),
            cursor: Some("c".repeat(1025)),
        };
        assert_eq!(messages.check().unwrap_err().code, "SRV_BAD_CURSOR");
        messages.cursor = None;
        assert!(messages.check().is_ok());
        assert!(SlackStatusInput {}.check().is_ok());
        let scopes: SlackSignInInput =
            serde_json::from_value(serde_json::json!({"scopes": "direct_messages"})).unwrap();
        assert_eq!(scopes.scopes, SlackScopeSet::DirectMessages);
    }

    #[test]
    fn status_shape() {
        let status = SlackStatus {
            offered: false,
            reason_not_offered: Some(SlackNotOfferedReason::NoSlackApp),
            signed_in: false,
            workspace_name: None,
            person_name: None,
            names_allowed: false,
            direct_messages_allowed: false,
            scope_needed: false,
            posting_allowed: true,
            rules_allowed: true,
            profile_app: false,
            own_app: false,
            manifest: None,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["reason_not_offered"], "no_slack_app");
        assert!(json.get("manifest").is_none());
    }
}
