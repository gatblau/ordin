//! The operations of the Jira Adapter (JiraAdapter.md Internal Logic 2 to 7).

use std::path::Path;
use std::sync::Arc;

use ordin_identity::SecretString;
use ordin_journal::NewExternalAction;
use ordin_types::domain::runs::ExternalActionKind;
use ordin_types::num::Sha256Hex;
use ordin_types::{CodedError as _, ExternalActionKey};
use reqwest::Method;
use serde_json::{json, Value};

use crate::address::{self, issue_key_ok, project_key_ok};
use crate::adf;
use crate::client::{basic, Credentials, Ctx, Fail, Req, Transport, Write, MAX_ATTACHMENT_BYTES};
use crate::parse;
use crate::query::{self, Built};
use crate::{
    JiraAdapter, JiraAttachment, JiraCodeBlock, JiraComment, JiraCommentBody, JiraDeps,
    JiraDownload, JiraEndpoints, JiraError, JiraIssueContent, JiraIssueRef, JiraIssueSummary,
    JiraProject, JiraProjectRef, JiraQuery, JiraUser,
};

/// `MAX_COMMENTS` per issue read.
const MAX_COMMENTS: usize = 2000;
/// `MAX_BODY_CHARS`: characters of ADF JSON text per comment written.
const MAX_BODY_CHARS: usize = 32_767;
/// `MAX_ASSIGNABLE` per `find_assignable`.
const MAX_ASSIGNABLE: u32 = 20;
/// Comments are read in pages of this size.
const COMMENT_PAGE: usize = 100;

const SUMMARY_FIELDS: &str = "summary,assignee,status,labels,updated";
const SEARCH_FIELDS: [&str; 5] = ["summary", "assignee", "status", "labels", "updated"];

fn account_id_ok(id: &str) -> bool {
    (1..=128).contains(&id.chars().count())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b':' || b == b'_' || b == b'-')
}

impl JiraAdapter {
    pub fn new(deps: JiraDeps) -> Arc<JiraAdapter> {
        Self::from_parts(
            deps.journal,
            deps.identity,
            deps.http,
            JiraEndpoints::default(),
        )
    }

    /// As [`JiraAdapter::new`], reaching Jira at `endpoints`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_endpoints(deps: JiraDeps, endpoints: JiraEndpoints) -> Arc<JiraAdapter> {
        Self::from_parts(deps.journal, deps.identity, deps.http, endpoints)
    }

    pub(crate) fn from_parts(
        journal: Arc<ordin_journal::Journal>,
        credentials: Arc<dyn Credentials>,
        http: reqwest::Client,
        endpoints: JiraEndpoints,
    ) -> Arc<JiraAdapter> {
        Arc::new(JiraAdapter {
            journal,
            transport: Transport::new(credentials, http, endpoints),
        })
    }

    // ---------------------------------------------------------------- addresses (pure)

    pub fn parse_project_address(address: &str) -> Result<JiraProjectRef, JiraError> {
        address::parse_project(address)
    }

    pub fn parse_issue_address(address: &str) -> Result<JiraIssueRef, JiraError> {
        address::parse_issue(address)
    }

    pub fn project_address(project: &JiraProjectRef) -> String {
        address::project_address(project)
    }

    pub fn issue_address(issue: &JiraIssueRef) -> String {
        address::issue_address(issue)
    }

    // ---------------------------------------------------------------- connection

    /// Checks a pasted API token with `GET /myself`, then keeps it through Identity.
    // spec: ordin-desktop/components/JiraAdapter#C-87
    // spec: ordin-desktop/components/JiraAdapter#SA-JIRA-04
    pub async fn connect_with_token(
        &self,
        site: &str,
        email: &str,
        token: SecretString,
    ) -> Result<JiraUser, JiraError> {
        let site = site.to_ascii_lowercase();
        if !address::site_ok(&site) {
            return Err(JiraError::AddressInvalid);
        }
        let email_chars = email.chars().count();
        let token_ok = (1..=1024).contains(&token.expose().len());
        if !(3..=254).contains(&email_chars) || email.matches('@').count() != 1 || !token_ok {
            return Err(JiraError::TokenRefused);
        }
        let ctx = Ctx {
            base: format!("{}/rest/api/3", self.transport.site_base(&site)),
            auth: basic(email, token.expose()),
            credential_id: None,
            oauth: false,
        };
        let checked = self
            .transport
            .exchange(&ctx, &Req::get("connect_token", &["myself"]))
            .await;
        let me = match checked {
            Ok(resp) => resp.json(),
            Err(Fail::Unauthorized) | Err(Fail::Jira(JiraError::AccessDenied)) => {
                Err(JiraError::TokenRefused)
            }
            Err(Fail::Jira(JiraError::NotFound)) => Err(JiraError::AddressInvalid),
            Err(Fail::Jira(e)) => Err(e),
        };
        let me = match me {
            Ok(me) if me.get("accountType").and_then(Value::as_str) == Some("atlassian") => me,
            Ok(_) => {
                tracing::info!(component = "jira", accepted = false, "jira_token_checked");
                return Err(JiraError::TokenRefused);
            }
            Err(e) => {
                tracing::info!(component = "jira", accepted = false, "jira_token_checked");
                return Err(e);
            }
        };
        let person = parse::user(&me)?;
        tracing::info!(component = "jira", accepted = true, "jira_token_checked");
        self.transport
            .credentials
            .save_jira_token(
                &site,
                email,
                token,
                &person.account_id,
                &person.display_name,
            )
            .await
            .map_err(|e| {
                if e.retryable() {
                    JiraError::Unavailable
                } else {
                    JiraError::SignInNeeded
                }
            })?;
        Ok(person)
    }

    pub async fn myself(&self, site: &str) -> Result<JiraUser, JiraError> {
        let resp = self
            .transport
            .call(site, &Req::get("myself", &["myself"]))
            .await?;
        parse::user(&resp.json()?)
    }

    // ---------------------------------------------------------------- reading

    pub async fn project(&self, project: &JiraProjectRef) -> Result<JiraProject, JiraError> {
        if !project_key_ok(&project.key) {
            return Err(JiraError::AddressInvalid);
        }
        let resp = self
            .transport
            .call(
                &project.site,
                &Req::get("project", &["project", &project.key]),
            )
            .await?;
        let v = resp.json()?;
        Ok(JiraProject {
            id: parse::str_field(&v, "id")?.to_owned(),
            key: parse::str_field(&v, "key")?.to_owned(),
            name: parse::str_field(&v, "name")?.to_owned(),
        })
    }

    pub async fn find_assignable(
        &self,
        project: &JiraProjectRef,
        query: &str,
        limit: u32,
    ) -> Result<Vec<JiraUser>, JiraError> {
        let query = query.trim();
        if !(1..=100).contains(&query.chars().count()) || !(1..=MAX_ASSIGNABLE).contains(&limit) {
            return Err(JiraError::QueryInvalid);
        }
        if !project_key_ok(&project.key) {
            return Err(JiraError::AddressInvalid);
        }
        let req = Req::get("find_assignable", &["user", "assignable", "search"])
            .query("project", project.key.clone())
            .query("query", query)
            .query("maxResults", limit.to_string());
        let v = self.transport.call(&project.site, &req).await?.json()?;
        let mut people = Vec::new();
        for entry in v.as_array().ok_or(JiraError::ResponseInvalid)? {
            if parse::is_real_person(entry) {
                people.push(parse::user(entry)?);
            }
        }
        people.truncate(limit as usize);
        Ok(people)
    }

    pub async fn is_assignable(
        &self,
        project: &JiraProjectRef,
        account_id: &str,
    ) -> Result<bool, JiraError> {
        if !account_id_ok(account_id) {
            return Err(JiraError::QueryInvalid);
        }
        if !project_key_ok(&project.key) {
            return Err(JiraError::AddressInvalid);
        }
        let req = Req::get("is_assignable", &["user", "assignable", "search"])
            .query("project", project.key.clone())
            .query("accountId", account_id)
            .query("maxResults", "1");
        let v = self.transport.call(&project.site, &req).await?.json()?;
        Ok(v.as_array()
            .ok_or(JiraError::ResponseInvalid)?
            .iter()
            .any(|u| {
                parse::is_real_person(u)
                    && u.get("accountId").and_then(Value::as_str) == Some(account_id)
            }))
    }

    pub async fn search(
        &self,
        site: &str,
        query: &JiraQuery,
        limit: u32,
    ) -> Result<Vec<JiraIssueSummary>, JiraError> {
        match query::build(query, limit)? {
            Built::Key(key) => {
                let req = Req::get("search", &["issue", &key]).query("fields", SUMMARY_FIELDS);
                match self.transport.call(site, &req).await {
                    Ok(resp) => Ok(vec![parse::issue_summary(site, &resp.json()?, None)?]),
                    Err(JiraError::NotFound) => Ok(Vec::new()),
                    Err(e) => Err(e),
                }
            }
            Built::Jql { jql, max_results } => {
                self.search_jql("search", site, &jql, max_results).await
            }
        }
    }

    /// One `POST /search/jql` (`nextPageToken` is not followed; A-54).
    async fn search_jql(
        &self,
        operation: &'static str,
        site: &str,
        jql: &str,
        max_results: u32,
    ) -> Result<Vec<JiraIssueSummary>, JiraError> {
        let body = json!({"jql": jql, "fields": SEARCH_FIELDS, "maxResults": max_results});
        let req = Req::get(operation, &["search", "jql"]).with_body(Method::POST, body);
        let v = self.transport.call(site, &req).await?.json()?;
        parse::array_field(&v, "issues")?
            .iter()
            .map(|i| parse::issue_summary(site, i, None))
            .collect()
    }

    fn check_issue(issue: &JiraIssueRef) -> Result<(), JiraError> {
        if issue_key_ok(&issue.key) {
            Ok(())
        } else {
            Err(JiraError::AddressInvalid)
        }
    }

    pub async fn issue(&self, issue: &JiraIssueRef) -> Result<JiraIssueContent, JiraError> {
        Self::check_issue(issue)?;
        let fields =
            "summary,description,assignee,status,labels,updated,attachment,subtasks,issuelinks";
        let req = Req::get("issue", &["issue", &issue.key]).query("fields", fields);
        let v = self.transport.call(&issue.site, &req).await?.json()?;
        let summary = parse::issue_summary(&issue.site, &v, None)?;
        let f = parse::object_field(&v, "fields")?;
        let description_markdown = match f.get("description") {
            None | Some(Value::Null) => String::new(),
            Some(adf_doc) => adf::adf_to_markdown(adf_doc)?.0,
        };
        let attachments = list_of(f, "attachment")?
            .iter()
            .map(parse::attachment)
            .collect::<Result<Vec<_>, _>>()?;
        let subtasks = list_of(f, "subtasks")?
            .iter()
            .map(|s| parse::issue_summary(&issue.site, s, Some(summary.updated)))
            .collect::<Result<Vec<_>, _>>()?;
        let linked = parse::linked(&issue.site, list_of(f, "issuelinks")?, summary.updated)?;
        let comments = self.comments(issue).await?;
        Ok(JiraIssueContent {
            summary,
            description_markdown,
            comments,
            attachments,
            subtasks,
            linked,
        })
    }

    pub async fn comments(&self, issue: &JiraIssueRef) -> Result<Vec<JiraComment>, JiraError> {
        Self::check_issue(issue)?;
        let mut comments: Vec<JiraComment> = Vec::new();
        let mut start_at = 0usize;
        loop {
            let req = Req::get("comments", &["issue", &issue.key, "comment"])
                .query("startAt", start_at.to_string())
                .query("maxResults", COMMENT_PAGE.to_string())
                .query("orderBy", "created");
            let v = self.transport.call(&issue.site, &req).await?.json()?;
            let total = v
                .get("total")
                .and_then(Value::as_u64)
                .ok_or(JiraError::ResponseInvalid)? as usize;
            if total > MAX_COMMENTS {
                return Err(JiraError::TooLarge);
            }
            let page = parse::array_field(&v, "comments")?;
            for c in page {
                comments.push(parse::comment(c)?);
            }
            start_at += page.len().max(1);
            if page.is_empty() || start_at >= total {
                break;
            }
        }
        parse::sort_comments(&mut comments);
        Ok(comments)
    }

    /// Streams the attachment to `<dest_path>.part`, then renames it to `dest_path`.
    pub async fn download_attachment(
        &self,
        issue: &JiraIssueRef,
        attachment_id: &str,
        dest_path: &Path,
    ) -> Result<JiraDownload, JiraError> {
        Self::check_issue(issue)?;
        let req =
            Req::get("download_attachment", &["issue", &issue.key]).query("fields", "attachment");
        let v = self.transport.call(&issue.site, &req).await?.json()?;
        let found: Option<JiraAttachment> =
            list_of(parse::object_field(&v, "fields")?, "attachment")?
                .iter()
                .map(parse::attachment)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .find(|a| a.id == attachment_id);
        let attachment = found.ok_or(JiraError::NotFound)?;
        if attachment.size > MAX_ATTACHMENT_BYTES {
            return Err(JiraError::TooLarge);
        }
        let mut part = dest_path.as_os_str().to_owned();
        part.push(".part");
        let part = std::path::PathBuf::from(part);
        let mut req = Req::get(
            "download_attachment",
            &["attachment", "content", &attachment.id],
        );
        req.download = Some(part.clone());
        let outcome = self.transport.call(&issue.site, &req).await;
        let done = match outcome {
            Ok(resp) => resp.downloaded.ok_or(JiraError::ResponseInvalid),
            Err(e) => Err(e),
        };
        let (bytes_written, sha256) = match done {
            Ok(done) => done,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e);
            }
        };
        if tokio::fs::rename(&part, dest_path).await.is_err() {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(JiraError::Unavailable);
        }
        Ok(JiraDownload {
            bytes_written,
            sha256: Sha256Hex(sha256),
        })
    }

    // ---------------------------------------------------------------- external actions (Internal Logic 7)

    /// The credential and the site check, before anything is journalled or sent.
    async fn ready(&self, site: &str) -> Result<(), JiraError> {
        self.transport.access(site).await.map(|_| ())
    }

    // spec: ordin-desktop/components/JiraAdapter#SA-JIRA-01
    // spec: ordin-desktop/components/JiraAdapter#SA-JIRA-02
    async fn begin(&self, key: ExternalActionKey, destination: String) -> Result<(), JiraError> {
        let new = NewExternalAction {
            action_key: key,
            run_id: None,
            kind: ExternalActionKind::HandoverWritten,
            destination,
        };
        match self.journal.begin_external_action(new).await {
            Ok(_) => Ok(()),
            Err(e) if e.code() == "EXT_KEY_DUPLICATE" => Err(JiraError::ActionKeyUsed),
            Err(_) => Err(JiraError::Unavailable),
        }
    }

    async fn confirm(&self, key: ExternalActionKey, operation: &'static str) {
        match self.journal.confirm_external_action(key).await {
            Ok(_) => tracing::info!(component = "jira", operation, "jira_written"),
            Err(_) => tracing::warn!(
                component = "jira",
                action_key = %key,
                "jira_confirm_failed"
            ),
        }
    }

    fn issue_destination(issue: &JiraIssueRef) -> String {
        format!("Jira \u{203a} {} \u{203a} {}", issue.site, issue.key)
    }

    // ---------------------------------------------------------------- writes (SA-JIRA-01)

    pub async fn create_issue(
        &self,
        project: &JiraProjectRef,
        summary: &str,
        key: ExternalActionKey,
    ) -> Result<JiraIssueRef, JiraError> {
        let summary = summary.trim();
        if !(1..=255).contains(&summary.chars().count()) || summary.contains(['\n', '\r']) {
            return Err(JiraError::QueryInvalid);
        }
        if !project_key_ok(&project.key) {
            return Err(JiraError::AddressInvalid);
        }
        self.ready(&project.site).await?;
        let type_id = self.issue_type_id(project).await?;
        self.begin(
            key,
            format!("Jira \u{203a} {} \u{203a} {}", project.site, project.key),
        )
        .await?;
        let body = json!({"fields": {
            "project": {"key": project.key},
            "summary": summary,
            "issuetype": {"id": type_id},
        }});
        let req = Req::get("create_issue", &["issue"])
            .with_body(Method::POST, body)
            .write(Write::NoResend);
        let made = match self.transport.call(&project.site, &req).await {
            Ok(resp) => {
                self.confirm(key, "create_issue").await;
                let created = parse::str_field(&resp.json()?, "key")?.to_owned();
                if !issue_key_ok(&created) {
                    return Err(JiraError::ResponseInvalid);
                }
                created
            }
            Err(JiraError::OutcomeUnknown) => {
                // Internal Logic 5.4: look for the issue this call may have made.
                match self.find_own_issue(project, summary).await {
                    Some(found) => {
                        self.confirm(key, "create_issue").await;
                        found
                    }
                    None => return Err(JiraError::OutcomeUnknown),
                }
            }
            Err(e) => return Err(e),
        };
        Ok(JiraIssueRef {
            site: project.site.clone(),
            key: made,
        })
    }

    /// Internal Logic 5.2: not a sub-task; `Task` when offered, else the first in Jira's order.
    async fn issue_type_id(&self, project: &JiraProjectRef) -> Result<String, JiraError> {
        let req = Req::get(
            "create_issue_types",
            &["issue", "createmeta", &project.key, "issuetypes"],
        )
        .query("maxResults", "50");
        let v = self.transport.call(&project.site, &req).await?.json()?;
        let mut usable = Vec::new();
        for t in parse::array_field(&v, "issueTypes")? {
            let subtask = t.get("subtask").and_then(Value::as_bool).unwrap_or(false);
            if !subtask {
                usable.push((
                    parse::str_field(t, "id")?.to_owned(),
                    parse::str_field(t, "name")?.to_owned(),
                ));
            }
        }
        usable
            .iter()
            .find(|(_, name)| name.eq_ignore_ascii_case("Task"))
            .or_else(|| usable.first())
            .map(|(id, _)| id.clone())
            .ok_or(JiraError::IssueTypeNone)
    }

    /// Exactly one issue whose summary equals `summary` is this call's own.
    async fn find_own_issue(&self, project: &JiraProjectRef, summary: &str) -> Option<String> {
        let jql = query::own_created(&project.key, summary).ok()?;
        let found = self
            .search_jql("create_issue_lookup", &project.site, &jql, 2)
            .await
            .ok()?;
        match found.as_slice() {
            [one] if one.summary == summary => Some(one.issue.key.clone()),
            _ => None,
        }
    }

    pub async fn add_label(
        &self,
        issue: &JiraIssueRef,
        label: &str,
        key: ExternalActionKey,
    ) -> Result<(), JiraError> {
        Self::check_issue(issue)?;
        if !query::label_ok(label) {
            return Err(JiraError::QueryInvalid);
        }
        self.ready(&issue.site).await?;
        self.begin(key, Self::issue_destination(issue)).await?;
        let body = json!({"update": {"labels": [{"add": label}]}});
        let req = Req::get("add_label", &["issue", &issue.key])
            .with_body(Method::PUT, body)
            .write(Write::Repeat);
        self.transport.call(&issue.site, &req).await?;
        self.confirm(key, "add_label").await;
        Ok(())
    }

    pub async fn add_comment(
        &self,
        issue: &JiraIssueRef,
        body: &JiraCommentBody,
        key: ExternalActionKey,
    ) -> Result<JiraComment, JiraError> {
        Self::check_issue(issue)?;
        let document = adf::markdown_to_adf(&body.markdown, body.code_block.as_ref())?;
        let serialised =
            serde_json::to_string(&document).map_err(|_| JiraError::ResponseInvalid)?;
        if serialised.chars().count() > MAX_BODY_CHARS {
            return Err(JiraError::TooLarge);
        }
        self.ready(&issue.site).await?;
        self.begin(key, Self::issue_destination(issue)).await?;
        let req = Req::get("add_comment", &["issue", &issue.key, "comment"])
            .with_body(Method::POST, json!({"body": document}))
            .write(Write::NoResend);
        let resp = self.transport.call(&issue.site, &req).await?;
        self.confirm(key, "add_comment").await;
        parse::comment(&resp.json()?)
    }

    pub async fn assign(
        &self,
        issue: &JiraIssueRef,
        account_id: &str,
        key: ExternalActionKey,
    ) -> Result<(), JiraError> {
        Self::check_issue(issue)?;
        if !account_id_ok(account_id) {
            return Err(JiraError::QueryInvalid);
        }
        self.ready(&issue.site).await?;
        self.begin(key, Self::issue_destination(issue)).await?;
        let req = Req::get("assign", &["issue", &issue.key, "assignee"])
            .with_body(Method::PUT, json!({"accountId": account_id}))
            .write(Write::Repeat);
        self.transport.call(&issue.site, &req).await?;
        self.confirm(key, "assign").await;
        Ok(())
    }

    // ---------------------------------------------------------------- conversions (pure)

    pub fn markdown_to_adf(
        markdown: &str,
        code_block: Option<&JiraCodeBlock>,
    ) -> Result<Value, JiraError> {
        adf::markdown_to_adf(markdown, code_block)
    }

    pub fn adf_to_markdown(adf: &Value) -> Result<(String, Vec<JiraCodeBlock>), JiraError> {
        adf::adf_to_markdown(adf)
    }
}

/// A list under `fields`, empty when Jira left it out.
fn list_of<'a>(fields: &'a Value, name: &str) -> Result<&'a [Value], JiraError> {
    match fields.get(name) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) => Ok(items.as_slice()),
        Some(_) => Err(JiraError::ResponseInvalid),
    }
}
