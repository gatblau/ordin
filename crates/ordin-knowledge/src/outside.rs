//! The `knowledge/` folder edited in another app (amendment January 2026, ADR-104, C-1 changed):
//! outside edits, renames and removals of page files, adoption of new files (§R), the start-up
//! scan (§R step 6) and, with ADR-112, reserved names (SA-OKF-08, AF).

use ordin_files::{
    FileError, FileVersionOrigin, FolderSort, PlaceChange, PlaceChangeKind, WriteVersion,
};
use ordin_types::domain::places::{KnowledgeAuthor, KnowledgePageState};
use ordin_types::time::Timestamp;
use ordin_types::{KnowledgePageId, PlaceId, WorkspaceId};
use serde_json::json;

use crate::error::KnowledgeError;
use crate::model::{NotAdoptedFile, NotAdoptedReason};
use crate::names::{is_page_path, name_key, path_stem};
use crate::okf;
use crate::pagefile::{self, read_page_file, EXTRA_HEADER_MAX};
use crate::pages::{file_unavailable, Commit};
use crate::service::{lock, KnowledgeService, MAX_PAGES_PER_WORKSPACE};
use crate::store::{self, PageRow};

const ADOPT_MAX_BYTES: usize = 1_048_576 + 65_536;

impl KnowledgeService {
    /// The workspace whose own folder is `place`: one already seen, a page row, or else the Workspace
    /// Service's list (FU-103).
    pub(crate) async fn workspace_of_place(
        &self,
        place: PlaceId,
    ) -> Result<Option<WorkspaceId>, KnowledgeError> {
        if let Some(w) = lock(&self.place_workspaces).get(&place).copied() {
            return Ok(Some(w));
        }
        let pool = self.pool()?;
        let w: Option<String> = sqlx::query_scalar(
            "SELECT workspace_id FROM knowledge_pages WHERE place_id = ?1 LIMIT 1",
        )
        .bind(place.to_string())
        .fetch_optional(pool)
        .await?;
        if let Some(w) = w.and_then(|w| WorkspaceId::parse(&w).ok()) {
            return Ok(Some(w));
        }
        let listed = self
            .workspaces
            .list_not_in_bin()
            .await
            .map_err(|_| KnowledgeError::DbUnavailable)?;
        let mut found = None;
        {
            let mut seen = lock(&self.place_workspaces);
            for w in listed {
                seen.insert(w.own_folder_place_id, w.id);
                if w.own_folder_place_id == place {
                    found = Some(w.id);
                }
            }
        }
        Ok(found)
    }

    pub(crate) fn not_adopted_of(&self, workspace: WorkspaceId) -> Vec<NotAdoptedFile> {
        let mut out: Vec<NotAdoptedFile> = lock(&self.not_adopted)
            .iter()
            .filter(|(_, (w, _, _))| *w == workspace)
            .map(|((_, path), (_, reason, _))| NotAdoptedFile {
                relative_path: path.clone(),
                reason: *reason,
            })
            .collect();
        out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        out
    }

    /// C-1 (changed) for a change in a workspace's own folder under `knowledge/`.
    pub(crate) async fn on_own_folder_change(
        &self,
        change: &PlaceChange,
    ) -> Result<(), KnowledgeError> {
        let in_knowledge = |p: &str| p.starts_with("knowledge/");
        let from = match &change.change {
            PlaceChangeKind::Renamed { from } => Some(from.clone()),
            _ => None,
        };
        if !in_knowledge(&change.path) && !from.as_deref().is_some_and(in_knowledge) {
            return Ok(());
        }
        let Some(workspace) = self.workspace_of_place(change.place_id).await? else {
            return Ok(());
        };
        let _guard = self.write_lock.lock().await;
        // A file that changes or goes leaves the not-adopted list.
        lock(&self.not_adopted).remove(&(change.place_id, change.path.clone()));
        if let Some(f) = &from {
            lock(&self.not_adopted).remove(&(change.place_id, f.clone()));
        }
        let mut conn = self.conn().await?;
        match &change.change {
            PlaceChangeKind::Created | PlaceChangeKind::Modified => {
                if okf::is_reserved(&change.path) || !is_page_path(&change.path) {
                    return Ok(());
                }
                match store::page_by_file(&mut conn, change.place_id, &change.path).await? {
                    Some(row) if row.state != KnowledgePageState::Removed => {
                        if change.version.is_some_and(|v| v <= row.file_version) {
                            return Ok(());
                        }
                        drop(conn);
                        self.outside_edit(row, change).await
                    }
                    Some(_) => Ok(()),
                    None => {
                        drop(conn);
                        self.adopt_file(
                            workspace,
                            change.place_id,
                            &change.path,
                            change.observed_at,
                        )
                        .await
                    }
                }
            }
            PlaceChangeKind::Removed => {
                match store::page_by_file(&mut conn, change.place_id, &change.path).await? {
                    Some(row) if row.state != KnowledgePageState::Removed => {
                        drop(conn);
                        self.removed_outside(row, &change.path).await
                    }
                    _ => Ok(()),
                }
            }
            PlaceChangeKind::Renamed { from } => {
                if let Some(at_new) =
                    store::page_by_file(&mut conn, change.place_id, &change.path).await?
                {
                    // Our own move (Q-1): the row is already at the new path.
                    if at_new.state != KnowledgePageState::Removed {
                        return Ok(());
                    }
                }
                match store::page_by_file(&mut conn, change.place_id, from).await? {
                    Some(row) if row.state != KnowledgePageState::Removed => {
                        drop(conn);
                        if is_page_path(&change.path) && !okf::is_reserved(&change.path) {
                            self.renamed_outside(row, &change.path).await
                        } else {
                            self.removed_outside(row, from).await
                        }
                    }
                    _ => {
                        drop(conn);
                        if is_page_path(&change.path) && !okf::is_reserved(&change.path) {
                            self.adopt_file(
                                workspace,
                                change.place_id,
                                &change.path,
                                change.observed_at,
                            )
                            .await
                        } else {
                            Ok(())
                        }
                    }
                }
            }
        }
    }

    /// C-1 step 1: a known page file changed by another app.
    async fn outside_edit(&self, row: PageRow, change: &PlaceChange) -> Result<(), KnowledgeError> {
        let file = match self.files.read_file(row.place_id, &row.relative_path).await {
            Ok(f) => f,
            Err(FileError::NotFound) => return Ok(()),
            Err(e) => return Err(file_unavailable(&e)),
        };
        let version = match change.version {
            Some(v) => v,
            None => match self
                .files
                .current_version(row.place_id, &row.relative_path)
                .await
            {
                Ok(Some(v)) => v.version,
                _ => row.file_version,
            },
        };
        let mut new = row.clone();
        new.file_version = version;
        new.memory_indexed_version = None;
        new.memory_next_attempt_at = None;
        let mut journal_invalid = false;
        let mut title_refused = false;
        let mut rename_keys: Vec<String> = Vec::new();
        let mut dropped: Vec<(&'static str, usize)> = Vec::new();
        match read_page_file(&row.relative_path, &file.bytes) {
            Ok(h) if h.page_id == Some(row.id) => {
                let mut conn = self.conn().await?;
                if name_key(&h.title) == name_key(&row.title) {
                    new.title = h.title.clone();
                    new.title_refused = None;
                } else if self
                    .name_free(&mut conn, row.workspace_id, &h.title, Some(row.id))
                    .await?
                {
                    rename_keys.push(name_key(&row.title));
                    new.title = h.title.clone();
                    new.title_refused = None;
                } else {
                    new.title_refused = Some(h.title.clone());
                    title_refused = true;
                }
                drop(conn);
                new.aliases = h.aliases;
                new.tags = h.tags;
                new.extra_header = h.extra_header;
                new.sources = h.sources;
                new.written_by = h.written_by.unwrap_or(row.written_by);
                new.edited_by_person = true;
                new.last_changed_at = change.observed_at;
                new.shared_by = h.shared_by;
                new.shared_at = h.shared_at;
                new.page_type = h.page_type;
                new.description = h.description;
                new.verified = h.verified;
                new.okf_tier = h.tier;
                new.okf_status = h.status;
                new.status_text = h.status_text;
                new.stale_after = h.stale_after;
                new.generated = h.generated;
                new.body = h.body;
                if h.dropped_tags > 0 {
                    dropped.push(("tags", h.dropped_tags));
                }
                if h.dropped_aliases > 0 {
                    dropped.push(("aliases", h.dropped_aliases));
                }
            }
            _ => {
                // Previous metadata kept; the body is the whole file text.
                new.body = String::from_utf8_lossy(&file.bytes).into_owned();
                if new.body.len() > crate::names::BODY_MAX_BYTES {
                    new.body = row.body.clone();
                }
                journal_invalid = true;
            }
        }
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        store::fts_delete(&mut tx, &row).await?;
        store::update_page(&mut tx, &new).await?;
        store::fts_insert(&mut tx, &new).await?;
        store::replace_names(&mut tx, &new).await?;
        store::replace_tags(&mut tx, &new).await?;
        self.rebuild_links(&mut tx, &mut new).await?;
        self.resolve_names(&mut tx, &new).await?;
        if !rename_keys.is_empty() {
            self.schedule_rewrite(&mut tx, row.id, &rename_keys).await?;
        }
        tx.commit().await?;
        if journal_invalid {
            tracing::warn!(component = "knowledge", page_id = %row.id, "knowledge_header_invalid");
            self.journal(
                "header_invalid",
                Some(row.workspace_id),
                None,
                "A knowledge page's header could not be read.",
                json!({ "page_id": row.id }),
            )
            .await;
        }
        if title_refused {
            self.journal(
                "title_refused",
                Some(row.workspace_id),
                None,
                "A knowledge page's title in its file is used by another page.",
                json!({ "page_id": row.id }),
            )
            .await;
        }
        for (field, count) in dropped {
            self.journal(
                "header_values_dropped",
                Some(row.workspace_id),
                None,
                "Some values of a knowledge page's header were left out.",
                json!({ "page_id": row.id, "field": field, "count": count }),
            )
            .await;
        }
        self.wake_indexer();
        self.schedule_index(row.workspace_id);
        self.refresh_suggestions_after(&new).await;
        Ok(())
    }

    /// C-1 step 3: a page file renamed or moved inside `knowledge/` by another app.
    async fn renamed_outside(&self, row: PageRow, to: &str) -> Result<(), KnowledgeError> {
        let mut conn = self.conn().await?;
        let mut new = row.clone();
        new.relative_path = to.to_owned();
        let old_stem = name_key(&path_stem(&row.relative_path));
        let new_stem_text = path_stem(to);
        let new_stem = name_key(&new_stem_text);
        let mut keys = crate::links::old_keys(&row);
        let retitle = new_stem != old_stem
            && new_stem != name_key(&row.title)
            && self
                .name_free(&mut conn, row.workspace_id, &new_stem_text, Some(row.id))
                .await?;
        drop(conn);
        if retitle {
            // The header's title line is rewritten as a new file version.
            if let Ok(file) = self.files.read_file(row.place_id, to).await {
                let text = String::from_utf8_lossy(&file.bytes).into_owned();
                if let Some(updated) = replace_title_line(&text, &new_stem_text) {
                    match self
                        .files
                        .write_version(WriteVersion {
                            place_id: row.place_id,
                            path: to.to_owned(),
                            bytes: updated.into_bytes(),
                            origin: FileVersionOrigin::Person,
                            expected_hash: Some(file.hash.clone()),
                        })
                        .await
                    {
                        Ok(v) => {
                            new.file_version = v.version;
                            new.title = new_stem_text.clone();
                            new.edited_by_person = true;
                        }
                        Err(e) => tracing::warn!(
                            component = "knowledge",
                            code = e.code(),
                            "knowledge_title_line_failed"
                        ),
                    }
                }
            }
        }
        keys.retain(|k| *k != name_key(&new.title) && *k != new_stem);
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        store::fts_delete(&mut tx, &row).await?;
        new.memory_indexed_version = None;
        store::update_page(&mut tx, &new).await?;
        store::fts_insert(&mut tx, &new).await?;
        store::replace_names(&mut tx, &new).await?;
        self.rebuild_links(&mut tx, &mut new).await?;
        self.resolve_names(&mut tx, &new).await?;
        if !keys.is_empty() {
            self.schedule_rewrite(&mut tx, row.id, &keys).await?;
        }
        tx.commit().await?;
        self.wake_indexer();
        self.schedule_index(row.workspace_id);
        Ok(())
    }

    /// C-1 step 4: a page file deleted (or moved out) in another app. Undo restores the file
    /// from its latest version copy.
    async fn removed_outside(&self, row: PageRow, path: &str) -> Result<(), KnowledgeError> {
        let versions = self
            .files
            .versions(row.place_id, path)
            .await
            .map_err(|e| file_unavailable(&e))?;
        let Some(latest) = versions.iter().max_by_key(|v| v.version) else {
            return Ok(());
        };
        let ticket = ordin_files::BinTicket {
            place_id: row.place_id,
            path: path.to_owned(),
            file_version_id: latest.id,
        };
        self.mark_removed(&row, ticket).await?;
        self.journal(
            "page_removed_outside",
            Some(row.workspace_id),
            None,
            "A knowledge page's file was deleted in another app.",
            json!({ "page_id": row.id }),
        )
        .await;
        Ok(())
    }

    fn refuse_adoption(
        &self,
        workspace: WorkspaceId,
        place: PlaceId,
        path: &str,
        reason: NotAdoptedReason,
        version: u32,
    ) -> bool {
        let mut map = lock(&self.not_adopted);
        let first = map
            .get(&(place, path.to_owned()))
            .is_none_or(|(_, r, v)| *r != reason || *v != version);
        map.insert((place, path.to_owned()), (workspace, reason, version));
        first
    }

    /// §R (with ADR-112 R changes): adopts a Markdown file someone created in `knowledge/`.
    pub(crate) async fn adopt_file(
        &self,
        workspace: WorkspaceId,
        place: PlaceId,
        path: &str,
        observed_at: Timestamp,
    ) -> Result<(), KnowledgeError> {
        if !is_page_path(path) || okf::is_reserved(path) {
            return Ok(());
        }
        let version = match self.files.current_version(place, path).await {
            Ok(Some(v)) => v.version,
            _ => 0,
        };
        let refuse = |reason: NotAdoptedReason| async move {
            if self.refuse_adoption(workspace, place, path, reason, version) {
                tracing::info!(component = "knowledge", reason = ?reason, "knowledge_adoption_refused");
                self.journal(
                    "adoption_refused",
                    Some(workspace),
                    None,
                    "A file in the knowledge folder could not be adopted as a page.",
                    json!({ "reason": reason }),
                )
                .await;
            }
        };
        let mut conn = self.conn().await?;
        if store::count_live(&mut conn, workspace).await? >= MAX_PAGES_PER_WORKSPACE {
            drop(conn);
            refuse(NotAdoptedReason::PageLimit).await;
            return Ok(());
        }
        drop(conn);
        let file = match self.files.read_file(place, path).await {
            Ok(f) => f,
            Err(FileError::NotFound) => return Ok(()),
            Err(FileError::TooLarge { .. }) => {
                refuse(NotAdoptedReason::TooLarge).await;
                return Ok(());
            }
            Err(e) => return Err(file_unavailable(&e)),
        };
        if file.bytes.len() > ADOPT_MAX_BYTES {
            refuse(NotAdoptedReason::TooLarge).await;
            return Ok(());
        }
        let Ok(text) = std::str::from_utf8(&file.bytes) else {
            refuse(NotAdoptedReason::HeaderInvalid).await;
            return Ok(());
        };
        let concept_path = pagefile::concept_path(path);
        let header = match pagefile::read_concept_file(&concept_path, text) {
            Ok(h)
                if h.extra_header.len() <= EXTRA_HEADER_MAX
                    && h.body.len() <= crate::names::BODY_MAX_BYTES =>
            {
                h
            }
            _ => {
                refuse(NotAdoptedReason::HeaderInvalid).await;
                return Ok(());
            }
        };
        let workspace_row = self.workspace(workspace).await?;
        let mut conn = self.conn().await?;
        let id = match header.page_id {
            Some(pid) => match store::page(&mut conn, pid).await? {
                None => pid,
                Some(r) if r.workspace_id == workspace => KnowledgePageId::new(),
                Some(_) => {
                    drop(conn);
                    refuse(NotAdoptedReason::IdInOtherWorkspace).await;
                    return Ok(());
                }
            },
            None => KnowledgePageId::new(),
        };
        // R-3: the title, made free with ` (2)`, ` (3)` …
        let base = if header.ordin_page || header.title_written {
            header.title.clone()
        } else {
            path_stem(path)
        };
        let base: String = base.chars().take(190).collect();
        let mut title = base.clone();
        let mut n = 2;
        while !self.name_free(&mut conn, workspace, &title, None).await? {
            title = format!("{base} ({n})");
            n += 1;
        }
        drop(conn);
        let now = observed_at;
        let mut row = PageRow {
            rowid: 0,
            id,
            workspace_id: workspace,
            title,
            body: header.body.clone(),
            written_by: KnowledgeAuthor::Person,
            edited_by_person: true,
            sources: if header.ordin_page {
                Vec::new()
            } else {
                header.sources.clone()
            },
            state: KnowledgePageState::Current,
            last_changed_at: now,
            place_id: workspace_row.own_folder_place_id,
            relative_path: path.to_owned(),
            file_version: 1,
            state_before_removal: None,
            bin_ticket: None,
            memory_indexed_version: None,
            memory_index_attempts: 0,
            memory_next_attempt_at: None,
            memory_removed: false,
            created_at: Timestamp::now(),
            shared_by: None,
            shared_at: None,
            first_exported_at: None,
            extra_header: header.extra_header.clone(),
            tags: header.tags.clone(),
            aliases: header.aliases.clone(),
            title_refused: None,
            links_built_version: None,
            links_truncated: 0,
            page_type: header.page_type.clone(),
            description: header.description.clone(),
            okf_tier: header.tier,
            okf_status: header.status,
            status_text: header.status_text.clone(),
            stale_after: header.stale_after.clone(),
            verified: header.verified.clone(),
            generated: None,
        };
        if row.page_type.is_none() {
            row.page_type = Some("Explanation".to_owned());
        }
        match self
            .commit_page(Commit {
                row,
                old: None,
                origin: FileVersionOrigin::Person,
                expected_hash: Some(file.hash.clone()),
                verify: false,
                run_id: None,
                type_set: false,
            })
            .await
        {
            Ok(written) => {
                tracing::info!(component = "knowledge", page_id = %written.id, "knowledge_page_adopted");
                self.journal(
                    "page_adopted",
                    Some(workspace),
                    None,
                    "A file in the knowledge folder was adopted as a page.",
                    json!({ "page_id": written.id }),
                )
                .await;
                Ok(())
            }
            // FILE_CHANGED_MEANWHILE: stop; the next change adopts it.
            Err(KnowledgeError::FileUnavailable) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// §R step 6 and AF: once at start, every known workspace's `knowledge/` folder.
    pub(crate) async fn startup_scan(&self) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let started = std::time::Instant::now();
        let pool = self.pool()?;
        let places: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT workspace_id, place_id FROM knowledge_pages ORDER BY workspace_id",
        )
        .fetch_all(pool)
        .await?;
        let mut known: Vec<(WorkspaceId, PlaceId)> = places
            .iter()
            .filter_map(|(w, p)| Some((WorkspaceId::parse(w).ok()?, PlaceId::parse(p).ok()?)))
            .collect();
        // FU-103: every workspace the Workspace Service lists, met or not.
        let listed = self
            .workspaces
            .list_not_in_bin()
            .await
            .map_err(|_| KnowledgeError::DbUnavailable)?;
        for w in listed {
            lock(&self.place_workspaces).insert(w.own_folder_place_id, w.id);
        }
        for (p, w) in lock(&self.place_workspaces).iter() {
            if !known.iter().any(|(_, kp)| kp == p) {
                known.push((*w, *p));
            }
        }
        // The seeded names of migration step 4 are rewritten with the full key first.
        {
            let _guard = self.write_lock.lock().await;
            let mut conn = self.conn().await?;
            for row in store::all_live_pages(&mut conn).await? {
                store::replace_names(&mut conn, &row).await?;
            }
        }
        let mut pages = 0usize;
        let mut adopted = 0usize;
        for (workspace, place) in known {
            lock(&self.place_workspaces).insert(place, workspace);
            self.move_reserved(workspace).await?;
            for path in self.knowledge_files(place).await {
                let mut conn = self.conn().await?;
                let row = store::page_by_file(&mut conn, place, &path).await?;
                drop(conn);
                if row.is_none() && !okf::is_reserved(&path) && is_page_path(&path) {
                    let _guard = self.write_lock.lock().await;
                    let before = self.count_pages(workspace).await?;
                    self.adopt_file(workspace, place, &path, Timestamp::now())
                        .await?;
                    if self.count_pages(workspace).await? > before {
                        adopted += 1;
                    }
                }
            }
            let _guard = self.write_lock.lock().await;
            let mut conn = self.conn().await?;
            let rows = store::workspace_pages(&mut conn, workspace).await?;
            for mut r in rows {
                pages += 1;
                if r.links_built_version != Some(r.file_version) {
                    self.rebuild_links(&mut conn, &mut r).await?;
                    self.resolve_names(&mut conn, &r).await?;
                }
            }
            drop(conn);
            self.schedule_index(workspace);
        }
        self.run_due_rewrites(Timestamp::now()).await?;
        tracing::info!(
            component = "knowledge",
            pages,
            adopted,
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "knowledge_scan_finished"
        );
        Ok(())
    }

    async fn count_pages(&self, workspace: WorkspaceId) -> Result<u32, KnowledgeError> {
        let mut conn = self.conn().await?;
        store::count_live(&mut conn, workspace).await
    }

    /// Every file below `knowledge/` to SA-KLINK-06's depth, through `list_folder` pages of 100.
    async fn knowledge_files(&self, place: PlaceId) -> Vec<String> {
        let mut out = Vec::new();
        let mut folders = vec![("knowledge".to_owned(), 0usize)];
        while let Some((folder, depth)) = folders.pop() {
            let mut cursor = None;
            loop {
                let Ok(page) = self
                    .files
                    .list_folder(place, &folder, FolderSort::NameAsc, cursor)
                    .await
                else {
                    break;
                };
                for e in page.items {
                    let name = e
                        .path
                        .trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    if e.is_folder {
                        if depth < 4 {
                            folders.push((e.path.trim_end_matches('/').to_owned(), depth + 1));
                        }
                    } else {
                        out.push(e.path.clone());
                    }
                }
                match page.next_cursor {
                    Some(c) => cursor = Some(c),
                    None => break,
                }
            }
        }
        out.sort();
        out
    }

    /// SA-OKF-08 / AF: a page row whose file is a reserved name is moved to `<stem> page.md`.
    async fn move_reserved(&self, workspace: WorkspaceId) -> Result<(), KnowledgeError> {
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let rows = store::workspace_pages(&mut conn, workspace).await?;
        drop(conn);
        for row in rows
            .into_iter()
            .filter(|r| okf::is_reserved(&r.relative_path))
        {
            let folder = crate::names::folder_of(&row.relative_path).to_owned();
            let stem = path_stem(&row.relative_path);
            let to = format!("{folder}/{stem} page.md");
            match self
                .files
                .move_file(row.place_id, &row.relative_path, &to)
                .await
            {
                Ok(_) => {
                    let pool = self.pool()?;
                    sqlx::query("UPDATE knowledge_pages SET relative_path = ?2 WHERE id = ?1")
                        .bind(row.id.to_string())
                        .bind(&to)
                        .execute(pool)
                        .await?;
                    self.journal(
                        "page_moved_from_reserved",
                        Some(workspace),
                        None,
                        "A knowledge page's file was renamed away from a reserved name.",
                        json!({ "page_id": row.id }),
                    )
                    .await;
                }
                Err(e) => tracing::warn!(
                    component = "knowledge",
                    code = e.code(),
                    "knowledge_reserved_move_failed"
                ),
            }
        }
        Ok(())
    }
}

/// Replaces the `title:` line of a file's front matter.
pub(crate) fn replace_title_line(text: &str, title: &str) -> Option<String> {
    let (header, _) = okf::split_front_matter(text).ok()?;
    let header = header?;
    let header_start = text.find('\n')? + 1;
    let mut offset = header_start;
    for line in header.split_inclusive('\n') {
        if line.starts_with("title:") {
            let end = offset + line.trim_end_matches('\n').len();
            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..offset]);
            out.push_str("title: ");
            out.push_str(&okf::yaml_str(title));
            out.push_str(&text[end..]);
            return Some(out);
        }
        offset += line.len();
    }
    None
}
