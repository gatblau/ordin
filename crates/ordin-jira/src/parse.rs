//! Reading Jira's JSON into the public types. Every missing required field, unparseable time or
//! unknown status category is `JIRA_RESPONSE_INVALID` (Internal Logic 1.4).

use chrono::{DateTime, SubsecRound, Utc};
use ordin_types::time::Timestamp;
use serde_json::Value;

use crate::address::issue_key_ok;
use crate::adf::adf_to_markdown;
use crate::{
    JiraAttachment, JiraComment, JiraError, JiraIssueRef, JiraIssueSummary, JiraLinkedIssue,
    JiraStatusCategory, JiraUser,
};

pub(crate) fn invalid<T>() -> Result<T, JiraError> {
    Err(JiraError::ResponseInvalid)
}

pub(crate) fn str_field<'a>(v: &'a Value, name: &str) -> Result<&'a str, JiraError> {
    v.get(name)
        .and_then(Value::as_str)
        .ok_or(JiraError::ResponseInvalid)
}

pub(crate) fn object_field<'a>(v: &'a Value, name: &str) -> Result<&'a Value, JiraError> {
    match v.get(name) {
        Some(o) if o.is_object() => Ok(o),
        _ => invalid(),
    }
}

pub(crate) fn array_field<'a>(v: &'a Value, name: &str) -> Result<&'a Vec<Value>, JiraError> {
    v.get(name)
        .and_then(Value::as_array)
        .ok_or(JiraError::ResponseInvalid)
}

/// Jira sends `2026-01-10T09:00:00.000+0000`; RFC 3339 is read too (Shared Context).
pub(crate) fn timestamp(raw: &str) -> Result<Timestamp, JiraError> {
    let parsed = DateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f%z")
        .or_else(|_| DateTime::parse_from_rfc3339(raw))
        .map_err(|_| JiraError::ResponseInvalid)?;
    Ok(Timestamp(parsed.with_timezone(&Utc).trunc_subsecs(3)))
}

pub(crate) fn timestamp_field(v: &Value, name: &str) -> Result<Timestamp, JiraError> {
    timestamp(str_field(v, name)?)
}

pub(crate) fn user(v: &Value) -> Result<JiraUser, JiraError> {
    let account_id = str_field(v, "accountId")?;
    let display_name = str_field(v, "displayName")?;
    if !(1..=128).contains(&account_id.chars().count())
        || !(1..=255).contains(&display_name.chars().count())
    {
        return invalid();
    }
    let email = v
        .get("emailAddress")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty())
        .map(str::to_owned);
    Ok(JiraUser {
        account_id: account_id.to_owned(),
        display_name: display_name.to_owned(),
        email,
    })
}

/// A user the person can be handed work: active, and an Atlassian account (not an app).
pub(crate) fn is_real_person(v: &Value) -> bool {
    v.get("active").and_then(Value::as_bool) != Some(false)
        && v.get("accountType")
            .and_then(Value::as_str)
            .is_none_or(|t| t == "atlassian")
}

pub(crate) fn status_category(fields: &Value) -> Result<JiraStatusCategory, JiraError> {
    let key = fields
        .get("status")
        .and_then(|s| s.get("statusCategory"))
        .and_then(|c| c.get("key"))
        .and_then(Value::as_str)
        .ok_or(JiraError::ResponseInvalid)?;
    match key {
        "new" => Ok(JiraStatusCategory::ToDo),
        "indeterminate" => Ok(JiraStatusCategory::InProgress),
        "done" => Ok(JiraStatusCategory::Done),
        _ => invalid(),
    }
}

/// One issue as Jira lists it. `fallback_updated` stands in for the time of a sub-task or linked
/// issue, whose listing on the parent carries none.
pub(crate) fn issue_summary(
    site: &str,
    v: &Value,
    fallback_updated: Option<Timestamp>,
) -> Result<JiraIssueSummary, JiraError> {
    let key = str_field(v, "key")?;
    if !issue_key_ok(key) {
        return invalid();
    }
    let id = str_field(v, "id")?;
    let fields = object_field(v, "fields")?;
    let assignee = match fields.get("assignee") {
        None | Some(Value::Null) => None,
        Some(a) => Some(user(a)?),
    };
    let labels = match fields.get("labels") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|l| {
                l.as_str()
                    .map(str::to_owned)
                    .ok_or(JiraError::ResponseInvalid)
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return invalid(),
    };
    let updated = match fields.get("updated").and_then(Value::as_str) {
        Some(raw) => timestamp(raw)?,
        None => fallback_updated.ok_or(JiraError::ResponseInvalid)?,
    };
    Ok(JiraIssueSummary {
        issue: JiraIssueRef {
            site: site.to_owned(),
            key: key.to_owned(),
        },
        id: id.to_owned(),
        summary: str_field(fields, "summary")?.to_owned(),
        assignee,
        status_category: status_category(fields)?,
        labels,
        updated,
    })
}

pub(crate) fn comment(v: &Value) -> Result<JiraComment, JiraError> {
    let body = v.get("body").ok_or(JiraError::ResponseInvalid)?;
    let (body_markdown, code_blocks) = adf_to_markdown(body)?;
    Ok(JiraComment {
        id: str_field(v, "id")?.to_owned(),
        author: user(v.get("author").ok_or(JiraError::ResponseInvalid)?)?,
        created: timestamp_field(v, "created")?,
        updated: timestamp_field(v, "updated")?,
        body_markdown,
        code_blocks,
    })
}

pub(crate) fn attachment(v: &Value) -> Result<JiraAttachment, JiraError> {
    Ok(JiraAttachment {
        id: str_field(v, "id")?.to_owned(),
        filename: str_field(v, "filename")?.to_owned(),
        mime_type: str_field(v, "mimeType")?.to_owned(),
        size: v
            .get("size")
            .and_then(Value::as_u64)
            .ok_or(JiraError::ResponseInvalid)?,
        created: timestamp_field(v, "created")?,
    })
}

/// `fields.issuelinks`: `outwardIssue` with `type.outward`, `inwardIssue` with `type.inward`.
pub(crate) fn linked(
    site: &str,
    links: &[Value],
    parent_updated: Timestamp,
) -> Result<Vec<JiraLinkedIssue>, JiraError> {
    let mut out = Vec::new();
    for link in links {
        let (issue, wording) = match (link.get("outwardIssue"), link.get("inwardIssue")) {
            (Some(issue), _) => (issue, "outward"),
            (None, Some(issue)) => (issue, "inward"),
            (None, None) => return invalid(),
        };
        let relation = link
            .get("type")
            .map(|t| str_field(t, wording))
            .transpose()?
            .ok_or(JiraError::ResponseInvalid)?;
        out.push(JiraLinkedIssue {
            relation: relation.to_owned(),
            issue: issue_summary(site, issue, Some(parent_updated))?,
        });
    }
    Ok(out)
}

/// C-86: Jira's order with `orderBy=created`, then a stable sort on `(created, id as integer)`.
// spec: ordin-desktop/components/JiraAdapter#C-86
pub(crate) fn sort_comments(comments: &mut [JiraComment]) {
    comments.sort_by_key(|c| (c.created, c.id.parse::<u64>().unwrap_or(u64::MAX)));
}
