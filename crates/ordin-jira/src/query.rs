//! Building queries: JQL is never taken from a caller (JiraAdapter.md Internal Logic 3).

use crate::address::{issue_key_ok, project_key_ok};
use crate::{JiraError, JiraQuery};

/// `MAX_SEARCH`: results per `search` of `HandoversFor` (A-54).
pub(crate) const MAX_SEARCH: u32 = 50;
/// The most results of a `Text` search (Internal Logic 3.3).
pub(crate) const MAX_TEXT_SEARCH: u32 = 25;
/// `MAX_ALSO_KEYS`.
pub(crate) const MAX_ALSO_KEYS: usize = 50;

/// `LABEL_RULE`: `^[a-z0-9][a-z0-9-]{0,63}$`.
pub(crate) fn label_ok(label: &str) -> bool {
    let bytes = label.as_bytes();
    (1..=64).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// A string value in double quotes: `\` and `"` escaped, control characters removed.
pub(crate) fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What `search` sends.
#[derive(Debug, PartialEq)]
pub(crate) enum Built {
    /// One `POST /search/jql`.
    Jql { jql: String, max_results: u32 },
    /// One `GET /issue/{key}` (the search text is an issue key).
    Key(String),
}

pub(crate) fn build(query: &JiraQuery, limit: u32) -> Result<Built, JiraError> {
    match query {
        JiraQuery::HandoversFor { label, also_keys } => {
            if !label_ok(label)
                || also_keys.len() > MAX_ALSO_KEYS
                || !also_keys.iter().all(|k| issue_key_ok(k))
                || !(1..=MAX_SEARCH).contains(&limit)
            {
                return Err(JiraError::QueryInvalid);
            }
            let mut jql = format!(
                "labels = {} AND ((assignee = currentUser() AND statusCategory != Done)",
                quote(label)
            );
            if !also_keys.is_empty() {
                jql.push_str(&format!(" OR key in ({})", also_keys.join(", ")));
            }
            jql.push_str(") ORDER BY updated DESC");
            Ok(Built::Jql {
                jql,
                max_results: limit,
            })
        }
        JiraQuery::Text(text) => {
            let text = text.trim();
            let chars = text.chars().count();
            if !(1..=200).contains(&chars) || !(1..=MAX_TEXT_SEARCH).contains(&limit) {
                return Err(JiraError::QueryInvalid);
            }
            let upper = text.to_ascii_uppercase();
            if issue_key_ok(&upper) {
                return Ok(Built::Key(upper));
            }
            Ok(Built::Jql {
                jql: format!("text ~ {} ORDER BY updated DESC", quote(text)),
                max_results: limit,
            })
        }
    }
}

/// The JQL that finds an issue `create_issue` may have made (Internal Logic 5.4).
pub(crate) fn own_created(project_key: &str, summary: &str) -> Result<String, JiraError> {
    if !project_key_ok(project_key) {
        return Err(JiraError::AddressInvalid);
    }
    Ok(format!(
        "project = {} AND reporter = currentUser() AND summary ~ {} AND created >= -15m ORDER BY created DESC",
        quote(project_key),
        quote(summary)
    ))
}
