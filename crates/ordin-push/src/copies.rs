//! Published copies (PushService.md Internal Logic E; ADR-069).

use ordin_approval::ApprovalRequest;
use ordin_confluence::RemoteItemRef;
use ordin_craft_toolkit::MakeCopy;
use ordin_files::PlaceChange;
use ordin_projects::RepositoryOriginChange;
use ordin_sharepoint::{ExternalWrite, SharePointItemRef, SharePointLocation};
use ordin_types::domain::places::{
    CopyDestination, CopyFormat, CopyMaker, CopyOriginal, PublishedCopy, PublishedCopyState,
    PublishedVia,
};
use ordin_types::domain::runs::ExternalActionKind;
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{AffectedItem, ApprovalAsker, RejectStyle};
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{
    ApprovalId, CodedError, ExternalActionKey, PlaceId, PublishedCopyId, WorkspaceId,
};

use crate::documents::{digest, file_name, provider_error};
use crate::error::{classify_send, PushError};
use crate::model::{CopyAction, CopyActionInput, PublishCopyRequest, PushNotice};
use crate::service::PushService;
use crate::store::{self, AudienceRow, CopyOpRow};
use crate::text;

/// What the copy is made from, and what it is called at the destination.
struct CopyInfo {
    original_name: String,
    file_name: String,
    page_file: Option<(PlaceId, String)>,
}

/// The bytes of one copy and the words around it.
struct Made {
    markdown: String,
    header: String,
    bytes: Vec<u8>,
    file_name: String,
}

/// How one copy's automatic update ended (E3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Update {
    Updated,
    Held,
    ChangedAtDestination,
    Failed,
}

fn maker_for(format: CopyFormat) -> CopyMaker {
    match format {
        CopyFormat::Page => CopyMaker::Ordin,
        CopyFormat::Word => CopyMaker::WordCraft,
        CopyFormat::Pdf => CopyMaker::PdfCraft,
        CopyFormat::Slides => CopyMaker::DeckCraft,
        CopyFormat::ExactFile => CopyMaker::None,
    }
}

fn extension(format: CopyFormat) -> &'static str {
    match format {
        CopyFormat::Word => "docx",
        CopyFormat::Pdf => "pdf",
        CopyFormat::Slides => "pptx",
        CopyFormat::Page | CopyFormat::ExactFile => "md",
    }
}

fn destination_line(destination: &CopyDestination) -> String {
    match destination {
        CopyDestination::ConfluencePage { space, .. }
        | CopyDestination::ConfluenceAttachment { space, .. } => {
            format!("Confluence space {space}")
        }
        CopyDestination::SharepointFile { site, folder, .. } => {
            format!("SharePoint site {site}, folder {folder}")
        }
    }
}

/// E.1: SHA-256 of the sorted principals joined by "\n", plus "\n" and the link scope when present.
fn principals_digest(principals: &[String], link_scope: Option<&str>) -> String {
    let mut sorted: Vec<&str> = principals.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let mut joined = sorted.join("\n");
    if let Some(scope) = link_scope {
        joined.push('\n');
        joined.push_str(scope);
    }
    digest(joined.as_bytes())
}

fn grew(stored: &AudienceRow, now: &AudienceRow) -> bool {
    if stored.everyone_who_can_open || now.everyone_who_can_open {
        return false;
    }
    let more_people = matches!((stored.people, now.people), (Some(a), Some(b)) if b > a);
    more_people || stored.principals_digest != now.principals_digest
}

impl PushService {
    /// E.1: raises the card for a first publish.
    pub async fn publish_copy(
        &self,
        req: PublishCopyRequest,
    ) -> Result<PublishedCopyId, PushError> {
        let pool = self.pool()?;
        // The 2C `PUB_*` format rules, before anything is recorded: `exact_file` only with
        // `confluence_attachment` or `sharepoint_file` (amended January 2026 by the user's decision).
        // spec: ordin-desktop/components/PushService#PS-TW-1
        if req.format == CopyFormat::ExactFile
            && !matches!(
                req.destination,
                CopyDestination::ConfluenceAttachment { .. }
                    | CopyDestination::SharepointFile { .. }
            )
        {
            return Err(PushError::new(
                "PUSH_REQUEST_INVALID",
                "PUB_FORMAT_MAKER_MISMATCH",
            ));
        }
        if req.format == CopyFormat::Page
            && !matches!(req.destination, CopyDestination::ConfluencePage { .. })
        {
            return Err(PushError::new(
                "PUSH_REQUEST_INVALID",
                "PUB_FORMAT_DESTINATION_MISMATCH",
            ));
        }
        if matches!(
            req.format,
            CopyFormat::Word | CopyFormat::Pdf | CopyFormat::Slides
        ) && !self.deps.craft.conversion_available(req.format)
        {
            return Err(PushError::new(
                "PUSH_FORMAT_NOT_READY",
                "conversion not confirmed",
            ));
        }
        if matches!(req.original, CopyOriginal::KnowledgePage { .. }) && req.page_file.is_none() {
            return Err(PushError::new(
                "PUSH_REQUEST_INVALID",
                "knowledge page without its file",
            ));
        }
        if req.file_name.trim().is_empty() || req.original_name.trim().is_empty() {
            return Err(PushError::new("PUSH_REQUEST_INVALID", "name"));
        }
        // Destination checks and the audience.
        let (audience, replaces) = self.check_destination(&req).await?;
        let line = if audience.everyone_who_can_open {
            text::audience_unknown(&self.deps.localisation, &destination_line(&req.destination))
        } else {
            audience.line.clone()
        };
        let copy_id = PublishedCopyId::new();
        let copy = PublishedCopy {
            id: copy_id,
            original: req.original.clone(),
            destination: req.destination.clone(),
            destination_item_id: None,
            format: req.format,
            maker: maker_for(req.format),
            published_via: PublishedVia::PublishACopy,
            followed_branch: req.followed_branch.clone(),
            keep_up_to_date: req.keep_up_to_date,
            audience: text::cut(&line, 500),
            original_version_published: None,
            destination_version_pushed: None,
            last_updated_at: None,
            state: PublishedCopyState::AwaitingApproval,
            failure_reason: None,
        };
        self.deps
            .files
            .record_published_copy(copy)
            .await
            .map_err(|e| PushError::new("PUSH_REQUEST_INVALID", e.code()))?;
        let place = destination_line(&req.destination);
        let mut changes = format!(
            "A {} copy of {}{}.",
            match req.format {
                CopyFormat::Page => "Confluence page",
                CopyFormat::Word => "Word",
                CopyFormat::Pdf => "PDF",
                CopyFormat::Slides => "slides",
                CopyFormat::ExactFile => "exact-file",
            },
            req.original_name,
            req.followed_branch
                .as_deref()
                .map(|b| format!(" from branch {b}"))
                .unwrap_or_default()
        );
        changes.push_str(if req.keep_up_to_date {
            " It is kept up to date automatically."
        } else {
            " It is not kept up to date."
        });
        if replaces {
            changes.push_str(" It will replace the file already there with this name.");
        }
        changes.push_str(&format!(" Who can see it: {line}."));
        let changes = text::cut(&changes, 1000);
        let affects = vec![AffectedItem::Record {
            label: text::cut(&format!("{} in {place}", req.file_name), 200),
        }];
        let request = ApprovalRequest {
            workspace_id: req.workspace_id,
            asked_by: ApprovalAsker::Person,
            question: text::cut(&format!("Publish a copy of {}", req.original_name), 119) + "?",
            verb: text::COPY_VERB.to_owned(),
            where_line: text::cut(&format!("{place} ({line})"), 200),
            affects: affects.clone(),
            changes: changes.clone(),
            content: None,
            reject_style: RejectStyle::Reject,
            show_details: false,
            proposal: ActionProposal {
                action: text::COPY_ACTION.to_owned(),
                destination: text::cut(&place, 2048),
                affects,
                consequence: changes,
            },
            asker: None,
            leaves_company: false,
        };
        let card = match self.deps.approvals.request(request).await {
            Ok(card) => card,
            Err(error) => {
                let _ = self
                    .deps
                    .files
                    .set_published_copy_state(
                        copy_id,
                        PublishedCopyState::Failed,
                        Some("The approval couldn't be prepared.".to_owned()),
                    )
                    .await;
                return Err(PushError::new("PUSH_APPROVAL_FAILED", error.code()));
            }
        };
        store::put_audience(pool, copy_id, &audience).await?;
        store::insert_copy_op(
            pool,
            copy_id,
            "first_publish",
            Some(card.approval_id),
            &req.original_version,
            "awaiting_approval",
        )
        .await?;
        if let CopyOriginal::KnowledgePage { knowledge_page_id } = &req.original {
            if let Some((place, path)) = &req.page_file {
                // spec: ordin-desktop/components/PushService#SA-CLOSE-01
                store::put_page_file(pool, *knowledge_page_id, *place, path).await?;
            }
        }
        store::put_copy_request(
            pool,
            copy_id,
            &store::CopyRequestRow {
                workspace_id: req.workspace_id,
                original_name: req.original_name.clone(),
                file_name: req.file_name.clone(),
            },
        )
        .await?;
        tracing::info!(
            component = "push",
            published_copy_id = copy_id.to_string().as_str(),
            operation = "first_publish",
            result = "proposed",
            "push.copy"
        );
        Ok(copy_id)
    }

    /// E.1: the destination exists, the name's use, and who can see the place. Returns the audience row
    /// and whether the name is already in use.
    async fn check_destination(
        &self,
        req: &PublishCopyRequest,
    ) -> Result<(AudienceRow, bool), PushError> {
        match &req.destination {
            CopyDestination::SharepointFile {
                library, folder, ..
            } => {
                let sp = &self.deps.sharepoint;
                let loc = sp
                    .resolve_origin(&format!("graph:drives/{library}/items/{folder}"))
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                let taken = sp
                    .exists(&loc, &req.file_name)
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                let audience = sp
                    .audience(&loc)
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                let scope = audience
                    .link_scope
                    .map(|s| format!("{s:?}").to_ascii_lowercase());
                Ok((
                    AudienceRow {
                        line: audience.line,
                        people: audience.people,
                        everyone_who_can_open: audience.everyone_who_can_open,
                        principals_digest: principals_digest(
                            &audience.principals,
                            scope.as_deref(),
                        ),
                    },
                    taken,
                ))
            }
            CopyDestination::ConfluencePage { parent_page_id, .. }
            | CopyDestination::ConfluenceAttachment { parent_page_id, .. } => {
                let Some(confluence) = self.deps.confluence.as_ref() else {
                    return Err(PushError::new("PUSH_CONFLUENCE_UNAVAILABLE", "no adapter"));
                };
                let loc = confluence
                    .resolve_origin(&format!("confluence:page/{parent_page_id}"))
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                let audience = confluence
                    .audience(&loc)
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                Ok((
                    AudienceRow {
                        line: audience.line,
                        people: audience.people,
                        everyone_who_can_open: audience.everyone_who_can_open,
                        principals_digest: principals_digest(&audience.principals, None),
                    },
                    false,
                ))
            }
        }
    }

    // ---- decisions ----

    pub(crate) async fn decide_copies(
        &self,
        ops: Vec<CopyOpRow>,
        state: &str,
    ) -> Result<(), PushError> {
        let pool = self.pool()?;
        for op in ops {
            if op.state != "awaiting_approval" && op.state != "held" {
                continue;
            }
            match (op.kind.as_str(), state) {
                ("first_publish", "approved") => {
                    store::set_copy_op(pool, &op.id, "sending", None).await?;
                    let copy = self.copy_of(op.published_copy_id).await?;
                    self.execute_first_publish(&op, copy).await;
                }
                ("audience_hold" | "update_copies", "approved") => {
                    store::set_copy_op(pool, &op.id, "sending", None).await?;
                    let copy = self.copy_of(op.published_copy_id).await?;
                    if op.kind == "audience_hold" {
                        // The person approved the new audience: it becomes the one on record.
                        if let Some(fresh) = self.read_audience(&copy).await {
                            store::put_audience(pool, copy.id, &fresh).await?;
                        }
                    }
                    let result = self
                        .update_copy(copy, &op.original_version, false, Some(&op.id))
                        .await;
                    store::set_copy_op(
                        pool,
                        &op.id,
                        if result == Update::Updated {
                            "done"
                        } else {
                            "failed"
                        },
                        None,
                    )
                    .await?;
                }
                ("first_publish", _) => {
                    store::set_copy_op(pool, &op.id, "failed", None).await?;
                    // Nothing was published: the copy never existed.
                    let _ = self
                        .deps
                        .files
                        .set_published_copy_state(
                            op.published_copy_id,
                            PublishedCopyState::Removed,
                            None,
                        )
                        .await;
                }
                _ => {
                    store::set_copy_op(pool, &op.id, "failed", None).await?;
                }
            }
        }
        Ok(())
    }

    async fn copy_of(&self, id: PublishedCopyId) -> Result<PublishedCopy, PushError> {
        self.deps
            .files
            .published_copy(id)
            .await
            .map_err(|e| PushError::new("PUSH_UNKNOWN", e.code()))
    }

    async fn info_for(&self, copy: &PublishedCopy) -> CopyInfo {
        let request = match self.pool() {
            Ok(pool) => store::get_copy_request(pool, copy.id).await.ok().flatten(),
            Err(_) => None,
        };
        let page_file = match (&copy.original, self.pool()) {
            (CopyOriginal::KnowledgePage { knowledge_page_id }, Ok(pool)) => {
                store::get_page_file(pool, *knowledge_page_id)
                    .await
                    .ok()
                    .flatten()
            }
            _ => None,
        };
        let original_name = match (&request, &copy.original, &page_file) {
            (Some(r), _, _) => r.original_name.clone(),
            (None, CopyOriginal::PlaceFile { path, .. }, _) => file_name(path),
            (None, _, Some((_, path))) => file_name(path),
            (None, _, None) => "page.md".to_owned(),
        };
        // A copy published before the request was stored falls back to the original's name with the
        // format's extension.
        let name = request
            .map(|r| r.file_name)
            .unwrap_or_else(|| text::default_copy_name(&original_name, extension(copy.format)));
        CopyInfo {
            original_name,
            file_name: name,
            page_file,
        }
    }

    // ---- E.6 reading an original ----

    async fn read_original(
        &self,
        copy: &PublishedCopy,
        version: &str,
        info: &CopyInfo,
    ) -> Result<Vec<u8>, PushError> {
        let unreadable =
            |detail: &str| PushError::new("PUSH_ORIGINAL_UNREADABLE", detail.to_owned());
        let (place, path) = match (&copy.original, &info.page_file) {
            (CopyOriginal::PlaceFile { place_id, path }, _) => (*place_id, path.clone()),
            (CopyOriginal::KnowledgePage { .. }, Some((place, path))) => (*place, path.clone()),
            (CopyOriginal::KnowledgePage { .. }, None) => return Err(unreadable("no page file")),
        };
        if copy.followed_branch.is_some() {
            return self
                .deps
                .projects
                .read_at_commit(place, version, &path)
                .await
                .map_err(|e| unreadable(e.code()));
        }
        let wanted: u32 = version.parse().map_err(|_| unreadable("version"))?;
        let versions = self
            .deps
            .files
            .versions(place, &path)
            .await
            .map_err(|e| unreadable(e.code()))?;
        let Some(found) = versions.into_iter().find(|v| v.version == wanted) else {
            return Err(unreadable("no such version"));
        };
        self.deps
            .files
            .read_version(found.id)
            .await
            .map(|b| b.bytes)
            .map_err(|e| unreadable(e.code()))
    }

    async fn original_from(&self, copy: &PublishedCopy, info: &CopyInfo) -> String {
        let place = match (&copy.original, &info.page_file) {
            (CopyOriginal::PlaceFile { place_id, .. }, _) => Some(*place_id),
            (_, Some((place, _))) => Some(*place),
            _ => None,
        };
        match place {
            Some(place) => self
                .deps
                .files
                .place(place)
                .await
                .map(|p| p.friendly_name)
                .unwrap_or_else(|_| "ordin".to_owned()),
            None => "ordin".to_owned(),
        }
    }

    /// E.2: reads the original at `version` and makes the bytes for the copy's format.
    // spec: ordin-desktop/components/PushService#SA-RECON-11
    async fn make_content(
        &self,
        copy: &PublishedCopy,
        version: &str,
        info: &CopyInfo,
    ) -> Result<Made, PushError> {
        let original = self.read_original(copy, version, info).await?;
        let from = self.original_from(copy, info).await;
        let now = Timestamp::now();
        let header = text::copy_header(
            &self.deps.localisation,
            &info.original_name,
            &from,
            copy.followed_branch.as_deref(),
            now,
        );
        if copy.format == CopyFormat::ExactFile {
            return Ok(Made {
                markdown: String::new(),
                header,
                bytes: original,
                file_name: info.file_name.clone(),
            });
        }
        let markdown = String::from_utf8(original)
            .map_err(|_| PushError::new("PUSH_ORIGINAL_UNREADABLE", "not text"))?;
        if copy.format == CopyFormat::Page {
            return Ok(Made {
                markdown,
                header,
                bytes: Vec::new(),
                file_name: info.file_name.clone(),
            });
        }
        let made = self
            .deps
            .craft
            .make_copy(MakeCopy {
                original_name: info.original_name.clone(),
                original_from: from,
                branch: copy.followed_branch.clone(),
                markdown,
                format: copy.format,
                updated_at: now,
            })
            .await
            .map_err(|e| PushError::new("PUSH_COPY_MAKER_FAILED", e.code()))?;
        Ok(Made {
            markdown: String::new(),
            header,
            bytes: made.bytes,
            file_name: info.file_name.clone(),
        })
    }

    fn copy_error(code: &'static str) -> &'static str {
        match code {
            "PUSH_COPY_MAKER_FAILED" => "The copy couldn't be made.",
            "PUSH_NAME_EXISTS" => "Something with that name is already there.",
            "PUSH_ORIGINAL_UNREADABLE" => "ordin couldn't read the original at that version.",
            "PUSH_OUTCOME_UNKNOWN" => "ordin isn't sure whether this copy arrived.",
            "PUSH_SIGN_IN_NEEDED" => "Sign in again to send this copy.",
            "PUSH_FORBIDDEN" => "You don't have permission to change this destination.",
            "PUSH_ORIGIN_UNREACHABLE" => "Couldn't reach the destination.",
            "PUSH_TOO_LARGE" => "This copy is too large to send there.",
            "PUSH_CONFLUENCE_UNAVAILABLE" => "Confluence isn't available on this server.",
            _ => "The destination refused the copy.",
        }
    }

    fn map_send(code: &str) -> &'static str {
        match code {
            "SP_NAME_EXISTS" | "CONF_TITLE_EXISTS" => "PUSH_NAME_EXISTS",
            other => classify_send(other),
        }
    }

    // ---- E.2 first publish ----

    async fn execute_first_publish(&self, op: &CopyOpRow, copy: PublishedCopy) {
        let info = self.info_for(&copy).await;
        let key = ExternalActionKey::new();
        let result = self
            .publish_new(&op.id, &copy, &op.original_version, &info, key)
            .await;
        match result {
            Ok((item_id, tag)) => {
                let mut done = copy.clone();
                done.state = PublishedCopyState::Current;
                done.destination_item_id = Some(item_id);
                done.destination_version_pushed = Some(tag);
                done.original_version_published = Some(op.original_version.clone());
                done.last_updated_at = Some(Timestamp::now());
                done.failure_reason = None;
                if let Err(error) = self.deps.files.record_published_copy(done).await {
                    tracing::warn!(
                        component = "push",
                        code = error.code(),
                        "push.copy_record_failed"
                    );
                }
                if copy.keep_up_to_date {
                    if let (CopyOriginal::PlaceFile { place_id, path }, Some(branch)) =
                        (&copy.original, &copy.followed_branch)
                    {
                        let _ = self
                            .deps
                            .projects
                            .follow_original(*place_id, path, branch)
                            .await;
                    }
                }
                if let Ok(pool) = self.pool() {
                    let _ = store::set_copy_op(pool, &op.id, "done", None).await;
                }
                tracing::info!(
                    component = "push",
                    published_copy_id = copy.id.to_string().as_str(),
                    operation = "first_publish",
                    result = "done",
                    from_version = op.original_version.as_str(),
                    "push.copy"
                );
            }
            Err(error) => {
                let code = error.code;
                let _ = self
                    .deps
                    .files
                    .set_published_copy_state(
                        copy.id,
                        PublishedCopyState::Failed,
                        Some(Self::copy_error(code).to_owned()),
                    )
                    .await;
                if let Ok(pool) = self.pool() {
                    let _ = store::set_copy_op(pool, &op.id, "failed", None).await;
                }
                tracing::info!(
                    component = "push",
                    published_copy_id = copy.id.to_string().as_str(),
                    operation = "first_publish",
                    result = code,
                    "push.copy"
                );
                self.copy_needs_you(&copy).await;
            }
        }
    }

    /// The copy's workspace, from the request stored when it was first published; `None` for a copy
    /// published before the request was stored.
    async fn workspace_of_copy(&self, copy: &PublishedCopy) -> Option<WorkspaceId> {
        let pool = self.pool().ok()?;
        store::get_copy_request(pool, copy.id)
            .await
            .ok()
            .flatten()
            .map(|r| r.workspace_id)
    }

    async fn copy_needs_you(&self, copy: &PublishedCopy) {
        if let Some(workspace) = self.workspace_of_copy(copy).await {
            self.needs_you(workspace).await;
        }
    }

    /// Makes the copy and sends it to a destination that has none yet; returns (destination item id, version tag).
    async fn publish_new(
        &self,
        op_id: &str,
        copy: &PublishedCopy,
        version: &str,
        info: &CopyInfo,
        key: ExternalActionKey,
    ) -> Result<(String, String), PushError> {
        let pool = self.pool()?;
        let made = self.make_content(copy, version, info).await?;
        store::set_copy_op(pool, op_id, "sending", Some(key)).await?;
        let sent: Result<(String, String), PushError> =
            match &copy.destination {
                CopyDestination::ConfluencePage { .. } => {
                    let confluence = self.deps.confluence.as_ref().ok_or_else(|| {
                        PushError::new("PUSH_CONFLUENCE_UNAVAILABLE", "no adapter")
                    })?;
                    confluence
                        .publish_page(
                            &copy.destination,
                            &made.file_name,
                            &made.markdown,
                            Some(&made.header),
                            key,
                            None,
                        )
                        .await
                        .map(|(id, v)| (id, v.tag))
                        .map_err(|e| PushError::new(Self::map_send(e.code()), e.code()))
                }
                CopyDestination::ConfluenceAttachment { parent_page_id, .. } => {
                    let confluence = self.deps.confluence.as_ref().ok_or_else(|| {
                        PushError::new("PUSH_CONFLUENCE_UNAVAILABLE", "no adapter")
                    })?;
                    confluence
                        .attach(parent_page_id, &made.file_name, made.bytes, None, key, None)
                        .await
                        .map(|(id, v)| (id, v.tag))
                        .map_err(|e| PushError::new(Self::map_send(e.code()), e.code()))
                }
                CopyDestination::SharepointFile {
                    library, folder, ..
                } => {
                    let sp = &self.deps.sharepoint;
                    let loc: SharePointLocation = sp
                        .resolve_origin(&format!("graph:drives/{library}/items/{folder}"))
                        .await
                        .map_err(|e| provider_error(e.code()))?;
                    sp.create_file(
                        &loc,
                        &made.file_name,
                        made.bytes,
                        ExternalWrite {
                            key,
                            run_id: None,
                            kind: ExternalActionKind::CopyPublish,
                        },
                    )
                    .await
                    .map(|(item, v)| (item.item_id, v.tag))
                    .map_err(|e| PushError::new(Self::map_send(e.code()), e.code()))
                }
            };
        sent
    }

    // ---- E.3 keeping copies up to date ----

    /// The destination's current version tag, read from the adapter.
    async fn destination_version(&self, copy: &PublishedCopy) -> Result<String, PushError> {
        let Some(item) = copy.destination_item_id.clone() else {
            return Err(PushError::new(
                "PUSH_UNKNOWN",
                "copy has no destination item",
            ));
        };
        match &copy.destination {
            CopyDestination::SharepointFile { library, .. } => self
                .deps
                .sharepoint
                .item_version(&SharePointItemRef {
                    drive_id: library.clone(),
                    item_id: item,
                })
                .await
                .map(|v| v.tag)
                .map_err(|e| provider_error(e.code())),
            CopyDestination::ConfluencePage { .. }
            | CopyDestination::ConfluenceAttachment { .. } => {
                let Some(confluence) = self.deps.confluence.as_ref() else {
                    return Err(PushError::new("PUSH_CONFLUENCE_UNAVAILABLE", "no adapter"));
                };
                let reference = match &copy.destination {
                    CopyDestination::ConfluenceAttachment { parent_page_id, .. } => {
                        RemoteItemRef::Attachment {
                            page_id: parent_page_id.clone(),
                            attachment_id: item,
                        }
                    }
                    _ => RemoteItemRef::Page { page_id: item },
                };
                confluence
                    .item_version(&reference)
                    .await
                    .map(|v| v.tag)
                    .map_err(|e| provider_error(e.code()))
            }
        }
    }

    async fn read_audience(&self, copy: &PublishedCopy) -> Option<AudienceRow> {
        match &copy.destination {
            CopyDestination::SharepointFile {
                library, folder, ..
            } => {
                let loc = self
                    .deps
                    .sharepoint
                    .resolve_origin(&format!("graph:drives/{library}/items/{folder}"))
                    .await
                    .ok()?;
                let a = self.deps.sharepoint.audience(&loc).await.ok()?;
                let scope = a.link_scope.map(|s| format!("{s:?}").to_ascii_lowercase());
                Some(AudienceRow {
                    line: a.line,
                    people: a.people,
                    everyone_who_can_open: a.everyone_who_can_open,
                    principals_digest: principals_digest(&a.principals, scope.as_deref()),
                })
            }
            CopyDestination::ConfluencePage { parent_page_id, .. }
            | CopyDestination::ConfluenceAttachment { parent_page_id, .. } => {
                let confluence = self.deps.confluence.as_ref()?;
                let loc = confluence
                    .resolve_origin(&format!("confluence:page/{parent_page_id}"))
                    .await
                    .ok()?;
                let a = confluence.audience(&loc).await.ok()?;
                Some(AudienceRow {
                    line: a.line,
                    people: a.people,
                    everyone_who_can_open: a.everyone_who_can_open,
                    principals_digest: principals_digest(&a.principals, None),
                })
            }
        }
    }

    /// E.3: one copy, from the original at `version`.
    async fn update_copy(
        &self,
        copy: PublishedCopy,
        version: &str,
        check_audience: bool,
        op_id: Option<&str>,
    ) -> Update {
        let Ok(pool) = self.pool() else {
            return Update::Failed;
        };
        let files = &self.deps.files;
        // The destination moved since ordin last pushed: updates stop (never overwritten).
        if matches!(
            copy.state,
            PublishedCopyState::Current | PublishedCopyState::OutOfDate
        ) {
            match self.destination_version(&copy).await {
                Ok(tag) if Some(&tag) != copy.destination_version_pushed.as_ref() => {
                    let _ = files
                        .set_published_copy_state(
                            copy.id,
                            PublishedCopyState::ChangedAtDestination,
                            None,
                        )
                        .await;
                    self.copy_needs_you(&copy).await;
                    return Update::ChangedAtDestination;
                }
                Ok(_) => {}
                Err(_) => {
                    let _ = files
                        .set_published_copy_state(copy.id, PublishedCopyState::OutOfDate, None)
                        .await;
                    return Update::Failed;
                }
            }
        }
        // The audience approved for the copy must not have grown.
        if check_audience {
            if let (Ok(Some(stored)), Some(now)) = (
                store::get_audience(pool, copy.id).await,
                self.read_audience(&copy).await,
            ) {
                if grew(&stored, &now) {
                    return self.hold_for_audience(copy, version, now).await;
                }
            }
        }
        // Current -> out of date -> updating (the state machine has no shorter way).
        if copy.state == PublishedCopyState::Current {
            let _ = files
                .set_published_copy_state(copy.id, PublishedCopyState::OutOfDate, None)
                .await;
        }
        if files
            .set_published_copy_state(copy.id, PublishedCopyState::Updating, None)
            .await
            .is_err()
        {
            return Update::Failed;
        }
        let info = self.info_for(&copy).await;
        let key = ExternalActionKey::new();
        let result = self.send_update(op_id, &copy, version, &info, key).await;
        match result {
            Ok(tag) => {
                let mut done = copy.clone();
                done.state = PublishedCopyState::Current;
                done.destination_version_pushed = Some(tag);
                done.original_version_published = Some(version.to_owned());
                done.last_updated_at = Some(Timestamp::now());
                done.failure_reason = None;
                if let Err(error) = files.record_published_copy(done).await {
                    tracing::warn!(
                        component = "push",
                        code = error.code(),
                        "push.copy_record_failed"
                    );
                }
                tracing::info!(
                    component = "push",
                    published_copy_id = copy.id.to_string().as_str(),
                    operation = "update",
                    result = "done",
                    from_version = version,
                    "push.copy"
                );
                Update::Updated
            }
            Err(error) => {
                let _ = files
                    .set_published_copy_state(
                        copy.id,
                        PublishedCopyState::Failed,
                        Some(Self::copy_error(error.code).to_owned()),
                    )
                    .await;
                tracing::info!(
                    component = "push",
                    published_copy_id = copy.id.to_string().as_str(),
                    operation = "update",
                    result = error.code,
                    from_version = version,
                    "push.copy"
                );
                self.copy_needs_you(&copy).await;
                Update::Failed
            }
        }
    }

    async fn send_update(
        &self,
        op_id: Option<&str>,
        copy: &PublishedCopy,
        version: &str,
        info: &CopyInfo,
        key: ExternalActionKey,
    ) -> Result<String, PushError> {
        let pool = self.pool()?;
        let made = self.make_content(copy, version, info).await?;
        let expected = copy.destination_version_pushed.clone().unwrap_or_default();
        let Some(item) = copy.destination_item_id.clone() else {
            return Err(PushError::new(
                "PUSH_UNKNOWN",
                "copy has no destination item",
            ));
        };
        let own_op = match op_id {
            Some(id) => id.to_owned(),
            None => {
                store::insert_copy_op(pool, copy.id, "update", None, version, "sending").await?
            }
        };
        store::set_copy_op(pool, &own_op, "sending", Some(key)).await?;
        let sent: Result<String, PushError> =
            match &copy.destination {
                CopyDestination::SharepointFile { library, .. } => self
                    .deps
                    .sharepoint
                    .upload_version(
                        &SharePointItemRef {
                            drive_id: library.clone(),
                            item_id: item,
                        },
                        made.bytes,
                        &expected,
                        ExternalWrite {
                            key,
                            run_id: None,
                            kind: ExternalActionKind::CopyPublish,
                        },
                    )
                    .await
                    .map(|v| v.tag)
                    .map_err(|e| PushError::new(Self::map_send(e.code()), e.code())),
                CopyDestination::ConfluencePage { .. } => {
                    let confluence = self.deps.confluence.as_ref().ok_or_else(|| {
                        PushError::new("PUSH_CONFLUENCE_UNAVAILABLE", "no adapter")
                    })?;
                    confluence
                        .update_page(
                            &item,
                            &made.file_name,
                            &made.markdown,
                            Some(&made.header),
                            &expected,
                            key,
                            None,
                        )
                        .await
                        .map(|v| v.tag)
                        .map_err(|e| PushError::new(Self::map_send(e.code()), e.code()))
                }
                CopyDestination::ConfluenceAttachment { parent_page_id, .. } => {
                    let confluence = self.deps.confluence.as_ref().ok_or_else(|| {
                        PushError::new("PUSH_CONFLUENCE_UNAVAILABLE", "no adapter")
                    })?;
                    confluence
                        .attach(
                            parent_page_id,
                            &made.file_name,
                            made.bytes,
                            Some(&expected),
                            key,
                            None,
                        )
                        .await
                        .map(|(_, v)| v.tag)
                        .map_err(|e| PushError::new(Self::map_send(e.code()), e.code()))
                }
            };
        let end = if sent.is_ok() { "done" } else { "failed" };
        let _ = store::set_copy_op(pool, &own_op, end, None).await;
        sent
    }

    /// E.3: holds the update and raises a card that names the change in audience.
    async fn hold_for_audience(
        &self,
        copy: PublishedCopy,
        version: &str,
        now: AudienceRow,
    ) -> Update {
        let Ok(pool) = self.pool() else {
            return Update::Failed;
        };
        if copy.state == PublishedCopyState::Current {
            let _ = self
                .deps
                .files
                .set_published_copy_state(copy.id, PublishedCopyState::OutOfDate, None)
                .await;
        }
        let info = self.info_for(&copy).await;
        let place = destination_line(&copy.destination);
        let line = if now.everyone_who_can_open {
            text::audience_unknown(&self.deps.localisation, &place)
        } else {
            now.line.clone()
        };
        let changes = text::cut(
            &format!(
                "More people can now see where the copy of {} lives: {line}. It will be updated with the changes.",
                info.original_name
            ),
            1000,
        );
        let affects = vec![AffectedItem::Record {
            label: text::cut(&format!("{} in {place}", info.file_name), 200),
        }];
        let Some(workspace) = self.workspace_of_copy(&copy).await else {
            // No card can be raised without a workspace; the copy stays out of date, never updated unseen.
            tracing::warn!(
                component = "push",
                published_copy_id = copy.id.to_string().as_str(),
                operation = "audience_hold",
                result = "PUSH_AUDIENCE_GREW",
                "push.copy"
            );
            return Update::Held;
        };
        let request = ApprovalRequest {
            workspace_id: workspace,
            asked_by: ApprovalAsker::Person,
            question: text::cut(
                &format!(
                    "Update the copy of {} for a larger audience",
                    info.original_name
                ),
                119,
            ) + "?",
            verb: text::UPDATE_VERB.to_owned(),
            where_line: text::cut(&format!("{place} ({line})"), 200),
            affects: affects.clone(),
            changes: changes.clone(),
            content: None,
            reject_style: RejectStyle::Reject,
            show_details: false,
            proposal: ActionProposal {
                action: text::COPY_ACTION.to_owned(),
                destination: text::cut(&place, 2048),
                affects,
                consequence: changes,
            },
            asker: None,
            leaves_company: false,
        };
        match self.deps.approvals.request(request).await {
            Ok(card) => {
                let _ = store::insert_copy_op(
                    pool,
                    copy.id,
                    "audience_hold",
                    Some(card.approval_id),
                    version,
                    "awaiting_approval",
                )
                .await;
                tracing::info!(
                    component = "push",
                    published_copy_id = copy.id.to_string().as_str(),
                    operation = "audience_hold",
                    result = "PUSH_AUDIENCE_GREW",
                    from_version = version,
                    "push.copy"
                );
                Update::Held
            }
            Err(_) => Update::Failed,
        }
    }

    /// E.3: every copy of `original` that follows `branch` (or no branch), from `version`.
    pub(crate) async fn update_copies_for(
        &self,
        original: CopyOriginal,
        version: &str,
        branch: Option<&str>,
    ) {
        let Ok(copies) = self.deps.files.published_copies_of(&original).await else {
            return;
        };
        let (mut updated, mut held, mut failed, mut seen) = (0u32, 0u32, 0u32, 0u32);
        for copy in copies {
            if copy.followed_branch.as_deref() != branch {
                continue;
            }
            // A linked folder's copy is sent by the Confluence Adapter only (ADR-029).
            // spec: ordin-desktop/components/PushService#SA-PUSH-LINKED-01
            if matches!(copy.published_via, PublishedVia::ConfluenceLink { .. }) {
                continue;
            }
            if !matches!(
                copy.state,
                PublishedCopyState::Current | PublishedCopyState::OutOfDate
            ) {
                continue;
            }
            seen += 1;
            if !copy.keep_up_to_date {
                if copy.state == PublishedCopyState::Current {
                    let _ = self
                        .deps
                        .files
                        .set_published_copy_state(copy.id, PublishedCopyState::OutOfDate, None)
                        .await;
                }
                continue;
            }
            match self.update_copy(copy, version, true, None).await {
                Update::Updated => updated += 1,
                Update::Held => held += 1,
                Update::Failed | Update::ChangedAtDestination => failed += 1,
            }
        }
        if seen > 0 {
            self.notify(PushNotice::CopiesUpdated {
                original,
                updated,
                held,
                failed,
                from_version: version.to_owned(),
            });
        }
    }

    pub(crate) async fn on_repository_change(&self, change: RepositoryOriginChange) {
        let original = CopyOriginal::PlaceFile {
            place_id: change.place_id,
            path: change.path,
        };
        self.update_copies_for(original, &change.commit, Some(&change.branch))
            .await;
    }

    pub(crate) async fn on_place_change(&self, change: PlaceChange) {
        let (false, Some(version)) = (change.by_ordin, change.version) else {
            return;
        };
        let version = version.to_string();
        self.update_copies_for(
            CopyOriginal::PlaceFile {
                place_id: change.place_id,
                path: change.path.clone(),
            },
            &version,
            None,
        )
        .await;
        let pages = match self.pool() {
            Ok(pool) => store::pages_made_from(pool, change.place_id, &change.path)
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        for knowledge_page_id in pages {
            self.update_copies_for(
                CopyOriginal::KnowledgePage { knowledge_page_id },
                &version,
                None,
            )
            .await;
        }
    }

    /// The 15-minute comparison (Configuration): a copy whose original moved while nobody was watching.
    /// A copy is behind when the original's current File Service version is not the one published.
    pub(crate) async fn compare_originals(&self) {
        let Ok(pool) = self.pool() else {
            return;
        };
        let Ok(ids) = store::copy_ids(pool).await else {
            return;
        };
        let mut done = std::collections::HashSet::new();
        for id in ids {
            let Ok(copy) = self.deps.files.published_copy(id).await else {
                continue;
            };
            if copy.followed_branch.is_some() || !copy.keep_up_to_date {
                continue;
            }
            let CopyOriginal::PlaceFile { place_id, path } = &copy.original else {
                continue;
            };
            if !done.insert((*place_id, path.clone())) {
                continue;
            }
            let Ok(Some(current)) = self.deps.files.current_version(*place_id, path).await else {
                continue;
            };
            let current = current.version.to_string();
            if copy.original_version_published.as_deref() != Some(current.as_str()) {
                self.update_copies_for(copy.original.clone(), &current, None)
                    .await;
            }
        }
    }

    /// C.5: after an approved push of a Markdown original, its copies follow it.
    pub(crate) async fn refresh_copies_after_push(
        &self,
        entry: &crate::model::WorkingCopyEntry,
        version: &str,
    ) {
        let original = CopyOriginal::PlaceFile {
            place_id: entry.place_id,
            path: entry.relative_path.clone(),
        };
        if let Some(branch) = entry.git_branch.as_deref() {
            self.update_copies_for(original, version, Some(branch))
                .await;
        } else if let Ok(Some(current)) = self
            .deps
            .files
            .current_version(entry.place_id, &entry.relative_path)
            .await
        {
            self.update_copies_for(original, &current.version.to_string(), None)
                .await;
        }
    }

    /// The version of the original a copy would be made from now.
    async fn current_original_version(
        &self,
        copy: &PublishedCopy,
        info: &CopyInfo,
    ) -> Result<String, PushError> {
        let (place, path) = match (&copy.original, &info.page_file) {
            (CopyOriginal::PlaceFile { place_id, path }, _) => (*place_id, path.clone()),
            (_, Some((place, path))) => (*place, path.clone()),
            _ => return Err(PushError::new("PUSH_ORIGINAL_UNREADABLE", "no page file")),
        };
        if let Some(branch) = &copy.followed_branch {
            return self
                .deps
                .projects
                .remote_state(place, branch)
                .await
                .map(|s| s.commit)
                .map_err(|e| PushError::new("PUSH_ORIGINAL_UNREADABLE", e.code()));
        }
        match self.deps.files.current_version(place, &path).await {
            Ok(Some(v)) => Ok(v.version.to_string()),
            _ => Err(PushError::new("PUSH_ORIGINAL_UNREADABLE", "no version")),
        }
    }

    /// E.5: one card for every out-of-date copy of the original that is not kept up to date.
    pub async fn update_copies(
        &self,
        original: CopyOriginal,
        workspace_id: WorkspaceId,
    ) -> Result<Option<ApprovalId>, PushError> {
        let pool = self.pool()?;
        let copies: Vec<PublishedCopy> = self
            .deps
            .files
            .published_copies_of(&original)
            .await
            .map_err(|e| PushError::new("PUSH_UNKNOWN", e.code()))?
            .into_iter()
            .filter(|c| c.state == PublishedCopyState::OutOfDate && !c.keep_up_to_date)
            // A linked folder's copy is not on this card: its card is the Confluence Adapter's.
            // spec: ordin-desktop/components/PushService#SA-PUSH-LINKED-03
            .filter(|c| !matches!(c.published_via, PublishedVia::ConfluenceLink { .. }))
            .collect();
        let Some(first) = copies.first() else {
            return Ok(None);
        };
        let info = self.info_for(first).await;
        let version = self.current_original_version(first, &info).await?;
        let mut affects: Vec<AffectedItem> = Vec::new();
        for c in copies.iter().take(50) {
            affects.push(AffectedItem::Record {
                label: text::cut(
                    &format!(
                        "{} in {}",
                        self.info_for(c).await.file_name,
                        destination_line(&c.destination)
                    ),
                    200,
                ),
            });
        }
        let changes = text::cut(
            &format!(
                "{} copies are brought up to date with {}.",
                copies.len(),
                info.original_name
            ),
            1000,
        );
        let where_line = text::cut(
            &copies
                .iter()
                .map(|c| destination_line(&c.destination))
                .collect::<Vec<_>>()
                .join(", "),
            200,
        );
        let request = ApprovalRequest {
            workspace_id,
            asked_by: ApprovalAsker::Person,
            question: text::cut(&format!("Update the copies of {}", info.original_name), 119) + "?",
            verb: text::UPDATE_VERB.to_owned(),
            where_line: where_line.clone(),
            affects: affects.clone(),
            changes: changes.clone(),
            content: None,
            reject_style: RejectStyle::Reject,
            show_details: false,
            proposal: ActionProposal {
                action: text::COPY_ACTION.to_owned(),
                destination: where_line,
                affects,
                consequence: changes,
            },
            asker: None,
            leaves_company: false,
        };
        let card = self
            .deps
            .approvals
            .request(request)
            .await
            .map_err(|e| PushError::new("PUSH_APPROVAL_FAILED", e.code()))?;
        for copy in &copies {
            store::insert_copy_op(
                pool,
                copy.id,
                "update_copies",
                Some(card.approval_id),
                &version,
                "awaiting_approval",
            )
            .await?;
        }
        Ok(Some(card.approval_id))
    }

    // ---- E.4 the person's actions ----

    pub async fn copy_action(&self, input: CopyActionInput) -> Result<PublishedCopy, PushError> {
        let files = &self.deps.files;
        let copy = files
            .published_copy(input.published_copy_id)
            .await
            .map_err(|_| PushError::new("PUSH_UNKNOWN", "no such copy"))?;
        // A linked folder's copy is sent by the Confluence Adapter only, from the folder's Ready to
        // publish list, under its agent test and hash-bound card. `TryAgain` is the one action here
        // that sends the original's bytes (a first publish or an update); the others send none.
        // spec: ordin-desktop/components/PushService#SA-PUSH-LINKED-02
        if matches!(copy.published_via, PublishedVia::ConfluenceLink { .. })
            && matches!(input.action, CopyAction::TryAgain)
        {
            return Err(PushError::new(
                "PUSH_COPY_LINKED",
                "a linked folder's copy is sent from its Ready to publish list",
            ));
        }
        let wrong_state = || PushError::new("PUSH_COPY_STATE", "action not allowed in this state");
        let info = self.info_for(&copy).await;
        let repository_original = copy.followed_branch.is_some();
        let original_path = match (&copy.original, &info.page_file) {
            (CopyOriginal::PlaceFile { place_id, path }, _) => Some((*place_id, path.clone())),
            (_, Some((place, path))) => Some((*place, path.clone())),
            _ => None,
        };
        match input.action {
            CopyAction::TryAgain => {
                if copy.state != PublishedCopyState::Failed {
                    return Err(wrong_state());
                }
                let version = match self.current_original_version(&copy, &info).await {
                    Ok(v) => v,
                    Err(error) => return Err(error),
                };
                if copy.destination_item_id.is_none() {
                    // The card was approved once; Try again repeats the same first send.
                    self.retry_first_publish(&copy, &version, &info).await;
                    return files
                        .published_copy(copy.id)
                        .await
                        .map_err(|_| PushError::new("PUSH_UNKNOWN", "copy vanished"));
                }
                let mut ready = copy.clone();
                ready.failure_reason = None;
                self.update_copy_from_failed(ready, &version).await;
            }
            CopyAction::BringItIn => {
                if copy.state != PublishedCopyState::ChangedAtDestination {
                    return Err(wrong_state());
                }
                if matches!(copy.format, CopyFormat::Pdf | CopyFormat::Slides) {
                    return Err(PushError::new("PUSH_COPY_CANNOT_BRING_BACK", "format"));
                }
                let markdown = self.bring_in(&copy).await?;
                let Some((place, path)) = original_path else {
                    return Err(PushError::new("PUSH_ORIGINAL_UNREADABLE", "no page file"));
                };
                files
                    .save_person_edit(place, &path, markdown.into_bytes())
                    .await
                    .map_err(|e| PushError::new("PUSH_STORE_FAILED", e.code()))?;
                let tag = self.destination_version(&copy).await?;
                self.set_current_at(&copy, tag).await;
            }
            CopyAction::LeaveIt | CopyAction::ReplaceAtNextUpdate => {
                if copy.state != PublishedCopyState::ChangedAtDestination {
                    return Err(wrong_state());
                }
                let tag = self.destination_version(&copy).await?;
                self.set_current_at(&copy, tag).await;
            }
            CopyAction::StopKeepingUpToDate => {
                if matches!(
                    copy.state,
                    PublishedCopyState::NotKeptUpToDate | PublishedCopyState::Removed
                ) {
                    return Err(wrong_state());
                }
                let mut stopped = copy.clone();
                stopped.keep_up_to_date = false;
                stopped.state = PublishedCopyState::NotKeptUpToDate;
                stopped.failure_reason = None;
                files
                    .record_published_copy(stopped)
                    .await
                    .map_err(|_| wrong_state())?;
                if let (true, Some((place, path)), Some(branch)) = (
                    repository_original,
                    original_path.clone(),
                    copy.followed_branch.clone(),
                ) {
                    let others = files
                        .published_copies_of(&copy.original)
                        .await
                        .unwrap_or_default();
                    let kept = others.iter().any(|c| {
                        c.id != copy.id
                            && c.keep_up_to_date
                            && c.followed_branch.as_deref() == Some(branch.as_str())
                    });
                    if !kept {
                        let _ = self
                            .deps
                            .projects
                            .unfollow_original(place, &path, &branch)
                            .await;
                    }
                }
            }
            CopyAction::KeepUpToDate => {
                if copy.state != PublishedCopyState::NotKeptUpToDate {
                    return Err(wrong_state());
                }
                let mut kept = copy.clone();
                kept.keep_up_to_date = true;
                kept.state = PublishedCopyState::OutOfDate;
                files
                    .record_published_copy(kept)
                    .await
                    .map_err(|_| wrong_state())?;
                if let (true, Some((place, path)), Some(branch)) = (
                    repository_original,
                    original_path,
                    copy.followed_branch.clone(),
                ) {
                    let _ = self
                        .deps
                        .projects
                        .follow_original(place, &path, &branch)
                        .await;
                }
            }
            CopyAction::Remove => {
                if copy.state == PublishedCopyState::Removed {
                    return Err(wrong_state());
                }
                self.remove_at_destination(&copy).await?;
            }
        }
        files
            .published_copy(copy.id)
            .await
            .map_err(|_| PushError::new("PUSH_UNKNOWN", "copy vanished"))
    }

    /// `try_again` on a first publish that failed: Failed -> Updating -> Current|Failed.
    async fn retry_first_publish(&self, copy: &PublishedCopy, version: &str, info: &CopyInfo) {
        let Ok(pool) = self.pool() else {
            return;
        };
        if self
            .deps
            .files
            .set_published_copy_state(copy.id, PublishedCopyState::Updating, None)
            .await
            .is_err()
        {
            return;
        }
        let Ok(op) =
            store::insert_copy_op(pool, copy.id, "first_publish", None, version, "sending").await
        else {
            return;
        };
        let key = ExternalActionKey::new();
        match self.publish_new(&op, copy, version, info, key).await {
            Ok((item_id, tag)) => {
                let mut done = copy.clone();
                done.state = PublishedCopyState::Current;
                done.destination_item_id = Some(item_id);
                done.destination_version_pushed = Some(tag);
                done.original_version_published = Some(version.to_owned());
                done.last_updated_at = Some(Timestamp::now());
                done.failure_reason = None;
                let _ = self.deps.files.record_published_copy(done).await;
                let _ = store::set_copy_op(pool, &op, "done", None).await;
            }
            Err(error) => {
                let _ = self
                    .deps
                    .files
                    .set_published_copy_state(
                        copy.id,
                        PublishedCopyState::Failed,
                        Some(Self::copy_error(error.code).to_owned()),
                    )
                    .await;
                let _ = store::set_copy_op(pool, &op, "failed", None).await;
            }
        }
    }

    /// `try_again` on a failed update: Failed -> Updating -> Current|Failed.
    async fn update_copy_from_failed(&self, copy: PublishedCopy, version: &str) {
        let _ = self.update_copy(copy, version, false, None).await;
    }

    async fn set_current_at(&self, copy: &PublishedCopy, tag: String) {
        let mut current = copy.clone();
        current.state = PublishedCopyState::Current;
        current.destination_version_pushed = Some(tag);
        current.failure_reason = None;
        if let Err(error) = self.deps.files.record_published_copy(current).await {
            tracing::warn!(
                component = "push",
                code = error.code(),
                "push.copy_record_failed"
            );
        }
    }

    /// E.4 *bring it in*: the destination's Markdown (page: storage format; Word: the Craft Toolkit).
    async fn bring_in(&self, copy: &PublishedCopy) -> Result<String, PushError> {
        let Some(item) = copy.destination_item_id.clone() else {
            return Err(PushError::new(
                "PUSH_UNKNOWN",
                "copy has no destination item",
            ));
        };
        match (&copy.destination, copy.format) {
            (CopyDestination::ConfluencePage { .. }, _) => {
                let confluence =
                    self.deps.confluence.as_ref().ok_or_else(|| {
                        PushError::new("PUSH_CONFLUENCE_UNAVAILABLE", "no adapter")
                    })?;
                let fetched = confluence
                    .fetch(&RemoteItemRef::Page { page_id: item })
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                let xhtml = String::from_utf8(fetched.bytes)
                    .map_err(|_| PushError::new("PUSH_DESTINATION_REFUSED", "not text"))?;
                ordin_confluence::ConfluenceAdapter::storage_to_markdown(&xhtml)
                    .map_err(|e| provider_error(e.code()))
            }
            (CopyDestination::SharepointFile { library, .. }, CopyFormat::Word) => {
                let fetched = self
                    .deps
                    .sharepoint
                    .fetch(&SharePointItemRef {
                        drive_id: library.clone(),
                        item_id: item,
                    })
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                self.deps
                    .craft
                    .to_markdown(CopyFormat::Word, fetched.bytes)
                    .await
                    .map_err(|e| PushError::new("PUSH_COPY_MAKER_FAILED", e.code()))
            }
            _ => Err(PushError::new("PUSH_COPY_CANNOT_BRING_BACK", "format")),
        }
    }

    /// E.4 *remove*: deletes at the destination under a key, then `removed`.
    async fn remove_at_destination(&self, copy: &PublishedCopy) -> Result<(), PushError> {
        let pool = self.pool()?;
        let Some(item) = copy.destination_item_id.clone() else {
            // Never published: nothing at the destination.
            return self
                .deps
                .files
                .set_published_copy_state(copy.id, PublishedCopyState::Removed, None)
                .await
                .map(|_| ())
                .map_err(|_| PushError::new("PUSH_COPY_STATE", "cannot remove"));
        };
        let key = ExternalActionKey::new();
        let op = store::insert_copy_op(
            pool,
            copy.id,
            "remove",
            None,
            copy.original_version_published.as_deref().unwrap_or("-"),
            "sending",
        )
        .await?;
        store::set_copy_op(pool, &op, "sending", Some(key)).await?;
        let expected = copy.destination_version_pushed.clone();
        let deleted: Result<(), (String, &'static str)> = match &copy.destination {
            CopyDestination::SharepointFile { library, .. } => self
                .deps
                .sharepoint
                .delete(
                    &SharePointItemRef {
                        drive_id: library.clone(),
                        item_id: item,
                    },
                    expected.as_deref(),
                    ExternalWrite {
                        key,
                        run_id: None,
                        kind: ExternalActionKind::CopyPublish,
                    },
                )
                .await
                .map_err(|e| (e.code().to_owned(), Self::map_send(e.code()))),
            CopyDestination::ConfluencePage { .. }
            | CopyDestination::ConfluenceAttachment { .. } => {
                let confluence =
                    self.deps.confluence.as_ref().ok_or_else(|| {
                        PushError::new("PUSH_CONFLUENCE_UNAVAILABLE", "no adapter")
                    })?;
                let reference = match &copy.destination {
                    CopyDestination::ConfluenceAttachment { parent_page_id, .. } => {
                        RemoteItemRef::Attachment {
                            page_id: parent_page_id.clone(),
                            attachment_id: item,
                        }
                    }
                    _ => RemoteItemRef::Page { page_id: item },
                };
                confluence
                    .delete(&reference, expected.as_deref(), key, None)
                    .await
                    .map_err(|e| (e.code().to_owned(), Self::map_send(e.code())))
            }
        };
        match deleted {
            Ok(()) => {
                store::set_copy_op(pool, &op, "done", None).await?;
                self.deps
                    .files
                    .set_published_copy_state(copy.id, PublishedCopyState::Removed, None)
                    .await
                    .map(|_| ())
                    .map_err(|_| PushError::new("PUSH_COPY_STATE", "cannot remove"))
            }
            Err((provider, code)) => {
                store::set_copy_op(pool, &op, "failed", None).await?;
                if provider == "SP_ORIGIN_CHANGED" || provider == "CONF_ORIGIN_CHANGED" {
                    // The destination changed since ordin pushed: nothing is removed.
                    let _ = self
                        .deps
                        .files
                        .set_published_copy_state(
                            copy.id,
                            PublishedCopyState::ChangedAtDestination,
                            None,
                        )
                        .await;
                    return Ok(());
                }
                Err(PushError::new(code, provider))
            }
        }
    }

    // ---- G. recovery ----

    /// G.1 for a copy operation: an update that arrived is recorded; anything else waits for the person.
    pub(crate) async fn recover_copy_operation(&self, op: CopyOpRow) -> Result<(), PushError> {
        let pool = self.pool()?;
        let Ok(copy) = self.deps.files.published_copy(op.published_copy_id).await else {
            return Ok(());
        };
        if op.state != "sending" {
            return Ok(());
        }
        let key = op.action_key;
        let arrived = if copy.destination_item_id.is_some() && op.kind != "first_publish" {
            match self.destination_version(&copy).await {
                Ok(tag) if Some(&tag) != copy.destination_version_pushed.as_ref() => Some(tag),
                _ => None,
            }
        } else {
            None
        };
        match arrived {
            Some(tag) => {
                let mut current = copy.clone();
                if current.state != PublishedCopyState::Updating {
                    let _ = self
                        .deps
                        .files
                        .set_published_copy_state(copy.id, PublishedCopyState::Updating, None)
                        .await;
                }
                current.state = PublishedCopyState::Current;
                current.destination_version_pushed = Some(tag);
                current.original_version_published = Some(op.original_version.clone());
                current.failure_reason = None;
                let _ = self.deps.files.record_published_copy(current).await;
                store::set_copy_op(pool, &op.id, "done", None).await?;
                tracing::info!(
                    component = "push",
                    action_key = key.map(|k| k.to_string()).as_deref(),
                    origin_kind = "copy",
                    "push.recovered_arrived"
                );
            }
            None => {
                if matches!(
                    copy.state,
                    PublishedCopyState::Updating | PublishedCopyState::AwaitingApproval
                ) {
                    let _ = self
                        .deps
                        .files
                        .set_published_copy_state(
                            copy.id,
                            PublishedCopyState::Failed,
                            Some(Self::copy_error("PUSH_OUTCOME_UNKNOWN").to_owned()),
                        )
                        .await;
                }
                store::set_copy_op(pool, &op.id, "failed", None).await?;
                tracing::info!(
                    component = "push",
                    action_key = key.map(|k| k.to_string()).as_deref(),
                    origin_kind = "copy",
                    "push.unknown_outcome"
                );
            }
        }
        Ok(())
    }
}
