//! Team knowledge (amendment January 2026, ADR-103 §K–§O, with ADR-104 §W): the page files given
//! to the Team Knowledge Service, a colleague's page taken back, a colleague's removal. The newer
//! edit wins and the other is kept as the page's earlier version (G11); no file carries conflict
//! markers.

use ordin_files::{FileError, FileVersionOrigin, WriteVersion};
use ordin_types::domain::places::KnowledgePageState;
use ordin_types::time::Timestamp;
use ordin_types::{KnowledgePageId, WorkspaceId};
use serde_json::json;

use crate::error::KnowledgeError;
use crate::model::{TeamPageFile, TeamPageImport, TeamPageImported};
use crate::names::name_key;
use crate::pagefile::{render, FileHeader};
use crate::pages::file_unavailable;
use crate::read::{team_header, team_title};
use crate::service::{KnowledgeService, MAX_PAGES_PER_WORKSPACE};
use crate::store::{self, PageRow};

/// The row values a header gives (M-3, W).
fn apply_header(row: &mut PageRow, h: &FileHeader, title: &str) {
    row.title = title.to_owned();
    row.body = h.body.clone();
    row.written_by = h.written_by.unwrap_or(row.written_by);
    row.sources = h.sources.clone();
    row.last_changed_at = h.last_changed.unwrap_or_else(Timestamp::now);
    row.shared_by = h.shared_by.clone();
    row.shared_at = h.shared_at;
    row.edited_by_person = false;
    row.state = KnowledgePageState::Current;
    row.tags = h.tags.clone();
    row.aliases = h.aliases.clone();
    row.extra_header = h.extra_header.clone();
    row.page_type = h.page_type.clone();
    row.description = h.description.clone();
    row.verified = h.verified.clone();
    row.okf_tier = h.tier;
    row.okf_status = h.status;
    row.status_text = h.status_text.clone();
    row.stale_after = h.stale_after.clone();
    row.generated = h.generated.clone();
    row.title_refused = None;
}

impl KnowledgeService {
    /// L: each non-removed page of the workspace as the file to share, ordered by id.
    pub async fn export_team_pages(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<TeamPageFile>, KnowledgeError> {
        self.desktop_only()?;
        self.workspace(workspace_id).await?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let rows = store::workspace_pages(&mut conn, workspace_id).await?;
        let mut out = Vec::with_capacity(rows.len());
        for mut row in rows {
            if row.shared_by.is_none() {
                let at = match row.first_exported_at {
                    Some(t) => t,
                    None => {
                        let now = Timestamp::now();
                        sqlx::query("UPDATE knowledge_pages SET first_exported_at = ?2 WHERE id = ?1 AND first_exported_at IS NULL")
                            .bind(row.id.to_string())
                            .bind(now.to_wire())
                            .execute(&mut *conn)
                            .await?;
                        now
                    }
                };
                // L-2: `shared_by: ""` until the Team Knowledge Service names the person.
                row.shared_by = Some(String::new());
                row.shared_at = Some(at);
            }
            out.push(TeamPageFile {
                page_id: row.id,
                bytes: render(&row),
                last_changed: row.last_changed_at,
            });
        }
        tracing::info!(component = "knowledge", workspace_id = %workspace_id, pages = out.len(), "knowledge_team_exported");
        Ok(out)
    }

    /// Writes a colleague's bytes as the page's next version and stores the row (M-6).
    async fn write_team_version(
        &self,
        mut row: PageRow,
        old: Option<&PageRow>,
        bytes: Vec<u8>,
    ) -> Result<u32, KnowledgeError> {
        let v = self
            .files
            .write_version(WriteVersion {
                place_id: row.place_id,
                path: row.relative_path.clone(),
                bytes,
                origin: FileVersionOrigin::Person,
                expected_hash: None,
            })
            .await
            .map_err(|e| file_unavailable(&e))?;
        row.file_version = v.version;
        row.memory_indexed_version = None;
        row.memory_index_attempts = 0;
        row.memory_next_attempt_at = None;
        row.memory_removed = false;
        row.state_before_removal = None;
        row.bin_ticket = None;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        match old {
            Some(o) => {
                if o.state != KnowledgePageState::Removed {
                    store::fts_delete(&mut tx, o).await?;
                }
                store::update_page(&mut tx, &row).await?;
            }
            None => row.rowid = store::insert_page(&mut tx, &row).await?,
        }
        store::fts_insert(&mut tx, &row).await?;
        store::clear_marks_and_update(&mut tx, row.id).await?;
        store::replace_names(&mut tx, &row).await?;
        store::replace_tags(&mut tx, &row).await?;
        self.rebuild_links(&mut tx, &mut row).await?;
        self.resolve_names(&mut tx, &row).await?;
        tx.commit().await?;
        self.wake_indexer();
        self.schedule_index(row.workspace_id);
        Ok(v.version)
    }

    /// SA-KTEAM-02: the title, renamed on arrival while another page of the workspace holds it.
    async fn team_free_title(
        &self,
        workspace: WorkspaceId,
        h: &FileHeader,
        except: Option<KnowledgePageId>,
    ) -> Result<String, KnowledgeError> {
        let mut conn = self.conn().await?;
        let by = h.shared_by.clone().unwrap_or_default();
        let mut title = h.title.clone();
        let mut n = 1u32;
        while !self.name_free(&mut conn, workspace, &title, except).await? {
            n += 1;
            title = team_title(&h.title, &by, n);
        }
        Ok(title)
    }

    /// M: a colleague's page file taken into the workspace's `knowledge/` folder.
    pub async fn import_team_page(
        &self,
        workspace_id: WorkspaceId,
        page: TeamPageImport,
    ) -> Result<TeamPageImported, KnowledgeError> {
        self.desktop_only()?;
        let workspace = self.workspace(workspace_id).await?;
        let refused = |e: KnowledgeError| {
            tracing::warn!(
                component = "knowledge",
                code = e.code(),
                "knowledge_team_page_refused"
            );
            e
        };
        let h = team_header(&page.bytes).map_err(refused)?;
        if h.page_id != Some(page.page_id) {
            return Err(refused(KnowledgeError::TeamPageInvalid));
        }
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let existing = store::page(&mut conn, page.page_id).await?;
        drop(conn);
        if existing
            .as_ref()
            .is_some_and(|r| r.workspace_id != workspace_id)
        {
            return Err(refused(KnowledgeError::TeamPageInvalid));
        }
        let result = match existing {
            None => self.import_new(&workspace, &page, &h).await?,
            Some(row) if row.state == KnowledgePageState::Removed => {
                let ticket = row
                    .bin_ticket
                    .clone()
                    .ok_or(KnowledgeError::DbUnavailable)?;
                match self.files.restore_from_bin(&ticket).await {
                    Ok(v) => {
                        let mut restored = row.clone();
                        restored.file_version = v.version;
                        restored.relative_path = ticket.path.clone();
                        self.import_existing(restored, Some(&row), &page, &h)
                            .await?
                    }
                    Err(FileError::NotFound) => {
                        let mut conn = self.conn().await?;
                        store::delete_page_row(&mut conn, row.id).await?;
                        drop(conn);
                        self.import_new(&workspace, &page, &h).await?
                    }
                    Err(e) => return Err(file_unavailable(&e)),
                }
            }
            Some(row) => {
                self.import_existing(row.clone(), Some(&row), &page, &h)
                    .await?
            }
        };
        let (word, version) = match result {
            TeamPageImported::Written { version } => ("written", Some(version)),
            TeamPageImported::KeptLocal { remote_version } => ("kept_local", Some(remote_version)),
            TeamPageImported::Unchanged => ("unchanged", None),
        };
        tracing::info!(component = "knowledge", page_id = %page.page_id, result = word, "knowledge_team_page_got");
        self.journal(
            "team_page_got",
            Some(workspace_id),
            None,
            "A knowledge page came from the team.",
            json!({ "page_id": page.page_id, "result": word, "version": version }),
        )
        .await;
        Ok(result)
    }

    /// M-3: a new page.
    async fn import_new(
        &self,
        workspace: &ordin_types::domain::places::Workspace,
        page: &TeamPageImport,
        h: &FileHeader,
    ) -> Result<TeamPageImported, KnowledgeError> {
        let mut conn = self.conn().await?;
        if store::count_live(&mut conn, workspace.id).await? >= MAX_PAGES_PER_WORKSPACE {
            return Err(KnowledgeError::PageLimit);
        }
        drop(conn);
        let title = self.team_free_title(workspace.id, h, None).await?;
        let mut conn = self.conn().await?;
        let path = self
            .free_file_name(
                &mut conn,
                workspace.own_folder_place_id,
                "knowledge",
                &title,
            )
            .await?;
        drop(conn);
        let now = Timestamp::now();
        let mut row = PageRow {
            rowid: 0,
            id: page.page_id,
            workspace_id: workspace.id,
            title: title.clone(),
            body: String::new(),
            written_by: ordin_types::domain::places::KnowledgeAuthor::Person,
            edited_by_person: false,
            sources: Vec::new(),
            state: KnowledgePageState::Current,
            last_changed_at: now,
            place_id: workspace.own_folder_place_id,
            relative_path: path,
            file_version: 1,
            state_before_removal: None,
            bin_ticket: None,
            memory_indexed_version: None,
            memory_index_attempts: 0,
            memory_next_attempt_at: None,
            memory_removed: false,
            created_at: now,
            shared_by: None,
            shared_at: None,
            first_exported_at: None,
            extra_header: String::new(),
            tags: Vec::new(),
            aliases: Vec::new(),
            title_refused: None,
            links_built_version: None,
            links_truncated: 0,
            page_type: None,
            description: None,
            okf_tier: ordin_types::wire::knowledge_query::PageTier::Unverified,
            okf_status: ordin_types::wire::knowledge_query::PageStatus::Stable,
            status_text: None,
            stale_after: None,
            verified: Vec::new(),
            generated: None,
        };
        apply_header(&mut row, h, &title);
        let bytes = self.final_bytes(&page.bytes, h, &title);
        let version = self.write_team_version(row, None, bytes).await?;
        Ok(TeamPageImported::Written { version })
    }

    /// The file re-rendered with the final title: the colleague's bytes unchanged when the title
    /// stayed, else with the header's title line replaced.
    fn final_bytes(&self, bytes: &[u8], h: &FileHeader, title: &str) -> Vec<u8> {
        if title == h.title {
            return bytes.to_vec();
        }
        let text = String::from_utf8_lossy(bytes);
        crate::outside::replace_title_line(&text, title)
            .map(String::into_bytes)
            .unwrap_or_else(|| bytes.to_vec())
    }

    /// M-5: an existing page.
    async fn import_existing(
        &self,
        row: PageRow,
        old: Option<&PageRow>,
        page: &TeamPageImport,
        h: &FileHeader,
    ) -> Result<TeamPageImported, KnowledgeError> {
        let current = match self.files.read_file(row.place_id, &row.relative_path).await {
            Ok(f) => Some(f.bytes),
            Err(FileError::NotFound) => None,
            Err(e) => return Err(file_unavailable(&e)),
        };
        if current.as_deref() == Some(page.bytes.as_slice()) {
            return Ok(TeamPageImported::Unchanged);
        }
        let remote_later = h.last_changed.is_some_and(|t| t > row.last_changed_at);
        if !page.both_changed || remote_later {
            let title = if name_key(&h.title) == name_key(&row.title) {
                h.title.clone()
            } else {
                self.team_free_title(row.workspace_id, h, Some(row.id))
                    .await?
            };
            let mut new = row.clone();
            apply_header(&mut new, h, &title);
            let bytes = self.final_bytes(&page.bytes, h, &title);
            let version = self.write_team_version(new, old, bytes).await?;
            return Ok(TeamPageImported::Written { version });
        }
        // The local edit is newer (or equal times with different bytes): the colleague's text
        // becomes version v, the local text v + 1; the row is unchanged.
        let local = match current {
            Some(b) => b,
            None => render(&row),
        };
        let v = self
            .files
            .write_version(WriteVersion {
                place_id: row.place_id,
                path: row.relative_path.clone(),
                bytes: page.bytes.clone(),
                origin: FileVersionOrigin::Person,
                expected_hash: None,
            })
            .await
            .map_err(|e| file_unavailable(&e))?;
        let mut kept = row.clone();
        kept.state = if row.state == KnowledgePageState::Removed {
            row.state_before_removal
                .unwrap_or(KnowledgePageState::Current)
        } else {
            row.state
        };
        let v1 = self.write_team_version(kept, old, local).await?;
        debug_assert!(v1 > v.version);
        Ok(TeamPageImported::KeptLocal {
            remote_version: v.version,
        })
    }

    /// N: a colleague removed the page and it is unchanged here.
    pub async fn remove_team_page(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
    ) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let row = store::page(&mut conn, page_id)
            .await?
            .filter(|r| r.workspace_id == workspace_id)
            .ok_or(KnowledgeError::PageNotFound)?;
        drop(conn);
        if row.state == KnowledgePageState::Removed {
            return Ok(());
        }
        self.remove_locked(page_id).await?;
        tracing::info!(component = "knowledge", page_id = %page_id, "knowledge_team_page_removed");
        self.journal(
            "team_page_removed",
            Some(workspace_id),
            None,
            "A knowledge page a colleague removed was moved to the bin.",
            json!({ "page_id": page_id }),
        )
        .await;
        Ok(())
    }
}
