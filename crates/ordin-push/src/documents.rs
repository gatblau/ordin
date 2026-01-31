//! Documents back to their origins (PushService.md Internal Logic A, B, C and D; ADR-029, ADR-035).

use std::time::Instant;

use futures::StreamExt;
use ordin_approval::ApprovalRequest;
use ordin_confluence::RemoteItemRef;
use ordin_journal::NewExternalAction;
use ordin_sharepoint::{ExternalWrite, SharePointItemRef};
use ordin_types::domain::places::{
    CopyDestination, CopyOriginal, PendingPush, PendingPushState, SourceLinkKind, WorkingCopyState,
};
use ordin_types::domain::runs::{ApprovalState, ExternalActionKind};
use ordin_types::wire::approval::{
    AffectedItem, ApprovalAsker, ApprovalContent, ContentRef, RejectStyle,
};
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{
    CodedError, ExternalActionKey, PendingPushId, RunId, SourceLinkId, WorkingCopyItemId,
    WorkspaceId,
};

use crate::error::{classify_send, PushError};
use crate::gitops::{self, Creds, GitFailure, GitStep};
use crate::model::{
    ChooseVersionInput, HeldChoice, PushNotice, PushProposal, StoreFailure, TryAgainInput,
    WorkingCopyEntry,
};
use crate::service::PushService;
use crate::store::{self, ItemRow, PushRow};
use crate::text;

/// Origin checks run at most this many at once (Performance, *chosen here*).
const CHECK_CONCURRENCY: usize = 4;
/// `affects` of a card holds 1–50 items (2C card limits).
const MAX_AFFECTS: usize = 50;
/// `Files.files` of a card holds 1–100 references.
const MAX_FILES: usize = 100;

/// What the origin said about one item (A.3a).
pub(crate) struct Probe {
    /// `None` for a local folder: the folder is the origin and there is nothing to compare.
    pub version: Option<String>,
    pub sharepoint: Option<SharePointItemRef>,
    pub confluence: Option<RemoteItemRef>,
}

pub(crate) fn provider_error(code: &str) -> PushError {
    PushError::new(classify_send(code), code.to_owned())
}

fn store_error(failure: StoreFailure) -> PushError {
    PushError::store(failure.retryable, failure.code)
}

/// The outcome of one item's send (C.4 to C.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ItemOutcome {
    Pushed,
    OriginChanged,
    Failed(&'static str),
}

/// One item as a card describes it.
pub(crate) struct CardItem {
    pub entry: WorkingCopyEntry,
    pub summary: String,
}

fn is_markdown(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".md")
}

pub(crate) fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

/// The approved-version code (C.2): the document changed on this computer after the card was approved.
const APPROVED_VERSION_CHANGED: &str = "PUSH_APPROVED_VERSION_CHANGED";

/// C.2 (Git): true only when the file at `relative` under `folder` holds exactly the approved bytes.
/// An unreadable file is not the approved version.
fn file_is(folder: &std::path::Path, relative: &str, approved: &[u8]) -> bool {
    std::fs::read(folder.join(relative)).is_ok_and(|on_disk| on_disk == approved)
}

fn destination_text(destination: &CopyDestination) -> String {
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

impl PushService {
    // ---- A. Propose ----

    /// A: raises one approval card for the changed documents of one request.
    // spec: ordin-desktop/components/PushService#SA-RECON-08
    pub async fn propose(&self, proposal: PushProposal) -> Result<PendingPush, PushError> {
        let pool = self.pool()?;
        // A.1: the 2C `PendingPush` rules.
        let chars = proposal.request.chars().count();
        if !(1..=4000).contains(&chars) {
            return Err(PushError::new("PUSH_REQUEST_INVALID", "request length"));
        }
        if !(1..=200).contains(&proposal.items.len()) {
            return Err(PushError::new("PUSH_ITEMS_OUT_OF_RANGE", "item count"));
        }
        for item in &proposal.items {
            let n = item.summary.chars().count();
            if !(1..=120).contains(&n) || item.summary.contains(['\n', '\r']) {
                return Err(PushError::new("PUSH_SUMMARY_INVALID", "summary"));
            }
        }
        let mut seen = std::collections::HashSet::new();
        for item in &proposal.items {
            if !seen.insert(item.working_copy_item_id) {
                return Err(PushError::new(
                    "PUSH_ITEM_ALREADY_PENDING",
                    "item listed twice",
                ));
            }
        }
        // D: a held push is superseded when the Ordin Host proposes the same items again.
        for item in &proposal.items {
            if let Some(held) =
                store::push_holding(pool, item.working_copy_item_id, &["held"]).await?
            {
                self.supersede(held).await?;
            }
        }
        for item in &proposal.items {
            if store::push_holding(
                pool,
                item.working_copy_item_id,
                &["proposed", "held", "approved"],
            )
            .await?
            .is_some()
            {
                return Err(PushError::new(
                    "PUSH_ITEM_ALREADY_PENDING",
                    "item in an open push",
                ));
            }
        }
        // A.2
        let mut entries = Vec::with_capacity(proposal.items.len());
        for item in &proposal.items {
            let entry = self
                .deps
                .working_copy
                .entry(item.working_copy_item_id)
                .await
                .map_err(store_error)?;
            if entry.item.state != WorkingCopyState::Changed {
                return Err(PushError::new(
                    "PUSH_ITEM_NOT_CHANGED",
                    "item has no change",
                ));
            }
            entries.push(entry);
        }
        // A.3: concurrent origin checks, at most four at once.
        let probes: Vec<Result<Probe, PushError>> = futures::stream::iter(0..entries.len())
            .map(|index| self.read_origin(&entries[index]))
            .buffered(CHECK_CONCURRENCY)
            .collect()
            .await;
        let mut kept: Vec<usize> = Vec::new();
        let mut origin_changed = Vec::new();
        let mut failed = Vec::new();
        for (index, probe) in probes.into_iter().enumerate() {
            let entry = &entries[index];
            match probe {
                Ok(p) => match p.version {
                    Some(version) if version != entry.item.origin_version_read => {
                        self.deps
                            .working_copy
                            .set_state(entry.item.id, WorkingCopyState::OriginChanged, None)
                            .await
                            .map_err(store_error)?;
                        origin_changed.push(entry.item.id);
                    }
                    _ => kept.push(index),
                },
                // The document is gone, or cannot be pushed from here: left out and reported.
                Err(error)
                    if matches!(
                        error.code,
                        "PUSH_ORIGIN_NOT_FOUND" | "PUSH_GIT_BRANCH_UNKNOWN"
                    ) =>
                {
                    failed.push((entry.item.id, error.code));
                }
                // An unreachable origin keeps the item in; it is re-checked before its send (C.1).
                Err(_) => kept.push(index),
            }
        }
        // A.4
        if kept.is_empty() {
            return Err(PushError::new(
                "PUSH_ALL_ORIGINS_CHANGED",
                "no item left to push",
            ));
        }
        let kept_items: Vec<_> = kept.iter().map(|i| proposal.items[*i].clone()).collect();
        let kept_entries: Vec<&WorkingCopyEntry> = kept.iter().map(|i| &entries[*i]).collect();
        let mut origins: Vec<SourceLinkId> = Vec::new();
        for entry in &kept_entries {
            if !origins.contains(&entry.item.source_link_id) {
                origins.push(entry.item.source_link_id);
            }
        }
        let card_items: Vec<CardItem> = kept_entries
            .iter()
            .zip(&kept_items)
            .map(|(entry, item)| CardItem {
                entry: (*entry).clone(),
                summary: item.summary.clone(),
            })
            .collect();
        let request = self
            .build_card(
                proposal.workspace_id,
                proposal.run_id,
                &card_items,
                kept_items.len() == 1 && kept_items[0].change_view.is_some(),
                kept_items.first().and_then(|i| i.change_view),
            )
            .await;
        // A.7: rows first, then the working-copy states, then the card.
        let kept_proposal = PushProposal {
            items: kept_items.clone(),
            ..proposal.clone()
        };
        let id = PendingPushId::new();
        let local_versions: Vec<u32> = kept_entries.iter().map(|e| e.item.local_version).collect();
        store::insert_push(pool, &kept_proposal, id, &origins, &local_versions).await?;
        let mut moved = Vec::new();
        for entry in &kept_entries {
            match self
                .deps
                .working_copy
                .set_state(entry.item.id, WorkingCopyState::AwaitingPush, None)
                .await
            {
                Ok(()) => moved.push(entry.item.id),
                Err(failure) => {
                    self.revert_items(&moved).await;
                    let _ = store::delete_push(pool, id).await;
                    return Err(store_error(failure));
                }
            }
        }
        match self.deps.approvals.request(request).await {
            Ok(card) => store::set_push_approval(pool, id, card.approval_id).await?,
            Err(error) => {
                self.revert_items(&moved).await;
                let _ = store::delete_push(pool, id).await;
                return Err(PushError::new("PUSH_APPROVAL_FAILED", error.code()));
            }
        }
        tracing::info!(
            component = "push",
            pending_push_id = %id,
            workspace_id = %proposal.workspace_id,
            items = kept_items.len(),
            origins = origins.len(),
            "push.proposed"
        );
        if !origin_changed.is_empty() || !failed.is_empty() {
            self.notify(PushNotice::PushFinished {
                pending_push_id: id,
                workspace_id: proposal.workspace_id,
                pushed: Vec::new(),
                failed,
                origin_changed,
            });
        }
        store::get_push(pool, id)
            .await?
            .map(|row| row.push)
            .ok_or_else(|| PushError::new("PUSH_UNKNOWN", "push vanished"))
    }

    async fn revert_items(&self, items: &[WorkingCopyItemId]) {
        for id in items {
            if let Err(failure) = self
                .deps
                .working_copy
                .set_state(*id, WorkingCopyState::Changed, None)
                .await
            {
                tracing::warn!(
                    component = "push",
                    code = failure.code,
                    "push.revert_failed"
                );
            }
        }
    }

    /// A.3a: reads the origin version of one item.
    pub(crate) async fn read_origin(&self, entry: &WorkingCopyEntry) -> Result<Probe, PushError> {
        match entry.link_kind {
            SourceLinkKind::LocalFolder => Ok(Probe {
                version: None,
                sharepoint: None,
                confluence: None,
            }),
            SourceLinkKind::SharepointLocation => {
                let sp = &self.deps.sharepoint;
                let root = sp
                    .resolve_origin(&entry.origin_location)
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                let found = sp
                    .item_at(&root, &entry.relative_path)
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                let Some(found) = found else {
                    return Err(PushError::new("PUSH_ORIGIN_NOT_FOUND", "no such item"));
                };
                let Some(version) = found.version else {
                    return Err(PushError::new("PUSH_ORIGIN_NOT_FOUND", "no version"));
                };
                Ok(Probe {
                    version: Some(version.tag),
                    sharepoint: Some(found.item),
                    confluence: None,
                })
            }
            SourceLinkKind::ConfluenceLocation => {
                let Some(confluence) = self.deps.confluence.as_ref() else {
                    return Err(PushError::new("PUSH_CONFLUENCE_UNAVAILABLE", "no adapter"));
                };
                let item = confluence
                    .item_for_path(entry.item.source_link_id, &entry.item.origin_path)
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                let version = confluence
                    .item_version(&item)
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                Ok(Probe {
                    version: Some(version.tag),
                    sharepoint: None,
                    confluence: Some(item),
                })
            }
            SourceLinkKind::GitRepositoryFolder => {
                let Some(branch) = entry.git_branch.as_deref() else {
                    return Err(PushError::new("PUSH_GIT_BRANCH_UNKNOWN", "no branch"));
                };
                let state = self
                    .deps
                    .projects
                    .remote_state(entry.place_id, branch)
                    .await
                    .map_err(|e| provider_error(e.code()))?;
                Ok(Probe {
                    version: Some(state.commit),
                    sharepoint: None,
                    confluence: None,
                })
            }
            // A repository folder linked without a clone and a Jira issue are not sent back from here.
            SourceLinkKind::GitRemote | SourceLinkKind::JiraIssue => Err(PushError::new(
                "PUSH_DESTINATION_REFUSED",
                "this kind of link is not pushed",
            )),
        }
    }

    /// A.6: the approval request for the items of one push.
    pub(crate) async fn build_card(
        &self,
        workspace_id: WorkspaceId,
        run_id: Option<RunId>,
        items: &[CardItem],
        single_change_view: bool,
        change_view: Option<ordin_types::ArtefactId>,
    ) -> ApprovalRequest {
        let mut origins: Vec<String> = Vec::new();
        for item in items {
            if !origins.contains(&item.entry.display_name) {
                origins.push(item.entry.display_name.clone());
            }
        }
        let question = if origins.len() == 1 {
            text::question(&self.deps.localisation, items.len(), Some(&origins[0]))
        } else {
            text::question(&self.deps.localisation, items.len(), None)
        };
        // PS-CARD-1: where names the service with the link; each affected file is the document itself.
        // spec: ordin-desktop/components/PushService#PS-CARD-1
        let mut wheres: Vec<String> = Vec::new();
        for item in items {
            let place = where_of(&item.entry);
            if !wheres.contains(&place) {
                wheres.push(place);
            }
        }
        let where_line = text::cut(&wheres.join(", "), 200);
        let mut affects: Vec<AffectedItem> = Vec::new();
        for item in items {
            affects.push(AffectedItem::File {
                label: text::cut(&document_name(&item.entry), 200),
                version: Some(item.entry.item.local_version.to_string()),
                place_id: Some(item.entry.place_id),
            });
            // ADR-069: the card updates the copies with the original.
            if is_markdown(&item.entry.relative_path) {
                let original = CopyOriginal::PlaceFile {
                    place_id: item.entry.place_id,
                    path: item.entry.relative_path.clone(),
                };
                if let Ok(copies) = self.deps.files.published_copies_of(&original).await {
                    for copy in copies {
                        if copy.state == ordin_types::domain::places::PublishedCopyState::Removed {
                            continue;
                        }
                        affects.push(AffectedItem::Record {
                            label: text::cut(
                                &format!(
                                    "Also updates: {} in {}",
                                    file_name(&item.entry.relative_path),
                                    destination_text(&copy.destination)
                                ),
                                200,
                            ),
                        });
                    }
                }
            }
        }
        if affects.len() > MAX_AFFECTS {
            let extra = affects.len() - (MAX_AFFECTS - 1);
            affects.truncate(MAX_AFFECTS - 1);
            affects.push(AffectedItem::Record {
                label: format!("and {extra} more"),
            });
        }
        let summaries: Vec<&str> = items.iter().map(|i| i.summary.as_str()).collect();
        let changes = text::cut(&summaries.join("; "), 1000);
        let content = match (single_change_view, change_view) {
            (true, Some(artefact_id)) => ApprovalContent::Changes {
                label: "See the changes".to_owned(),
                change_set: ContentRef::Artefact { artefact_id },
            },
            _ => ApprovalContent::Files {
                label: "See the files".to_owned(),
                files: items
                    .iter()
                    .take(MAX_FILES)
                    .map(|i| ContentRef::PlaceFile {
                        place_id: i.entry.place_id,
                        relative_path: i.entry.relative_path.clone(),
                    })
                    .collect(),
            },
        };
        ApprovalRequest {
            workspace_id,
            asked_by: run_id.map_or(ApprovalAsker::Person, |run_id| ApprovalAsker::Run {
                run_id,
            }),
            question,
            verb: text::verb(&self.deps.localisation),
            where_line: where_line.clone(),
            affects: affects.clone(),
            changes: changes.clone(),
            content: Some(content),
            reject_style: RejectStyle::Reject,
            show_details: true,
            proposal: ActionProposal {
                action: text::SHARE_ACTION.to_owned(),
                destination: where_line,
                affects,
                consequence: changes,
            },
            asker: None,
            leaves_company: false,
        }
    }

    pub async fn list_pending(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<PendingPush>, PushError> {
        let rows = store::list_pushes(self.pool()?, workspace_id).await?;
        Ok(rows.into_iter().map(|r| r.push).collect())
    }

    // ---- B. Decisions ----

    pub(crate) async fn decide_push(&self, push: PushRow, state: &str) -> Result<(), PushError> {
        let pool = self.pool()?;
        let id = push.push.id;
        tracing::info!(component = "push", pending_push_id = %id, state, "push.decision");
        match (state, push.push.state) {
            ("approved", PendingPushState::Proposed) => {
                if push.push.approval_id.is_none() {
                    return Err(PushError::new("PUSH_APPROVAL_MISSING", "no approval id"));
                }
                store::set_push_state(pool, id, PendingPushState::Approved).await?;
                self.send_all(id).await
            }
            // 3: a push in `held` stays `held` when its card expires.
            ("expired", PendingPushState::Held) => Ok(()),
            // spec: ordin-desktop/components/PushService#SA-RECON-12
            ("rejected" | "expired", PendingPushState::Proposed) => {
                store::set_push_state(pool, id, PendingPushState::Rejected).await?;
                for item in &push.items {
                    self.revert_items(&[item.id]).await;
                }
                store::set_push_state(pool, id, PendingPushState::Closed).await?;
                self.notify(PushNotice::PushFinished {
                    pending_push_id: id,
                    workspace_id: push.push.workspace_id,
                    pushed: Vec::new(),
                    failed: Vec::new(),
                    origin_changed: Vec::new(),
                });
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// C: sends every item that has not been attempted, then closes the push.
    pub(crate) async fn send_all(&self, id: PendingPushId) -> Result<(), PushError> {
        let pool = self.pool()?;
        let Some(push) = store::get_push(pool, id).await? else {
            return Ok(());
        };
        for item in &push.items {
            if item.pending_action_key.is_some()
                || item.external_action_key.is_some()
                || item.result_code.is_some()
            {
                continue;
            }
            self.send_item(&push, item).await;
        }
        self.finish_push(id).await
    }

    /// C.7: closes the push and reports every item's outcome.
    pub(crate) async fn finish_push(&self, id: PendingPushId) -> Result<(), PushError> {
        let pool = self.pool()?;
        let Some(push) = store::get_push(pool, id).await? else {
            return Ok(());
        };
        let mut pushed = Vec::new();
        let mut failed = Vec::new();
        let mut origin_changed = Vec::new();
        let mut needs_person = false;
        for item in &push.items {
            if item.external_action_key.is_some() {
                pushed.push(item.id);
            } else if let Some(code) = &item.result_code {
                let code = crate::error::ALL_CODES
                    .iter()
                    .copied()
                    .find(|c| c == code)
                    .unwrap_or("PUSH_DESTINATION_REFUSED");
                if code == "PUSH_ORIGIN_CHANGED" {
                    origin_changed.push(item.id);
                } else {
                    needs_person |= PushError::new(code, "").action.is_some();
                    failed.push((item.id, code));
                }
            } else if self.is_local_pushed(&push, item).await {
                pushed.push(item.id);
            }
        }
        store::set_push_state(pool, id, PendingPushState::Closed).await?;
        self.notify(PushNotice::PushFinished {
            pending_push_id: id,
            workspace_id: push.push.workspace_id,
            pushed,
            failed,
            origin_changed,
        });
        if needs_person {
            self.needs_you(push.push.workspace_id).await;
        }
        Ok(())
    }

    /// A local-folder item has no external action; it counts as pushed when its working-copy state says so.
    async fn is_local_pushed(&self, _push: &PushRow, item: &ItemRow) -> bool {
        matches!(
            self.deps.working_copy.entry(item.id).await,
            Ok(entry) if entry.item.state == WorkingCopyState::Pushed
        )
    }

    // ---- C. Send one item ----

    /// C: re-checks the origin, mints and records the key, sends, and records the outcome.
    pub(crate) async fn send_item(&self, push: &PushRow, item: &ItemRow) -> ItemOutcome {
        let started = Instant::now();
        let outcome = self.send_item_inner(push, item).await;
        let result = match &outcome {
            ItemOutcome::Pushed => "pushed",
            ItemOutcome::OriginChanged => "origin_changed",
            ItemOutcome::Failed(code) => code,
        };
        tracing::info!(
            component = "push",
            pending_push_id = %push.push.id,
            item = %item.id,
            result,
            duration_ms = started.elapsed().as_millis() as u64,
            "push.item_sent"
        );
        outcome
    }

    async fn fail(&self, push: &PushRow, item: &ItemRow, code: &'static str) -> ItemOutcome {
        if let Ok(pool) = self.pool() {
            let _ = store::set_item_result(pool, push.push.id, item.id, code).await;
            if code != "PUSH_OUTCOME_UNKNOWN" {
                self.refuse_sent(pool, push.push.id, item.id).await;
            }
        }
        if code == "PUSH_ORIGIN_CHANGED" {
            if let Err(failure) = self
                .deps
                .working_copy
                .set_state(item.id, WorkingCopyState::OriginChanged, None)
                .await
            {
                tracing::warn!(component = "push", code = failure.code, "push.state_failed");
            }
            return ItemOutcome::OriginChanged;
        }
        if code == APPROVED_VERSION_CHANGED {
            // Back to `changed`, so the document can be proposed again under a new card (C.2).
            if let Err(failure) = self
                .deps
                .working_copy
                .set_state(item.id, WorkingCopyState::Changed, None)
                .await
            {
                tracing::warn!(component = "push", code = failure.code, "push.state_failed");
            }
        }
        ItemOutcome::Failed(code)
    }

    /// PS-REF-1: the destination's answer is known and nothing arrived, so the item's external action, when
    /// one was begun, is `refused` rather than left `sent` for the start-up recovery to call unknown.
    // spec: ordin-desktop/components/PushService#PS-REF-1
    async fn refuse_sent(
        &self,
        pool: &sqlx::SqlitePool,
        push: PendingPushId,
        item: WorkingCopyItemId,
    ) {
        let key = match store::get_push(pool, push).await {
            Ok(Some(row)) => row
                .items
                .iter()
                .find(|i| i.id == item)
                .and_then(|i| i.pending_action_key),
            _ => None,
        };
        let Some(key) = key else { return };
        match self.deps.journal.refuse_external_action(key).await {
            Ok(_) => {}
            // No action was begun for this key (the send stopped before it).
            Err(e) if e.code() == "JRNL_NOT_FOUND" => {}
            Err(e) => {
                tracing::warn!(component = "push", action_key = %key, code = e.code(), "push.refuse_failed")
            }
        }
    }

    // spec: ordin-desktop/components/PushService#SA-RECON-03
    async fn send_item_inner(&self, push: &PushRow, item: &ItemRow) -> ItemOutcome {
        let Ok(pool) = self.pool() else {
            return ItemOutcome::Failed("PUSH_STORE_FAILED");
        };
        let entry = match self.deps.working_copy.entry(item.id).await {
            Ok(entry) => entry,
            Err(_) => return self.fail(push, item, "PUSH_STORE_FAILED").await,
        };
        // C.2, checked before anything is read or sent: only the version the person approved leaves.
        // spec: ordin-desktop/components/PushService#PUSH_APPROVED_VERSION_CHANGED
        if entry.item.local_version != item.ordins_local_version {
            return self.fail(push, item, APPROVED_VERSION_CHANGED).await;
        }
        // C.1
        let probe = match self.read_origin(&entry).await {
            Ok(probe) => probe,
            Err(error) => return self.fail(push, item, error.code).await,
        };
        if let Some(version) = &probe.version {
            if *version != entry.item.origin_version_read {
                return self.fail(push, item, "PUSH_ORIGIN_CHANGED").await;
            }
        }
        // C.2
        // FU-113: the approved version is read by number, so no edit slips in after the check above.
        let bytes = match self
            .deps
            .working_copy
            .content(item.id, Some(item.ordins_local_version))
            .await
        {
            Ok(bytes) => bytes,
            Err(_) => return self.fail(push, item, "PUSH_STORE_FAILED").await,
        };
        // C.3
        let key = ExternalActionKey::new();
        if store::set_item_pending_key(pool, push.push.id, item.id, key)
            .await
            .is_err()
        {
            return ItemOutcome::Failed("PUSH_STORE_FAILED");
        }
        let run_id = push.run_id;
        // C.4
        let sent: Result<String, &'static str> = match entry.link_kind {
            SourceLinkKind::SharepointLocation => match probe.sharepoint {
                Some(target) => self
                    .deps
                    .sharepoint
                    .upload_version(
                        &target,
                        bytes,
                        &entry.item.origin_version_read,
                        ExternalWrite {
                            key,
                            run_id,
                            kind: ExternalActionKind::OriginPush,
                        },
                    )
                    .await
                    .map(|v| v.tag)
                    .map_err(|e| classify_send(e.code())),
                None => Err("PUSH_ORIGIN_NOT_FOUND"),
            },
            SourceLinkKind::ConfluenceLocation => {
                match (self.deps.confluence.as_ref(), probe.confluence) {
                    (None, _) => Err("PUSH_CONFLUENCE_UNAVAILABLE"),
                    (Some(confluence), Some(RemoteItemRef::Page { page_id })) => {
                        match String::from_utf8(bytes) {
                            // The version passed is the one read, unchanged: the adapter adds 1 (R19).
                            // spec: ordin-desktop/components/PushService#SA-RECON-04
                            Ok(xhtml) => confluence
                                .update_page_storage(
                                    &page_id,
                                    &xhtml,
                                    &entry.item.origin_version_read,
                                    key,
                                    run_id,
                                )
                                .await
                                .map(|v| v.tag)
                                .map_err(|e| classify_send(e.code())),
                            Err(_) => Err("PUSH_DESTINATION_REFUSED"),
                        }
                    }
                    (Some(confluence), Some(RemoteItemRef::Attachment { page_id, .. })) => {
                        confluence
                            .attach(
                                &page_id,
                                &file_name(&entry.item.origin_path),
                                bytes,
                                Some(&entry.item.origin_version_read),
                                key,
                                run_id,
                            )
                            .await
                            .map(|(_, v)| v.tag)
                            .map_err(|e| classify_send(e.code()))
                    }
                    (Some(_), None) => Err("PUSH_ORIGIN_NOT_FOUND"),
                }
            }
            SourceLinkKind::GitRepositoryFolder => self.send_git(&entry, item, key, run_id).await,
            // The working copy is the origin: nothing leaves the computer.
            SourceLinkKind::LocalFolder => Ok(format!("fver:{}", entry.item.local_version)),
            SourceLinkKind::GitRemote | SourceLinkKind::JiraIssue => {
                Err("PUSH_DESTINATION_REFUSED")
            }
        };
        match sent {
            Ok(version) => {
                let local = entry.link_kind == SourceLinkKind::LocalFolder;
                // C.5
                if let Err(failure) = self
                    .deps
                    .working_copy
                    .set_state(item.id, WorkingCopyState::Pushed, Some(version.clone()))
                    .await
                {
                    tracing::warn!(component = "push", code = failure.code, "push.state_failed");
                }
                let _ =
                    store::set_item_pushed(pool, push.push.id, item.id, (!local).then_some(key))
                        .await;
                if let Err(error) = self
                    .deps
                    .files
                    .mark_edit_sent(entry.place_id, &entry.relative_path)
                    .await
                {
                    tracing::warn!(
                        component = "push",
                        code = error.code(),
                        "push.mark_sent_failed"
                    );
                }
                if is_markdown(&entry.relative_path) {
                    self.refresh_copies_after_push(&entry, &version).await;
                }
                ItemOutcome::Pushed
            }
            Err(code) => self.fail(push, item, code).await,
        }
    }

    /// C.4 (Git): commit exactly `relative_path`, push `refs/heads/<branch>` without force.
    async fn send_git(
        &self,
        entry: &WorkingCopyEntry,
        item: &ItemRow,
        key: ExternalActionKey,
        run_id: Option<RunId>,
    ) -> Result<String, &'static str> {
        let Some(branch) = entry.git_branch.clone() else {
            return Err("PUSH_GIT_BRANCH_UNKNOWN");
        };
        let folder = match self.deps.files.place(entry.place_id).await {
            Ok(place) => std::path::PathBuf::from(place.path),
            Err(error) => return Err(classify_send(error.code())),
        };
        let creds = match entry.git_host.as_deref() {
            Some(host) => match self.deps.github.git_credentials(host).await {
                Ok(c) => Some(Creds {
                    username: c.username.clone(),
                    token: c.token.expose().to_owned(),
                }),
                Err(error) => return Err(classify_send(error.code())),
            },
            None => None,
        };
        // C.2 (Git): the commit takes the file from disk, so the file must be the approved version.
        let approved = match self
            .deps
            .working_copy
            .content(item.id, Some(item.ordins_local_version))
            .await
        {
            Ok(bytes) => bytes,
            Err(_) => return Err("PUSH_STORE_FAILED"),
        };
        let message = format!("{}\n\nSent from ordin after approval", item.summary);
        let relative = entry.relative_path.clone();
        let commit_branch = branch.clone();
        let commit_folder = folder.clone();
        // The commit is local and harmless; the external action brackets what leaves the computer.
        // `None` is a file on disk that is not the approved version: nothing is committed.
        let committed = tokio::task::spawn_blocking(move || {
            if !file_is(&commit_folder, &relative, &approved) {
                return None;
            }
            Some(gitops::commit_paths(
                &commit_folder,
                Some(&commit_branch),
                &[relative.as_str()],
                &message,
            ))
        })
        .await;
        let oid = match committed {
            Ok(Some(Ok(oid))) => oid,
            Ok(None) => return Err(APPROVED_VERSION_CHANGED),
            Ok(Some(Err(GitStep::IdentityMissing))) => return Err("PUSH_GIT_IDENTITY_MISSING"),
            Ok(Some(Err(GitStep::WrongBranch))) => return Err("PUSH_GIT_BRANCH_UNKNOWN"),
            Ok(Some(Err(GitStep::Failed(failure)))) => return Err(failure.code()),
            Err(_) => return Err("PUSH_DESTINATION_REFUSED"),
        };
        let destination = format!(
            "{}/{}#{}",
            entry.git_host.as_deref().unwrap_or("git"),
            entry.git_repository.as_deref().unwrap_or("repository"),
            branch
        );
        if let Err(error) = self
            .deps
            .journal
            .begin_external_action(NewExternalAction {
                action_key: key,
                run_id,
                kind: ExternalActionKind::OriginPush,
                destination,
            })
            .await
        {
            return Err(classify_send(error.code()));
        }
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        let pushed = tokio::time::timeout(
            gitops::GIT_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let repo = git2::Repository::open(&folder)
                    .map_err(|e| GitFailure::Other(e.message().to_owned()))?;
                gitops::push_refspec(&repo, "origin", &refspec, creds.as_ref())
            }),
        )
        .await;
        match pushed {
            Ok(Ok(Ok(()))) => {
                if let Err(error) = self.deps.journal.confirm_external_action(key).await {
                    tracing::warn!(
                        component = "push",
                        code = error.code(),
                        "push.confirm_failed"
                    );
                }
                Ok(oid.to_string())
            }
            Ok(Ok(Err(failure))) => Err(failure.code()),
            Ok(Err(_)) => Err("PUSH_DESTINATION_REFUSED"),
            Err(_) => Err("PUSH_OUTCOME_UNKNOWN"),
        }
    }

    /// The person's Try again on one failed item (C.6): sent under the same approval.
    pub async fn try_again(&self, input: TryAgainInput) -> Result<PendingPush, PushError> {
        let pool = self.pool()?;
        let Some(push) = store::get_push(pool, input.pending_push_id).await? else {
            return Err(PushError::new("PUSH_UNKNOWN", "no such push"));
        };
        let Some(item) = push
            .items
            .iter()
            .find(|i| i.id == input.working_copy_item_id)
            .cloned()
        else {
            return Err(PushError::new("PUSH_UNKNOWN", "no such item"));
        };
        if item.external_action_key.is_some() || item.result_code.is_none() {
            return Err(PushError::new("PUSH_UNKNOWN", "nothing to try again"));
        }
        let approved = match push.push.approval_id {
            Some(approval) => self
                .deps
                .approvals
                .get(approval)
                .await
                .map(|r| r.state == ApprovalState::Approved)
                .unwrap_or(false),
            None => false,
        };
        if !approved {
            return Err(PushError::new("PUSH_APPROVAL_MISSING", "no approved card"));
        }
        let outcome = self.send_item(&push, &item).await;
        let (mut pushed, mut failed, mut origin_changed) = (Vec::new(), Vec::new(), Vec::new());
        match outcome {
            ItemOutcome::Pushed => pushed.push(item.id),
            ItemOutcome::OriginChanged => origin_changed.push(item.id),
            ItemOutcome::Failed(code) => failed.push((item.id, code)),
        }
        self.notify(PushNotice::PushFinished {
            pending_push_id: push.push.id,
            workspace_id: push.push.workspace_id,
            pushed,
            failed,
            origin_changed,
        });
        store::get_push(pool, push.push.id)
            .await?
            .map(|r| r.push)
            .ok_or_else(|| PushError::new("PUSH_UNKNOWN", "push vanished"))
    }

    // ---- D. Local edit while a push waits ----

    pub(crate) async fn on_local_change(&self, item: WorkingCopyItemId) {
        let Ok(pool) = self.pool() else {
            return;
        };
        let held = match store::push_holding(pool, item, &["proposed"]).await {
            Ok(Some(id)) => id,
            _ => return,
        };
        if let Err(error) = self.hold(held, item).await {
            tracing::warn!(component = "push", code = error.code, "push.hold_failed");
        }
    }

    async fn hold(&self, id: PendingPushId, item: WorkingCopyItemId) -> Result<(), PushError> {
        let pool = self.pool()?;
        let Some(push) = store::get_push(pool, id).await? else {
            return Ok(());
        };
        if push.push.state != PendingPushState::Proposed {
            return Ok(());
        }
        // Held first, so the `expired` decision that follows the withdrawal is read as a hold.
        store::set_push_state(pool, id, PendingPushState::Held).await?;
        if let Some(approval) = push.push.approval_id {
            if let Err(error) = self.deps.approvals.withdraw(approval).await {
                tracing::warn!(
                    component = "push",
                    code = error.code(),
                    "push.withdraw_failed"
                );
            }
        }
        tracing::info!(component = "push", pending_push_id = %id, item = %item, "push.held");
        self.notify(PushNotice::PushHeld {
            pending_push_id: id,
            item,
        });
        self.needs_you(push.push.workspace_id).await;
        Ok(())
    }

    /// Closes a held push that a new proposal replaces; its other items go back to `changed`.
    async fn supersede(&self, id: PendingPushId) -> Result<(), PushError> {
        let pool = self.pool()?;
        let Some(push) = store::get_push(pool, id).await? else {
            return Ok(());
        };
        store::set_push_state(pool, id, PendingPushState::Closed).await?;
        for item in &push.items {
            if let Ok(entry) = self.deps.working_copy.entry(item.id).await {
                if entry.item.state == WorkingCopyState::AwaitingPush {
                    self.revert_items(&[item.id]).await;
                }
            }
        }
        self.needs_you(push.push.workspace_id).await;
        Ok(())
    }

    /// D: the person's choice for one item of a held push.
    pub async fn choose_version(
        &self,
        input: ChooseVersionInput,
    ) -> Result<PendingPush, PushError> {
        let pool = self.pool()?;
        let Some(push) = store::get_push(pool, input.pending_push_id).await? else {
            return Err(PushError::new("PUSH_UNKNOWN", "no such push"));
        };
        let Some(item) = push
            .items
            .iter()
            .find(|i| i.id == input.working_copy_item_id)
            .cloned()
        else {
            return Err(PushError::new("PUSH_UNKNOWN", "no such item"));
        };
        if push.push.state != PendingPushState::Held {
            return Err(PushError::new("PUSH_NOT_HELD", "push is not held"));
        }
        match input.choice {
            HeldChoice::KeepMine => {
                store::delete_item(pool, push.push.id, item.id).await?;
                self.deps
                    .working_copy
                    .set_state(item.id, WorkingCopyState::Changed, None)
                    .await
                    .map_err(store_error)?;
            }
            HeldChoice::KeepOrdins => {
                self.deps
                    .working_copy
                    .restore_version(item.id, item.ordins_local_version)
                    .await
                    .map_err(store_error)?;
            }
            HeldChoice::ApplyOrdinsToMine => {
                // The push stays held; the Ordin Host re-runs the change and proposes again.
                self.notify(PushNotice::PushHeld {
                    pending_push_id: push.push.id,
                    item: item.id,
                });
                return Ok(push.push);
            }
        }
        let Some(fresh) = store::get_push(pool, push.push.id).await? else {
            return Err(PushError::new("PUSH_UNKNOWN", "push vanished"));
        };
        if fresh.items.is_empty() {
            store::set_push_state(pool, fresh.push.id, PendingPushState::Closed).await?;
            self.needs_you(fresh.push.workspace_id).await;
            return store::get_push(pool, fresh.push.id)
                .await?
                .map(|r| r.push)
                .ok_or_else(|| PushError::new("PUSH_UNKNOWN", "push vanished"));
        }
        let mut card_items = Vec::new();
        for row in &fresh.items {
            let entry = self
                .deps
                .working_copy
                .entry(row.id)
                .await
                .map_err(store_error)?;
            // FU-113: the new card is for the version now on it (a restored or edited one), so the
            // approved-version check at the send counts that version.
            if entry.item.local_version != row.ordins_local_version {
                store::set_item_local_version(
                    pool,
                    fresh.push.id,
                    row.id,
                    entry.item.local_version,
                )
                .await?;
            }
            card_items.push(CardItem {
                entry,
                summary: row.summary.clone(),
            });
        }
        let single = fresh.items.len() == 1 && fresh.items[0].change_view.is_some();
        let request = self
            .build_card(
                fresh.push.workspace_id,
                fresh.run_id,
                &card_items,
                single,
                fresh.items.first().and_then(|i| i.change_view),
            )
            .await;
        let card = self
            .deps
            .approvals
            .request(request)
            .await
            .map_err(|e| PushError::new("PUSH_APPROVAL_FAILED", e.code()))?;
        store::set_push_approval(pool, fresh.push.id, card.approval_id).await?;
        store::set_push_state(pool, fresh.push.id, PendingPushState::Proposed).await?;
        self.needs_you(fresh.push.workspace_id).await;
        store::get_push(pool, fresh.push.id)
            .await?
            .map(|r| r.push)
            .ok_or_else(|| PushError::new("PUSH_UNKNOWN", "push vanished"))
    }

    // ---- G. Recovery ----

    /// G.1 and G.2: settles what the previous run left, and resumes approved pushes never attempted.
    pub(crate) async fn recover_external_actions(&self) -> Result<(), PushError> {
        let pool = self.pool()?;
        let actions = self
            .deps
            .journal
            .unsettled_external_actions(&[
                ExternalActionKind::OriginPush,
                ExternalActionKind::CopyPublish,
                ExternalActionKind::PullRequestOpened,
            ])
            .await
            .map_err(|e| PushError::store(e.retryable(), e.code()))?;
        for action in actions {
            let key = action.action_key;
            if let Some((push_id, item)) = store::item_by_pending_key(pool, key).await? {
                self.recover_item(push_id, item, key).await?;
            } else if let Some(op) = store::copy_op_by_key(pool, key).await? {
                self.recover_copy_operation(op).await?;
            }
            // Submissions and team shares are settled by §G.3 and §I.7; keys held by no row are ignored.
        }
        // G.2: pushes in `proposed` or `held` keep waiting. An approved push whose send was
        // interrupted resumes with the items never attempted (nothing was begun for them).
        let approved = sqlx::query_scalar::<_, String>(
            "SELECT id FROM pending_pushes WHERE state = 'approved'",
        )
        .fetch_all(pool)
        .await?;
        for id in approved {
            if let Ok(id) = PendingPushId::parse(&id) {
                self.send_all(id).await?;
            }
        }
        // PS-REC-1: a `proposed` push whose card was decided while this service was not running (its
        // decision event was never delivered) is decided now as the card says; a pending card waits.
        // spec: ordin-desktop/components/PushService#PS-REC-1
        let proposed = sqlx::query_scalar::<_, String>(
            "SELECT id FROM pending_pushes WHERE state = 'proposed' AND approval_id IS NOT NULL",
        )
        .fetch_all(pool)
        .await?;
        for id in proposed {
            let Ok(id) = PendingPushId::parse(&id) else {
                continue;
            };
            let Some(push) = store::get_push(pool, id).await? else {
                continue;
            };
            let Some(approval) = push.push.approval_id else {
                continue;
            };
            let state = match self.deps.approvals.get(approval).await {
                Ok(card) => card.state,
                Err(_) => continue,
            };
            let decided = match state {
                ApprovalState::Approved => "approved",
                ApprovalState::Rejected => "rejected",
                ApprovalState::Expired => "expired",
                ApprovalState::Pending => continue,
            };
            tracing::info!(component = "push", pending_push_id = %id, state = decided, "push.recovered_decision");
            self.decide_push(push, decided).await?;
        }
        Ok(())
    }

    async fn recover_item(
        &self,
        push_id: PendingPushId,
        item: ItemRow,
        key: ExternalActionKey,
    ) -> Result<(), PushError> {
        let pool = self.pool()?;
        // The destination answered: its answer is known, so nothing is checked. A refusal left from
        // before PS-REF-1 is settled as refused.
        if item.external_action_key.is_some() {
            return Ok(());
        }
        if item
            .result_code
            .as_deref()
            .is_some_and(|c| c != "PUSH_OUTCOME_UNKNOWN")
        {
            let _ = self.deps.journal.refuse_external_action(key).await;
            return Ok(());
        }
        let entry = match self.deps.working_copy.entry(item.id).await {
            Ok(entry) => entry,
            Err(_) => return Ok(()),
        };
        let arrived = self.change_arrived(&entry).await;
        if let Some(version) = arrived {
            let _ = self
                .deps
                .working_copy
                .set_state(item.id, WorkingCopyState::Pushed, Some(version))
                .await;
            store::set_item_pushed(pool, push_id, item.id, Some(key)).await?;
            let _ = self
                .deps
                .files
                .mark_edit_sent(entry.place_id, &entry.relative_path)
                .await;
            tracing::info!(component = "push", action_key = %key, origin_kind = origin_kind(&entry), "push.recovered_arrived");
        } else {
            store::set_item_result(pool, push_id, item.id, "PUSH_OUTCOME_UNKNOWN").await?;
            tracing::info!(component = "push", action_key = %key, origin_kind = origin_kind(&entry), "push.unknown_outcome");
        }
        if let Some(push) = store::get_push(pool, push_id).await? {
            let settled = push
                .items
                .iter()
                .all(|i| i.external_action_key.is_some() || i.result_code.is_some());
            if settled && push.push.state == PendingPushState::Approved {
                self.finish_push(push_id).await?;
            }
        }
        Ok(())
    }

    /// G.1: re-reads the destination; `Some(version)` when the item's change is already there.
    async fn change_arrived(&self, entry: &WorkingCopyEntry) -> Option<String> {
        let local = self
            .deps
            .working_copy
            .content(entry.item.id, None)
            .await
            .ok()?;
        match entry.link_kind {
            SourceLinkKind::SharepointLocation => {
                let probe = self.read_origin(entry).await.ok()?;
                let version = probe.version?;
                if version == entry.item.origin_version_read {
                    return None;
                }
                let fetched = self.deps.sharepoint.fetch(&probe.sharepoint?).await.ok()?;
                (digest(&fetched.bytes) == digest(&local)).then_some(version)
            }
            SourceLinkKind::ConfluenceLocation => {
                let probe = self.read_origin(entry).await.ok()?;
                let version = probe.version?;
                if version == entry.item.origin_version_read {
                    return None;
                }
                let fetched = self
                    .deps
                    .confluence
                    .as_ref()?
                    .fetch(&probe.confluence?)
                    .await
                    .ok()?;
                (digest(&fetched.bytes) == digest(&local)).then_some(version)
            }
            SourceLinkKind::GitRepositoryFolder => {
                let branch = entry.git_branch.clone()?;
                let remote = self
                    .deps
                    .projects
                    .remote_state(entry.place_id, &branch)
                    .await
                    .ok()?;
                let folder = std::path::PathBuf::from(
                    self.deps.files.place(entry.place_id).await.ok()?.path,
                );
                let tip = remote.commit.clone();
                let contains = tokio::task::spawn_blocking(move || {
                    let repo = git2::Repository::open(folder).ok()?;
                    let local_tip = repo.refname_to_id(&format!("refs/heads/{branch}")).ok()?;
                    let remote_tip = git2::Oid::from_str(&tip).ok()?;
                    Some(gitops::contains_commit(&repo, remote_tip, local_tip))
                })
                .await
                .ok()
                .flatten()?;
                contains.then_some(remote.commit)
            }
            _ => None,
        }
    }
}

fn origin_kind(entry: &WorkingCopyEntry) -> &'static str {
    match entry.link_kind {
        SourceLinkKind::LocalFolder => "local_folder",
        SourceLinkKind::GitRepositoryFolder => "git",
        SourceLinkKind::SharepointLocation => "sharepoint",
        SourceLinkKind::ConfluenceLocation => "confluence",
        SourceLinkKind::GitRemote => "git_remote",
        SourceLinkKind::JiraIssue => "jira",
    }
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// PS-CARD-1: "SharePoint — <link>" or "Confluence — <link>"; a folder on the computer is its own name.
fn where_of(entry: &WorkingCopyEntry) -> String {
    match entry.link_kind {
        SourceLinkKind::SharepointLocation => format!("SharePoint \u{2014} {}", entry.display_name),
        SourceLinkKind::ConfluenceLocation => format!("Confluence \u{2014} {}", entry.display_name),
        _ => entry.display_name.clone(),
    }
}

/// PS-CARD-1: the document's own name — the last part of its origin path, without the
/// `.confluence.xhtml` a Confluence page's working-copy file carries; the link's name when the path
/// is empty.
fn document_name(entry: &WorkingCopyEntry) -> String {
    let last = entry
        .item
        .origin_path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or_default();
    let name = last.strip_suffix(".confluence.xhtml").unwrap_or(last);
    if name.is_empty() {
        entry.display_name.clone()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::file_is;

    #[test]
    fn only_the_approved_bytes_on_disk_are_the_approved_version() {
        let folder = std::env::temp_dir().join(format!("ordin-push-doc-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(folder.join("notes")).expect("temporary folder");
        std::fs::write(folder.join("notes/rates.md"), b"approved").expect("write");
        assert!(file_is(&folder, "notes/rates.md", b"approved"));
        std::fs::write(folder.join("notes/rates.md"), b"edited after approval").expect("write");
        assert!(!file_is(&folder, "notes/rates.md", b"approved"));
        assert!(
            !file_is(&folder, "notes/missing.md", b"approved"),
            "an unreadable file is not the approved version"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }
}
