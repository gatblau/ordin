//! The SQLite statements of the workspace pages (*Data Model*, SQLite slice, steps 1 and 3–6).
//! Every statement is a constant string with bound values; lists travel as one bound JSON array
//! read with `json_each`, so no statement text is ever built from input.

use ordin_files::BinTicket;
use ordin_types::domain::places::{KnowledgeAuthor, KnowledgePageState, KnowledgeSource};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::wire::knowledge_query::{PageStatus, PageTier};
use ordin_types::{KnowledgePageId, PlaceId, WorkspaceId};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::error::KnowledgeError;
use crate::names::name_key;

pub(crate) type Conn = SqliteConnection;

/// One element of the `sources` column: 2C `KnowledgeSource` plus the members ADR-112 adds,
/// absent in rows written before step 6 (SA-OKF-05).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredSource {
    pub origin: String,
    pub location: String,
    pub version_read: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<String>,
}

impl StoredSource {
    pub(crate) fn plain(&self) -> KnowledgeSource {
        KnowledgeSource {
            origin: self.origin.clone(),
            location: self.location.clone(),
            version_read: self.version_read.clone(),
        }
    }

    pub(crate) fn from_plain(s: &KnowledgeSource) -> StoredSource {
        StoredSource {
            origin: s.origin.clone(),
            location: s.location.clone(),
            version_read: s.version_read.clone(),
            resource: None,
            id: None,
            last_modified: None,
            extra: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Verification {
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Generated {
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

/// One `knowledge_pages` row with every column of steps 1, 3, 4 and 6.
#[derive(Debug, Clone)]
pub(crate) struct PageRow {
    pub rowid: i64,
    pub id: KnowledgePageId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub body: String,
    pub written_by: KnowledgeAuthor,
    pub edited_by_person: bool,
    pub sources: Vec<StoredSource>,
    pub state: KnowledgePageState,
    pub last_changed_at: Timestamp,
    pub place_id: PlaceId,
    pub relative_path: String,
    pub file_version: u32,
    pub state_before_removal: Option<KnowledgePageState>,
    pub bin_ticket: Option<BinTicket>,
    pub memory_indexed_version: Option<u32>,
    pub memory_index_attempts: u32,
    pub memory_next_attempt_at: Option<Timestamp>,
    pub memory_removed: bool,
    pub created_at: Timestamp,
    pub shared_by: Option<String>,
    pub shared_at: Option<Timestamp>,
    pub first_exported_at: Option<Timestamp>,
    pub extra_header: String,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub title_refused: Option<String>,
    pub links_built_version: Option<u32>,
    pub links_truncated: u32,
    pub page_type: Option<String>,
    pub description: Option<String>,
    pub okf_tier: PageTier,
    pub okf_status: PageStatus,
    pub status_text: Option<String>,
    pub stale_after: Option<String>,
    pub verified: Vec<Verification>,
    pub generated: Option<Generated>,
}

pub(crate) fn corrupt() -> KnowledgeError {
    tracing::error!(component = "knowledge", "knowledge_row_unreadable");
    KnowledgeError::DbUnavailable
}

pub(crate) fn ts(raw: &str) -> Result<Timestamp, KnowledgeError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| corrupt())
}

pub(crate) fn opt_ts(raw: Option<String>) -> Result<Option<Timestamp>, KnowledgeError> {
    raw.map(|r| ts(&r)).transpose()
}

pub(crate) fn author_text(a: KnowledgeAuthor) -> &'static str {
    match a {
        KnowledgeAuthor::Ordin => "ordin",
        KnowledgeAuthor::Person => "person",
    }
}

pub(crate) fn author_of(raw: &str) -> Result<KnowledgeAuthor, KnowledgeError> {
    match raw {
        "ordin" => Ok(KnowledgeAuthor::Ordin),
        "person" => Ok(KnowledgeAuthor::Person),
        _ => Err(corrupt()),
    }
}

pub(crate) fn state_text(s: KnowledgePageState) -> &'static str {
    match s {
        KnowledgePageState::Current => "current",
        KnowledgePageState::MayBeOutOfDate => "may_be_out_of_date",
        KnowledgePageState::Removed => "removed",
    }
}

pub(crate) fn state_of(raw: &str) -> Result<KnowledgePageState, KnowledgeError> {
    match raw {
        "current" => Ok(KnowledgePageState::Current),
        "may_be_out_of_date" => Ok(KnowledgePageState::MayBeOutOfDate),
        "removed" => Ok(KnowledgePageState::Removed),
        _ => Err(corrupt()),
    }
}

pub(crate) fn tier_text(t: PageTier) -> &'static str {
    match t {
        PageTier::HumanReviewed => "human_reviewed",
        PageTier::MachineConfirmed => "machine_confirmed",
        PageTier::Unverified => "unverified",
    }
}

pub(crate) fn tier_of(raw: &str) -> PageTier {
    match raw {
        "human_reviewed" => PageTier::HumanReviewed,
        "machine_confirmed" => PageTier::MachineConfirmed,
        _ => PageTier::Unverified,
    }
}

pub(crate) fn status_text(s: PageStatus) -> &'static str {
    match s {
        PageStatus::Stable => "stable",
        PageStatus::Draft => "draft",
        PageStatus::Deprecated => "deprecated",
    }
}

pub(crate) fn status_of(raw: &str) -> PageStatus {
    match raw {
        "draft" => PageStatus::Draft,
        "deprecated" => PageStatus::Deprecated,
        _ => PageStatus::Stable,
    }
}

fn json<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, KnowledgeError> {
    serde_json::from_str(raw).map_err(|_| corrupt())
}

pub(crate) fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".to_owned())
}

fn u32_of(v: i64) -> u32 {
    u32::try_from(v).unwrap_or(0)
}

pub(crate) fn page_from(row: &SqliteRow) -> Result<PageRow, KnowledgeError> {
    let id: String = row.try_get("id")?;
    let workspace: String = row.try_get("workspace_id")?;
    let place: String = row.try_get("place_id")?;
    let before: Option<String> = row.try_get("state_before_removal")?;
    let ticket: Option<String> = row.try_get("bin_ticket")?;
    let generated: Option<String> = row.try_get("generated")?;
    Ok(PageRow {
        rowid: row.try_get("rowid")?,
        id: KnowledgePageId::parse(&id).map_err(|_| corrupt())?,
        workspace_id: WorkspaceId::parse(&workspace).map_err(|_| corrupt())?,
        title: row.try_get("title")?,
        body: row.try_get("body_markdown")?,
        written_by: author_of(&row.try_get::<String, _>("written_by")?)?,
        edited_by_person: row.try_get::<i64, _>("edited_by_person")? != 0,
        sources: json(&row.try_get::<String, _>("sources")?)?,
        state: state_of(&row.try_get::<String, _>("state")?)?,
        last_changed_at: ts(&row.try_get::<String, _>("last_changed_at")?)?,
        place_id: PlaceId::parse(&place).map_err(|_| corrupt())?,
        relative_path: row.try_get("relative_path")?,
        file_version: u32_of(row.try_get("file_version")?),
        state_before_removal: before.map(|b| state_of(&b)).transpose()?,
        bin_ticket: ticket.map(|t| json(&t)).transpose()?,
        memory_indexed_version: row
            .try_get::<Option<i64>, _>("memory_indexed_version")?
            .map(u32_of),
        memory_index_attempts: u32_of(row.try_get("memory_index_attempts")?),
        memory_next_attempt_at: opt_ts(row.try_get("memory_next_attempt_at")?)?,
        memory_removed: row.try_get::<i64, _>("memory_removed")? != 0,
        created_at: ts(&row.try_get::<String, _>("created_at")?)?,
        shared_by: row.try_get("shared_by")?,
        shared_at: opt_ts(row.try_get("shared_at")?)?,
        first_exported_at: opt_ts(row.try_get("first_exported_at")?)?,
        extra_header: row.try_get("extra_header")?,
        tags: json(&row.try_get::<String, _>("tags")?)?,
        aliases: json(&row.try_get::<String, _>("aliases")?)?,
        title_refused: row.try_get("title_refused")?,
        links_built_version: row
            .try_get::<Option<i64>, _>("links_built_version")?
            .map(u32_of),
        links_truncated: u32_of(row.try_get("links_truncated")?),
        page_type: row.try_get("page_type")?,
        description: row.try_get("description")?,
        okf_tier: tier_of(&row.try_get::<String, _>("okf_tier")?),
        okf_status: status_of(&row.try_get::<String, _>("okf_status")?),
        status_text: row.try_get("status_text")?,
        stale_after: row.try_get("stale_after")?,
        verified: json(&row.try_get::<String, _>("verified")?)?,
        generated: generated.map(|g| json(&g)).transpose()?,
    })
}

const SELECT_PAGE: &str = "SELECT rowid, id, workspace_id, title, body_markdown, written_by, edited_by_person, \
    sources, state, last_changed_at, place_id, relative_path, file_version, state_before_removal, bin_ticket, \
    memory_indexed_version, memory_index_attempts, memory_next_attempt_at, memory_removed, created_at, \
    shared_by, shared_at, first_exported_at, extra_header, tags, aliases, title_refused, links_built_version, \
    links_truncated, page_type, description, okf_tier, okf_status, status_text, stale_after, verified, generated \
    FROM knowledge_pages WHERE id = ?1";

const SELECT_WORKSPACE_PAGES: &str = "SELECT rowid, id, workspace_id, title, body_markdown, written_by, edited_by_person, \
    sources, state, last_changed_at, place_id, relative_path, file_version, state_before_removal, bin_ticket, \
    memory_indexed_version, memory_index_attempts, memory_next_attempt_at, memory_removed, created_at, \
    shared_by, shared_at, first_exported_at, extra_header, tags, aliases, title_refused, links_built_version, \
    links_truncated, page_type, description, okf_tier, okf_status, status_text, stale_after, verified, generated \
    FROM knowledge_pages WHERE workspace_id = ?1 AND state <> 'removed' ORDER BY id";

const SELECT_PAGE_BY_FILE: &str = "SELECT rowid, id, workspace_id, title, body_markdown, written_by, edited_by_person, \
    sources, state, last_changed_at, place_id, relative_path, file_version, state_before_removal, bin_ticket, \
    memory_indexed_version, memory_index_attempts, memory_next_attempt_at, memory_removed, created_at, \
    shared_by, shared_at, first_exported_at, extra_header, tags, aliases, title_refused, links_built_version, \
    links_truncated, page_type, description, okf_tier, okf_status, status_text, stale_after, verified, generated \
    FROM knowledge_pages WHERE place_id = ?1 AND relative_path = ?2";

const SELECT_ALL_LIVE: &str = "SELECT rowid, id, workspace_id, title, body_markdown, written_by, edited_by_person, \
    sources, state, last_changed_at, place_id, relative_path, file_version, state_before_removal, bin_ticket, \
    memory_indexed_version, memory_index_attempts, memory_next_attempt_at, memory_removed, created_at, \
    shared_by, shared_at, first_exported_at, extra_header, tags, aliases, title_refused, links_built_version, \
    links_truncated, page_type, description, okf_tier, okf_status, status_text, stale_after, verified, generated \
    FROM knowledge_pages WHERE state <> 'removed' ORDER BY workspace_id, id";

pub(crate) async fn page(
    conn: &mut Conn,
    id: KnowledgePageId,
) -> Result<Option<PageRow>, KnowledgeError> {
    let row = sqlx::query(SELECT_PAGE)
        .bind(id.to_string())
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref().map(page_from).transpose()
}

pub(crate) async fn workspace_pages(
    conn: &mut Conn,
    workspace: WorkspaceId,
) -> Result<Vec<PageRow>, KnowledgeError> {
    let rows = sqlx::query(SELECT_WORKSPACE_PAGES)
        .bind(workspace.to_string())
        .fetch_all(&mut *conn)
        .await?;
    rows.iter().map(page_from).collect()
}

pub(crate) async fn page_by_file(
    conn: &mut Conn,
    place: PlaceId,
    path: &str,
) -> Result<Option<PageRow>, KnowledgeError> {
    let row = sqlx::query(SELECT_PAGE_BY_FILE)
        .bind(place.to_string())
        .bind(path)
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref().map(page_from).transpose()
}

pub(crate) async fn all_live_pages(conn: &mut Conn) -> Result<Vec<PageRow>, KnowledgeError> {
    let rows = sqlx::query(SELECT_ALL_LIVE).fetch_all(&mut *conn).await?;
    rows.iter().map(page_from).collect()
}

pub(crate) async fn count_live(
    conn: &mut Conn,
    workspace: WorkspaceId,
) -> Result<u32, KnowledgeError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM knowledge_pages WHERE workspace_id = ?1 AND state <> 'removed'",
    )
    .bind(workspace.to_string())
    .fetch_one(&mut *conn)
    .await?;
    Ok(u32_of(n))
}

/// The FTS title column: the title and every alias on lines of their own (B-6, S-4).
pub(crate) fn fts_title(title: &str, aliases: &[String]) -> String {
    let mut t = title.to_owned();
    for a in aliases {
        t.push('\n');
        t.push_str(a);
    }
    t
}

pub(crate) async fn fts_delete(conn: &mut Conn, row: &PageRow) -> Result<(), KnowledgeError> {
    sqlx::query(
        "INSERT INTO knowledge_pages_fts (knowledge_pages_fts, rowid, title, body) VALUES ('delete', ?1, ?2, ?3)",
    )
    .bind(row.rowid)
    .bind(fts_title(&row.title, &row.aliases))
    .bind(&row.body)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub(crate) async fn fts_insert(conn: &mut Conn, row: &PageRow) -> Result<(), KnowledgeError> {
    sqlx::query("INSERT INTO knowledge_pages_fts (rowid, title, body) VALUES (?1, ?2, ?3)")
        .bind(row.rowid)
        .bind(fts_title(&row.title, &row.aliases))
        .bind(&row.body)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Inserts a new row and returns its rowid.
pub(crate) async fn insert_page(conn: &mut Conn, r: &PageRow) -> Result<i64, KnowledgeError> {
    let rowid: i64 = sqlx::query_scalar(
        "INSERT INTO knowledge_pages (id, workspace_id, title, body_markdown, written_by, edited_by_person, \
         sources, state, last_changed_at, place_id, relative_path, file_version, state_before_removal, bin_ticket, \
         memory_indexed_version, memory_index_attempts, memory_next_attempt_at, memory_removed, created_at, \
         shared_by, shared_at, first_exported_at, extra_header, tags, aliases, title_refused, links_built_version, \
         links_truncated, page_type, description, okf_tier, okf_status, status_text, stale_after, verified, generated) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, \
         ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36) RETURNING rowid",
    )
    .bind(r.id.to_string())
    .bind(r.workspace_id.to_string())
    .bind(&r.title)
    .bind(&r.body)
    .bind(author_text(r.written_by))
    .bind(i64::from(r.edited_by_person))
    .bind(to_json(&r.sources))
    .bind(state_text(r.state))
    .bind(r.last_changed_at.to_wire())
    .bind(r.place_id.to_string())
    .bind(&r.relative_path)
    .bind(i64::from(r.file_version))
    .bind(r.state_before_removal.map(state_text))
    .bind(r.bin_ticket.as_ref().map(to_json))
    .bind(r.memory_indexed_version.map(i64::from))
    .bind(i64::from(r.memory_index_attempts))
    .bind(r.memory_next_attempt_at.map(|t| t.to_wire()))
    .bind(i64::from(r.memory_removed))
    .bind(r.created_at.to_wire())
    .bind(&r.shared_by)
    .bind(r.shared_at.map(|t| t.to_wire()))
    .bind(r.first_exported_at.map(|t| t.to_wire()))
    .bind(&r.extra_header)
    .bind(to_json(&r.tags))
    .bind(to_json(&r.aliases))
    .bind(&r.title_refused)
    .bind(r.links_built_version.map(i64::from))
    .bind(i64::from(r.links_truncated))
    .bind(&r.page_type)
    .bind(&r.description)
    .bind(tier_text(r.okf_tier))
    .bind(status_text(r.okf_status))
    .bind(&r.status_text)
    .bind(&r.stale_after)
    .bind(to_json(&r.verified))
    .bind(r.generated.as_ref().map(to_json))
    .fetch_one(&mut *conn)
    .await?;
    Ok(rowid)
}

/// Writes every column of an existing row.
pub(crate) async fn update_page(conn: &mut Conn, r: &PageRow) -> Result<(), KnowledgeError> {
    sqlx::query(
        "UPDATE knowledge_pages SET workspace_id = ?2, title = ?3, body_markdown = ?4, written_by = ?5, \
         edited_by_person = ?6, sources = ?7, state = ?8, last_changed_at = ?9, place_id = ?10, relative_path = ?11, \
         file_version = ?12, state_before_removal = ?13, bin_ticket = ?14, memory_indexed_version = ?15, \
         memory_index_attempts = ?16, memory_next_attempt_at = ?17, memory_removed = ?18, shared_by = ?19, \
         shared_at = ?20, first_exported_at = ?21, extra_header = ?22, tags = ?23, aliases = ?24, \
         title_refused = ?25, links_built_version = ?26, links_truncated = ?27, page_type = ?28, description = ?29, \
         okf_tier = ?30, okf_status = ?31, status_text = ?32, stale_after = ?33, verified = ?34, generated = ?35 \
         WHERE id = ?1",
    )
    .bind(r.id.to_string())
    .bind(r.workspace_id.to_string())
    .bind(&r.title)
    .bind(&r.body)
    .bind(author_text(r.written_by))
    .bind(i64::from(r.edited_by_person))
    .bind(to_json(&r.sources))
    .bind(state_text(r.state))
    .bind(r.last_changed_at.to_wire())
    .bind(r.place_id.to_string())
    .bind(&r.relative_path)
    .bind(i64::from(r.file_version))
    .bind(r.state_before_removal.map(state_text))
    .bind(r.bin_ticket.as_ref().map(to_json))
    .bind(r.memory_indexed_version.map(i64::from))
    .bind(i64::from(r.memory_index_attempts))
    .bind(r.memory_next_attempt_at.map(|t| t.to_wire()))
    .bind(i64::from(r.memory_removed))
    .bind(&r.shared_by)
    .bind(r.shared_at.map(|t| t.to_wire()))
    .bind(r.first_exported_at.map(|t| t.to_wire()))
    .bind(&r.extra_header)
    .bind(to_json(&r.tags))
    .bind(to_json(&r.aliases))
    .bind(&r.title_refused)
    .bind(r.links_built_version.map(i64::from))
    .bind(i64::from(r.links_truncated))
    .bind(&r.page_type)
    .bind(&r.description)
    .bind(tier_text(r.okf_tier))
    .bind(status_text(r.okf_status))
    .bind(&r.status_text)
    .bind(&r.stale_after)
    .bind(to_json(&r.verified))
    .bind(r.generated.as_ref().map(to_json))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub(crate) async fn delete_page_row(
    conn: &mut Conn,
    id: KnowledgePageId,
) -> Result<(), KnowledgeError> {
    sqlx::query("DELETE FROM knowledge_pages WHERE id = ?1")
        .bind(id.to_string())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- names and tags (ADR-104)

/// The page holding a name key in the workspace, if any.
pub(crate) async fn name_holder(
    conn: &mut Conn,
    workspace: WorkspaceId,
    key: &str,
) -> Result<Option<KnowledgePageId>, KnowledgeError> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT n.page_id FROM knowledge_page_names n JOIN knowledge_pages p ON p.id = n.page_id \
         WHERE n.workspace_id = ?1 AND n.name_key = ?2 AND p.state <> 'removed'",
    )
    .bind(workspace.to_string())
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?;
    id.map(|i| KnowledgePageId::parse(&i).map_err(|_| corrupt()))
        .transpose()
}

/// Replaces the page's names: its title, then each alias whose key is free. Returns the aliases
/// left unregistered because another page holds them.
pub(crate) async fn replace_names(
    conn: &mut Conn,
    row: &PageRow,
) -> Result<Vec<String>, KnowledgeError> {
    sqlx::query("DELETE FROM knowledge_page_names WHERE page_id = ?1")
        .bind(row.id.to_string())
        .execute(&mut *conn)
        .await?;
    if row.state == KnowledgePageState::Removed {
        return Ok(Vec::new());
    }
    let mut unregistered = Vec::new();
    let mut entries: Vec<(String, &str, String)> =
        vec![(name_key(&row.title), "title", row.title.clone())];
    entries.extend(
        row.aliases
            .iter()
            .map(|a| (name_key(a), "alias", a.clone())),
    );
    for (key, kind, text) in entries {
        let done = sqlx::query(
            "INSERT OR IGNORE INTO knowledge_page_names (workspace_id, name_key, page_id, kind) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(row.workspace_id.to_string())
        .bind(&key)
        .bind(row.id.to_string())
        .bind(kind)
        .execute(&mut *conn)
        .await?;
        if done.rows_affected() == 0 {
            let holder = name_holder(conn, row.workspace_id, &key).await?;
            if holder != Some(row.id) {
                unregistered.push(text);
            }
        }
    }
    Ok(unregistered)
}

pub(crate) async fn registered_keys(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<Vec<String>, KnowledgeError> {
    Ok(
        sqlx::query_scalar("SELECT name_key FROM knowledge_page_names WHERE page_id = ?1")
            .bind(page.to_string())
            .fetch_all(&mut *conn)
            .await?,
    )
}

pub(crate) async fn replace_tags(conn: &mut Conn, row: &PageRow) -> Result<(), KnowledgeError> {
    sqlx::query("DELETE FROM knowledge_page_tags WHERE page_id = ?1")
        .bind(row.id.to_string())
        .execute(&mut *conn)
        .await?;
    if row.state == KnowledgePageState::Removed {
        return Ok(());
    }
    for t in &row.tags {
        sqlx::query("INSERT OR IGNORE INTO knowledge_page_tags (page_id, tag_key) VALUES (?1, ?2)")
            .bind(row.id.to_string())
            .bind(name_key(t))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------- marks and updates

#[derive(Debug, Clone)]
pub(crate) struct MarkRow {
    pub origin: String,
    pub location: String,
    pub newer_version: String,
    pub changed_by: Option<String>,
    pub changed_at: Option<Timestamp>,
    pub left_at: Option<Timestamp>,
}

pub(crate) async fn marks(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<Vec<MarkRow>, KnowledgeError> {
    let rows = sqlx::query(
        "SELECT origin, location, newer_version, changed_by, changed_at, marked_at, left_at \
         FROM knowledge_source_marks WHERE page_id = ?1 ORDER BY origin, location",
    )
    .bind(page.to_string())
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(MarkRow {
                origin: r.try_get("origin")?,
                location: r.try_get("location")?,
                newer_version: r.try_get("newer_version")?,
                changed_by: r.try_get("changed_by")?,
                changed_at: opt_ts(r.try_get("changed_at")?)?,
                left_at: opt_ts(r.try_get("left_at")?)?,
            })
        })
        .collect()
}

/// Upserts a mark; `marked_at` is set when the mark is inserted or its `newer_version` changes,
/// and a mark left at this same version stays left. Returns whether the mark is now open.
pub(crate) async fn upsert_mark(
    conn: &mut Conn,
    page: KnowledgePageId,
    origin: &str,
    location: &str,
    newer_version: &str,
    changed_by: Option<&str>,
    changed_at: Option<Timestamp>,
) -> Result<bool, KnowledgeError> {
    let now = Timestamp::now().to_wire();
    sqlx::query(
        "INSERT INTO knowledge_source_marks (page_id, origin, location, newer_version, changed_by, changed_at, marked_at, left_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL) \
         ON CONFLICT (page_id, origin, location) DO UPDATE SET \
           changed_by = CASE WHEN newer_version = excluded.newer_version THEN changed_by ELSE excluded.changed_by END, \
           changed_at = CASE WHEN newer_version = excluded.newer_version THEN changed_at ELSE excluded.changed_at END, \
           marked_at = CASE WHEN newer_version = excluded.newer_version THEN marked_at ELSE excluded.marked_at END, \
           left_at = CASE WHEN newer_version = excluded.newer_version THEN left_at ELSE NULL END, \
           newer_version = excluded.newer_version",
    )
    .bind(page.to_string())
    .bind(origin)
    .bind(location)
    .bind(newer_version)
    .bind(changed_by)
    .bind(changed_at.map(|t| t.to_wire()))
    .bind(now)
    .execute(&mut *conn)
    .await?;
    let left: Option<Option<String>> = sqlx::query_scalar(
        "SELECT left_at FROM knowledge_source_marks WHERE page_id = ?1 AND origin = ?2 AND location = ?3",
    )
    .bind(page.to_string())
    .bind(origin)
    .bind(location)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(matches!(left, Some(None)))
}

pub(crate) async fn delete_mark(
    conn: &mut Conn,
    page: KnowledgePageId,
    origin: &str,
    location: &str,
) -> Result<(), KnowledgeError> {
    sqlx::query(
        "DELETE FROM knowledge_source_marks WHERE page_id = ?1 AND origin = ?2 AND location = ?3",
    )
    .bind(page.to_string())
    .bind(origin)
    .bind(location)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub(crate) async fn open_marks(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<u32, KnowledgeError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM knowledge_source_marks WHERE page_id = ?1 AND left_at IS NULL",
    )
    .bind(page.to_string())
    .fetch_one(&mut *conn)
    .await?;
    Ok(u32_of(n))
}

pub(crate) async fn clear_marks_and_update(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<(), KnowledgeError> {
    sqlx::query("DELETE FROM knowledge_source_marks WHERE page_id = ?1")
        .bind(page.to_string())
        .execute(&mut *conn)
        .await?;
    delete_update(conn, page).await
}

pub(crate) async fn delete_update(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<(), KnowledgeError> {
    sqlx::query("DELETE FROM knowledge_page_updates WHERE page_id = ?1")
        .bind(page.to_string())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct UpdateRow {
    pub body: String,
    pub sources: Vec<StoredSource>,
    pub run_id: ordin_types::RunId,
}

pub(crate) async fn pending_update(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<Option<UpdateRow>, KnowledgeError> {
    let row = sqlx::query(
        "SELECT body_markdown, sources, run_id FROM knowledge_page_updates WHERE page_id = ?1",
    )
    .bind(page.to_string())
    .fetch_optional(&mut *conn)
    .await?;
    row.map(|r| {
        let run: String = r.try_get("run_id")?;
        Ok(UpdateRow {
            body: r.try_get("body_markdown")?,
            sources: json(&r.try_get::<String, _>("sources")?)?,
            run_id: ordin_types::RunId::parse(&run).map_err(|_| corrupt())?,
        })
    })
    .transpose()
}

pub(crate) async fn store_update(
    conn: &mut Conn,
    page: KnowledgePageId,
    body: &str,
    sources: &[StoredSource],
    run_id: ordin_types::RunId,
) -> Result<(), KnowledgeError> {
    sqlx::query(
        "INSERT INTO knowledge_page_updates (page_id, body_markdown, sources, run_id, proposed_at) VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT (page_id) DO UPDATE SET body_markdown = excluded.body_markdown, sources = excluded.sources, \
         run_id = excluded.run_id, proposed_at = excluded.proposed_at",
    )
    .bind(page.to_string())
    .bind(body)
    .bind(to_json(&sources))
    .bind(run_id.to_string())
    .bind(Timestamp::now().to_wire())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub(crate) async fn set_state(
    conn: &mut Conn,
    page: KnowledgePageId,
    state: KnowledgePageState,
) -> Result<(), KnowledgeError> {
    sqlx::query("UPDATE knowledge_pages SET state = ?2 WHERE id = ?1 AND state <> 'removed'")
        .bind(page.to_string())
        .bind(state_text(state))
        .execute(&mut *conn)
        .await?;
    Ok(())
}
