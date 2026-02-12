//! Writing, reading and the person's choices on workspace pages: *Internal Logic* B, D, E and
//! their amendments (ADR-104 B changes and Q-1, ADR-112 B changes, ADR-110 SA-KREAD-01).

use ordin_files::{FileError, FileVersionOrigin, Sha256Hex, WriteVersion};
use ordin_push::PublishCopyRequest;
use ordin_types::domain::places::{
    CopyDestination, CopyFormat, CopyOriginal, KnowledgeAuthor, KnowledgePage, KnowledgePageState,
    KnowledgeSource, Workspace,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::PageStatus;
use ordin_types::{AgentInstanceId, KnowledgePageId, PublishedCopyId, RunId, WorkspaceId};
use serde_json::json;

use crate::error::KnowledgeError;
use crate::markdown::changed_lines;
use crate::model::{
    AgentReferencePage, KnowledgeBlock, PageListing, PageSummary, PageView, SourceChange,
    UpdateDiff, VerifiedView, WritePage,
};
use crate::names::{
    check_aliases, check_body, check_description, check_page_type, check_sources, check_tags,
    check_title, file_stem_of_title, folder_of, name_key, numbered_file_name, path_stem,
};
use crate::okf;
use crate::pagefile::render;
use crate::service::{KnowledgeService, MAX_PAGES_PER_WORKSPACE, MAX_VERIFIED};
use crate::store::{self, Conn, Generated, PageRow, StoredSource, Verification};

/// Maps a File Service failure of a page write (B-5).
pub(crate) fn file_unavailable(e: &FileError) -> KnowledgeError {
    tracing::warn!(
        component = "knowledge",
        code = e.code(),
        retryable = e.retryable(),
        "knowledge_file_failed"
    );
    // Always retryable (KnowledgeService.md B-5): the cause is in the log event above.
    KnowledgeError::FileUnavailable
}

/// One write of a page file and its row (B-5 to B-7). The caller holds the write lock.
pub(crate) struct Commit {
    pub row: PageRow,
    pub old: Option<PageRow>,
    pub origin: FileVersionOrigin,
    pub expected_hash: Option<Sha256Hex>,
    /// Adds the person's verification (SA-OKF-03).
    pub verify: bool,
    pub run_id: Option<RunId>,
    pub type_set: bool,
}

impl KnowledgeService {
    /// The page row of an id, or `KNOW_READ_ONLY` for a read page's id, else `KNOW_PAGE_NOT_FOUND`.
    pub(crate) async fn own_row(
        &self,
        conn: &mut Conn,
        id: KnowledgePageId,
    ) -> Result<PageRow, KnowledgeError> {
        if let Some(row) = store::page(conn, id).await? {
            return Ok(row);
        }
        if crate::read::is_read_page_id(conn, id).await? {
            return Err(KnowledgeError::ReadOnly);
        }
        Err(KnowledgeError::PageNotFound)
    }

    /// Whether a relative path is taken in the place (a page row, compared case-insensitively,
    /// or a file the File Service holds a version of).
    pub(crate) async fn path_taken(
        &self,
        conn: &mut Conn,
        row_place: ordin_types::PlaceId,
        path: &str,
    ) -> Result<bool, KnowledgeError> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM knowledge_pages WHERE place_id = ?1 AND lower(relative_path) = lower(?2)",
        )
        .bind(row_place.to_string())
        .bind(path)
        .fetch_one(&mut *conn)
        .await?;
        if n > 0 {
            return Ok(true);
        }
        match self.files.current_version(row_place, path).await {
            Ok(Some(_)) => Ok(true),
            Ok(None) | Err(FileError::NotFound) => Ok(false),
            Err(e) => Err(file_unavailable(&e)),
        }
    }

    /// SA-KLINK-04: the free file name of a title in a folder (` (2)`, ` (3)` … when taken).
    pub(crate) async fn free_file_name(
        &self,
        conn: &mut Conn,
        place: ordin_types::PlaceId,
        folder: &str,
        title: &str,
    ) -> Result<String, KnowledgeError> {
        let stem = file_stem_of_title(title);
        for n in 1..=1_000u32 {
            let path = format!("{folder}/{}", numbered_file_name(&stem, n));
            if !self.path_taken(conn, place, &path).await? {
                return Ok(path);
            }
        }
        Err(KnowledgeError::FileUnavailable)
    }

    /// B-3: a name key must be unused among the workspace's titles and aliases of other pages.
    pub(crate) async fn name_free(
        &self,
        conn: &mut Conn,
        workspace: WorkspaceId,
        name: &str,
        except: Option<KnowledgePageId>,
    ) -> Result<bool, KnowledgeError> {
        let holder = store::name_holder(conn, workspace, &name_key(name)).await?;
        Ok(holder.is_none() || holder == except)
    }

    async fn check_names(
        &self,
        conn: &mut Conn,
        workspace: WorkspaceId,
        title: &str,
        aliases: &[String],
        except: Option<KnowledgePageId>,
    ) -> Result<(), KnowledgeError> {
        if !self.name_free(conn, workspace, title, except).await? {
            return Err(KnowledgeError::TitleDuplicate);
        }
        for a in aliases {
            if name_key(a) != name_key(title) && !self.name_free(conn, workspace, a, except).await?
            {
                return Err(KnowledgeError::TitleDuplicate);
            }
        }
        Ok(())
    }

    /// The person's verifier (SA-OKF-03).
    pub(crate) async fn verifier(&self) -> String {
        match self.identity.git_login().await {
            Some(login) if !login.trim().is_empty() && !login.contains('@') => {
                format!("human:{login}")
            }
            _ => "human:person".to_owned(),
        }
    }

    /// B-5 to B-7 for one page. Returns the row as stored.
    pub(crate) async fn commit_page(&self, c: Commit) -> Result<PageRow, KnowledgeError> {
        let Commit {
            mut row,
            old,
            origin,
            expected_hash,
            verify,
            run_id,
            type_set,
        } = c;
        let now = Timestamp::now();
        // ADR-112 B-5: type, description, generated, verified, sources.
        if row.page_type.is_none() {
            row.page_type = Some("Explanation".to_owned());
        }
        if row.description.is_none() {
            row.description = Some(
                okf::first_sentence(&row.body)
                    .unwrap_or_else(|| row.title.chars().take(200).collect()),
            );
        }
        row.generated = Some(Generated {
            by: format!("ordin/{}", self.app_version),
            at: Some(row.last_changed_at.to_wire()),
        });
        let mut verified_added = false;
        if verify {
            let by = self.verifier().await;
            match row.verified.last_mut() {
                Some(last) if last.by == by => last.at = Some(now.to_wire()),
                _ => row.verified.push(Verification {
                    by,
                    at: Some(now.to_wire()),
                }),
            }
            while row.verified.len() > MAX_VERIFIED {
                row.verified.remove(0);
            }
            verified_added = true;
        }
        row.okf_tier = okf::tier_of(row.verified.iter().map(|v| v.by.as_str()));
        let old_sources = old.as_ref().map(|o| o.sources.clone()).unwrap_or_default();
        for s in &mut row.sources {
            match old_sources
                .iter()
                .find(|o| o.origin == s.origin && o.location == s.location)
            {
                Some(o) if o.version_read == s.version_read => {
                    if s.last_modified.is_none() {
                        s.last_modified = o.last_modified.clone();
                    }
                    if s.extra.is_none() {
                        s.extra = o.extra.clone();
                    }
                }
                Some(o) => {
                    s.last_modified = Some(now.to_wire());
                    if s.extra.is_none() {
                        s.extra = o.extra.clone();
                    }
                }
                None if s.last_modified.is_none() => s.last_modified = Some(now.to_wire()),
                None => {}
            }
        }
        let bytes = render(&row);
        let written = self
            .files
            .write_version(WriteVersion {
                place_id: row.place_id,
                path: row.relative_path.clone(),
                bytes,
                origin,
                expected_hash,
            })
            .await
            .map_err(|e| file_unavailable(&e))?;
        row.file_version = written.version;
        row.memory_indexed_version = None;
        row.memory_index_attempts = 0;
        row.memory_next_attempt_at = None;
        row.memory_removed = false;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        if let Some(o) = &old {
            if o.state != KnowledgePageState::Removed {
                store::fts_delete(&mut tx, o).await?;
            }
            store::update_page(&mut tx, &row).await?;
        } else {
            row.rowid = store::insert_page(&mut tx, &row).await?;
        }
        if row.state != KnowledgePageState::Removed {
            store::fts_insert(&mut tx, &row).await?;
        }
        store::clear_marks_and_update(&mut tx, row.id).await?;
        store::replace_names(&mut tx, &row).await?;
        store::replace_tags(&mut tx, &row).await?;
        self.rebuild_links(&mut tx, &mut row).await?;
        self.resolve_names(&mut tx, &row).await?;
        tx.commit().await?;
        crate::service::lock(&self.place_workspaces).insert(row.place_id, row.workspace_id);
        tracing::info!(
            component = "knowledge",
            page_id = row.id.to_string().as_str(),
            written_by = store::author_text(row.written_by),
            version = row.file_version,
            "knowledge_page_written"
        );
        self.journal(
            "page_written",
            Some(row.workspace_id),
            run_id,
            "A knowledge page was written.",
            json!({
                "page_id": row.id,
                "written_by": store::author_text(row.written_by),
                "version": row.file_version,
                "source_count": row.sources.len(),
                "type_set": type_set,
                "verified_added": verified_added,
            }),
        )
        .await;
        self.pages_changed(row.workspace_id).await;
        self.wake_indexer();
        self.schedule_index(row.workspace_id);
        self.refresh_suggestions_after(&row).await;
        Ok(row)
    }

    fn new_row(
        &self,
        id: KnowledgePageId,
        workspace: &Workspace,
        path: String,
        title: String,
        body: String,
        written_by: KnowledgeAuthor,
    ) -> PageRow {
        let now = Timestamp::now();
        PageRow {
            rowid: 0,
            id,
            workspace_id: workspace.id,
            title,
            body,
            written_by,
            edited_by_person: written_by == KnowledgeAuthor::Person,
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
            okf_tier: okf::PageTier::Unverified,
            okf_status: PageStatus::Stable,
            status_text: None,
            stale_after: None,
            verified: Vec::new(),
            generated: None,
        }
    }

    /// Ordin's `write_page` (via the Ordin Host) and the person's "write down…" request.
    pub async fn write_page(
        &self,
        by: KnowledgeAuthor,
        req: WritePage,
    ) -> Result<KnowledgePageId, KnowledgeError> {
        self.desktop_only()?;
        // B-1
        let title = check_title(&req.title)?;
        check_body(&req.body_markdown)?;
        check_sources(&req.sources, false)?;
        let tags = req.tags.as_deref().map(check_tags).transpose()?;
        let aliases = req.aliases.as_deref().map(check_aliases).transpose()?;
        let page_type = req.page_type.as_deref().map(check_page_type).transpose()?;
        let description = req
            .description
            .as_deref()
            .map(check_description)
            .transpose()?;
        // B-2
        let workspace = self.workspace(req.workspace_id).await?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let sources: Vec<StoredSource> = req.sources.iter().map(StoredSource::from_plain).collect();
        let origin = match (by, req.run_id) {
            (KnowledgeAuthor::Ordin, Some(run_id)) => FileVersionOrigin::Run {
                run_id,
                tool: "write_page".to_owned(),
            },
            _ => FileVersionOrigin::Person,
        };
        let type_set = page_type.is_some();
        let row = match req.page_id {
            None => {
                // B-3
                if store::count_live(&mut conn, workspace.id).await? >= MAX_PAGES_PER_WORKSPACE {
                    return Err(KnowledgeError::PageLimit);
                }
                let aliases = aliases.unwrap_or_default();
                self.check_names(&mut conn, workspace.id, &title, &aliases, None)
                    .await?;
                let path = self
                    .free_file_name(
                        &mut conn,
                        workspace.own_folder_place_id,
                        "knowledge",
                        &title,
                    )
                    .await?;
                let mut row = self.new_row(
                    KnowledgePageId::new(),
                    &workspace,
                    path,
                    title,
                    req.body_markdown,
                    by,
                );
                row.sources = sources;
                row.tags = tags.unwrap_or_default();
                row.aliases = aliases;
                row.page_type = page_type;
                row.description = description;
                drop(conn);
                self.commit_page(Commit {
                    row,
                    old: None,
                    origin,
                    expected_hash: None,
                    verify: by == KnowledgeAuthor::Person,
                    run_id: req.run_id,
                    type_set,
                })
                .await?
            }
            Some(id) => {
                // B-4
                let old = self.own_row(&mut conn, id).await?;
                if old.workspace_id != workspace.id {
                    return Err(KnowledgeError::PageNotFound);
                }
                if old.state == KnowledgePageState::Removed {
                    return Err(KnowledgeError::PageRemoved);
                }
                let mut row = old.clone();
                let aliases = aliases.unwrap_or_else(|| old.aliases.clone());
                self.check_names(&mut conn, workspace.id, &title, &aliases, Some(id))
                    .await?;
                let renamed = name_key(&title) != name_key(&old.title);
                row.title = title;
                row.body = req.body_markdown;
                row.sources = sources;
                if let Some(t) = tags {
                    row.tags = t;
                }
                row.aliases = aliases;
                if page_type.is_some() {
                    row.page_type = page_type;
                }
                if description.is_some() {
                    row.description = description;
                }
                row.written_by = if by == KnowledgeAuthor::Ordin {
                    old.written_by
                } else {
                    by
                };
                row.edited_by_person = by == KnowledgeAuthor::Person;
                row.last_changed_at = Timestamp::now();
                row.state = KnowledgePageState::Current;
                row.title_refused = None;
                drop(conn);
                self.write_existing(
                    row,
                    old,
                    origin,
                    by == KnowledgeAuthor::Person,
                    req.run_id,
                    type_set,
                    renamed,
                )
                .await?
            }
        };
        Ok(row.id)
    }

    /// An existing page's write, renaming its file first when the title's name key changed (Q-1).
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn write_existing(
        &self,
        mut row: PageRow,
        old: PageRow,
        origin: FileVersionOrigin,
        verify: bool,
        run_id: Option<RunId>,
        type_set: bool,
        renamed: bool,
    ) -> Result<PageRow, KnowledgeError> {
        if renamed {
            row.relative_path = self.move_for_title(&old, &row.title).await?;
        }
        let written = self
            .commit_page(Commit {
                row,
                old: Some(old.clone()),
                origin,
                expected_hash: None,
                verify,
                run_id,
                type_set,
            })
            .await?;
        if renamed {
            self.journal(
                "page_renamed",
                Some(written.workspace_id),
                run_id,
                "A knowledge page was renamed.",
                json!({ "page_id": written.id }),
            )
            .await;
            let keys = crate::links::old_keys(&old);
            self.rewrite_linking_pages(&written, &keys).await;
        }
        Ok(written)
    }

    /// Q-1: moves the page's file to its new title's name in the same folder.
    async fn move_for_title(&self, old: &PageRow, title: &str) -> Result<String, KnowledgeError> {
        let folder = folder_of(&old.relative_path).to_owned();
        let stem = file_stem_of_title(title);
        let mut conn = self.conn().await?;
        for n in 1u32..=20 {
            let to = format!("{folder}/{}", numbered_file_name(&stem, n));
            if to.eq_ignore_ascii_case(&old.relative_path) && to != old.relative_path {
                // A change of case only: the file system may treat both names as one.
            } else if self.path_taken(&mut conn, old.place_id, &to).await? {
                continue;
            }
            match self
                .files
                .move_file(old.place_id, &old.relative_path, &to)
                .await
            {
                Ok(_) => return Ok(to),
                Err(FileError::MoveTargetExists) => continue,
                Err(e) => return Err(file_unavailable(&e)),
            }
        }
        Err(KnowledgeError::FileUnavailable)
    }

    /// The person's edit from the page's screen (sources unchanged). ADR-104: tags and aliases
    /// optional (None = unchanged), a changed title renames the file; ADR-112: type and
    /// description optional, and the person's verification is added.
    #[allow(clippy::too_many_arguments)]
    pub async fn save_edit(
        &self,
        page_id: KnowledgePageId,
        title: String,
        body_markdown: String,
        tags: Option<Vec<String>>,
        aliases: Option<Vec<String>>,
        page_type: Option<String>,
        description: Option<String>,
    ) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let title = check_title(&title)?;
        check_body(&body_markdown)?;
        let tags = tags.as_deref().map(check_tags).transpose()?;
        let aliases = aliases.as_deref().map(check_aliases).transpose()?;
        let page_type = page_type.as_deref().map(check_page_type).transpose()?;
        let description = description.as_deref().map(check_description).transpose()?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let old = self.own_row(&mut conn, page_id).await?;
        if old.state == KnowledgePageState::Removed {
            return Err(KnowledgeError::PageRemoved);
        }
        let aliases = aliases.unwrap_or_else(|| old.aliases.clone());
        self.check_names(&mut conn, old.workspace_id, &title, &aliases, Some(page_id))
            .await?;
        drop(conn);
        let renamed = name_key(&title) != name_key(&old.title);
        let mut row = old.clone();
        row.title = title;
        row.body = body_markdown;
        if let Some(t) = tags {
            row.tags = t;
        }
        row.aliases = aliases;
        let type_set = page_type.is_some();
        if page_type.is_some() {
            row.page_type = page_type;
        }
        if description.is_some() {
            row.description = description;
        }
        row.edited_by_person = true;
        row.last_changed_at = Timestamp::now();
        row.title_refused = None;
        self.write_existing(
            row,
            old,
            FileVersionOrigin::Person,
            true,
            None,
            type_set,
            renamed,
        )
        .await?;
        Ok(())
    }

    /// The PageSummary of an own row.
    pub(crate) fn summary_of(&self, row: &PageRow) -> PageSummary {
        let now = Timestamp::now();
        PageSummary {
            workspace_id: row.workspace_id,
            id: row.id,
            title: row.title.clone(),
            state: row.state,
            written_by: row.written_by,
            last_changed_at: row.last_changed_at,
            shared_by: row.shared_by.clone(),
            shared_at: row.shared_at,
            cites_other_computer: self.cites_other_computer(&row.sources),
            tags: row.tags.clone(),
            folder: None,
            folder_description: None,
            set_by_company: None,
            folder_id: None,
            read_only: false,
            adopted_from: None,
            adoption_mark: None,
            page_type: row.page_type.clone(),
            description: row.description.clone(),
            trust: row.okf_tier,
            status: (row.okf_status != PageStatus::Stable).then_some(row.okf_status),
            past_stale_after: row
                .stale_after
                .as_deref()
                .and_then(okf::parse_stale_after)
                .is_some_and(|s| s <= now),
        }
    }

    pub async fn list_pages(
        &self,
        workspace_id: WorkspaceId,
        filter: Option<String>,
    ) -> Result<PageListing, KnowledgeError> {
        self.desktop_only()?;
        let workspace = self.workspace(workspace_id).await?;
        let mut conn = self.conn().await?;
        let rows = store::workspace_pages(&mut conn, workspace.id).await?;
        let filter = crate::search::ListFilter::parse(filter.as_deref().unwrap_or(""));
        let mut kept: Vec<PageRow> = rows.into_iter().filter(|r| filter.keeps(r)).collect();
        // G and V (ADR-112): deprecated pages only when nothing else matches.
        if kept.iter().any(|r| r.okf_status != PageStatus::Deprecated) {
            kept.retain(|r| r.okf_status != PageStatus::Deprecated);
        }
        kept.sort_by_key(|r| name_key(&r.title));
        let mut this_workspace = Vec::with_capacity(kept.len());
        for r in &kept {
            let mut s = self.summary_of(r);
            if let Some(label) = self.folders.adoption_for(workspace.id, r.id).await {
                s.adopted_from = Some(label.adopted_from);
                s.adoption_mark = label.mark;
            }
            this_workspace.push(s);
        }
        let from_agents = self.agent_pages(workspace.id).await;
        let not_adopted = self.not_adopted_of(workspace.id);
        drop(conn);
        let from_other_teams = self.list_read_pages(workspace.id, None).await?;
        Ok(PageListing {
            this_workspace,
            from_agents,
            not_adopted,
            from_other_teams,
        })
    }

    async fn agent_pages(&self, workspace: WorkspaceId) -> Vec<AgentReferencePage> {
        let Ok(agents) = self.workspaces.agents_in_workspace(workspace).await else {
            tracing::warn!(component = "knowledge", "knowledge_agents_unavailable");
            return Vec::new();
        };
        let mut out = Vec::new();
        for a in agents {
            let Ok(files) = self.files.list_agent_knowledge(a.agent_instance_id).await else {
                continue;
            };
            for f in files {
                if !f.relative_path.to_ascii_lowercase().ends_with(".md") {
                    continue;
                }
                out.push(AgentReferencePage {
                    agent_instance_id: a.agent_instance_id,
                    agent_name: a.display_name.clone(),
                    title: path_stem(&f.relative_path),
                    relative_path: f.relative_path,
                });
            }
        }
        out
    }

    pub async fn read_agent_page(
        &self,
        workspace_id: WorkspaceId,
        agent: AgentInstanceId,
        relative_path: &str,
    ) -> Result<String, KnowledgeError> {
        self.desktop_only()?;
        self.workspace(workspace_id).await?;
        let agents = self
            .workspaces
            .agents_in_workspace(workspace_id)
            .await
            .map_err(|_| KnowledgeError::AgentPageNotFound)?;
        if !agents.iter().any(|a| a.agent_instance_id == agent) {
            return Err(KnowledgeError::AgentPageNotFound);
        }
        // Only a file the folder lists is read, so no path outside it can be named.
        let listed = self
            .files
            .list_agent_knowledge(agent)
            .await
            .map_err(|e| match e {
                FileError::NotFound | FileError::PlaceNotFound => KnowledgeError::AgentPageNotFound,
                other => file_unavailable(&other),
            })?;
        if !listed.iter().any(|f| f.relative_path == relative_path) {
            return Err(KnowledgeError::AgentPageNotFound);
        }
        let bytes = self
            .files
            .read_agent_knowledge(agent, relative_path)
            .await
            .map_err(|e| match e {
                FileError::NotFound => KnowledgeError::AgentPageNotFound,
                other => file_unavailable(&other),
            })?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// The `PageView` of an own row (T-1 links and U suggestions for the person).
    pub(crate) async fn view_of(&self, row: &PageRow) -> Result<PageView, KnowledgeError> {
        let mut conn = self.conn().await?;
        let marks = store::marks(&mut conn, row.id).await?;
        let changes: Vec<SourceChange> = marks
            .iter()
            .filter(|m| m.left_at.is_none())
            .map(|m| SourceChange {
                origin: m.origin.clone(),
                location: m.location.clone(),
                version_read: row
                    .sources
                    .iter()
                    .find(|s| s.origin == m.origin && s.location == m.location)
                    .map(|s| s.version_read.clone())
                    .unwrap_or_default(),
                newer_version: m.newer_version.clone(),
                changed_by: m.changed_by.clone(),
                changed_at: m.changed_at,
            })
            .collect();
        let pending_update = store::pending_update(&mut conn, row.id).await?.is_some();
        let registered = store::registered_keys(&mut conn, row.id).await?;
        let unregistered_aliases = row
            .aliases
            .iter()
            .filter(|a| !registered.contains(&name_key(a)))
            .cloned()
            .collect();
        let (links_out, links_in) = crate::links::person_links(&mut conn, row).await?;
        let suggestions = crate::suggest::suggestions_of(&mut conn, row.id).await?;
        drop(conn);
        let summary = self.summary_of(row);
        let adoption = self.folders.adoption_for(row.workspace_id, row.id).await;
        Ok(PageView {
            page: page_of(row),
            changes,
            pending_update,
            relative_path: row.relative_path.clone(),
            shared_by: row.shared_by.clone(),
            shared_at: row.shared_at,
            cites_other_computer: summary.cites_other_computer,
            tags: row.tags.clone(),
            aliases: row.aliases.clone(),
            unregistered_aliases,
            title_refused: row.title_refused.clone(),
            links_out,
            links_in,
            links_truncated: row.links_truncated,
            suggestions,
            folder: None,
            folder_description: None,
            set_by_company: None,
            folder_id: None,
            read_only: false,
            adopted_from: adoption.as_ref().map(|a| a.adopted_from.clone()),
            adoption_mark: adoption.and_then(|a| a.mark),
            page_type: row.page_type.clone(),
            description: row.description.clone(),
            trust: row.okf_tier,
            status: summary.status,
            past_stale_after: summary.past_stale_after,
            verified: row
                .verified
                .iter()
                .rev()
                .take(MAX_VERIFIED)
                .map(|v| VerifiedView {
                    by: v.by.clone(),
                    at: v.at.clone(),
                })
                .collect(),
        })
    }

    /// `knowledge.get`: runs `check_page` first; a read page's id returns its read-only view.
    pub async fn get_page(&self, page_id: KnowledgePageId) -> Result<PageView, KnowledgeError> {
        self.desktop_only()?;
        let mut conn = self.conn().await?;
        let Some(_) = store::page(&mut conn, page_id).await? else {
            drop(conn);
            return self.read_page_view(page_id).await;
        };
        drop(conn);
        self.check_page(page_id).await?;
        let mut conn = self.conn().await?;
        let row = store::page(&mut conn, page_id)
            .await?
            .ok_or(KnowledgeError::PageNotFound)?;
        drop(conn);
        self.view_of(&row).await
    }

    /// D-2.
    pub async fn propose_update(
        &self,
        page_id: KnowledgePageId,
        body_markdown: String,
        sources: Vec<KnowledgeSource>,
        run_id: RunId,
    ) -> Result<UpdateDiff, KnowledgeError> {
        self.desktop_only()?;
        check_body(&body_markdown)?;
        check_sources(&sources, false)?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let row = self.own_row(&mut conn, page_id).await?;
        if row.state != KnowledgePageState::MayBeOutOfDate {
            return Err(KnowledgeError::NotOutOfDate);
        }
        let stored: Vec<StoredSource> = sources.iter().map(StoredSource::from_plain).collect();
        store::store_update(&mut conn, page_id, &body_markdown, &stored, run_id).await?;
        let changed = changed_lines(&row.body, &body_markdown);
        drop(conn);
        self.journal(
            "update_proposed",
            Some(row.workspace_id),
            Some(run_id),
            "An update of a knowledge page was proposed.",
            json!({ "page_id": page_id, "run_id": run_id, "changed_lines": changed }),
        )
        .await;
        Ok(UpdateDiff {
            page_id,
            before: row.body,
            after: body_markdown,
            changed_lines: changed,
        })
    }

    /// D-3 "Use the update".
    pub async fn use_update(&self, page_id: KnowledgePageId) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let old = self.own_row(&mut conn, page_id).await?;
        if old.state == KnowledgePageState::Removed {
            return Err(KnowledgeError::PageRemoved);
        }
        let update = store::pending_update(&mut conn, page_id)
            .await?
            .ok_or(KnowledgeError::NoPendingUpdate)?;
        drop(conn);
        let mut row = old.clone();
        row.body = update.body;
        row.sources = update.sources;
        row.state = KnowledgePageState::Current;
        row.last_changed_at = Timestamp::now();
        let written = self
            .commit_page(Commit {
                row,
                old: Some(old),
                origin: FileVersionOrigin::Run {
                    run_id: update.run_id,
                    tool: "bring_up_to_date".to_owned(),
                },
                expected_hash: None,
                verify: true,
                run_id: Some(update.run_id),
                type_set: false,
            })
            .await?;
        self.journal(
            "update_used",
            Some(written.workspace_id),
            Some(update.run_id),
            "An update of a knowledge page was used.",
            json!({ "page_id": page_id }),
        )
        .await;
        Ok(())
    }

    /// D-4 "Keep the page as it is".
    pub async fn keep_page(&self, page_id: KnowledgePageId) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let row = self.own_row(&mut conn, page_id).await?;
        if store::pending_update(&mut conn, page_id).await?.is_none() {
            return Err(KnowledgeError::NoPendingUpdate);
        }
        store::delete_update(&mut conn, page_id).await?;
        drop(conn);
        self.journal(
            "update_kept_page",
            Some(row.workspace_id),
            None,
            "A knowledge page was kept as it is.",
            json!({ "page_id": page_id }),
        )
        .await;
        Ok(())
    }

    /// C-5 "Leave it".
    pub async fn leave_it(&self, page_id: KnowledgePageId) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let row = self.own_row(&mut tx, page_id).await?;
        if row.state != KnowledgePageState::MayBeOutOfDate {
            return Err(KnowledgeError::NotOutOfDate);
        }
        sqlx::query(
            "UPDATE knowledge_source_marks SET left_at = ?2 WHERE page_id = ?1 AND left_at IS NULL",
        )
        .bind(page_id.to_string())
        .bind(Timestamp::now().to_wire())
        .execute(&mut *tx)
        .await?;
        store::set_state(&mut tx, page_id, KnowledgePageState::Current).await?;
        store::delete_update(&mut tx, page_id).await?;
        tx.commit().await?;
        self.journal(
            "page_left",
            Some(row.workspace_id),
            None,
            "A knowledge page was left as it is.",
            json!({ "page_id": page_id }),
        )
        .await;
        Ok(())
    }

    /// E-1 (with V): to the system's bin.
    // spec: ordin-desktop/components/KnowledgeService#KS-FE-1
    /// After everything Ordin remembered in a workspace was forgotten: every page of the workspace,
    /// and every page read from a folder, is marked not indexed so the indexer's next pass writes its
    /// passages and facts again. No file and no page changes. Returns the number of pages marked.
    pub async fn index_again(&self, workspace_id: WorkspaceId) -> Result<u64, KnowledgeError> {
        self.desktop_only()?;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let own = sqlx::query(
            "UPDATE knowledge_pages SET memory_indexed_version = NULL, memory_index_attempts = 0, \
             memory_next_attempt_at = NULL WHERE workspace_id = ?1 AND state <> 'removed'",
        )
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let read = sqlx::query(
            "UPDATE knowledge_read_pages SET memory_indexed_version = NULL, memory_index_attempts = 0, \
             memory_next_attempt_at = NULL WHERE workspace_id = ?1",
        )
        .bind(workspace_id.to_string())
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        let pages = own + read;
        self.wake_indexer();
        self.journal(
            "pages_to_index_again",
            Some(workspace_id),
            None,
            "A workspace's knowledge pages are indexed again after its memory was forgotten.",
            json!({ "workspace_id": workspace_id, "pages": pages }),
        )
        .await;
        Ok(pages)
    }

    pub async fn remove_page(&self, page_id: KnowledgePageId) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        self.remove_locked(page_id).await
    }

    pub(crate) async fn remove_locked(
        &self,
        page_id: KnowledgePageId,
    ) -> Result<(), KnowledgeError> {
        let mut conn = self.conn().await?;
        let old = self.own_row(&mut conn, page_id).await?;
        drop(conn);
        if old.state == KnowledgePageState::Removed {
            return Err(KnowledgeError::PageRemoved);
        }
        let ticket = self
            .files
            .move_to_bin(old.place_id, &old.relative_path)
            .await
            .map_err(|e| file_unavailable(&e))?;
        self.mark_removed(&old, ticket).await?;
        self.journal(
            "page_removed",
            Some(old.workspace_id),
            None,
            "A knowledge page was moved to the bin.",
            json!({ "page_id": page_id }),
        )
        .await;
        self.pages_changed(old.workspace_id).await;
        Ok(())
    }

    /// The row side of a removal (E-1, V, C-1 step 4, ADR-111 re-index of linking pages).
    pub(crate) async fn mark_removed(
        &self,
        old: &PageRow,
        ticket: ordin_files::BinTicket,
    ) -> Result<(), KnowledgeError> {
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        store::fts_delete(&mut tx, old).await?;
        let mut row = old.clone();
        row.state_before_removal = Some(old.state);
        row.state = KnowledgePageState::Removed;
        row.bin_ticket = Some(ticket);
        row.memory_removed = false;
        store::update_page(&mut tx, &row).await?;
        store::replace_names(&mut tx, &row).await?;
        store::replace_tags(&mut tx, &row).await?;
        crate::suggest::delete_for_page(&mut tx, row.id).await?;
        crate::links::unresolve_target(&mut tx, row.id).await?;
        tx.commit().await?;
        // spec: ordin-desktop/components/KnowledgeService#KNOW-REC-1
        match self
            .memory
            .remove_page(row.workspace_id, None, row.id)
            .await
        {
            Ok(()) => {
                sqlx::query("UPDATE knowledge_pages SET memory_removed = 1 WHERE id = ?1")
                    .bind(row.id.to_string())
                    .execute(pool)
                    .await?;
            }
            Err(e) => {
                tracing::warn!(
                    component = "knowledge",
                    page_id = row.id.to_string().as_str(),
                    code = e.code(),
                    "knowledge_memory_remove_failed"
                );
            }
        }
        self.wake_indexer();
        self.schedule_index(row.workspace_id);
        Ok(())
    }

    /// E-2 (with V).
    pub async fn undo_remove(&self, page_id: KnowledgePageId) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let old = self.own_row(&mut conn, page_id).await?;
        if old.state != KnowledgePageState::Removed {
            return Err(KnowledgeError::NotRemoved);
        }
        if !self
            .name_free(&mut conn, old.workspace_id, &old.title, Some(page_id))
            .await?
        {
            return Err(KnowledgeError::TitleDuplicate);
        }
        drop(conn);
        let ticket = old.bin_ticket.clone().ok_or(KnowledgeError::NotRemoved)?;
        let restored = match self.files.restore_from_bin(&ticket).await {
            Ok(v) => v,
            Err(FileError::NotFound) => {
                let mut conn = self.conn().await?;
                store::delete_page_row(&mut conn, page_id).await?;
                return Err(KnowledgeError::PageNotFound);
            }
            Err(e) => return Err(file_unavailable(&e)),
        };
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let mut row = old.clone();
        row.state = old
            .state_before_removal
            .unwrap_or(KnowledgePageState::Current);
        row.state_before_removal = None;
        row.bin_ticket = None;
        row.file_version = restored.version;
        row.memory_indexed_version = None;
        row.memory_index_attempts = 0;
        row.memory_next_attempt_at = None;
        row.memory_removed = false;
        row.relative_path = ticket.path.clone();
        store::update_page(&mut tx, &row).await?;
        store::fts_insert(&mut tx, &row).await?;
        store::replace_names(&mut tx, &row).await?;
        store::replace_tags(&mut tx, &row).await?;
        self.rebuild_links(&mut tx, &mut row).await?;
        self.resolve_names(&mut tx, &row).await?;
        tx.commit().await?;
        self.journal(
            "page_restored",
            Some(row.workspace_id),
            None,
            "A knowledge page was restored from the bin.",
            json!({ "page_id": page_id }),
        )
        .await;
        self.pages_changed(row.workspace_id).await;
        self.wake_indexer();
        self.schedule_index(row.workspace_id);
        self.refresh_suggestions_after(&row).await;
        Ok(())
    }

    /// E-3 (with V): only on the person's request. The copy carries the title, body, sources,
    /// tags and `written_by`; aliases only where free in the target workspace.
    pub async fn copy_to_workspace(
        &self,
        page_id: KnowledgePageId,
        to: WorkspaceId,
    ) -> Result<KnowledgePageId, KnowledgeError> {
        self.desktop_only()?;
        let target = self.workspace(to).await?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let src = self.own_row(&mut conn, page_id).await?;
        if src.state == KnowledgePageState::Removed {
            return Err(KnowledgeError::PageRemoved);
        }
        if store::count_live(&mut conn, target.id).await? >= MAX_PAGES_PER_WORKSPACE {
            return Err(KnowledgeError::PageLimit);
        }
        if !self
            .name_free(&mut conn, target.id, &src.title, None)
            .await?
        {
            return Err(KnowledgeError::TitleDuplicate);
        }
        let mut aliases = Vec::new();
        for a in &src.aliases {
            if name_key(a) != name_key(&src.title)
                && self.name_free(&mut conn, target.id, a, None).await?
            {
                aliases.push(a.clone());
            }
        }
        let path = self
            .free_file_name(
                &mut conn,
                target.own_folder_place_id,
                "knowledge",
                &src.title,
            )
            .await?;
        drop(conn);
        let mut row = self.new_row(
            KnowledgePageId::new(),
            &target,
            path,
            src.title.clone(),
            src.body.clone(),
            src.written_by,
        );
        row.edited_by_person = src.edited_by_person;
        row.sources = src.sources.clone();
        row.tags = src.tags.clone();
        row.aliases = aliases;
        row.page_type = src.page_type.clone();
        row.description = src.description.clone();
        row.extra_header = src.extra_header.clone();
        let written = self
            .commit_page(Commit {
                row,
                old: None,
                origin: FileVersionOrigin::Person,
                expected_hash: None,
                verify: false,
                run_id: None,
                type_set: false,
            })
            .await?;
        Ok(written.id)
    }

    /// E-4 (SA-CLOSE-01, SA-RECON-10).
    pub async fn publish_to_confluence(
        &self,
        page_id: KnowledgePageId,
        destination: CopyDestination,
        keep_up_to_date: bool,
    ) -> Result<PublishedCopyId, KnowledgeError> {
        self.desktop_only()?;
        let mut conn = self.conn().await?;
        let row = self.own_row(&mut conn, page_id).await?;
        drop(conn);
        if row.state == KnowledgePageState::Removed {
            return Err(KnowledgeError::PageRemoved);
        }
        let original_name = row
            .relative_path
            .rsplit('/')
            .next()
            .unwrap_or(&row.relative_path)
            .to_owned();
        // spec: ordin-desktop/components/KnowledgeService#SA-CLOSE-01
        let req = PublishCopyRequest {
            workspace_id: row.workspace_id,
            original: CopyOriginal::KnowledgePage {
                knowledge_page_id: page_id,
            },
            original_version: row.file_version.to_string(),
            original_name,
            page_file: Some((row.place_id, row.relative_path.clone())),
            destination,
            format: CopyFormat::Page,
            followed_branch: None,
            keep_up_to_date,
            file_name: row.title.clone(),
        };
        let id = self.push.publish_copy(req).await.map_err(|e| {
            tracing::warn!(
                component = "knowledge",
                code = e.code,
                "knowledge_publish_failed"
            );
            KnowledgeError::PublishFailed
        })?;
        self.journal(
            "publish_requested",
            Some(row.workspace_id),
            None,
            "A knowledge page was offered for publishing to Confluence.",
            json!({ "page_id": page_id }),
        )
        .await;
        Ok(id)
    }

    /// Block check: resolves a KnowledgeBlock (the caller raises BLOCK_REF_UNRESOLVED on
    /// `Err(PageNotFound)`).
    pub async fn resolve_block(&self, block: &KnowledgeBlock) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let Some(page) = block.page else {
            self.workspace(block.workspace_id).await?;
            return Ok(());
        };
        let mut conn = self.conn().await?;
        match store::page(&mut conn, page).await? {
            Some(r)
                if r.workspace_id == block.workspace_id
                    && r.state != KnowledgePageState::Removed =>
            {
                Ok(())
            }
            Some(_) => Err(KnowledgeError::PageNotFound),
            None => {
                if crate::read::read_row(&mut conn, block.workspace_id, page)
                    .await?
                    .is_some()
                {
                    Ok(())
                } else {
                    Err(KnowledgeError::PageNotFound)
                }
            }
        }
    }

    /// Refreshes suggestions after a body or name change; a failure is logged (U-1).
    pub(crate) async fn refresh_suggestions_after(&self, row: &PageRow) {
        if let Err(e) = self.refresh_suggestions(row).await {
            tracing::warn!(
                component = "knowledge",
                page_id = row.id.to_string().as_str(),
                code = e.code(),
                "knowledge_suggestions_failed"
            );
        }
    }
}

pub(crate) fn page_of(row: &PageRow) -> KnowledgePage {
    KnowledgePage {
        id: row.id,
        workspace_id: row.workspace_id,
        title: row.title.clone(),
        body_markdown: row.body.clone(),
        written_by: row.written_by,
        edited_by_person: row.edited_by_person,
        sources: row.sources.iter().map(StoredSource::plain).collect(),
        state: row.state,
        last_changed_at: row.last_changed_at,
    }
}
