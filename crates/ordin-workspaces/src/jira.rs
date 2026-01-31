//! Jira issue links (WorkspaceService.md amendment ADR-106, §J): one issue becomes read-only
//! working-copy items; a newer version at Jira is shown and never overwrites what is local until the
//! person chooses **Bring in the new version**. This service never writes to Jira.

use std::collections::{HashMap, HashSet};

use ordin_jira::{JiraAdapter, JiraComment, JiraIssueContent, JiraIssueRef, JiraStatusCategory};
use ordin_types::domain::places::{SourceLink, SourceLinkKind, WorkingCopyItem, WorkingCopyState};
use ordin_types::time::Timestamp;
use ordin_types::{PlaceId, SourceLinkId};
use sqlx::Row;

use crate::error::{file_failed, WorkspaceError};
use crate::links::{check_display_name, cut, map_adapter};
use crate::log;
use crate::remote::{Fetched, ItemRef};
use crate::service::WorkspaceService;
use crate::store;
use crate::types::{OriginChanged, OriginVersion};
use crate::{JIRA_ATTACHMENT_MAX_BYTES, LINK_READ_BUDGET};

const RECORD_MARKER: &str = "# ordin.handover/v1 record";
const EVENT_MARKER: &str = "# ordin.handover/v1 event";

pub(crate) enum PartBody {
    Text(String),
    Attachment { id: String, size: u64 },
}

/// One item of an issue, as §J.2 lays them out.
pub(crate) struct JiraPart {
    pub origin_path: String,
    pub version: String,
    pub changed_by: Option<String>,
    pub changed_at: Timestamp,
    pub body: PartBody,
}

fn status_text(category: JiraStatusCategory) -> &'static str {
    match category {
        JiraStatusCategory::ToDo => "To Do",
        JiraStatusCategory::InProgress => "In Progress",
        JiraStatusCategory::Done => "Done",
    }
}

/// The first line of a code block, trimmed.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default().trim()
}

fn has_marker(comment: &JiraComment, marker: &str) -> bool {
    comment
        .code_blocks
        .iter()
        .any(|b| first_line(&b.text) == marker)
}

/// Removes every fenced block whose first line is the handover record marker (the note stays).
pub(crate) fn strip_record_block(markdown: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut lines = markdown.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("```") {
            let mut block = vec![line];
            for inner in lines.by_ref() {
                block.push(inner);
                if inner.trim_start().starts_with("```") {
                    break;
                }
            }
            if block.get(1).map(|l| l.trim()) == Some(RECORD_MARKER) {
                continue;
            }
            out.extend(block);
        } else {
            out.push(line);
        }
    }
    out.join("\n").trim_end().to_owned()
}

fn clean_name(name: &str) -> String {
    name.chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
        .collect()
}

/// The items an issue yields, in §J.2's order.
pub(crate) fn jira_parts(issue: &JiraIssueRef, content: &JiraIssueContent) -> Vec<JiraPart> {
    let summary = &content.summary;
    let assignee = summary.assignee.as_ref().map(|u| u.display_name.clone());
    let issue_version = format!("issue:{}", summary.updated);
    let mut parts = Vec::new();
    let description = format!(
        "# {} {}\nStatus: {} · Assignee: {}\n\n{}",
        issue.key,
        summary.summary,
        status_text(summary.status_category),
        assignee.as_deref().unwrap_or("nobody"),
        content.description_markdown
    );
    parts.push(JiraPart {
        origin_path: "summary-and-description.md".to_owned(),
        version: issue_version.clone(),
        changed_by: assignee,
        changed_at: summary.updated,
        body: PartBody::Text(description),
    });
    for comment in &content.comments {
        if has_marker(comment, EVENT_MARKER) {
            continue;
        }
        let body = if has_marker(comment, RECORD_MARKER) {
            strip_record_block(&comment.body_markdown)
        } else {
            comment.body_markdown.clone()
        };
        parts.push(JiraPart {
            origin_path: format!(
                "comments/{}-{}.md",
                comment.created.0.format("%Y%m%dT%H%M%SZ"),
                clean_name(&comment.id)
            ),
            version: format!("comment:{}", comment.updated),
            changed_by: Some(comment.author.display_name.clone()),
            changed_at: comment.updated,
            body: PartBody::Text(format!(
                "**{}** · {}\n\n{}",
                comment.author.display_name, comment.created, body
            )),
        });
    }
    for attachment in &content.attachments {
        parts.push(JiraPart {
            origin_path: format!(
                "attachments/{}-{}",
                attachment.id,
                clean_name(&attachment.filename)
            ),
            version: format!("attachment:{}", attachment.id),
            changed_by: None,
            changed_at: attachment.created,
            body: PartBody::Attachment {
                id: attachment.id.clone(),
                size: attachment.size,
            },
        });
    }
    let mut related = String::from("## Sub-tasks\n");
    if content.subtasks.is_empty() {
        related.push_str("None\n");
    }
    for subtask in &content.subtasks {
        related.push_str(&format!(
            "{} {} — {}\n",
            subtask.issue.key,
            subtask.summary,
            status_text(subtask.status_category)
        ));
    }
    related.push_str("\n## Linked issues\n");
    if content.linked.is_empty() {
        related.push_str("None\n");
    }
    for linked in &content.linked {
        related.push_str(&format!(
            "{}: {} {} — {}\n",
            linked.relation,
            linked.issue.issue.key,
            linked.issue.summary,
            status_text(linked.issue.status_category)
        ));
    }
    parts.push(JiraPart {
        origin_path: "related.md".to_owned(),
        version: issue_version,
        changed_by: None,
        changed_at: summary.updated,
        body: PartBody::Text(related),
    });
    parts
}

impl WorkspaceService {
    fn jira_adapter(&self) -> Result<&std::sync::Arc<JiraAdapter>, WorkspaceError> {
        self.jira.as_ref().ok_or(WorkspaceError::JiraUnavailable)
    }

    fn jira_issue_of(link: &SourceLink) -> Result<JiraIssueRef, WorkspaceError> {
        JiraAdapter::parse_issue_address(&link.origin_location)
            .map_err(|_| WorkspaceError::JiraAddressInvalid)
    }

    /// §J.2: the issue is read once at confirmation and its items are written into the working copy.
    pub(crate) async fn jira_first_read(&self, link: &SourceLink) -> Result<(), WorkspaceError> {
        let started = std::time::Instant::now();
        let jira = self.jira_adapter()?;
        let issue = Self::jira_issue_of(link)?;
        let content = jira.issue(&issue).await.map_err(|e| map_adapter(&e))?;
        let place = link
            .place_id
            .ok_or(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"))?;
        let parts = jira_parts(&issue, &content);
        self.set_progress(link.id, |p| {
            *p = crate::types::LinkProgress {
                source_link_id: link.id,
                listed: parts.len() as u64,
                read: 0,
                not_read: 0,
                done: false,
            }
        });
        let mut used: u64 = 0;
        for part in &parts {
            let size = match &part.body {
                PartBody::Text(text) => text.len() as u64,
                PartBody::Attachment { size, .. } => *size,
            };
            let too_big = matches!(part.body, PartBody::Attachment { .. })
                && (size > JIRA_ATTACHMENT_MAX_BYTES
                    || used.saturating_add(size) > LINK_READ_BUDGET);
            if too_big {
                self.keep_part_unread(link.id, part, size).await;
                self.set_progress(link.id, |p| p.not_read += 1);
                continue;
            }
            match self.write_part(link, place, &issue, part).await {
                Ok(written) => {
                    used += written;
                    self.set_progress(link.id, |p| p.read += 1);
                }
                Err(error) => {
                    log::link_item_read_failed(link.id, error.code());
                    if matches!(part.body, PartBody::Attachment { .. }) {
                        self.keep_part_unread(link.id, part, size).await;
                    }
                    self.set_progress(link.id, |p| p.not_read += 1);
                }
            }
        }
        let name = check_display_name(&cut(
            &format!("{} {}", issue.key, content.summary.summary),
            120,
        ))
        .unwrap_or_else(|_| link.display_name.clone());
        let now = Timestamp::now();
        sqlx::query("UPDATE source_links SET display_name = ?2, last_read_at = ?3 WHERE id = ?1")
            .bind(link.id.to_string())
            .bind(&name)
            .bind(now.to_wire())
            .execute(&self.pool)
            .await?;
        let (read, not_read) = crate::service::lock(&self.progress)
            .get(&link.id)
            .map_or((0, 0), |p| (p.read, p.not_read));
        self.record(
            "source_link_read",
            Some(link.workspace_id),
            "A linked location was read into the workspace.",
            serde_json::json!({ "source_link_id": link.id, "read": read, "not_read": not_read }),
        )
        .await;
        log::source_link_read(
            link.id,
            read,
            not_read,
            started.elapsed().as_millis() as u64,
        );
        self.set_progress(link.id, |p| p.done = true);
        Ok(())
    }

    async fn keep_part_unread(&self, link: SourceLinkId, part: &JiraPart, size: u64) {
        if let PartBody::Attachment { id, .. } = &part.body {
            let _ = sqlx::query(
                "INSERT OR REPLACE INTO working_copy_unread (source_link_id, origin_path, item_ref, size_bytes) VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(link.to_string())
            .bind(&part.origin_path)
            .bind(ItemRef::JiraAttachment(id.clone()).to_json())
            .bind(i64::try_from(size).unwrap_or(i64::MAX))
            .execute(&self.pool)
            .await;
        }
    }

    /// Writes one part into the working copy and records it as an item in state `read`.
    async fn write_part(
        &self,
        link: &SourceLink,
        place: PlaceId,
        issue: &JiraIssueRef,
        part: &JiraPart,
    ) -> Result<u64, WorkspaceError> {
        let bytes = self.part_bytes(issue, part).await?;
        let length = bytes.len() as u64;
        store::check_item(&part.origin_path, &part.version)?;
        self.files
            .store_fetched(
                place,
                &part.origin_path,
                bytes,
                part.changed_by.clone(),
                part.changed_at,
            )
            .await
            .map_err(file_failed)?;
        match store::insert_item(
            &self.pool,
            link.id,
            &part.origin_path,
            &part.version,
            1,
            WorkingCopyState::Read,
            None,
        )
        .await
        {
            Ok(_) => {
                store::set_item_size(&self.pool, link.id, &part.origin_path, length).await;
                Ok(length)
            }
            Err(WorkspaceError::Invalid("WCI_DUPLICATE")) => Ok(length),
            Err(error) => Err(error),
        }
    }

    async fn part_bytes(
        &self,
        issue: &JiraIssueRef,
        part: &JiraPart,
    ) -> Result<Vec<u8>, WorkspaceError> {
        match &part.body {
            PartBody::Text(text) => Ok(text.clone().into_bytes()),
            PartBody::Attachment { id, .. } => self.download_attachment(issue, id).await,
        }
    }

    /// `jira.download_attachment` into `JIRA_DOWNLOAD_DIR`, read whole, and the file deleted whether
    /// or not the read succeeded (§J, Constants).
    async fn download_attachment(
        &self,
        issue: &JiraIssueRef,
        attachment_id: &str,
    ) -> Result<Vec<u8>, WorkspaceError> {
        let jira = self.jira_adapter()?;
        let dir = self.data_root.join("tmp").join("jira");
        let made = dir.clone();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(made))
            .await
            .map_err(|_| WorkspaceError::FilesFailed)?
            .map_err(|_| WorkspaceError::FilesFailed)?;
        let dest = dir.join(uuid::Uuid::now_v7().hyphenated().to_string());
        let downloaded = jira.download_attachment(issue, attachment_id, &dest).await;
        let read = match downloaded {
            Ok(_) => {
                let path = dest.clone();
                tokio::task::spawn_blocking(move || std::fs::read(path))
                    .await
                    .map_err(|_| WorkspaceError::FilesFailed)
                    .and_then(|r| r.map_err(|_| WorkspaceError::FilesFailed))
            }
            Err(error) => Err(map_adapter(&error)),
        };
        let gone = dest.clone();
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(gone)).await;
        read
    }

    /// An unread Jira attachment read on first use: the issue is read once more for its creation time.
    pub(crate) async fn jira_fetch_attachment(
        &self,
        link: &SourceLink,
        attachment_id: &str,
    ) -> Result<Fetched, WorkspaceError> {
        let jira = self.jira_adapter()?;
        let issue = Self::jira_issue_of(link)?;
        let content = jira.issue(&issue).await.map_err(|e| map_adapter(&e))?;
        let attachment = content
            .attachments
            .iter()
            .find(|a| a.id == attachment_id)
            .ok_or(WorkspaceError::SourceNotFound)?;
        if attachment.size > JIRA_ATTACHMENT_MAX_BYTES {
            return Err(WorkspaceError::ItemTooLarge);
        }
        let bytes = self.download_attachment(&issue, attachment_id).await?;
        Ok(Fetched {
            bytes,
            version: format!("attachment:{attachment_id}"),
            changed_by: None,
            changed_at: attachment.created,
        })
    }

    /// §J.4: one `jira.issue` per link per origin check. Returns how many items changed or arrived.
    pub(crate) async fn jira_check(&self, link: &SourceLink) -> Result<u32, WorkspaceError> {
        let jira = self.jira_adapter()?;
        let issue = Self::jira_issue_of(link)?;
        let content = jira.issue(&issue).await.map_err(|e| map_adapter(&e))?;
        let parts = jira_parts(&issue, &content);
        let place = link
            .place_id
            .ok_or(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"))?;
        let rows = sqlx::query("SELECT *, origin_changed_at AS changed_stamp FROM working_copy_items WHERE source_link_id = ?1")
            .bind(link.id.to_string())
            .fetch_all(&self.pool)
            .await?;
        let mut stored: HashMap<String, (WorkingCopyItem, Option<Timestamp>)> = HashMap::new();
        for row in &rows {
            let item = store::item_row(row)?;
            let stamp = store::ts_opt(row, "changed_stamp")?;
            stored.insert(item.origin_path.clone(), (item, stamp));
        }
        let unread: HashSet<String> =
            sqlx::query("SELECT origin_path FROM working_copy_unread WHERE source_link_id = ?1")
                .bind(link.id.to_string())
                .fetch_all(&self.pool)
                .await?
                .iter()
                .filter_map(|r| r.try_get::<String, _>("origin_path").ok())
                .collect();
        let mut changes = 0;
        let mut current: HashSet<&str> = HashSet::new();
        for part in &parts {
            current.insert(part.origin_path.as_str());
            match stored.get(&part.origin_path) {
                Some((item, stamp)) => {
                    let already_shown =
                        item.origin_newer_at.is_some() && *stamp == Some(part.changed_at);
                    if item.origin_version_read != part.version && !already_shown {
                        sqlx::query(
                            "UPDATE working_copy_items SET origin_newer_at = ?2, origin_changed_by = ?3, origin_changed_at = ?4 WHERE id = ?1",
                        )
                        .bind(item.id.to_string())
                        .bind(Timestamp::now().to_wire())
                        .bind(part.changed_by.as_deref())
                        .bind(part.changed_at.to_wire())
                        .execute(&self.pool)
                        .await?;
                        let _ = self.origin_tx.send(OriginChanged {
                            workspace_id: link.workspace_id,
                            source_link_id: link.id,
                            link_kind: SourceLinkKind::JiraIssue,
                            origin_path: part.origin_path.clone(),
                            version: OriginVersion {
                                version: part.version.clone(),
                                changed_by: part.changed_by.clone(),
                                changed_at: Some(part.changed_at),
                            },
                            removed: false,
                        });
                        changes += 1;
                    }
                }
                None if unread.contains(&part.origin_path) => {}
                None => {
                    if self.write_part(link, place, &issue, part).await.is_ok() {
                        changes += 1;
                    } else {
                        log::link_item_read_failed(link.id, "WS_FILES_FAILED");
                    }
                }
            }
        }
        for (path, (item, _)) in &stored {
            if current.contains(path.as_str()) {
                continue;
            }
            let first_time = crate::service::lock(&self.reported_removed).insert(item.id);
            if first_time {
                let _ = self.origin_tx.send(OriginChanged {
                    workspace_id: link.workspace_id,
                    source_link_id: link.id,
                    link_kind: SourceLinkKind::JiraIssue,
                    origin_path: path.clone(),
                    version: OriginVersion {
                        version: String::new(),
                        changed_by: None,
                        changed_at: None,
                    },
                    removed: true,
                });
            }
        }
        Ok(changes)
    }

    /// §J.4: re-reads one item with a newer version, writes it, clears `origin_newer_at`.
    pub(crate) async fn jira_bring_up_to_date(
        &self,
        link: &SourceLink,
        item: &WorkingCopyItem,
    ) -> Result<WorkingCopyItem, WorkspaceError> {
        let jira = self.jira_adapter()?;
        let issue = Self::jira_issue_of(link)?;
        let content = jira.issue(&issue).await.map_err(|e| map_adapter(&e))?;
        let place = link
            .place_id
            .ok_or(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"))?;
        let parts = jira_parts(&issue, &content);
        let part = parts
            .iter()
            .find(|p| p.origin_path == item.origin_path)
            .ok_or(WorkspaceError::WorkingCopyItemNotFound)?;
        let bytes = self.part_bytes(&issue, part).await?;
        store::check_item(&part.origin_path, &part.version)?;
        self.files
            .store_fetched(
                place,
                &part.origin_path,
                bytes,
                part.changed_by.clone(),
                part.changed_at,
            )
            .await
            .map_err(file_failed)?;
        sqlx::query("UPDATE working_copy_items SET origin_version_read = ?2, origin_newer_at = NULL, read_at = ?3 WHERE id = ?1")
            .bind(item.id.to_string())
            .bind(&part.version)
            .bind(Timestamp::now().to_wire())
            .execute(&self.pool)
            .await?;
        self.record(
            "jira_item_brought_up_to_date",
            Some(link.workspace_id),
            "A Jira item was brought up to date.",
            serde_json::json!({ "source_link_id": link.id }),
        )
        .await;
        log::jira_item_brought_up_to_date(link.id);
        store::load_item(&self.pool, item.id).await
    }
}
