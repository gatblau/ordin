//! Jira Cloud replies, written from the spec's examples. The stand-in serves these and nothing else.

use serde_json::{json, Value};

pub const SITE: &str = "acme.atlassian.net";
pub const EMAIL: &str = "priya.shah@acme.co.uk";
/// A made-up token; the log tests search every log line for it.
pub const TOKEN: &str = "ATATT3xSecretTokenValue0123456789";
pub const ACCOUNT_SAM: &str = "5b10ac8d82e05b22cc7d4ef5";
pub const ACCOUNT_PRIYA: &str = "712020:7d2a-priya";
pub const JIRA_TIME: &str = "2026-01-10T09:00:00.000+0000";

/// A person as Jira lists them. `emailAddress` is left out: the privacy setting hides it (C-88).
pub fn user(account_id: &str, display_name: &str) -> Value {
    json!({"accountId": account_id, "displayName": display_name, "active": true, "accountType": "atlassian"})
}

pub fn myself() -> Value {
    json!({"accountId": ACCOUNT_PRIYA, "displayName": "Priya Shah",
           "emailAddress": EMAIL, "accountType": "atlassian"})
}

/// A one-paragraph ADF document.
pub fn adf(text: &str) -> Value {
    json!({"version": 1, "type": "doc",
           "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}]})
}

pub fn fields(
    summary: &str,
    category: &str,
    assignee: Option<Value>,
    labels: &[&str],
    updated: &str,
) -> Value {
    json!({
        "summary": summary,
        "assignee": assignee,
        "status": {"name": "Open", "statusCategory": {"key": category}},
        "labels": labels,
        "updated": updated,
    })
}

pub fn issue(id: &str, key: &str, summary: &str) -> Value {
    json!({"id": id, "key": key,
           "fields": fields(summary, "indeterminate", Some(user(ACCOUNT_SAM, "Sam Lee")),
                            &["ordin-handover"], JIRA_TIME)})
}

pub fn search_reply(issues: Vec<Value>) -> Value {
    json!({"issues": issues})
}

/// ABC-12: a description, 2 attachments, 1 sub-task and 1 "blocks" link.
pub fn abc12() -> Value {
    json!({
        "id": "10012",
        "key": "ABC-12",
        "fields": {
            "summary": "Northwind assessment",
            "description": adf("Assess the Northwind account."),
            "assignee": user(ACCOUNT_SAM, "Sam Lee"),
            "status": {"name": "In Progress", "statusCategory": {"key": "indeterminate"}},
            "labels": ["ordin-handover"],
            "updated": "2026-01-11T10:30:00.000+0000",
            "attachment": [
                {"id": "10001", "filename": "terms.pdf", "mimeType": "application/pdf",
                 "size": 2048, "created": JIRA_TIME},
                {"id": "10002", "filename": "notes.txt", "mimeType": "text/plain",
                 "size": 12, "created": JIRA_TIME}
            ],
            "subtasks": [
                {"id": "10013", "key": "ABC-13",
                 "fields": {"summary": "Collect references",
                            "status": {"statusCategory": {"key": "new"}}}}
            ],
            "issuelinks": [
                {"type": {"name": "Blocks", "inward": "is blocked by", "outward": "blocks"},
                 "outwardIssue": {"id": "10020", "key": "ABC-20",
                                  "fields": {"summary": "Sign the order",
                                             "status": {"statusCategory": {"key": "new"}}}}}
            ]
        }
    })
}

pub fn comment(id: &str, created: &str, text: &str) -> Value {
    json!({"id": id, "author": user(ACCOUNT_SAM, "Sam Lee"),
           "created": created, "updated": created, "body": adf(text)})
}

pub fn comments_page(start_at: usize, total: usize, comments: Vec<Value>) -> Value {
    json!({"startAt": start_at, "maxResults": 100, "total": total, "comments": comments})
}

/// Project ABC's issue types as `createmeta` lists them.
pub fn issue_types(types: &[(&str, &str, bool)]) -> Value {
    let list: Vec<Value> = types
        .iter()
        .map(|(id, name, subtask)| json!({"id": id, "name": name, "subtask": subtask}))
        .collect();
    json!({"startAt": 0, "maxResults": 50, "total": list.len(), "issueTypes": list})
}

/// 1 attachment list for the download tests.
pub fn attachment_issue(size: u64) -> Value {
    json!({"id": "10012", "key": "ABC-12", "fields": {"attachment": [
        {"id": "10001", "filename": "terms.pdf", "mimeType": "application/pdf",
         "size": size, "created": JIRA_TIME}]}})
}
