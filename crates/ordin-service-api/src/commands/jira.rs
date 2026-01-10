//! Jira — restated from `JiraAdapter.md` (SA-RECON-02; amended January 2026, ADR-106).
//!
//! Both commands are `Local`: Jira is on the desktop only. `jira.connect_token` carries a secret in;
//! its input is never logged, journalled or echoed (SA-JIRA-04), so its `Debug` hides the token.

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::rules;
use crate::api::{CommandInput, InputRejection};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct JiraIssueRef {
    pub site: String,
    /// `ABC-12`.
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct JiraUser {
    /// 1–128 characters.
    pub account_id: String,
    /// 1–255 characters.
    pub display_name: String,
    /// Absent under the person's privacy settings (C-88).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JiraStatusCategory {
    ToDo,
    InProgress,
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct JiraIssueSummary {
    pub issue: JiraIssueRef,
    pub id: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<JiraUser>,
    pub status_category: JiraStatusCategory,
    pub labels: Vec<String>,
    pub updated: Timestamp,
}

/// `jira.connect_token`: a pasted API token, checked with `GET /myself`, then kept through Identity.
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct JiraConnectTokenInput {
    /// The site's host, `{name}.atlassian.net`.
    pub site: String,
    /// 3–254 characters.
    pub email: String,
    /// 1–1,024 characters; never logged.
    pub token: String,
}

impl std::fmt::Debug for JiraConnectTokenInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JiraConnectTokenInput")
            .field("site", &self.site)
            .field("email", &self.email)
            .field("token", &"***")
            .finish()
    }
}

/// `jira.find_issues`: the person's search text on the connected site.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct JiraFindIssuesInput {
    pub text: String,
    pub limit: u32,
}

impl CommandInput for JiraConnectTokenInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::host(&self.site, "/site")?;
        rules::email(&self.email, 3, 254, "/email")?;
        rules::text(&self.token, 1, 1024, "/token")
    }
}

impl CommandInput for JiraFindIssuesInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.text, 1, 200, "/text")?;
        rules::range(self.limit, 1, 25, "/limit")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(site: &str, email: &str, token: &str) -> Option<String> {
        JiraConnectTokenInput {
            site: site.into(),
            email: email.into(),
            token: token.into(),
        }
        .check()
        .err()
        .map(|r| r.at)
    }

    #[test]
    fn connect_token_rules() {
        assert_eq!(token("acme.atlassian.net", "sam@acme.com", "t"), None);
        assert_eq!(
            token("bad host", "sam@acme.com", "t").as_deref(),
            Some("/input/site")
        );
        assert_eq!(
            token("acme.atlassian.net", "sam", "t").as_deref(),
            Some("/input/email")
        );
        assert_eq!(
            token("acme.atlassian.net", "sam@acme.com", "").as_deref(),
            Some("/input/token")
        );
        assert_eq!(
            token("acme.atlassian.net", "sam@acme.com", &"t".repeat(1025)).as_deref(),
            Some("/input/token")
        );
    }

    #[test]
    fn token_never_in_debug() {
        let input = JiraConnectTokenInput {
            site: "a.atlassian.net".into(),
            email: "a@b.c".into(),
            token: "s3cret-token".into(),
        };
        assert!(!format!("{input:?}").contains("s3cret"));
    }

    #[test]
    fn find_issues_rules() {
        assert!(JiraFindIssuesInput {
            text: "printer".into(),
            limit: 25
        }
        .check()
        .is_ok());
        assert_eq!(
            JiraFindIssuesInput {
                text: String::new(),
                limit: 5
            }
            .check()
            .unwrap_err()
            .at,
            "/input/text"
        );
        assert_eq!(
            JiraFindIssuesInput {
                text: "x".into(),
                limit: 26
            }
            .check()
            .unwrap_err()
            .at,
            "/input/limit"
        );
    }

    #[test]
    fn status_category_wire_form() {
        assert_eq!(
            serde_json::to_value(JiraStatusCategory::InProgress).unwrap(),
            "in_progress"
        );
    }
}
