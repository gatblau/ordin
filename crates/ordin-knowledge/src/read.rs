//! Reading other teams' knowledge (amendment January 2026, ADR-110 §R–§X) and plain OKF
//! bundles as read folders (ADR-112 §AE). Read pages live in `knowledge_read_pages`, never in
//! `knowledge_pages`, so they can never be shared, renamed into another team or written into the
//! person's `knowledge/` folder (SA-KREAD-01 to -06).

use ordin_files::FileVersionOrigin;
use ordin_memory_service::PersonalDataFinding;
use ordin_types::domain::places::{KnowledgeAuthor, KnowledgePage, KnowledgePageState};
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::{PageStatus, PageTier};
use ordin_types::{KnowledgePageId, TeamFolderId, WorkspaceId};
use serde_json::json;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::error::KnowledgeError;
use crate::markdown::{fts_any_word, resolve_bundle_path, scan_links, split_sections, LinkForm};
use crate::model::{
    AdoptEdit, PageLinkView, PageRef, PageSummary, PageView, TeamPageImported, VerifiedView,
};
use crate::names::{check_body, check_sources, check_title, folder_of, name_key, strip_md};
use crate::okf;
use crate::pagefile::{read_concept_file, sha256_hex, FileHeader, EXTRA_HEADER_MAX};
use crate::pages::Commit;
use crate::search::ToolFilter;
use crate::service::{KnowledgeService, MAX_PAGES_PER_WORKSPACE, MAX_READ_PAGES_PER_FOLDER};
use crate::store::{self, Conn, Generated, StoredSource, Verification};

/// A team page file's bound: 1 MiB + 4 KiB (M-1).
pub(crate) const TEAM_PAGE_MAX: usize = 1_048_576 + 4_096;

/// A read page's id may be a UUIDv5 (SA-OKF-10), so it is parsed as any UUID.
pub(crate) fn parse_page_id(raw: &str) -> Result<KnowledgePageId, KnowledgeError> {
    uuid::Uuid::parse_str(raw)
        .map(KnowledgePageId)
        .map_err(|_| store::corrupt())
}

/// The id a concept of a plain OKF bundle has: UUIDv5 of the folder (namespace) and its path.
// spec: ordin-desktop/components/KnowledgeService#SA-OKF-10
pub(crate) fn concept_id(folder_id: TeamFolderId, path: &str) -> KnowledgePageId {
    KnowledgePageId(uuid::Uuid::new_v5(&folder_id.0, path.as_bytes()))
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ReadRow {
    pub rowid: i64,
    pub workspace_id: WorkspaceId,
    pub page_id: KnowledgePageId,
    pub folder_id: TeamFolderId,
    pub title: String,
    pub body: String,
    pub written_by: KnowledgeAuthor,
    pub sources: Vec<StoredSource>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub extra_header: String,
    pub shared_by: String,
    pub shared_at: Timestamp,
    pub last_changed_at: Timestamp,
    pub bytes_sha256: String,
    pub version: u32,
    pub memory_index_attempts: u32,
    pub page_type: Option<String>,
    pub description: Option<String>,
    pub okf_tier: PageTier,
    pub okf_status: PageStatus,
    pub status_text: Option<String>,
    pub stale_after: Option<String>,
    pub verified: Vec<Verification>,
    pub generated: Option<Generated>,
    pub concept_path: Option<String>,
}

const READ_COLUMNS_SQL: &str = "SELECT rowid, workspace_id, page_id, folder_id, title, body_markdown, written_by, sources, tags, \
    aliases, extra_header, shared_by, shared_at, last_changed_at, bytes_sha256, version, memory_index_attempts, \
    page_type, description, okf_tier, okf_status, status_text, stale_after, verified, generated, concept_path \
    FROM knowledge_read_pages WHERE workspace_id = ?1 AND page_id = ?2";

fn read_from(r: &SqliteRow) -> Result<ReadRow, KnowledgeError> {
    let w: String = r.try_get("workspace_id")?;
    let p: String = r.try_get("page_id")?;
    let f: String = r.try_get("folder_id")?;
    let generated: Option<String> = r.try_get("generated")?;
    fn json<T: serde::de::DeserializeOwned>(raw: String) -> Result<T, KnowledgeError> {
        serde_json::from_str(&raw).map_err(|_| store::corrupt())
    }
    Ok(ReadRow {
        rowid: r.try_get("rowid")?,
        workspace_id: WorkspaceId::parse(&w).map_err(|_| store::corrupt())?,
        page_id: parse_page_id(&p)?,
        folder_id: TeamFolderId(uuid::Uuid::parse_str(&f).map_err(|_| store::corrupt())?),
        title: r.try_get("title")?,
        body: r.try_get("body_markdown")?,
        written_by: store::author_of(&r.try_get::<String, _>("written_by")?)?,
        sources: json(r.try_get("sources")?)?,
        tags: json(r.try_get("tags")?)?,
        aliases: json(r.try_get("aliases")?)?,
        extra_header: r.try_get("extra_header")?,
        shared_by: r.try_get("shared_by")?,
        shared_at: store::ts(&r.try_get::<String, _>("shared_at")?)?,
        last_changed_at: store::ts(&r.try_get::<String, _>("last_changed_at")?)?,
        bytes_sha256: r.try_get("bytes_sha256")?,
        version: u32::try_from(r.try_get::<i64, _>("version")?).unwrap_or(1),
        memory_index_attempts: u32::try_from(r.try_get::<i64, _>("memory_index_attempts")?)
            .unwrap_or(0),
        page_type: r.try_get("page_type")?,
        description: r.try_get("description")?,
        okf_tier: store::tier_of(&r.try_get::<String, _>("okf_tier")?),
        okf_status: store::status_of(&r.try_get::<String, _>("okf_status")?),
        status_text: r.try_get("status_text")?,
        stale_after: r.try_get("stale_after")?,
        verified: json(r.try_get("verified")?)?,
        generated: generated.map(json).transpose()?,
        concept_path: r.try_get("concept_path")?,
    })
}

pub(crate) async fn read_row(
    conn: &mut Conn,
    workspace: WorkspaceId,
    page: KnowledgePageId,
) -> Result<Option<ReadRow>, KnowledgeError> {
    let row = sqlx::query(READ_COLUMNS_SQL)
        .bind(workspace.to_string())
        .bind(page.to_string())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref().map(read_from).transpose()
}

pub(crate) async fn is_read_page_id(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<bool, KnowledgeError> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_read_pages WHERE page_id = ?1")
        .bind(page.to_string())
        .fetch_one(&mut *conn)
        .await?;
    Ok(n > 0)
}

fn fts_title(row: &ReadRow) -> String {
    store::fts_title(&row.title, &row.aliases)
}

async fn fts_delete(conn: &mut Conn, row: &ReadRow) -> Result<(), KnowledgeError> {
    sqlx::query(
        "INSERT INTO knowledge_read_pages_fts (knowledge_read_pages_fts, rowid, title, body) VALUES ('delete', ?1, ?2, ?3)",
    )
    .bind(row.rowid)
    .bind(fts_title(row))
    .bind(&row.body)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

async fn fts_insert(conn: &mut Conn, row: &ReadRow) -> Result<(), KnowledgeError> {
    sqlx::query("INSERT INTO knowledge_read_pages_fts (rowid, title, body) VALUES (?1, ?2, ?3)")
        .bind(row.rowid)
        .bind(fts_title(row))
        .bind(&row.body)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// The bundle path of a read page: its concept path, or `pages/<page id>` for a team folder.
fn bundle_path(row: &ReadRow) -> String {
    row.concept_path
        .clone()
        .unwrap_or_else(|| format!("pages/{}", row.page_id))
}

/// SA-KREAD-03 / P (changed): a link of a read page resolves within its folder only — by concept
/// path for a Markdown link, then by title or alias.
async fn resolve_read_target(
    conn: &mut Conn,
    row: &ReadRow,
    form: LinkForm,
    target: &str,
) -> Result<Option<KnowledgePageId>, KnowledgeError> {
    if form == LinkForm::Markdown {
        let id: Option<String> = sqlx::query_scalar(
            "SELECT page_id FROM knowledge_read_pages WHERE workspace_id = ?1 AND folder_id = ?2 AND concept_path = ?3",
        )
        .bind(row.workspace_id.to_string())
        .bind(row.folder_id.to_string())
        .bind(target)
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(id) = id {
            return Ok(Some(parse_page_id(&id)?));
        }
    }
    let id: Option<String> = sqlx::query_scalar(
        "SELECT page_id FROM knowledge_read_page_names WHERE workspace_id = ?1 AND folder_id = ?2 AND name_key = ?3",
    )
    .bind(row.workspace_id.to_string())
    .bind(row.folder_id.to_string())
    .bind(name_key(target))
    .fetch_optional(&mut *conn)
    .await?;
    id.map(|i| parse_page_id(&i)).transpose()
}

/// The links of a read page, resolved now: (form, name, section, target).
async fn read_links(
    conn: &mut Conn,
    row: &ReadRow,
) -> Result<Vec<(String, Option<String>, Option<KnowledgePageId>)>, KnowledgeError> {
    let folder = folder_of(&bundle_path(row)).to_owned();
    let mut out = Vec::new();
    for l in scan_links(&row.body)
        .into_iter()
        .take(crate::service::MAX_LINKS_PER_PAGE)
    {
        let target = match l.form {
            LinkForm::Markdown => match resolve_bundle_path(&folder, &l.target) {
                Some(t) => t,
                None => continue,
            },
            _ => l.target.clone(),
        };
        let resolved = resolve_read_target(conn, row, l.form, &target).await?;
        out.push((target, l.section, resolved));
    }
    Ok(out)
}

async fn rebuild_read_links(conn: &mut Conn, row: &ReadRow) -> Result<(), KnowledgeError> {
    sqlx::query("DELETE FROM knowledge_read_links WHERE workspace_id = ?1 AND page_id = ?2")
        .bind(row.workspace_id.to_string())
        .bind(row.page_id.to_string())
        .execute(&mut *conn)
        .await?;
    for (i, (target, _, resolved)) in read_links(conn, row).await?.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO knowledge_read_links (workspace_id, page_id, ordinal, target_key, target_page_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(row.workspace_id.to_string())
        .bind(row.page_id.to_string())
        .bind(i64::try_from(i + 1).unwrap_or(i64::MAX))
        .bind(name_key(&target))
        .bind(resolved.map(|r| r.to_string()))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

async fn replace_read_names(conn: &mut Conn, row: &ReadRow) -> Result<(), KnowledgeError> {
    sqlx::query("DELETE FROM knowledge_read_page_names WHERE workspace_id = ?1 AND page_id = ?2")
        .bind(row.workspace_id.to_string())
        .bind(row.page_id.to_string())
        .execute(&mut *conn)
        .await?;
    let entries = std::iter::once((name_key(&row.title), "title"))
        .chain(row.aliases.iter().map(|a| (name_key(a), "alias")));
    for (key, kind) in entries {
        sqlx::query(
            "INSERT OR IGNORE INTO knowledge_read_page_names (workspace_id, folder_id, name_key, page_id, kind) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(row.workspace_id.to_string())
        .bind(row.folder_id.to_string())
        .bind(key)
        .bind(row.page_id.to_string())
        .bind(kind)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Re-resolves the links of the folder's pages naming one of `keys`; a page whose resolved link
/// changes is re-indexed (ADR-111 F).
async fn reresolve_folder(
    conn: &mut Conn,
    row: &ReadRow,
    keys: &[String],
) -> Result<(), KnowledgeError> {
    let pages: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT l.page_id FROM knowledge_read_links l JOIN knowledge_read_pages p \
         ON p.workspace_id = l.workspace_id AND p.page_id = l.page_id \
         WHERE l.workspace_id = ?1 AND p.folder_id = ?2 AND l.page_id <> ?3 \
         AND (l.target_key IN (SELECT value FROM json_each(?4)) OR l.target_page_id = ?3)",
    )
    .bind(row.workspace_id.to_string())
    .bind(row.folder_id.to_string())
    .bind(row.page_id.to_string())
    .bind(store::to_json(&keys))
    .fetch_all(&mut *conn)
    .await?;
    for p in pages {
        let id = parse_page_id(&p)?;
        if let Some(other) = read_row(conn, row.workspace_id, id).await? {
            rebuild_read_links(conn, &other).await?;
            sqlx::query("UPDATE knowledge_read_pages SET memory_indexed_version = NULL WHERE workspace_id = ?1 AND page_id = ?2")
                .bind(row.workspace_id.to_string())
                .bind(&p)
                .execute(&mut *conn)
                .await?;
        }
    }
    Ok(())
}

/// What a read row is built from.
struct ReadInput {
    page_id: KnowledgePageId,
    title: String,
    shared_by: String,
    shared_at: Timestamp,
    last_changed_at: Timestamp,
    written_by: KnowledgeAuthor,
    header: FileHeader,
    concept_path: Option<String>,
    bytes_sha256: String,
}

impl KnowledgeService {
    /// ADR-110 §R steps 3–7 (and AE step 4), from a validated header.
    async fn store_read_page(
        &self,
        workspace: WorkspaceId,
        folder: TeamFolderId,
        input: ReadInput,
    ) -> Result<TeamPageImported, KnowledgeError> {
        let _guard = self.write_lock.lock().await;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        if store::page(&mut tx, input.page_id).await?.is_some() {
            return Err(KnowledgeError::TeamPageElsewhere);
        }
        let existing = read_row(&mut tx, workspace, input.page_id).await?;
        if let Some(e) = &existing {
            if e.folder_id != folder {
                return Err(KnowledgeError::TeamPageElsewhere);
            }
            if e.bytes_sha256 == input.bytes_sha256 {
                return Ok(TeamPageImported::Unchanged);
            }
        } else {
            let n: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM knowledge_read_pages WHERE workspace_id = ?1 AND folder_id = ?2",
            )
            .bind(workspace.to_string())
            .bind(folder.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if n >= i64::from(MAX_READ_PAGES_PER_FOLDER) {
                return Err(KnowledgeError::PageLimit);
            }
        }
        // SA-KREAD-02: titles unique per folder by name key; a clash takes SA-KTEAM-02's rename.
        let mut title = input.title.clone();
        let mut n = 1u32;
        loop {
            let holder: Option<String> = sqlx::query_scalar(
                "SELECT page_id FROM knowledge_read_page_names WHERE workspace_id = ?1 AND folder_id = ?2 AND name_key = ?3",
            )
            .bind(workspace.to_string())
            .bind(folder.to_string())
            .bind(name_key(&title))
            .fetch_optional(&mut *tx)
            .await?;
            if holder.is_none() || holder.as_deref() == Some(input.page_id.to_string().as_str()) {
                break;
            }
            n += 1;
            title = team_title(&input.title, &input.shared_by, n);
        }
        let h = &input.header;
        let version = existing.as_ref().map_or(1, |e| e.version + 1);
        let now = Timestamp::now();
        let old_keys: Vec<String> = existing
            .as_ref()
            .map(|e| {
                std::iter::once(&e.title)
                    .chain(e.aliases.iter())
                    .map(|n| name_key(n))
                    .collect()
            })
            .unwrap_or_default();
        let renamed_without_rewrite = existing
            .as_ref()
            .is_some_and(|e| name_key(&e.title) != name_key(&title));
        if let Some(e) = &existing {
            fts_delete(&mut tx, e).await?;
        }
        sqlx::query(
            "INSERT INTO knowledge_read_pages (workspace_id, page_id, folder_id, title, body_markdown, written_by, sources, tags, \
             aliases, extra_header, shared_by, shared_at, last_changed_at, bytes_sha256, version, memory_indexed_version, \
             memory_index_attempts, memory_next_attempt_at, got_at, page_type, description, okf_tier, okf_status, status_text, \
             stale_after, verified, generated, concept_path) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL, 0, NULL, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25) \
             ON CONFLICT (workspace_id, page_id) DO UPDATE SET folder_id = excluded.folder_id, title = excluded.title, \
             body_markdown = excluded.body_markdown, written_by = excluded.written_by, sources = excluded.sources, \
             tags = excluded.tags, aliases = excluded.aliases, extra_header = excluded.extra_header, shared_by = excluded.shared_by, \
             shared_at = excluded.shared_at, last_changed_at = excluded.last_changed_at, bytes_sha256 = excluded.bytes_sha256, \
             version = excluded.version, memory_indexed_version = NULL, memory_index_attempts = 0, memory_next_attempt_at = NULL, \
             got_at = excluded.got_at, page_type = excluded.page_type, description = excluded.description, okf_tier = excluded.okf_tier, \
             okf_status = excluded.okf_status, status_text = excluded.status_text, stale_after = excluded.stale_after, \
             verified = excluded.verified, generated = excluded.generated, concept_path = excluded.concept_path",
        )
        .bind(workspace.to_string())
        .bind(input.page_id.to_string())
        .bind(folder.to_string())
        .bind(&title)
        .bind(&h.body)
        .bind(store::author_text(input.written_by))
        .bind(store::to_json(&h.sources))
        .bind(store::to_json(&h.tags))
        .bind(store::to_json(&h.aliases))
        .bind(&h.extra_header)
        .bind(&input.shared_by)
        .bind(input.shared_at.to_wire())
        .bind(input.last_changed_at.to_wire())
        .bind(&input.bytes_sha256)
        .bind(i64::from(version))
        .bind(now.to_wire())
        .bind(&h.page_type)
        .bind(&h.description)
        .bind(store::tier_text(h.tier))
        .bind(store::status_text(h.status))
        .bind(&h.status_text)
        .bind(&h.stale_after)
        .bind(store::to_json(&h.verified))
        .bind(h.generated.as_ref().map(store::to_json))
        .bind(&input.concept_path)
        .execute(&mut *tx)
        .await?;
        let row = read_row(&mut tx, workspace, input.page_id)
            .await?
            .ok_or(KnowledgeError::DbUnavailable)?;
        fts_insert(&mut tx, &row).await?;
        replace_read_names(&mut tx, &row).await?;
        rebuild_read_links(&mut tx, &row).await?;
        let mut keys = old_keys;
        keys.extend(
            std::iter::once(&row.title)
                .chain(row.aliases.iter())
                .map(|n| name_key(n)),
        );
        if renamed_without_rewrite {
            // ADR-111 F: a read page renamed in its folder re-indexes the pages linking to it.
            sqlx::query(
                "UPDATE knowledge_read_pages SET memory_indexed_version = NULL WHERE workspace_id = ?1 AND page_id IN \
                 (SELECT page_id FROM knowledge_read_links WHERE workspace_id = ?1 AND target_page_id = ?2)",
            )
            .bind(workspace.to_string())
            .bind(row.page_id.to_string())
            .execute(&mut *tx)
            .await?;
        }
        reresolve_folder(&mut tx, &row, &keys).await?;
        tx.commit().await?;
        let result = TeamPageImported::Written { version };
        tracing::info!(component = "knowledge", page_id = %row.page_id, folder_id = %folder, result = "written", "knowledge_read_page_got");
        self.journal(
            "read_page_got",
            Some(workspace),
            None,
            "A page of another team's folder was taken in.",
            json!({ "page_id": row.page_id, "folder_id": folder, "result": "written", "version": version }),
        )
        .await;
        self.wake_indexer();
        Ok(result)
    }

    /// ADR-110 §R: a page file from a read folder. Never KeptLocal.
    pub async fn import_read_page(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
        bytes: Vec<u8>,
    ) -> Result<TeamPageImported, KnowledgeError> {
        self.desktop_only()?;
        self.workspace(workspace_id).await?;
        let h = team_header(&bytes)?;
        let (Some(page_id), Some(shared_by), Some(shared_at)) =
            (h.page_id, h.shared_by.clone(), h.shared_at)
        else {
            return Err(KnowledgeError::TeamPageInvalid);
        };
        let input = ReadInput {
            page_id,
            title: h.title.clone(),
            shared_by,
            shared_at,
            last_changed_at: h.last_changed.unwrap_or_else(Timestamp::now),
            written_by: h.written_by.unwrap_or(KnowledgeAuthor::Person),
            bytes_sha256: sha256_hex(&bytes),
            header: h,
            concept_path: None,
        };
        let result = self.store_read_page(workspace_id, folder_id, input).await;
        if let Ok(TeamPageImported::Unchanged) = &result {
            self.journal(
                "read_page_got",
                Some(workspace_id),
                None,
                "A page of another team's folder was taken in.",
                json!({ "page_id": page_id, "folder_id": folder_id, "result": "unchanged", "version": null }),
            )
            .await;
        }
        result
    }

    /// ADR-112 §AE: a concept file of a read folder that is a plain OKF bundle.
    pub async fn import_read_concept(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
        path: String,
        bytes: Vec<u8>,
    ) -> Result<TeamPageImported, KnowledgeError> {
        self.desktop_only()?;
        self.workspace(workspace_id).await?;
        if okf::is_reserved(&path) {
            return Ok(TeamPageImported::Unchanged);
        }
        let invalid = |code: &'static str| {
            tracing::info!(component = "knowledge", folder_id = %folder_id, code, "knowledge_concept_invalid");
            KnowledgeError::ConceptInvalid
        };
        if bytes.len() > okf::CONCEPT_MAX_BYTES {
            return Err(invalid("KNOW_OKF_TOO_LARGE"));
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| invalid("KNOW_OKF_NOT_UTF8"))?;
        let h = match read_concept_file(&path, text) {
            Ok(h) => h,
            Err(okf::OkfError::Reserved) => return Ok(TeamPageImported::Unchanged),
            Err(e) => return Err(invalid(e.code())),
        };
        if h.extra_header.len() > EXTRA_HEADER_MAX || check_body(&h.body).is_err() {
            return Err(invalid("KNOW_OKF_TOO_LARGE"));
        }
        let page_id = concept_id(folder_id, &path);
        let label = self.folders.label_for(workspace_id, folder_id, &path).await;
        let shared_by: String = label
            .map(|l| l.name)
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| folder_id.to_string())
            .chars()
            .take(200)
            .collect();
        let written_by = if h
            .generated
            .as_ref()
            .is_some_and(|g| g.by.starts_with("ordin/"))
        {
            KnowledgeAuthor::Ordin
        } else {
            KnowledgeAuthor::Person
        };
        let got = Timestamp::now();
        let last_changed = h
            .generated
            .as_ref()
            .and_then(|g| g.at.as_deref())
            .and_then(okf::parse_stale_after)
            .unwrap_or(got);
        let input = ReadInput {
            page_id,
            title: h.title.chars().take(200).collect(),
            shared_by,
            shared_at: got,
            last_changed_at: last_changed,
            written_by,
            bytes_sha256: sha256_hex(&bytes),
            header: h,
            concept_path: Some(strip_md(&path).to_owned()),
        };
        self.store_read_page(workspace_id, folder_id, input).await
    }

    /// The id a concept of that folder has (UUIDv5, SA-OKF-10), for removal by path.
    pub fn concept_page_id(folder_id: TeamFolderId, path: &str) -> KnowledgePageId {
        concept_id(folder_id, path)
    }

    /// Deletes one read row with its FTS row, names and links; a tombstone holds the memory
    /// removal until it succeeds.
    async fn delete_read(
        &self,
        conn: &mut Conn,
        row: &ReadRow,
        tombstone: bool,
    ) -> Result<(), KnowledgeError> {
        fts_delete(conn, row).await?;
        for sql in [
            "DELETE FROM knowledge_read_page_names WHERE workspace_id = ?1 AND page_id = ?2",
            "DELETE FROM knowledge_read_links WHERE workspace_id = ?1 AND page_id = ?2",
            "DELETE FROM knowledge_read_pages WHERE workspace_id = ?1 AND page_id = ?2",
        ] {
            sqlx::query(sql)
                .bind(row.workspace_id.to_string())
                .bind(row.page_id.to_string())
                .execute(&mut *conn)
                .await?;
        }
        sqlx::query(
            "UPDATE knowledge_read_pages SET memory_indexed_version = NULL WHERE workspace_id = ?1 AND page_id IN \
             (SELECT page_id FROM knowledge_read_links WHERE workspace_id = ?1 AND target_page_id = ?2)",
        )
        .bind(row.workspace_id.to_string())
        .bind(row.page_id.to_string())
        .execute(&mut *conn)
        .await?;
        sqlx::query("UPDATE knowledge_read_links SET target_page_id = NULL WHERE workspace_id = ?1 AND target_page_id = ?2")
            .bind(row.workspace_id.to_string())
            .bind(row.page_id.to_string())
            .execute(&mut *conn)
            .await?;
        if tombstone {
            sqlx::query(
                "INSERT INTO knowledge_read_pages_removed (page_id, workspace_id, folder_id, memory_removed, removed_at) \
                 VALUES (?1, ?2, ?3, 0, ?4) ON CONFLICT (page_id) DO UPDATE SET memory_removed = 0",
            )
            .bind(row.page_id.to_string())
            .bind(row.workspace_id.to_string())
            .bind(row.folder_id.to_string())
            .bind(Timestamp::now().to_wire())
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    /// ADR-110 §S: the page's file is gone from the folder (idempotent).
    pub async fn remove_read_page(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
        page_id: KnowledgePageId,
    ) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let Some(row) = read_row(&mut tx, workspace_id, page_id)
            .await?
            .filter(|r| r.folder_id == folder_id)
        else {
            return Ok(());
        };
        self.delete_read(&mut tx, &row, false).await?;
        tx.commit().await?;
        // spec: ordin-desktop/components/KnowledgeService#KNOW-REC-1
        if let Err(e) = self
            .memory
            .remove_page(workspace_id, Some(folder_id), page_id)
            .await
        {
            tracing::warn!(component = "knowledge", page_id = %page_id, code = e.code(), "knowledge_memory_remove_failed");
            let mut conn = self.conn().await?;
            sqlx::query(
                "INSERT INTO knowledge_read_pages_removed (page_id, workspace_id, folder_id, memory_removed, removed_at) \
                 VALUES (?1, ?2, ?3, 0, ?4) ON CONFLICT (page_id) DO UPDATE SET memory_removed = 0",
            )
            .bind(page_id.to_string())
            .bind(workspace_id.to_string())
            .bind(folder_id.to_string())
            .bind(Timestamp::now().to_wire())
            .execute(&mut *conn)
            .await?;
        }
        tracing::info!(component = "knowledge", page_id = %page_id, folder_id = %folder_id, "knowledge_read_page_removed");
        self.journal(
            "read_page_removed",
            Some(workspace_id),
            None,
            "A page of another team's folder was removed.",
            json!({ "page_id": page_id, "folder_id": folder_id }),
        )
        .await;
        Ok(())
    }

    /// ADR-110 §T: **Stop reading**, or a folder the company stopped naming.
    pub async fn remove_read_folder_pages(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
    ) -> Result<u32, KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT page_id FROM knowledge_read_pages WHERE workspace_id = ?1 AND folder_id = ?2 ORDER BY page_id",
        )
        .bind(workspace_id.to_string())
        .bind(folder_id.to_string())
        .fetch_all(&mut *tx)
        .await?;
        let mut count = 0u32;
        for id in ids {
            if let Some(row) = read_row(&mut tx, workspace_id, parse_page_id(&id)?).await? {
                self.delete_read(&mut tx, &row, true).await?;
                count += 1;
            }
        }
        tx.commit().await?;
        tracing::info!(component = "knowledge", folder_id = %folder_id, pages = count, "knowledge_read_folder_pages_removed");
        self.journal(
            "read_folder_pages_removed",
            Some(workspace_id),
            None,
            "The pages of another team's folder were removed.",
            json!({ "folder_id": folder_id, "pages": count }),
        )
        .await;
        self.wake_indexer();
        Ok(count)
    }

    async fn read_summary(&self, row: &ReadRow) -> Option<PageSummary> {
        let item = match &row.concept_path {
            Some(p) => format!("{p}.md"),
            None => format!("pages/{}.md", row.page_id),
        };
        let label = self
            .folders
            .label_for(row.workspace_id, row.folder_id, &item)
            .await?;
        let now = Timestamp::now();
        Some(PageSummary {
            workspace_id: row.workspace_id,
            id: row.page_id,
            title: row.title.clone(),
            state: KnowledgePageState::Current,
            written_by: row.written_by,
            last_changed_at: row.last_changed_at,
            shared_by: Some(row.shared_by.clone()),
            shared_at: Some(row.shared_at),
            cites_other_computer: row.sources.iter().any(|s| s.origin == "place"),
            tags: row.tags.clone(),
            folder: Some(label.name),
            folder_description: label.description,
            set_by_company: Some(label.set_by_company),
            folder_id: Some(row.folder_id),
            read_only: true,
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
        })
    }

    pub(crate) async fn read_summary_of(&self, row: &ReadRow) -> Option<PageSummary> {
        self.read_summary(row).await
    }

    /// ADR-110 §U: the workspace's read pages, of one folder when given.
    pub async fn list_read_pages(
        &self,
        workspace_id: WorkspaceId,
        folder_id: Option<TeamFolderId>,
    ) -> Result<Vec<PageSummary>, KnowledgeError> {
        self.desktop_only()?;
        let pool = self.pool()?;
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT page_id FROM knowledge_read_pages WHERE workspace_id = ?1 AND (?2 IS NULL OR folder_id = ?2)",
        )
        .bind(workspace_id.to_string())
        .bind(folder_id.map(|f| f.to_string()))
        .fetch_all(pool)
        .await?;
        let mut conn = self.conn().await?;
        let mut out = Vec::new();
        for id in ids {
            if let Some(row) = read_row(&mut conn, workspace_id, parse_page_id(&id)?).await? {
                if let Some(s) = self.read_summary(&row).await {
                    out.push(s);
                }
            }
        }
        out.sort_by(|a, b| {
            a.folder
                .cmp(&b.folder)
                .then_with(|| name_key(&a.title).cmp(&name_key(&b.title)))
        });
        Ok(out)
    }

    /// `knowledge.get` on a read page id (ADR-110 §U).
    // Read rows are keyed by `(workspace_id, page_id)`; the most recently got row with that id is
    // shown (KnowledgeService.md, SA-KBUILT-04).
    pub(crate) async fn read_page_view(
        &self,
        page_id: KnowledgePageId,
    ) -> Result<PageView, KnowledgeError> {
        let pool = self.pool()?;
        let workspace: Option<String> = sqlx::query_scalar(
            "SELECT workspace_id FROM knowledge_read_pages WHERE page_id = ?1 ORDER BY got_at DESC LIMIT 1",
        )
        .bind(page_id.to_string())
        .fetch_optional(pool)
        .await?;
        let workspace = workspace
            .and_then(|w| WorkspaceId::parse(&w).ok())
            .ok_or(KnowledgeError::PageNotFound)?;
        let mut conn = self.conn().await?;
        let row = read_row(&mut conn, workspace, page_id)
            .await?
            .ok_or(KnowledgeError::PageNotFound)?;
        self.read_view(&mut conn, &row).await
    }

    pub(crate) async fn read_view(
        &self,
        conn: &mut Conn,
        row: &ReadRow,
    ) -> Result<PageView, KnowledgeError> {
        let links = read_links(conn, row).await?;
        let mut links_out = Vec::with_capacity(links.len());
        for (name, section, target) in links {
            let target = match target {
                Some(t) => read_row(conn, row.workspace_id, t).await?.map(|r| PageRef {
                    page_id: r.page_id,
                    title: r.title,
                }),
                None => None,
            };
            links_out.push(PageLinkView {
                target,
                name,
                section,
            });
        }
        let ins: Vec<(String, String)> = sqlx::query_as(
            "SELECT DISTINCT p.page_id, p.title FROM knowledge_read_links l JOIN knowledge_read_pages p \
             ON p.workspace_id = l.workspace_id AND p.page_id = l.page_id \
             WHERE l.workspace_id = ?1 AND l.target_page_id = ?2 AND l.page_id <> ?2 ORDER BY p.title",
        )
        .bind(row.workspace_id.to_string())
        .bind(row.page_id.to_string())
        .fetch_all(&mut *conn)
        .await?;
        let links_in = ins
            .into_iter()
            .map(|(id, title)| {
                Ok(PageRef {
                    page_id: parse_page_id(&id)?,
                    title,
                })
            })
            .collect::<Result<Vec<_>, KnowledgeError>>()?;
        let summary = self.read_summary(row).await;
        let now = Timestamp::now();
        Ok(PageView {
            page: KnowledgePage {
                id: row.page_id,
                workspace_id: row.workspace_id,
                title: row.title.clone(),
                body_markdown: row.body.clone(),
                written_by: row.written_by,
                edited_by_person: false,
                sources: row.sources.iter().map(StoredSource::plain).collect(),
                state: KnowledgePageState::Current,
                last_changed_at: row.last_changed_at,
            },
            changes: Vec::new(),
            pending_update: false,
            relative_path: format!("{}.md", bundle_path(row)),
            shared_by: Some(row.shared_by.clone()),
            shared_at: Some(row.shared_at),
            cites_other_computer: row.sources.iter().any(|s| s.origin == "place"),
            tags: row.tags.clone(),
            aliases: row.aliases.clone(),
            unregistered_aliases: Vec::new(),
            title_refused: None,
            links_out,
            links_in,
            links_truncated: 0,
            suggestions: Vec::new(),
            folder: summary.as_ref().and_then(|s| s.folder.clone()),
            folder_description: summary.as_ref().and_then(|s| s.folder_description.clone()),
            set_by_company: summary.as_ref().and_then(|s| s.set_by_company),
            folder_id: Some(row.folder_id),
            read_only: true,
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
            verified: row
                .verified
                .iter()
                .rev()
                .take(crate::service::MAX_VERIFIED)
                .map(|v| VerifiedView {
                    by: v.by.clone(),
                    at: v.at.clone(),
                })
                .collect(),
        })
    }

    /// V-1: the read-page candidates of `knowledge_search_pages`.
    pub(crate) async fn read_candidates(
        &self,
        workspace: WorkspaceId,
        f: &ToolFilter,
        limit: i64,
    ) -> Result<Vec<ReadRow>, KnowledgeError> {
        // SA-KREAD-04: read pages are always current.
        if f.state == Some(KnowledgePageState::MayBeOutOfDate) {
            return Ok(Vec::new());
        }
        let pool = self.pool()?;
        let by = f.written_by.map(store::author_text);
        let tag = f
            .tag
            .as_deref()
            .map(|t| name_key(t.trim_start_matches('#')));
        let ids: Vec<String> = match f.query.as_deref() {
            Some(q) => {
                let Some(m) = fts_any_word(q) else {
                    return Ok(Vec::new());
                };
                sqlx::query_scalar(
                    "SELECT p.page_id FROM knowledge_read_pages_fts JOIN knowledge_read_pages p ON p.rowid = knowledge_read_pages_fts.rowid \
                     WHERE knowledge_read_pages_fts MATCH ?1 AND p.workspace_id = ?2 AND (?3 IS NULL OR p.written_by = ?3) \
                     AND (?4 IS NULL OR EXISTS (SELECT 1 FROM json_each(p.tags) t WHERE lower(t.value) = ?4)) \
                     ORDER BY bm25(knowledge_read_pages_fts, 2.0, 1.0), p.page_id LIMIT ?5",
                )
                .bind(m)
                .bind(workspace.to_string())
                .bind(by)
                .bind(tag)
                .bind(limit)
                .fetch_all(pool)
                .await?
            }
            None => {
                sqlx::query_scalar(
                    "SELECT p.page_id FROM knowledge_read_pages p WHERE p.workspace_id = ?1 AND (?2 IS NULL OR p.written_by = ?2) \
                     AND (?3 IS NULL OR EXISTS (SELECT 1 FROM json_each(p.tags) t WHERE lower(t.value) = ?3)) \
                     ORDER BY p.last_changed_at DESC, p.page_id LIMIT ?4",
                )
                .bind(workspace.to_string())
                .bind(by)
                .bind(tag)
                .bind(limit)
                .fetch_all(pool)
                .await?
            }
        };
        let mut conn = self.conn().await?;
        let mut out = Vec::new();
        for id in ids {
            if let Some(r) = read_row(&mut conn, workspace, parse_page_id(&id)?).await? {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// ADR-110 §X: the person's **Adopt** (`again = Some(p)` for **Adopt again**).
    pub async fn adopt_page(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
        edit: Option<AdoptEdit>,
        again: Option<KnowledgePageId>,
    ) -> Result<KnowledgePageId, KnowledgeError> {
        self.desktop_only()?;
        let workspace = self.workspace(workspace_id).await?;
        let mut conn = self.conn().await?;
        let read = read_row(&mut conn, workspace_id, page_id)
            .await?
            .ok_or(KnowledgeError::PageNotFound)?;
        if let Some(p) = again {
            match store::page(&mut conn, p).await? {
                Some(r)
                    if r.workspace_id == workspace_id && r.state != KnowledgePageState::Removed => {
                }
                _ => return Err(KnowledgeError::PageNotFound),
            }
        }
        drop(conn);
        // X-2
        let edited = edit
            .as_ref()
            .is_some_and(|e| e.title.is_some() || e.body.is_some());
        let title = check_title(
            edit.as_ref()
                .and_then(|e| e.title.as_deref())
                .unwrap_or(&read.title),
        )?;
        let body = edit
            .as_ref()
            .and_then(|e| e.body.clone())
            .unwrap_or_else(|| read.body.clone());
        check_body(&body)?;
        let plain: Vec<_> = read.sources.iter().map(StoredSource::plain).collect();
        check_sources(&plain, true)?;
        // X-3: every section checked before anything is written; the first failure stops.
        let mut texts = vec![title.clone()];
        texts.extend(split_sections(&title, &body).into_iter().map(|(_, t)| t));
        for text in texts {
            // spec: ordin-desktop/components/KnowledgeService#KNOW-REC-1
            match self.memory.check_content(workspace_id, &text).await {
                Ok(c) if c.personal_data != PersonalDataFinding::None => {
                    tracing::warn!(
                        component = "knowledge",
                        code = "KNOW_ADOPT_PERSONAL_DATA",
                        "knowledge_adopt_refused"
                    );
                    return Err(KnowledgeError::AdoptPersonalData);
                }
                Ok(c) if c.instruction_like => {
                    tracing::warn!(
                        component = "knowledge",
                        code = "KNOW_ADOPT_REFUSED",
                        "knowledge_adopt_refused"
                    );
                    return Err(KnowledgeError::AdoptRefused { retryable: false });
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        component = "knowledge",
                        code = "KNOW_ADOPT_REFUSED",
                        "knowledge_adopt_refused"
                    );
                    return Err(KnowledgeError::AdoptRefused {
                        retryable: e.retryable(),
                    });
                }
            }
        }
        let own = match again {
            None => {
                let _guard = self.write_lock.lock().await;
                let mut conn = self.conn().await?;
                if store::count_live(&mut conn, workspace_id).await? >= MAX_PAGES_PER_WORKSPACE {
                    return Err(KnowledgeError::PageLimit);
                }
                if !self
                    .name_free(&mut conn, workspace_id, &title, None)
                    .await?
                {
                    return Err(KnowledgeError::TitleDuplicate);
                }
                for a in &read.aliases {
                    if name_key(a) != name_key(&title)
                        && !self.name_free(&mut conn, workspace_id, a, None).await?
                    {
                        return Err(KnowledgeError::TitleDuplicate);
                    }
                }
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
                let row = store::PageRow {
                    rowid: 0,
                    id: KnowledgePageId::new(),
                    workspace_id,
                    title,
                    body,
                    written_by: KnowledgeAuthor::Person,
                    edited_by_person: true,
                    sources: read.sources.clone(),
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
                    tags: read.tags.clone(),
                    aliases: read.aliases.clone(),
                    title_refused: None,
                    links_built_version: None,
                    links_truncated: 0,
                    page_type: read.page_type.clone(),
                    description: read.description.clone(),
                    okf_tier: PageTier::Unverified,
                    okf_status: PageStatus::Stable,
                    status_text: None,
                    stale_after: None,
                    verified: Vec::new(),
                    generated: None,
                };
                self.commit_page(Commit {
                    row,
                    old: None,
                    origin: FileVersionOrigin::Person,
                    expected_hash: None,
                    verify: true,
                    run_id: None,
                    type_set: false,
                })
                .await?
                .id
            }
            Some(p) => {
                let _guard = self.write_lock.lock().await;
                let mut conn = self.conn().await?;
                let old = store::page(&mut conn, p)
                    .await?
                    .ok_or(KnowledgeError::PageNotFound)?;
                if !self
                    .name_free(&mut conn, workspace_id, &title, Some(p))
                    .await?
                {
                    return Err(KnowledgeError::TitleDuplicate);
                }
                drop(conn);
                let renamed = name_key(&title) != name_key(&old.title);
                let mut row = old.clone();
                row.title = title;
                row.body = body;
                row.sources = read.sources.clone();
                row.written_by = KnowledgeAuthor::Person;
                row.edited_by_person = true;
                row.last_changed_at = Timestamp::now();
                self.write_existing(
                    row,
                    old,
                    FileVersionOrigin::Person,
                    true,
                    None,
                    false,
                    renamed,
                )
                .await?
                .id
            }
        };
        tracing::info!(component = "knowledge", page_id = %own, edited, again = again.is_some(), "knowledge_page_adopted");
        self.journal(
            "page_adopted",
            Some(workspace_id),
            None,
            "A page of another team's folder was adopted.",
            json!({ "page_id": own, "from_page_id": page_id, "edited": edited, "again": again.is_some() }),
        )
        .await;
        Ok(own)
    }
}

/// SA-KTEAM-02: "[title] (from [shared_by])", then "[title] (from [shared_by], 2)", "… 3".
pub(crate) fn team_title(title: &str, shared_by: &str, n: u32) -> String {
    let t = if n <= 2 {
        format!("{title} (from {shared_by})")
    } else {
        format!("{title} (from {shared_by}, {})", n - 1)
    };
    t.chars().take(200).collect()
}

/// M-1: a team page file's header and fields (`KNOW_TEAM_PAGE_INVALID`).
pub(crate) fn team_header(bytes: &[u8]) -> Result<FileHeader, KnowledgeError> {
    if bytes.len() > TEAM_PAGE_MAX {
        return Err(KnowledgeError::TeamPageInvalid);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| KnowledgeError::TeamPageInvalid)?;
    let h = read_concept_file("page.md", text).map_err(|_| KnowledgeError::TeamPageInvalid)?;
    if !h.ordin_page
        || h.extra_header.len() > EXTRA_HEADER_MAX
        || h.shared_by.is_none()
        || h.shared_at.is_none()
        || check_title(&h.title).is_err()
        || check_body(&h.body).is_err()
    {
        return Err(KnowledgeError::TeamPageInvalid);
    }
    let plain: Vec<_> = h.sources.iter().map(StoredSource::plain).collect();
    check_sources(&plain, true).map_err(|_| KnowledgeError::TeamPageInvalid)?;
    Ok(h)
}
