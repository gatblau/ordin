//! Linked folders (ConfluenceAdapter.md Internal Logic 4, 5, 6 and 7): asking to link a folder, comparing
//! it with its published copies, and publishing what the person checked. State is derived each time from
//! the File Service's published-copy records and file versions.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use ordin_approval::ApprovalRequest;
use ordin_files::{FileEntry, FolderSort, PublishedCopy};
use ordin_journal::{EventSink, JournalEvent, NewJournalEntry};
use ordin_types::domain::places::{
    ConfluenceLink, ConfluenceLinkState, CopyDestination, CopyFormat, CopyMaker, CopyOriginal,
    FileVersion, FileVersionOrigin, Place, PublishedCopyState, PublishedVia, Workspace,
};
use ordin_types::domain::runs::ApprovalState;
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{
    AffectedItem, ApprovalAsker, ApprovalContent, ContentRef, RejectStyle,
};
use ordin_types::wire::events::EventKind;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{ApprovalId, ConfluenceLinkId, ExternalActionKey, PlaceId, PublishedCopyId};
use serde::Deserialize;

use crate::error::ConfluenceError;
use crate::push::Bracket;
use crate::store::{self, ApprovedItem, PublishRequest};
use crate::types::{
    AutomaticPublishing, ConfluenceLocation, LinkSyncReport, ReadyItem, ReadyKind, RemoteItemRef,
    RemoteVersion,
};
use crate::ConfluenceAdapter;

/// `md`, `markdown`, `txt` become pages (Constants).
const TEXT_KINDS: [&str; 3] = ["md", "markdown", "txt"];
/// A link card names at most 50 files; a publish card holds at most 50 items.
const MAX_NAMED: usize = 50;
/// `question` is 1–120 characters (`APPR_*`).
const MAX_QUESTION_CHARS: usize = 120;
/// `where` is 1–200 characters.
const MAX_WHERE_CHARS: usize = 200;
/// The words of step 7.6.
const OUTCOME_UNKNOWN_REASON: &str = "ordin isn't sure whether Confluence received this change.";
/// The words of step 6.2 for a first push: there is no earlier version for the person to fall back on.
const CREATE_UNKNOWN_REASON: &str = "ordin isn't sure whether Confluence received this. \
     The page or attachment may already be live, so check Confluence before trying again.";
/// `events_since` per read after a lag.
const LAG_BATCH: u32 = 500;
const LINK_CHANGES: &str =
    "Text documents become pages. Other files are attached to the folder's page as they are.";

fn by_agent(version: &FileVersion) -> bool {
    matches!(
        version.origin,
        FileVersionOrigin::Run { .. } | FileVersionOrigin::ArtCraftEdit { .. }
    )
}

fn files_failed<E>(_: E) -> ConfluenceError {
    ConfluenceError::FilesFailed
}

fn approval_failed<E>(_: E) -> ConfluenceError {
    ConfluenceError::ApprovalFailed
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn name_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

fn stem_of(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn kind_of(name: &str) -> String {
    name.rsplit_once('.')
        .map_or(String::new(), |(_, ext)| ext.to_ascii_lowercase())
}

/// The link's folder as a prefix of place-relative paths: `""` or `"Supplier quotes/"`.
fn link_prefix(folder: &str) -> String {
    let trimmed = folder.trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

fn folder_name(folder: &str, place: &str) -> String {
    let trimmed = folder.trim_matches('/');
    if trimmed.is_empty() {
        place.to_owned()
    } else {
        name_of(trimmed).to_owned()
    }
}

fn place_path(copy: &PublishedCopy) -> Option<(PlaceId, &str)> {
    match &copy.original {
        CopyOriginal::PlaceFile { place_id, path } => Some((*place_id, path.as_str())),
        CopyOriginal::KnowledgePage { .. } => None,
    }
}

fn newer(current: &RemoteVersion, pushed: Option<&str>) -> bool {
    let pushed = pushed.and_then(|tag| tag.parse::<u64>().ok()).unwrap_or(0);
    current.tag.parse::<u64>().is_ok_and(|now| now > pushed)
}

/// A failure that stops the whole sync: every further push would fail the same way.
fn stops_the_sync(error: &ConfluenceError) -> bool {
    matches!(
        error,
        ConfluenceError::NotConfigured
            | ConfluenceError::SignInNeeded
            | ConfluenceError::SiteNotGranted
            | ConfluenceError::DatastoreUnavailable
    )
}

/// The page or attachment a copy holds. `ConfluenceAttachment.parent_page_id` is the page it sits on.
fn item_of_copy(copy: &PublishedCopy) -> Result<RemoteItemRef, ConfluenceError> {
    let id = copy
        .destination_item_id
        .clone()
        .ok_or(ConfluenceError::LocationInvalid)?;
    match &copy.destination {
        CopyDestination::ConfluencePage { .. } => Ok(RemoteItemRef::Page { page_id: id }),
        CopyDestination::ConfluenceAttachment { parent_page_id, .. } => {
            Ok(RemoteItemRef::Attachment {
                page_id: parent_page_id.clone(),
                attachment_id: id,
            })
        }
        CopyDestination::SharepointFile { .. } => Err(ConfluenceError::LocationInvalid),
    }
}

/// `approval_decided`'s data; other fields are not read.
#[derive(Debug, Deserialize)]
struct ApprovalDecided {
    approval_id: ApprovalId,
    state: ApprovalState,
}

/// What became of one item of an approved card.
enum ItemOutcome {
    /// Done: the item leaves Ready to publish.
    Sent,
    /// Refused without sending (step 6.2, with the reason logged): the item stays for a fresh approval.
    Held(&'static str),
}

/// The file differs from the bytes the card was made from.
const HELD_CHANGED: &str = "changed_since_card";
/// The latest version is the work of Ordin or an agent (ADR-029).
const HELD_AGENT: &str = "changed_by_agent";
/// The page moved in Confluence after the card (SA-CONF-AUTO-04).
const HELD_DESTINATION: &str = "changed_in_confluence";

/// The words of step 6.1 on a card holding an agent's work.
const AGENT_WROTE_ONE: &str = "An agent may have written 1 of them.";
const AGENT_WROTE_SOME: &str = "of them.";
/// The words of step 6.2 for an item held after its approval.
const HELD_CHANGED_WORDS: &str =
    "An approved file was not published: it changed after the card. Publish it again for a new card.";
const HELD_AGENT_WORDS: &str = "An approved file was not published: an agent changed it and the \
     card did not say so. Publish it again for a new card.";
const HELD_DESTINATION_WORDS: &str = "An approved file was not published: its page changed in \
     Confluence. Choose Bring a copy back or Keep mine, then publish it again.";

/// The words on a copy whose first push failed.
fn create_failure_reason(error: &ConfluenceError) -> String {
    match error {
        ConfluenceError::OutcomeUnknown => CREATE_UNKNOWN_REASON.to_owned(),
        other => format!(
            "ordin couldn't publish this to Confluence ({}).",
            other.code()
        ),
    }
}

/// One file going to Confluence: what `push_file` needs of a link, which the sink no longer holds.
struct Outgoing<'a> {
    link: ConfluenceLinkId,
    /// The link's folder as a prefix of the original's path.
    prefix: &'a str,
    place_id: PlaceId,
    bytes: Vec<u8>,
    hash: String,
    /// The destination version read (step 5.3); `None` for a first push.
    expected: Option<String>,
}

impl ConfluenceAdapter {
    /// Every file below `folder`, following every page and recursing.
    async fn walk_files(
        &self,
        place: PlaceId,
        folder: &str,
    ) -> Result<Vec<FileEntry>, ConfluenceError> {
        let mut files = Vec::new();
        let mut folders = vec![folder.trim_matches('/').to_owned()];
        while let Some(current) = folders.pop() {
            let mut cursor = None;
            loop {
                let page = self
                    .files
                    .list_folder(place, &current, FolderSort::NameAsc, cursor)
                    .await
                    .map_err(files_failed)?;
                for entry in page.items {
                    if entry.is_folder {
                        folders.push(entry.path.trim_end_matches('/').to_owned());
                    } else {
                        files.push(entry);
                    }
                }
                match page.next_cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
        }
        Ok(files)
    }

    /// `{audience.line} — Confluence — {space name} › {parent page title}`, cut to 200 characters, from a
    /// resolved space and parent page (`CONF_NOT_FOUND` if gone). The audience leads, so a long title is
    /// what the cut removes.
    async fn where_line(&self, link: &ConfluenceLink) -> Result<String, ConfluenceError> {
        let space = self.space_by_key(&link.confluence_space).await?;
        let parent = self.read_page(&link.parent_page_id).await?;
        let audience = self
            .audience(&ConfluenceLocation {
                space_key: space.key,
                space_name: space.name.clone(),
                page_id: Some(parent.id),
                title: parent.title.clone(),
                web_url: String::new(),
            })
            .await?;
        Ok(cut(
            &format!(
                "{} \u{2014} Confluence \u{2014} {} \u{203A} {}",
                audience.line, space.name, parent.title
            ),
            MAX_WHERE_CHARS,
        ))
    }

    /// Internal Logic 4.
    // spec: ordin-desktop/components/ConfluenceAdapter#ADR-026
    pub async fn request_link_approval(
        &self,
        link: &ConfluenceLink,
        place: &Place,
        workspace: &Workspace,
    ) -> Result<ApprovalId, ConfluenceError> {
        if link.state != ConfluenceLinkState::Proposed {
            return Err(ConfluenceError::LinkNotActive);
        }
        let where_line = self.where_line(link).await?;
        let files = self.walk_files(place.id, &link.folder).await?;
        let folder = folder_name(&link.folder, &place.friendly_name);
        let named: Vec<ContentRef> = files
            .iter()
            .take(MAX_NAMED)
            .map(|file| ContentRef::PlaceFile {
                place_id: place.id,
                relative_path: file.path.clone(),
            })
            .collect();
        let affects = vec![AffectedItem::File {
            label: format!("{} documents in {folder}", files.len()),
            version: None,
            place_id: Some(place.id),
        }];
        // The Approval Service refuses an empty list of files, so an empty folder shows none.
        let show_details = !named.is_empty();
        let content = show_details.then(|| ApprovalContent::Files {
            label: "See the files".to_owned(),
            files: named,
        });
        let frame = "Publish  to Confluence?".chars().count();
        let question = format!(
            "Publish {} to Confluence?",
            cut(&folder, MAX_QUESTION_CHARS - frame)
        );
        let card = self
            .approvals
            .request(ApprovalRequest {
                workspace_id: workspace.id,
                asked_by: ApprovalAsker::Person,
                question,
                verb: "Link and publish".to_owned(),
                where_line: where_line.clone(),
                affects: affects.clone(),
                changes: LINK_CHANGES.to_owned(),
                content,
                reject_style: RejectStyle::NotNow,
                show_details,
                proposal: ActionProposal {
                    action: "Link a folder to Confluence and publish it".to_owned(),
                    destination: where_line,
                    affects,
                    consequence: LINK_CHANGES.to_owned(),
                },
                asker: None,
                leaves_company: false,
            })
            .await
            .map_err(approval_failed)?;
        let appended = self
            .journal
            .append_entry(NewJournalEntry {
                kind: "confluence_link_approval_requested".to_owned(),
                component: "confluence".to_owned(),
                run_id: None,
                crew_run_id: None,
                workspace_id: Some(link.workspace_id),
                summary: "Asked to link a folder to Confluence.".to_owned(),
                data: serde_json::json!({"confluence_link_id": link.id}),
            })
            .await;
        if appended.is_err() {
            tracing::warn!(component = "confluence", "confluence_journal_failed");
        }
        tracing::info!(
            component = "confluence",
            confluence_link_id = link.id.to_string().as_str(),
            "confluence_link_approval_requested"
        );
        Ok(card.approval_id)
    }

    fn is_text(&self, kind: &str) -> bool {
        TEXT_KINDS.contains(&kind) || (kind == "docx" && self.word_to_markdown.is_some())
    }

    /// Step 5.2.1: a copy waiting in Ready to publish, as a page or as the exact file.
    // spec: ordin-desktop/components/ConfluenceAdapter#C-13
    async fn new_copy(
        &self,
        link: &ConfluenceLink,
        path: &str,
        kind: &str,
    ) -> Result<PublishedCopy, ConfluenceError> {
        let text = self.is_text(kind);
        let space = link.confluence_space.clone();
        let parent_page_id = link.parent_page_id.clone();
        let copy = PublishedCopy {
            id: PublishedCopyId::new(),
            original: CopyOriginal::PlaceFile {
                place_id: link.place_id,
                path: path.to_owned(),
            },
            destination: if text {
                CopyDestination::ConfluencePage {
                    space,
                    parent_page_id,
                }
            } else {
                CopyDestination::ConfluenceAttachment {
                    space,
                    parent_page_id,
                }
            },
            destination_item_id: None,
            format: if text {
                CopyFormat::Page
            } else {
                CopyFormat::ExactFile
            },
            maker: if text {
                CopyMaker::Ordin
            } else {
                CopyMaker::None
            },
            published_via: PublishedVia::ConfluenceLink {
                confluence_link_id: link.id,
            },
            followed_branch: None,
            keep_up_to_date: true,
            audience: link.audience.clone(),
            original_version_published: None,
            destination_version_pushed: None,
            last_updated_at: None,
            state: PublishedCopyState::ReadyToPublish,
            failure_reason: None,
        };
        self.files
            .record_published_copy(copy)
            .await
            .map_err(files_failed)
    }

    /// Step 5.2.2: are these bytes, or any version since `published`, the work of Ordin or an agent?
    /// A write reaches the disk before its version row, so bytes the latest version does not record
    /// have no known author, and neither has a file with no versions. Walking back to what was
    /// published stops a person's small edit carrying an agent's unreviewed text out with it.
    // spec: ordin-desktop/components/ConfluenceAdapter#ADR-029
    async fn agent_changed(
        &self,
        place: PlaceId,
        path: &str,
        hash: &str,
        published: Option<&str>,
    ) -> Result<bool, ConfluenceError> {
        let versions = self
            .files
            .versions(place, path)
            .await
            .map_err(files_failed)?;
        let Some(latest) = versions.last() else {
            return Ok(true);
        };
        Ok(latest.hash.0 != hash
            || versions
                .iter()
                .rev()
                .take_while(|version| Some(version.hash.0.as_str()) != published)
                .any(by_agent))
    }

    /// Whether a run may be writing this path now, so its author is not known yet (FU-073).
    async fn being_written(&self, place: PlaceId, path: &str) -> Result<bool, ConfluenceError> {
        self.files
            .agent_write_open(place, path)
            .await
            .map_err(files_failed)
    }

    /// Steps 5.2.2, 5.2.3 and 7: the person's own change, unless Confluence moved first.
    async fn sync_person_change(
        &self,
        link: &ConfluenceLink,
        prefix: &str,
        copy: PublishedCopy,
        bytes: Vec<u8>,
        hash: String,
        report: &mut LinkSyncReport,
    ) -> Result<(), ConfluenceError> {
        let item = item_of_copy(&copy)?;
        let current = self.version_read(&item).await?;
        if newer(&current, copy.destination_version_pushed.as_deref()) {
            self.files
                .set_published_copy_state(copy.id, PublishedCopyState::ChangedAtDestination, None)
                .await
                .map_err(files_failed)?;
            report.changed_in_confluence += 1;
            return Ok(());
        }
        let id = copy.id;
        let pushed = self
            .push_file(
                copy,
                Outgoing {
                    link: link.id,
                    prefix,
                    place_id: link.place_id,
                    bytes,
                    hash,
                    expected: Some(current.tag),
                },
            )
            .await;
        match pushed {
            Ok(()) => {
                report.pushed += 1;
                Ok(())
            }
            Err(ConfluenceError::OutcomeUnknown) => {
                self.mark_failed(id).await;
                report.failed += 1;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Step 7.6, for a copy already published: `failed` is reached through `out_of_date` and `updating`
    /// (the state table).
    async fn mark_failed(&self, id: PublishedCopyId) {
        for (state, reason) in [
            (PublishedCopyState::OutOfDate, None),
            (PublishedCopyState::Updating, None),
            (
                PublishedCopyState::Failed,
                Some(OUTCOME_UNKNOWN_REASON.to_owned()),
            ),
        ] {
            if self
                .files
                .set_published_copy_state(id, state, reason)
                .await
                .is_err()
            {
                tracing::warn!(component = "confluence", "confluence_copy_state_failed");
                return;
            }
        }
    }

    /// Internal Logic 5.
    // spec: ordin-desktop/components/ConfluenceAdapter#ADR-026
    // spec: ordin-desktop/components/ConfluenceAdapter#ADR-029
    pub async fn sync_linked_folder(
        &self,
        link: &ConfluenceLink,
        place: &Place,
    ) -> Result<LinkSyncReport, ConfluenceError> {
        self.sync_linked_folder_with(link, place, AutomaticPublishing::On)
            .await
    }

    /// Step 5 with automatic publishing as the Workspace Service derived it (SA-CONF-AUTO-02).
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-01
    pub async fn sync_linked_folder_with(
        &self,
        link: &ConfluenceLink,
        place: &Place,
        publishing: AutomaticPublishing,
    ) -> Result<LinkSyncReport, ConfluenceError> {
        if link.state != ConfluenceLinkState::Active {
            return Err(ConfluenceError::LinkNotActive);
        }
        let copies = self
            .files
            .published_copies_via_link(link.id)
            .await
            .map_err(files_failed)?;
        let mut by_path: HashMap<String, PublishedCopy> = HashMap::with_capacity(copies.len());
        for copy in copies {
            if let Some((_, path)) = place_path(&copy) {
                by_path.insert(path.to_owned(), copy);
            }
        }
        let files = self.walk_files(place.id, &link.folder).await?;
        let present: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
        let prefix = link_prefix(&link.folder);
        let relative = |path: &str| {
            path.strip_prefix(prefix.as_str())
                .unwrap_or(path)
                .to_owned()
        };

        // Copies whose file is gone, by the content they last published: a rename, if it turns up.
        let mut gone_by_hash: HashMap<String, PublishedCopy> = HashMap::new();
        let mut gone: Vec<PublishedCopy> = Vec::new();
        for (path, copy) in &by_path {
            if present.contains(path.as_str()) || copy.state == PublishedCopyState::Removed {
                continue;
            }
            match &copy.original_version_published {
                Some(hash) => {
                    gone_by_hash.insert(hash.clone(), copy.clone());
                }
                None => gone.push(copy.clone()),
            }
        }

        let mut report = LinkSyncReport::default();
        let mut ready: Vec<ReadyItem> = Vec::new();
        for file in &files {
            // `read_file` refuses a file above 64 MiB; one such file must not stop the others on
            // every run, so it counts as `failed`.
            let read = match self.files.read_file(place.id, &file.path).await {
                Ok(read) => read,
                Err(e) if e.code() == "FILE_TOO_LARGE" => {
                    report.failed += 1;
                    continue;
                }
                Err(e) => return Err(files_failed(e)),
            };
            let hash = read.hash.0;
            let Some(copy) = by_path.get(&file.path).cloned() else {
                if let Some(old) = gone_by_hash.remove(&hash) {
                    ready.push(ReadyItem {
                        confluence_link_id: link.id,
                        kind: ReadyKind::Renamed,
                        path: relative(&file.path),
                        from_path: place_path(&old).map(|(_, path)| relative(path)),
                        published_copy_id: Some(old.id),
                    });
                    continue;
                }
                let copy = self.new_copy(link, &file.path, &file.kind).await?;
                ready.push(ReadyItem {
                    confluence_link_id: link.id,
                    kind: ReadyKind::New,
                    path: relative(&file.path),
                    from_path: None,
                    published_copy_id: Some(copy.id),
                });
                continue;
            };
            match copy.state {
                PublishedCopyState::ReadyToPublish => ready.push(ReadyItem {
                    confluence_link_id: link.id,
                    kind: ReadyKind::New,
                    path: relative(&file.path),
                    from_path: None,
                    published_copy_id: Some(copy.id),
                }),
                PublishedCopyState::Current
                    if copy.original_version_published.as_deref() != Some(hash.as_str()) =>
                {
                    // Automatic push waits for the author; the next sync decides.
                    if self.being_written(place.id, &file.path).await? {
                        report.waiting_for_approval += 1;
                        continue;
                    }
                    let published = copy.original_version_published.as_deref();
                    // SA-CONF-AUTO-02: with automatic publishing off, no change is the person's
                    // to push without a card.
                    // spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-02
                    if publishing == AutomaticPublishing::Off
                        || self
                            .agent_changed(place.id, &file.path, &hash, published)
                            .await?
                    {
                        self.files
                            .set_published_copy_state(copy.id, PublishedCopyState::OutOfDate, None)
                            .await
                            .map_err(files_failed)?;
                        report.waiting_for_approval += 1;
                        ready.push(ReadyItem {
                            confluence_link_id: link.id,
                            kind: ReadyKind::Changed,
                            path: relative(&file.path),
                            from_path: None,
                            published_copy_id: Some(copy.id),
                        });
                        continue;
                    }
                    let pushed = self
                        .sync_person_change(link, &prefix, copy, read.bytes, hash, &mut report)
                        .await;
                    match pushed {
                        Ok(()) => {}
                        Err(e) if stops_the_sync(&e) => return Err(e),
                        Err(e) => {
                            tracing::warn!(
                                component = "confluence",
                                code = e.code(),
                                "confluence_push_failed"
                            );
                            report.failed += 1;
                        }
                    }
                }
                // SA-CONF-AUTO-03: a change held back earlier waits for the person's card.
                // spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-03
                PublishedCopyState::OutOfDate => ready.push(ReadyItem {
                    confluence_link_id: link.id,
                    kind: ReadyKind::Changed,
                    path: relative(&file.path),
                    from_path: None,
                    published_copy_id: Some(copy.id),
                }),
                // SA-CONF-AUTO-05: a failed copy goes back on the list for a fresh card; the Push
                // Service's Try again refuses it, and the sync sends nothing for it.
                // spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-05
                PublishedCopyState::Failed => {
                    let kind = if copy.destination_item_id.is_none() {
                        Some(ReadyKind::New)
                    } else if copy.original_version_published.as_deref() != Some(hash.as_str()) {
                        Some(ReadyKind::Changed)
                    } else {
                        None
                    };
                    if let Some(kind) = kind {
                        ready.push(ReadyItem {
                            confluence_link_id: link.id,
                            kind,
                            path: relative(&file.path),
                            from_path: None,
                            published_copy_id: Some(copy.id),
                        });
                    }
                }
                // Step 5.2.4 and every other state: not pushed by the sync.
                _ => {}
            }
        }
        // Step 5.3: nothing is deleted in Confluence without the person's check.
        gone.extend(gone_by_hash.into_values());
        for copy in gone {
            if let Some((_, path)) = place_path(&copy) {
                ready.push(ReadyItem {
                    confluence_link_id: link.id,
                    kind: ReadyKind::Removed,
                    path: relative(path),
                    from_path: None,
                    published_copy_id: Some(copy.id),
                });
            }
        }
        report.ready_to_publish = ready.len() as u32;
        store::replace_ready(self.pool()?, link.id, &ready, Timestamp::now()).await?;
        self.record_sync(link.id, &report).await;
        Ok(report)
    }

    async fn record_sync(&self, link: ConfluenceLinkId, report: &LinkSyncReport) {
        let appended = self
            .journal
            .append_entry(NewJournalEntry {
                kind: "confluence_link_synced".to_owned(),
                component: "confluence".to_owned(),
                run_id: None,
                crew_run_id: None,
                workspace_id: None,
                summary: "Compared a linked folder with Confluence.".to_owned(),
                data: serde_json::json!({
                    "confluence_link_id": link,
                    "pushed": report.pushed,
                    "waiting_for_approval": report.waiting_for_approval,
                    "ready_to_publish": report.ready_to_publish,
                    "changed_in_confluence": report.changed_in_confluence,
                    "failed": report.failed,
                }),
            })
            .await;
        if appended.is_err() {
            tracing::warn!(component = "confluence", "confluence_journal_failed");
        }
        tracing::info!(
            component = "confluence",
            confluence_link_id = link.to_string().as_str(),
            pushed = u64::from(report.pushed),
            waiting_for_approval = u64::from(report.waiting_for_approval),
            ready_to_publish = u64::from(report.ready_to_publish),
            changed_in_confluence = u64::from(report.changed_in_confluence),
            failed = u64::from(report.failed),
            "confluence_link_synced"
        );
    }

    pub async fn ready_to_publish(
        &self,
        link: ConfluenceLinkId,
    ) -> Result<Vec<ReadyItem>, ConfluenceError> {
        store::load_ready(self.pool()?, link).await
    }

    /// The file's hash as it stands, and its version number when the latest version holds those bytes.
    async fn content_of(
        &self,
        place: PlaceId,
        path: &str,
    ) -> Result<(String, Option<String>), ConfluenceError> {
        let read = self
            .files
            .read_file(place, path)
            .await
            .map_err(files_failed)?;
        let versions = self
            .files
            .versions(place, path)
            .await
            .map_err(files_failed)?;
        let version = versions
            .last()
            .filter(|latest| latest.hash == read.hash)
            .map(|latest| latest.version.to_string());
        Ok((read.hash.0, version))
    }

    /// Step 6.1: what the card shows of an item. The kept version with the bound hash, so that the
    /// person sees the bytes the approval covers even if the file has moved on since; the live file
    /// only when no version holds those bytes, and then a later change holds the item at send time.
    async fn bound_content(
        &self,
        place_id: PlaceId,
        path: String,
        hash: &Option<String>,
    ) -> Result<ContentRef, ConfluenceError> {
        let versions = self
            .files
            .versions(place_id, &path)
            .await
            .map_err(files_failed)?;
        let kept = versions
            .iter()
            .rev()
            .find(|version| hash.as_deref() == Some(version.hash.0.as_str()));
        Ok(match kept {
            Some(version) => ContentRef::FileVersion {
                file_version_id: version.id,
            },
            None => ContentRef::PlaceFile {
                place_id,
                relative_path: path,
            },
        })
    }

    /// Whether a version holds exactly these bytes.
    async fn bytes_are_kept(
        &self,
        place: PlaceId,
        path: &str,
        hash: &str,
    ) -> Result<bool, ConfluenceError> {
        let versions = self
            .files
            .versions(place, path)
            .await
            .map_err(files_failed)?;
        Ok(versions.iter().any(|version| version.hash.0 == hash))
    }

    /// Step 6.1: whether a card must say an agent may have written these bytes.
    async fn authored_by_agent(
        &self,
        place: PlaceId,
        path: &str,
        hash: &str,
    ) -> Result<bool, ConfluenceError> {
        if self.being_written(place, path).await? {
            return Ok(true);
        }
        let versions = self
            .files
            .versions(place, path)
            .await
            .map_err(files_failed)?;
        // No versions at all is a file the person added: only a claim or a version names an agent.
        Ok(!versions.is_empty() && self.agent_changed(place, path, hash, None).await?)
    }

    /// Internal Logic 6.1: **Publish checked items…**, one approval card for the chosen items, each bound
    /// to its content as the card is made.
    pub async fn request_publish_checked(
        &self,
        link: &ConfluenceLink,
        place: &Place,
        items: Vec<ReadyItem>,
    ) -> Result<ApprovalId, ConfluenceError> {
        if items.len() > MAX_NAMED {
            return Err(ConfluenceError::QueryInvalid);
        }
        if link.state != ConfluenceLinkState::Active {
            return Err(ConfluenceError::LinkNotActive);
        }
        let where_line = self.where_line(link).await?;
        let count = |kind: ReadyKind| items.iter().filter(|i| i.kind == kind).count();
        let changes = match count(ReadyKind::Changed) {
            0 => format!(
                "{} new, {} renamed, {} removed.",
                count(ReadyKind::New),
                count(ReadyKind::Renamed),
                count(ReadyKind::Removed)
            ),
            changed => format!(
                "{} new, {changed} changed, {} renamed, {} removed.",
                count(ReadyKind::New),
                count(ReadyKind::Renamed),
                count(ReadyKind::Removed)
            ),
        };
        let prefix = link_prefix(&link.folder);
        let mut affects: Vec<AffectedItem> = Vec::with_capacity(items.len());
        let mut approved: Vec<ApprovedItem> = Vec::with_capacity(items.len());
        let mut by_agents: Vec<ContentRef> = Vec::new();
        for item in items {
            let (content_hash, version) = match item.kind {
                ReadyKind::Removed => (None, None),
                ReadyKind::New | ReadyKind::Renamed | ReadyKind::Changed => {
                    let (hash, version) = self
                        .content_of(place.id, &format!("{prefix}{}", item.path))
                        .await?;
                    (Some(hash), version)
                }
            };
            let path = format!("{prefix}{}", item.path);
            let agent_written = match &content_hash {
                Some(hash) => self.authored_by_agent(place.id, &path, hash).await?,
                None => false,
            };
            if agent_written {
                by_agents.push(self.bound_content(place.id, path, &content_hash).await?);
            }
            affects.push(AffectedItem::File {
                label: item.path.clone(),
                version,
                place_id: Some(place.id),
            });
            approved.push(ApprovedItem {
                item,
                content_hash,
                agent_written,
            });
        }
        // Step 6.1: the person sees what an agent wrote before approving it (ADR-029).
        let changes = match by_agents.len() {
            0 => changes,
            1 => format!("{changes} {AGENT_WROTE_ONE}"),
            n => format!("{changes} An agent may have written {n} {AGENT_WROTE_SOME}"),
        };
        let show_details = !by_agents.is_empty();
        let content = show_details.then(|| ApprovalContent::Files {
            label: "See the files".to_owned(),
            files: by_agents,
        });
        let card = self
            .approvals
            .request(ApprovalRequest {
                workspace_id: link.workspace_id,
                asked_by: ApprovalAsker::Person,
                question: format!("Publish {} checked items to Confluence?", approved.len()),
                verb: "Publish".to_owned(),
                where_line: where_line.clone(),
                affects: affects.clone(),
                changes: changes.clone(),
                content,
                // Refused items stay in Ready to publish, as a link's card does (step 6.2).
                reject_style: RejectStyle::NotNow,
                show_details,
                proposal: ActionProposal {
                    action: "Publish checked items to Confluence".to_owned(),
                    destination: where_line,
                    affects,
                    consequence: changes,
                },
                asker: None,
                leaves_company: false,
            })
            .await
            .map_err(approval_failed)?;
        store::insert_publish_request(
            self.pool()?,
            link.id,
            card.approval_id,
            &approved,
            Timestamp::now(),
        )
        .await?;
        Ok(card.approval_id)
    }

    /// **Keep mine** on a page changed in Confluence: the next push replaces it. The copy takes the
    /// destination's version as the one pushed, so the check before the next push passes.
    pub async fn keep_local(&self, copy: PublishedCopyId) -> Result<(), ConfluenceError> {
        let mut copy = self
            .files
            .published_copy(copy)
            .await
            .map_err(files_failed)?;
        let current = self.version_read(&item_of_copy(&copy)?).await?;
        copy.destination_version_pushed = Some(current.tag);
        copy.state = PublishedCopyState::Current;
        self.files
            .record_published_copy(copy)
            .await
            .map_err(files_failed)?;
        Ok(())
    }

    /// The Workspace Service removed the link: deletes this crate's rows for it; no Confluence page is
    /// deleted.
    pub async fn forget_link(&self, link: ConfluenceLinkId) -> Result<(), ConfluenceError> {
        store::forget_link(self.pool()?, link).await
    }

    /// The page of a folder relative to the link's, made on the first push that needs it (step 5.5).
    /// The link root's page is titled with the folder's name and sits under `link.parent_page_id`.
    async fn folder_page(
        &self,
        outgoing: &Outgoing<'_>,
        dir: &str,
        space: &str,
        link_parent: &str,
    ) -> Result<String, ConfluenceError> {
        let root_name = if outgoing.prefix.is_empty() {
            self.files
                .place(outgoing.place_id)
                .await
                .map_err(files_failed)?
                .friendly_name
        } else {
            name_of(outgoing.prefix.trim_end_matches('/')).to_owned()
        };
        let mut folders: Vec<(String, String)> = vec![(String::new(), root_name)];
        let mut path = String::new();
        for segment in dir.split('/').filter(|s| !s.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(segment);
            folders.push((path.clone(), segment.to_owned()));
        }
        let pool = self.pool()?;
        let mut parent = link_parent.to_owned();
        for (folder_path, title) in folders {
            parent = match store::folder_page(pool, outgoing.link, &folder_path).await? {
                Some(page_id) => page_id,
                None => {
                    let (page_id, _) = self
                        .publish_page(
                            &CopyDestination::ConfluencePage {
                                space: space.to_owned(),
                                parent_page_id: parent.clone(),
                            },
                            &title,
                            "",
                            None,
                            ExternalActionKey::new(),
                            None,
                        )
                        .await?;
                    store::insert_folder_page(pool, outgoing.link, &folder_path, &page_id).await?;
                    page_id
                }
            };
        }
        Ok(parent)
    }

    /// The Markdown a file becomes, or `None` when a Word conversion fails and it is attached instead.
    // spec: ordin-desktop/components/ConfluenceAdapter#C-13
    async fn markdown_of(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> Result<Option<String>, ConfluenceError> {
        if kind_of(name) == "docx" {
            return Ok(match &self.word_to_markdown {
                Some(converter) => converter.convert(bytes.to_vec()).await.ok(),
                None => None,
            });
        }
        String::from_utf8(bytes.to_vec())
            .map(Some)
            .map_err(|_| ConfluenceError::ConversionFailed)
    }

    /// Steps 5.5 and 7: a linked file's page or attachment, then the copy's record. Automatic pushes mint
    /// their key here and pass no run.
    async fn push_file(
        &self,
        mut copy: PublishedCopy,
        outgoing: Outgoing<'_>,
    ) -> Result<(), ConfluenceError> {
        let (_, path) = place_path(&copy).ok_or(ConfluenceError::LocationInvalid)?;
        let path = path.to_owned();
        let relative = path
            .strip_prefix(outgoing.prefix)
            .unwrap_or(&path)
            .to_owned();
        let name = name_of(&path).to_owned();
        let key = ExternalActionKey::new();
        let (space, parent_page_id) = match &copy.destination {
            CopyDestination::ConfluencePage {
                space,
                parent_page_id,
            }
            | CopyDestination::ConfluenceAttachment {
                space,
                parent_page_id,
            } => (space.clone(), parent_page_id.clone()),
            CopyDestination::SharepointFile { .. } => return Err(ConfluenceError::LocationInvalid),
        };
        let as_page = matches!(copy.destination, CopyDestination::ConfluencePage { .. });
        let markdown = if as_page {
            self.markdown_of(&name, &outgoing.bytes).await?
        } else {
            None
        };
        let version = match (markdown, &copy.destination_item_id, &outgoing.expected) {
            (Some(text), Some(page_id), Some(expected)) => {
                self.update_page(page_id, stem_of(&name), &text, None, expected, key, None)
                    .await?
            }
            (Some(text), _, _) => {
                let parent = self
                    .folder_page(&outgoing, dir_of(&relative), &space, &parent_page_id)
                    .await?;
                let dest = CopyDestination::ConfluencePage {
                    space,
                    parent_page_id: parent,
                };
                let (page_id, version) = self
                    .publish_page(&dest, stem_of(&name), &text, None, key, None)
                    .await?;
                copy.destination = dest;
                copy.destination_item_id = Some(page_id);
                version
            }
            (None, item, expected) => {
                // Attached already and still an attachment: a new version on the same page.
                let same = !as_page;
                let existing = expected.as_deref().filter(|_| same && item.is_some());
                let parent = if existing.is_some() {
                    parent_page_id
                } else {
                    self.folder_page(&outgoing, dir_of(&relative), &space, &parent_page_id)
                        .await?
                };
                let (attachment_id, version) = self
                    .attach(&parent, &name, outgoing.bytes.clone(), existing, key, None)
                    .await?;
                copy.destination = CopyDestination::ConfluenceAttachment {
                    space,
                    parent_page_id: parent,
                };
                copy.destination_item_id = Some(attachment_id);
                // Step 9.3: the Word document that could not convert is the exact file.
                copy.format = CopyFormat::ExactFile;
                copy.maker = CopyMaker::None;
                version
            }
        };
        // A published file has a history from here, so its next change is recorded with its author.
        if self
            .files
            .keep_published_version(outgoing.place_id, &path, outgoing.bytes)
            .await
            .is_err()
        {
            tracing::warn!(component = "confluence", "confluence_baseline_failed");
        }
        copy.state = PublishedCopyState::Current;
        copy.failure_reason = None;
        copy.original_version_published = Some(outgoing.hash);
        copy.destination_version_pushed = Some(version.tag);
        copy.last_updated_at = Some(Timestamp::now());
        self.files
            .record_published_copy(copy)
            .await
            .map_err(files_failed)?;
        Ok(())
    }

    /// Step 6.2, `New`. What is sent is the bytes checked here against the card's hash, read once.
    // spec: ordin-desktop/components/ConfluenceAdapter#ADR-029
    async fn publish_new(
        &self,
        link: ConfluenceLinkId,
        approved: &ApprovedItem,
        copy: PublishedCopy,
    ) -> Result<ItemOutcome, ConfluenceError> {
        let item = &approved.item;
        let (place_id, path) = place_path(&copy).ok_or(ConfluenceError::LocationInvalid)?;
        let prefix = path
            .strip_suffix(item.path.as_str())
            .ok_or(ConfluenceError::LocationInvalid)?;
        let read = self
            .files
            .read_file(place_id, path)
            .await
            .map_err(files_failed)?;
        if approved.content_hash.as_deref() != Some(read.hash.0.as_str()) {
            return Ok(ItemOutcome::Held(HELD_CHANGED));
        }
        // The card's hash is proved above, so a card that named the agent covers these very bytes.
        if !approved.agent_written && self.authored_by_agent(place_id, path, &read.hash.0).await? {
            return Ok(ItemOutcome::Held(HELD_AGENT));
        }
        // A card that named the agent showed the kept bytes. With none kept it showed the live file,
        // which an agent could have swapped for the moment the person looked.
        if approved.agent_written && !self.bytes_are_kept(place_id, path, &read.hash.0).await? {
            return Ok(ItemOutcome::Held(HELD_AGENT));
        }
        // A fresh approval of an item whose first push failed is the table's way out of `failed`.
        let sending = if copy.state == PublishedCopyState::Failed {
            PublishedCopyState::Updating
        } else {
            PublishedCopyState::AwaitingApproval
        };
        self.files
            .set_published_copy_state(copy.id, sending, None)
            .await
            .map_err(files_failed)?;
        let outgoing = Outgoing {
            link,
            prefix,
            place_id,
            bytes: read.bytes,
            hash: read.hash.0,
            expected: None,
        };
        if let Err(error) = self.push_file(copy.clone(), outgoing).await {
            let reason = create_failure_reason(&error);
            let failed = self
                .files
                .set_published_copy_state(copy.id, PublishedCopyState::Failed, Some(reason))
                .await;
            if failed.is_err() {
                tracing::warn!(component = "confluence", "confluence_copy_state_failed");
            }
            return Err(error);
        }
        Ok(ItemOutcome::Sent)
    }

    /// Step 6.2, `Changed` (SA-CONF-AUTO-04): the checked bytes replace the published copy, unless the
    /// file moved since the card, an agent wrote them and the card did not say so, or the page moved
    /// in Confluence.
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-04
    async fn publish_changed(
        &self,
        link: ConfluenceLinkId,
        approved: &ApprovedItem,
        copy: PublishedCopy,
    ) -> Result<ItemOutcome, ConfluenceError> {
        let item = &approved.item;
        let (place_id, path) = place_path(&copy).ok_or(ConfluenceError::LocationInvalid)?;
        let prefix = path
            .strip_suffix(item.path.as_str())
            .ok_or(ConfluenceError::LocationInvalid)?;
        let read = self
            .files
            .read_file(place_id, path)
            .await
            .map_err(files_failed)?;
        if approved.content_hash.as_deref() != Some(read.hash.0.as_str()) {
            return Ok(ItemOutcome::Held(HELD_CHANGED));
        }
        if !approved.agent_written && self.authored_by_agent(place_id, path, &read.hash.0).await? {
            return Ok(ItemOutcome::Held(HELD_AGENT));
        }
        if approved.agent_written && !self.bytes_are_kept(place_id, path, &read.hash.0).await? {
            return Ok(ItemOutcome::Held(HELD_AGENT));
        }
        let destination = item_of_copy(&copy)?;
        let current = self.version_read(&destination).await?;
        if newer(&current, copy.destination_version_pushed.as_deref()) {
            // The state table has no move from `failed` to `changed_at_destination` (SA-CONF-AUTO-05).
            // spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-05
            if copy.state != PublishedCopyState::Failed {
                self.files
                    .set_published_copy_state(
                        copy.id,
                        PublishedCopyState::ChangedAtDestination,
                        None,
                    )
                    .await
                    .map_err(files_failed)?;
            }
            return Ok(ItemOutcome::Held(HELD_DESTINATION));
        }
        self.files
            .set_published_copy_state(copy.id, PublishedCopyState::Updating, None)
            .await
            .map_err(files_failed)?;
        let outgoing = Outgoing {
            link,
            prefix,
            place_id,
            bytes: read.bytes,
            hash: read.hash.0,
            expected: Some(current.tag),
        };
        if let Err(error) = self.push_file(copy.clone(), outgoing).await {
            let reason = if matches!(error, ConfluenceError::OutcomeUnknown) {
                OUTCOME_UNKNOWN_REASON.to_owned()
            } else {
                create_failure_reason(&error)
            };
            if self
                .files
                .set_published_copy_state(copy.id, PublishedCopyState::Failed, Some(reason))
                .await
                .is_err()
            {
                tracing::warn!(component = "confluence", "confluence_copy_state_failed");
            }
            return Err(error);
        }
        Ok(ItemOutcome::Sent)
    }

    /// Step 6.2, `Renamed`: the page takes its new title, or the attachment is uploaded under the new
    /// name; the copy follows the file. Nothing is deleted in Confluence.
    async fn publish_renamed(
        &self,
        approved: &ApprovedItem,
        mut copy: PublishedCopy,
    ) -> Result<ItemOutcome, ConfluenceError> {
        let item = &approved.item;
        let (place_id, old_path) = place_path(&copy).ok_or(ConfluenceError::LocationInvalid)?;
        let from = item
            .from_path
            .as_deref()
            .ok_or(ConfluenceError::LocationInvalid)?;
        let prefix = old_path
            .strip_suffix(from)
            .ok_or(ConfluenceError::LocationInvalid)?;
        let new_path = format!("{prefix}{}", item.path);
        let name = name_of(&new_path).to_owned();
        let target = item_of_copy(&copy)?;
        let read = self
            .files
            .read_file(place_id, &new_path)
            .await
            .map_err(files_failed)?;
        if approved.content_hash.as_deref() != Some(read.hash.0.as_str()) {
            return Ok(ItemOutcome::Held(HELD_CHANGED));
        }
        // An attachment sends the file's bytes, so a rename carries whatever an agent wrote since.
        if matches!(target, RemoteItemRef::Attachment { .. })
            && !approved.agent_written
            && self
                .authored_by_agent(place_id, &new_path, &read.hash.0)
                .await?
        {
            return Ok(ItemOutcome::Held(HELD_AGENT));
        }
        let key = ExternalActionKey::new();
        let version = match &target {
            RemoteItemRef::Page { page_id } => {
                let page = self.read_page(page_id).await?;
                let tag = page.version()?.remote()?.tag;
                let storage = page.storage().unwrap_or_default().to_owned();
                self.put_page(
                    page_id,
                    Some(stem_of(&name)),
                    &storage,
                    &tag,
                    Bracket {
                        key,
                        run_id: None,
                        kind: ordin_types::domain::runs::ExternalActionKind::CopyPublish,
                    },
                )
                .await?
            }
            RemoteItemRef::Attachment { page_id, .. } => {
                let (attachment_id, version) = self
                    .attach(page_id, &name, read.bytes.clone(), None, key, None)
                    .await?;
                copy.destination_item_id = Some(attachment_id);
                copy.original_version_published = Some(read.hash.0.clone());
                version
            }
        };
        // The path is new, so it has no history. Bytes Confluence holds start it; other bytes wait for
        // the next sync, which finds no known author and sends them to a card.
        if copy.original_version_published.as_deref() == Some(read.hash.0.as_str())
            && self
                .files
                .keep_published_version(place_id, &new_path, read.bytes)
                .await
                .is_err()
        {
            tracing::warn!(component = "confluence", "confluence_baseline_failed");
        }
        copy.original = CopyOriginal::PlaceFile {
            place_id,
            path: new_path,
        };
        copy.destination_version_pushed = Some(version.tag);
        self.files
            .record_published_copy(copy)
            .await
            .map_err(files_failed)?;
        Ok(ItemOutcome::Sent)
    }

    /// Step 6.2: one item of an approved request.
    async fn publish_item(
        &self,
        link: ConfluenceLinkId,
        approved: &ApprovedItem,
    ) -> Result<ItemOutcome, ConfluenceError> {
        let item = &approved.item;
        let id = item
            .published_copy_id
            .ok_or(ConfluenceError::LocationInvalid)?;
        let copy = self.files.published_copy(id).await.map_err(files_failed)?;
        match item.kind {
            ReadyKind::New => self.publish_new(link, approved, copy).await,
            ReadyKind::Renamed => self.publish_renamed(approved, copy).await,
            ReadyKind::Changed => self.publish_changed(link, approved, copy).await,
            // The page stays in Confluence: deleting it is the Push Service's **Remove copy…** (ADR-069).
            ReadyKind::Removed => self
                .files
                .set_published_copy_state(id, PublishedCopyState::Removed, None)
                .await
                .map(|_| ItemOutcome::Sent)
                .map_err(files_failed),
        }
    }

    async fn run_request(&self, request: &PublishRequest) -> Result<(), ConfluenceError> {
        for approved in &request.items {
            let item = &approved.item;
            let link = item.confluence_link_id;
            match self.publish_item(link, approved).await {
                Ok(ItemOutcome::Sent) => {
                    store::delete_ready_item(self.pool()?, link, &item.path).await?;
                }
                Ok(ItemOutcome::Held(reason)) => {
                    tracing::warn!(
                        component = "confluence",
                        confluence_link_id = link.to_string().as_str(),
                        reason,
                        "confluence_item_held"
                    );
                    self.tell_held(link, &item.path, reason).await;
                }
                Err(e) => tracing::warn!(
                    component = "confluence",
                    code = e.code(),
                    "confluence_push_failed"
                ),
            }
        }
        Ok(())
    }

    /// Step 6.2: the person learns why an approved item did not go.
    async fn tell_held(&self, link: ConfluenceLinkId, path: &str, reason: &str) {
        let summary = match reason {
            HELD_AGENT => HELD_AGENT_WORDS,
            HELD_DESTINATION => HELD_DESTINATION_WORDS,
            _ => HELD_CHANGED_WORDS,
        };
        let appended = self
            .journal
            .append_entry(NewJournalEntry {
                kind: "confluence_item_held".to_owned(),
                component: "confluence".to_owned(),
                run_id: None,
                crew_run_id: None,
                workspace_id: None,
                summary: summary.to_owned(),
                data: serde_json::json!({
                    "confluence_link_id": link,
                    "path": path,
                    "reason": reason,
                }),
            })
            .await;
        if appended.is_err() {
            tracing::warn!(component = "confluence", "confluence_journal_failed");
        }
    }

    /// Step 6.2: acts only on `approval_decided` for an approval id in `confluence_publish_requests`.
    async fn on_event(&self, event: &JournalEvent) -> Result<(), ConfluenceError> {
        if event.kind != EventKind::ApprovalDecided.as_str() {
            return Ok(());
        }
        let Ok(decided) = serde_json::from_value::<ApprovalDecided>(event.data.clone()) else {
            return Ok(());
        };
        let pool = self.pool()?;
        let Some(request) = store::publish_request(pool, decided.approval_id).await? else {
            return Ok(());
        };
        match decided.state {
            ApprovalState::Pending => return Ok(()),
            ApprovalState::Approved => self.run_request(&request).await?,
            // Items stay in Ready to publish.
            ApprovalState::Rejected | ApprovalState::Expired => {}
        }
        store::delete_publish_request(pool, &request.id).await
    }

    async fn deliver_logged(&self, event: &JournalEvent) {
        if let Err(e) = self.on_event(event).await {
            tracing::warn!(
                component = "confluence",
                code = e.code(),
                "confluence_event_failed"
            );
        }
    }

    /// `lagged(next_seq)` re-reads the missed events through `events_since`.
    async fn catch_up(&self, next_seq: u64) {
        let mut after = next_seq.saturating_sub(1);
        loop {
            let events = match self.journal.events_since(after, LAG_BATCH).await {
                Ok(events) => events,
                Err(_) => {
                    tracing::warn!(component = "confluence", "confluence_event_failed");
                    return;
                }
            };
            let Some(last) = events.last().map(|e| e.seq) else {
                return;
            };
            for event in &events {
                self.deliver_logged(event).await;
            }
            after = last;
        }
    }
}

/// Holds the adapter weakly: the Journal keeps its sinks, and the adapter keeps the Journal.
// spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-06
struct ConfluenceSink {
    adapter: Weak<ConfluenceAdapter>,
}

pub(crate) fn sink(adapter: &Arc<ConfluenceAdapter>) -> Arc<dyn EventSink> {
    Arc::new(ConfluenceSink {
        adapter: Arc::downgrade(adapter),
    })
}

#[async_trait::async_trait]
impl EventSink for ConfluenceSink {
    fn name(&self) -> &'static str {
        "confluence"
    }

    async fn deliver(&self, event: &JournalEvent) {
        if let Some(adapter) = self.adapter.upgrade() {
            adapter.deliver_logged(event).await;
        }
    }

    async fn lagged(&self, next_seq: u64) {
        if let Some(adapter) = self.adapter.upgrade() {
            adapter.catch_up(next_seq).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_split_into_folder_name_and_kind() {
        assert_eq!(dir_of("a/b/c.md"), "a/b");
        assert_eq!(dir_of("c.md"), "");
        assert_eq!(name_of("a/b/c.md"), "c.md");
        assert_eq!(stem_of("rates.v2.md"), "rates.v2");
        assert_eq!(stem_of("rates"), "rates");
        assert_eq!(kind_of("A.DOCX"), "docx");
        assert_eq!(kind_of("rates"), "");
    }

    #[test]
    fn the_link_folder_is_a_path_prefix() {
        assert_eq!(link_prefix(""), "");
        assert_eq!(link_prefix("Supplier quotes/"), "Supplier quotes/");
        assert_eq!(folder_name("a/Supplier quotes", "Place"), "Supplier quotes");
        assert_eq!(folder_name("", "Place"), "Place");
    }

    #[test]
    fn a_higher_destination_version_counts_as_moved() {
        let at = Timestamp::now();
        let at_eight = RemoteVersion {
            tag: "8".into(),
            changed_at: at,
            changed_by: None,
        };
        assert!(newer(&at_eight, Some("7")));
        assert!(!newer(&at_eight, Some("8")));
        assert!(newer(&at_eight, None));
    }
}
