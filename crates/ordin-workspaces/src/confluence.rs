//! Confluence links, the approvals that make them active and the linked-folder syncs
//! (WorkspaceService.md Internal Logic 8.2 to 8.5).

use ordin_files::PlaceChange;
use ordin_journal::{EventSink, JournalEvent};
use ordin_types::domain::places::{ConfluenceLink, ConfluenceLinkState, Grantee, WorkspaceState};
use ordin_types::domain::runs::ApprovalState;
use ordin_types::time::Timestamp;
use ordin_types::wire::events::EventKind;
use ordin_types::{ApprovalId, ConfluenceLinkId, PlaceId, WorkspaceId};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::error::{file_failed, WorkspaceError};
use crate::links::map_adapter;
use crate::log;
use crate::service::{lock, WorkspaceService};
use crate::store::{self, from_text, to_text};
use crate::types::ConfluencePublishing;
use crate::LINKED_FOLDER_DEBOUNCE;
use ordin_confluence::AutomaticPublishing;
use ordin_files::Place;

/// The body of `approval_decided` as Internal Logic 8.3 reads it.
#[derive(Debug, serde::Deserialize)]
struct ApprovalDecided {
    approval_id: ApprovalId,
    state: ApprovalState,
    #[allow(dead_code)]
    decided_on: String,
}

pub(crate) fn confluence_row(row: &SqliteRow) -> Result<ConfluenceLink, WorkspaceError> {
    Ok(ConfluenceLink {
        id: store::id_of(row, "id")?,
        workspace_id: store::id_of(row, "workspace_id")?,
        place_id: store::id_of(row, "place_id")?,
        folder: row.try_get("folder")?,
        confluence_space: row.try_get("confluence_space")?,
        parent_page_id: row.try_get("parent_page_id")?,
        audience: row.try_get("audience")?,
        approval_id: store::id_opt::<ApprovalId>(row, "approval_id")?,
        approved_at: store::ts_opt(row, "approved_at")?,
        state: from_text::<ConfluenceLinkState>(&row.try_get::<String, _>("state")?)?,
    })
}

/// `CFL_SPACE_INVALID`: 1–255 characters; `CFL_PARENT_INVALID`: 1–64 digits.
pub(crate) fn check_space_and_parent(
    space: &str,
    parent_page_id: &str,
) -> Result<(), WorkspaceError> {
    let space_chars = space.chars().count();
    if space_chars == 0 || space_chars > 255 {
        return Err(WorkspaceError::Invalid("CFL_SPACE_INVALID"));
    }
    if parent_page_id.is_empty()
        || parent_page_id.len() > 64
        || !parent_page_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(WorkspaceError::Invalid("CFL_PARENT_INVALID"));
    }
    Ok(())
}

/// `CFL_AUDIENCE_INVALID`: 1–500 characters.
pub(crate) fn check_audience(audience: &str) -> Result<(), WorkspaceError> {
    let audience_chars = audience.chars().count();
    if audience_chars == 0 || audience_chars > 500 {
        return Err(WorkspaceError::Invalid("CFL_AUDIENCE_INVALID"));
    }
    Ok(())
}

/// 2C: an `active` or `paused` link has its approval id and approval time (`CFL_NOT_APPROVED`).
pub(crate) fn check_approved(
    state: ConfluenceLinkState,
    approval_id: Option<ApprovalId>,
    approved_at: Option<Timestamp>,
) -> Result<(), WorkspaceError> {
    if state != ConfluenceLinkState::Proposed && (approval_id.is_none() || approved_at.is_none()) {
        return Err(WorkspaceError::Invalid("CFL_NOT_APPROVED"));
    }
    Ok(())
}

fn folder_ok(folder: &str) -> bool {
    !folder.starts_with('/')
        && !folder.contains('\\')
        && !folder.chars().any(char::is_control)
        && folder.len() <= 1024
        && !folder.split('/').any(|segment| segment == "..")
}

impl WorkspaceService {
    /// One Confluence link by its id, whatever its state; `WS_LINK_NOT_FOUND` for an unknown id.
    // spec: ordin-desktop/components/WorkspaceService#WS-R1-1
    pub async fn confluence_link(
        &self,
        id: ConfluenceLinkId,
    ) -> Result<ConfluenceLink, WorkspaceError> {
        self.load_confluence_link(id).await
    }

    async fn load_confluence_link(
        &self,
        id: ConfluenceLinkId,
    ) -> Result<ConfluenceLink, WorkspaceError> {
        let row = sqlx::query("SELECT * FROM confluence_links WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .ok_or(WorkspaceError::LinkNotFound)?;
        confluence_row(&row)
    }

    /// Internal Logic 8.2.
    pub async fn propose_confluence_link(
        &self,
        id: WorkspaceId,
        place_id: PlaceId,
        folder: String,
        location: ordin_confluence::ConfluenceLocation,
    ) -> Result<ConfluenceLink, WorkspaceError> {
        let workspace = self.ensure_active(id).await?;
        let confluence = self
            .confluence
            .as_ref()
            .ok_or(WorkspaceError::ConfluenceUnavailable)?;
        let member = sqlx::query(
            "SELECT count(*) AS n FROM workspace_places WHERE workspace_id = ?1 AND place_id = ?2",
        )
        .bind(id.to_string())
        .bind(place_id.to_string())
        .fetch_one(&self.pool)
        .await?
        .try_get::<i64, _>("n")?;
        if member == 0 {
            return Err(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"));
        }
        if !folder_ok(&folder) {
            return Err(WorkspaceError::Invalid("SRC_LOCATION_INVALID"));
        }
        let parent = location.page_id.clone().unwrap_or_default();
        check_space_and_parent(&location.space_key, &parent)?;
        let audience = confluence
            .audience(&location)
            .await
            .map_err(|e| map_adapter(&e))?
            .line;
        check_audience(&audience)?;
        let link = ConfluenceLink {
            id: ConfluenceLinkId::new(),
            workspace_id: id,
            place_id,
            folder,
            confluence_space: location.space_key.clone(),
            parent_page_id: parent,
            audience,
            approval_id: None,
            approved_at: None,
            state: ConfluenceLinkState::Proposed,
        };
        sqlx::query(
            "INSERT INTO confluence_links (id, workspace_id, place_id, folder, confluence_space, parent_page_id, \
             audience, approval_id, approved_at, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, 'proposed')",
        )
        .bind(link.id.to_string())
        .bind(id.to_string())
        .bind(place_id.to_string())
        .bind(&link.folder)
        .bind(&link.confluence_space)
        .bind(&link.parent_page_id)
        .bind(&link.audience)
        .execute(&self.pool)
        .await?;
        let place = self.files.place(place_id).await.map_err(file_failed)?;
        let approval = match confluence
            .request_link_approval(&link, &place, &workspace)
            .await
        {
            Ok(approval) => approval,
            Err(error) => {
                tracing::warn!(
                    component = crate::log::COMPONENT,
                    code = ordin_types::CodedError::code(&error),
                    "confluence_approval_failed"
                );
                let _ = sqlx::query("DELETE FROM confluence_links WHERE id = ?1")
                    .bind(link.id.to_string())
                    .execute(&self.pool)
                    .await;
                return Err(WorkspaceError::ConfluenceUnavailable);
            }
        };
        sqlx::query("UPDATE confluence_links SET approval_id = ?2 WHERE id = ?1")
            .bind(link.id.to_string())
            .bind(approval.to_string())
            .execute(&self.pool)
            .await?;
        self.changed(id).await;
        self.load_confluence_link(link.id).await
    }

    pub async fn confluence_links(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<ConfluenceLink>, WorkspaceError> {
        self.workspace(id).await?;
        let rows =
            sqlx::query("SELECT * FROM confluence_links WHERE workspace_id = ?1 ORDER BY rowid")
                .bind(id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(confluence_row).collect()
    }

    /// Internal Logic 8.5: active → paused.
    pub async fn pause_confluence_link(
        &self,
        link: ConfluenceLinkId,
    ) -> Result<ConfluenceLink, WorkspaceError> {
        let held = self.load_confluence_link(link).await?;
        if held.state != ConfluenceLinkState::Active {
            return Err(WorkspaceError::LinkStateInvalid);
        }
        self.set_confluence_state(&held, ConfluenceLinkState::Paused)
            .await
    }

    /// Internal Logic 8.5: paused → active, then a sync.
    pub async fn resume_confluence_link(
        &self,
        link: ConfluenceLinkId,
    ) -> Result<ConfluenceLink, WorkspaceError> {
        let held = self.load_confluence_link(link).await?;
        if held.state != ConfluenceLinkState::Paused {
            return Err(WorkspaceError::LinkStateInvalid);
        }
        let resumed = self
            .set_confluence_state(&held, ConfluenceLinkState::Active)
            .await?;
        self.sync_confluence_link(resumed.id).await;
        Ok(resumed)
    }

    async fn set_confluence_state(
        &self,
        held: &ConfluenceLink,
        state: ConfluenceLinkState,
    ) -> Result<ConfluenceLink, WorkspaceError> {
        check_approved(state, held.approval_id, held.approved_at)?;
        sqlx::query("UPDATE confluence_links SET state = ?2 WHERE id = ?1")
            .bind(held.id.to_string())
            .bind(to_text(&state))
            .execute(&self.pool)
            .await?;
        self.changed(held.workspace_id).await;
        self.load_confluence_link(held.id).await
    }

    /// Internal Logic 8.5: removes the row and the adapter's state; no Confluence page is deleted.
    pub async fn remove_confluence_link(
        &self,
        link: ConfluenceLinkId,
    ) -> Result<(), WorkspaceError> {
        let held = self.load_confluence_link(link).await?;
        if held.state == ConfluenceLinkState::Proposed {
            return Err(WorkspaceError::LinkStateInvalid);
        }
        if let Some(confluence) = &self.confluence {
            if let Err(error) = confluence.forget_link(link).await {
                log::background_failed("forget_link", ordin_types::CodedError::code(&error));
            }
        }
        sqlx::query("DELETE FROM confluence_links WHERE id = ?1")
            .bind(link.to_string())
            .execute(&self.pool)
            .await?;
        lock(&self.sync_changes).remove(&link);
        self.changed(held.workspace_id).await;
        Ok(())
    }

    // ---- 8.3 approvals

    /// Applies one decision to the proposed link that holds `approval_id`.
    async fn apply_decision(
        &self,
        approval_id: ApprovalId,
        state: ApprovalState,
    ) -> Result<(), WorkspaceError> {
        if state == ApprovalState::Pending {
            return Ok(());
        }
        let row = sqlx::query(
            "SELECT * FROM confluence_links WHERE approval_id = ?1 AND state = 'proposed'",
        )
        .bind(approval_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(()) };
        let link = confluence_row(&row)?;
        let outcome = match state {
            ApprovalState::Approved => {
                let now = Timestamp::now();
                check_approved(ConfluenceLinkState::Active, Some(approval_id), Some(now))?;
                sqlx::query(
                    "UPDATE confluence_links SET state = 'active', approved_at = ?2 WHERE id = ?1",
                )
                .bind(link.id.to_string())
                .bind(now.to_wire())
                .execute(&self.pool)
                .await?;
                "approved"
            }
            _ => {
                sqlx::query("DELETE FROM confluence_links WHERE id = ?1")
                    .bind(link.id.to_string())
                    .execute(&self.pool)
                    .await?;
                if state == ApprovalState::Rejected {
                    "rejected"
                } else {
                    "expired"
                }
            }
        };
        log::confluence_link_decided(link.id, outcome);
        self.record(
            "confluence_link_decided",
            Some(link.workspace_id),
            "A decision was made on a Confluence link.",
            serde_json::json!({ "confluence_link_id": link.id, "state": outcome }),
        )
        .await;
        self.changed(link.workspace_id).await;
        if state == ApprovalState::Approved {
            self.sync_confluence_link(link.id).await;
        }
        Ok(())
    }

    async fn on_event(&self, event: &JournalEvent) {
        if EventKind::parse(&event.kind) != Some(EventKind::ApprovalDecided) {
            return;
        }
        let Ok(decided) = serde_json::from_value::<ApprovalDecided>(event.data.clone()) else {
            log::approval_event_invalid(event.seq);
            return;
        };
        if let Err(error) = self
            .apply_decision(decided.approval_id, decided.state)
            .await
        {
            log::background_failed("approval_decision", error.code());
        }
    }

    // ---- 8.4 linked-folder syncs

    /// Runs `sync_linked_folder` for an active link; at most one sync per link at a time.
    pub(crate) async fn sync_confluence_link(&self, id: ConfluenceLinkId) {
        let Some(confluence) = &self.confluence else {
            return;
        };
        let Ok(link) = self.load_confluence_link(id).await else {
            return;
        };
        if link.state != ConfluenceLinkState::Active {
            return;
        }
        if !lock(&self.syncing).insert(id) {
            return;
        }
        let outcome = match self.files.place(link.place_id).await {
            Ok(place) => {
                // spec: ordin-desktop/components/WorkspaceService#SA-WS-AUTO-02
                let publishing = match self.derive_publishing(&link, &place).await {
                    Ok(value) => value.automatic,
                    Err(_) => {
                        tracing::warn!(
                            component = crate::log::COMPONENT,
                            confluence_link_id = %link.id,
                            "confluence_publishing_unknown"
                        );
                        AutomaticPublishing::Off
                    }
                };
                confluence
                    .sync_linked_folder_with(&link, &place, publishing)
                    .await
                    .map(|_| ())
            }
            Err(error) => {
                log::background_failed("sync_linked_folder", error.code());
                Ok(())
            }
        };
        lock(&self.syncing).remove(&id);
        if let Err(error) = outcome {
            log::background_failed("sync_linked_folder", ordin_types::CodedError::code(&error));
        }
    }

    /// A change under an active link's folder schedules a sync after `LINKED_FOLDER_DEBOUNCE`.
    pub(crate) async fn schedule_folder_syncs(&self, change: &PlaceChange) {
        let rows =
            sqlx::query("SELECT * FROM confluence_links WHERE place_id = ?1 AND state = 'active'")
                .bind(change.place_id.to_string())
                .fetch_all(&self.pool)
                .await;
        let Ok(rows) = rows else { return };
        for row in &rows {
            let Ok(link) = confluence_row(row) else {
                continue;
            };
            let under = link.folder.is_empty()
                || change.path == link.folder
                || change
                    .path
                    .starts_with(&format!("{}/", link.folder.trim_end_matches('/')));
            if under {
                self.schedule_sync(link.id);
            }
        }
    }

    fn schedule_sync(&self, link: ConfluenceLinkId) {
        let first = {
            let mut changes = lock(&self.sync_changes);
            let first = !changes.contains_key(&link);
            changes.insert(link, tokio::time::Instant::now());
            first
        };
        if !first {
            return;
        }
        let Some(service) = self.me.upgrade() else {
            return;
        };
        self.spawn(async move {
            loop {
                let last = lock(&service.sync_changes).get(&link).copied();
                let Some(last) = last else { return };
                let due = last + LINKED_FOLDER_DEBOUNCE;
                if tokio::time::Instant::now() >= due {
                    lock(&service.sync_changes).remove(&link);
                    service.sync_confluence_link(link).await;
                    return;
                }
                tokio::time::sleep_until(due).await;
            }
        });
    }

    /// Internal Logic 8.7: the value the linked folder reports (SA-WS-AUTO-01, SA-WS-AUTO-03).
    // spec: ordin-desktop/components/WorkspaceService#SA-WS-AUTO-01
    pub async fn confluence_publishing(
        &self,
        link: ConfluenceLinkId,
    ) -> Result<ConfluencePublishing, WorkspaceError> {
        let held = self.load_confluence_link(link).await?;
        let place = self.files.place(held.place_id).await.map_err(file_failed)?;
        self.derive_publishing(&held, &place).await
    }

    /// SA-WS-AUTO-01: off while a grant of an agent that can run commands, or of a crew, reaches the
    /// linked folder in any workspace not in the Bin. Fails closed: a place that cannot be read counts
    /// as reaching it.
    async fn derive_publishing(
        &self,
        link: &ConfluenceLink,
        place: &Place,
    ) -> Result<ConfluencePublishing, WorkspaceError> {
        let linked = if link.folder.is_empty() {
            std::path::PathBuf::from(&place.path)
        } else {
            std::path::Path::new(&place.path).join(&link.folder)
        };
        let rows = sqlx::query(
            "SELECT g.* FROM folder_grants g JOIN workspaces w ON w.id = g.workspace_id \
             WHERE w.state != ?1 ORDER BY g.granted_at, g.rowid",
        )
        .bind(to_text(&WorkspaceState::InBin))
        .fetch_all(&self.pool)
        .await?;
        let mut reaches: std::collections::HashMap<PlaceId, bool> =
            std::collections::HashMap::new();
        let mut turned_off_by: Vec<Grantee> = Vec::new();
        for row in &rows {
            let grant = crate::grants::grant_row(row)?;
            let runs_commands = match grant.grantee {
                Grantee::AgentInstance { agent_instance_id } => {
                    self.rules.may_run_commands(agent_instance_id)
                }
                // A crew's members are not known here.
                Grantee::Crew { .. } => true,
            };
            if !runs_commands || turned_off_by.contains(&grant.grantee) {
                continue;
            }
            let overlaps = match reaches.get(&grant.place_id) {
                Some(known) => *known,
                None => {
                    let known = match self.files.place(grant.place_id).await {
                        Ok(granted) => {
                            let granted = std::path::Path::new(&granted.path);
                            granted.starts_with(&linked) || linked.starts_with(granted)
                        }
                        Err(_) => true,
                    };
                    reaches.insert(grant.place_id, known);
                    known
                }
            };
            if overlaps {
                turned_off_by.push(grant.grantee);
            }
        }
        Ok(ConfluencePublishing {
            confluence_link_id: link.id,
            automatic: if turned_off_by.is_empty() {
                AutomaticPublishing::On
            } else {
                AutomaticPublishing::Off
            },
            turned_off_by,
        })
    }

    pub(crate) async fn active_workspace_state(&self, id: WorkspaceId) -> Option<WorkspaceState> {
        self.workspace(id).await.ok().map(|w| w.state)
    }
}

#[async_trait::async_trait]
impl EventSink for WorkspaceService {
    fn name(&self) -> &'static str {
        "workspaces"
    }

    /// Internal Logic 8.3: acts only on `approval_decided` for an approval id this service stored.
    async fn deliver(&self, event: &JournalEvent) {
        self.on_event(event).await;
    }

    /// Re-reads what the sink missed until it reaches the Journal's last event.
    async fn lagged(&self, next_seq: u64) {
        let mut after = next_seq.saturating_sub(1);
        loop {
            let Ok(events) = self.journal.events_since(after, 1000).await else {
                return;
            };
            let Some(last) = events.last().map(|e| e.seq) else {
                return;
            };
            for event in &events {
                self.on_event(event).await;
            }
            after = last;
            if after >= self.journal.last_seq() {
                return;
            }
        }
    }
}
