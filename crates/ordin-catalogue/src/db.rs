//! Row types and the queries every other module shares. The statements are this crate's own
//! (2B-1 rule 5); every value is bound, never spliced into the text.

use std::str::FromStr;

use ordin_agent_model::{PartStatus, BUNDLED_REPOSITORY_ID};
use ordin_types::domain::agents::{
    AgentFormat, AgentRepository, AgentRepositoryState, Part, PartKind, RepositorySource,
};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::{AgentRepositoryId, PartId, VocabularyTermId};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::error::CatalogueError;

/// The text a serde enum is stored as (`snake_case`, or UpperCamelCase for `PartKind`).
pub(crate) fn enum_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

pub(crate) fn enum_from<T: DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
}

fn corrupt(what: &str) -> CatalogueError {
    CatalogueError::store(format!("a stored {what} value is not readable"))
}

pub(crate) fn parse_timestamp(text: &str) -> Result<Timestamp, CatalogueError> {
    ordin_types::time::parse_rfc3339_ms_utc(text)
        .map(Timestamp)
        .map_err(|_| corrupt("timestamp"))
}

/// One `agent_repositories` row.
#[derive(Debug, Clone)]
pub(crate) struct RepoRow {
    pub id: AgentRepositoryId,
    pub display_name: String,
    pub host: String,
    pub repository: String,
    pub protected_branch: String,
    pub format: AgentFormat,
    pub format_version: String,
    pub source: RepositorySource,
    pub signed_merges_required: Option<bool>,
    pub person_can_publish: bool,
    pub person_can_curate: bool,
    pub last_fetched_commit: Option<String>,
    pub profile_key: Option<String>,
    pub clone_path: String,
    pub state: AgentRepositoryState,
    pub added_at: Timestamp,
}

impl RepoRow {
    pub(crate) fn is_bundled(&self) -> bool {
        self.id == BUNDLED_REPOSITORY_ID
    }

    /// `<host>/<owner>/<name>` or `bundled`.
    pub(crate) fn label(&self) -> String {
        if self.is_bundled() {
            "bundled".to_owned()
        } else {
            format!("{}/{}", self.host, self.repository)
        }
    }

    /// The name shown as "From:".
    pub(crate) fn shown_as(&self) -> String {
        if self.is_bundled() {
            "ordin".to_owned()
        } else {
            self.display_name.clone()
        }
    }

    pub(crate) fn public(&self) -> AgentRepository {
        AgentRepository {
            id: self.id,
            display_name: self.display_name.clone(),
            host: self.host.clone(),
            repository: self.repository.clone(),
            protected_branch: self.protected_branch.clone(),
            format: self.format,
            format_version: self.format_version.clone(),
            source: self.source,
            signed_merges_required: self.signed_merges_required,
            person_can_publish: self.person_can_publish,
            person_can_curate: self.person_can_curate,
            last_fetched_commit: self.last_fetched_commit.clone(),
            clone_path: self.clone_path.clone(),
            state: self.state,
            added_at: self.added_at,
        }
    }

    pub(crate) fn is_read(&self) -> bool {
        matches!(
            self.state,
            AgentRepositoryState::Named | AgentRepositoryState::Cloned
        )
    }
}

const REPO_COLUMNS: &str = "id, display_name, host, repository, protected_branch, format, \
    format_version, source, signed_merges_required, person_can_publish, person_can_curate, \
    last_fetched_commit, profile_key, clone_path, state, added_at";

fn repo_from_row(row: &SqliteRow) -> Result<RepoRow, CatalogueError> {
    let id: String = row.try_get("id")?;
    let format: String = row.try_get("format")?;
    let source: String = row.try_get("source")?;
    let state: String = row.try_get("state")?;
    let added_at: String = row.try_get("added_at")?;
    let signed: Option<i64> = row.try_get("signed_merges_required")?;
    Ok(RepoRow {
        id: AgentRepositoryId(uuid::Uuid::from_str(&id).map_err(|_| corrupt("repository id"))?),
        display_name: row.try_get("display_name")?,
        host: row.try_get("host")?,
        repository: row.try_get("repository")?,
        protected_branch: row.try_get("protected_branch")?,
        format: enum_from(&format).ok_or_else(|| corrupt("format"))?,
        format_version: row.try_get("format_version")?,
        source: enum_from(&source).ok_or_else(|| corrupt("source"))?,
        signed_merges_required: signed.map(|v| v != 0),
        person_can_publish: row.try_get::<i64, _>("person_can_publish")? != 0,
        person_can_curate: row.try_get::<i64, _>("person_can_curate")? != 0,
        last_fetched_commit: row.try_get("last_fetched_commit")?,
        profile_key: row.try_get("profile_key")?,
        clone_path: row.try_get("clone_path")?,
        state: enum_from(&state).ok_or_else(|| corrupt("state"))?,
        added_at: parse_timestamp(&added_at)?,
    })
}

pub(crate) async fn repo_by_id(
    pool: &SqlitePool,
    id: AgentRepositoryId,
) -> Result<Option<RepoRow>, CatalogueError> {
    let sql = format!("SELECT {REPO_COLUMNS} FROM agent_repositories WHERE id = ?1");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(repo_from_row).transpose()
}

pub(crate) async fn all_repos(pool: &SqlitePool) -> Result<Vec<RepoRow>, CatalogueError> {
    let sql = format!("SELECT {REPO_COLUMNS} FROM agent_repositories ORDER BY rowid");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    rows.iter().map(repo_from_row).collect()
}

/// Rows in `named` or `cloned` state, in the order they were inserted (the profile's order for
/// profile rows).
pub(crate) async fn read_repos(pool: &SqlitePool) -> Result<Vec<RepoRow>, CatalogueError> {
    Ok(all_repos(pool)
        .await?
        .into_iter()
        .filter(RepoRow::is_read)
        .collect())
}

/// One part with the index data beside it.
#[derive(Debug, Clone)]
pub(crate) struct PartRecord {
    pub part: Part,
    pub offered: bool,
    pub summary: String,
    pub area: Option<String>,
    pub audience: Vec<String>,
    pub publisher: String,
    pub files: Vec<String>,
}

pub(crate) const PART_SELECT: &str = "SELECT p.id, p.agent_repository_id, p.kind, p.identifier, \
    p.name, p.version, p.status, p.supersedes, p.owner, p.file_path, p.\"commit\" AS commit_sha, \
    p.content_digest, p.vocabulary_terms_cited, p.withdrawn, i.at_head, i.offered, \
    i.not_offered_reason, i.summary, i.area, i.audience, i.publisher, i.files \
    FROM parts p JOIN part_index i ON i.part_id = p.id";

pub(crate) fn part_from_row(row: &SqliteRow) -> Result<PartRecord, CatalogueError> {
    let id: String = row.try_get("id")?;
    let repository: String = row.try_get("agent_repository_id")?;
    let kind: String = row.try_get("kind")?;
    let status: String = row.try_get("status")?;
    let digest: String = row.try_get("content_digest")?;
    let cited: String = row.try_get("vocabulary_terms_cited")?;
    let audience: String = row.try_get("audience")?;
    let files: String = row.try_get("files")?;
    let cited: Vec<VocabularyTermId> =
        serde_json::from_str(&cited).map_err(|_| corrupt("citation list"))?;
    let part = Part {
        id: PartId(uuid::Uuid::from_str(&id).map_err(|_| corrupt("part id"))?),
        agent_repository_id: AgentRepositoryId(
            uuid::Uuid::from_str(&repository).map_err(|_| corrupt("repository id"))?,
        ),
        kind: enum_from::<PartKind>(&kind).ok_or_else(|| corrupt("kind"))?,
        identifier: row.try_get("identifier")?,
        name: row.try_get("name")?,
        version: row.try_get("version")?,
        status: enum_from::<PartStatus>(&status).ok_or_else(|| corrupt("status"))?,
        supersedes: row.try_get("supersedes")?,
        owner: row.try_get("owner")?,
        file_path: row.try_get("file_path")?,
        commit: row.try_get("commit_sha")?,
        content_digest: Sha256Hex::parse(&digest).map_err(|_| corrupt("digest"))?,
        vocabulary_terms_cited: cited,
        uses: Vec::new(),
        withdrawn: row.try_get::<i64, _>("withdrawn")? != 0,
    };
    Ok(PartRecord {
        part,
        offered: row.try_get::<i64, _>("offered")? != 0,
        summary: row.try_get("summary")?,
        area: row.try_get("area")?,
        audience: serde_json::from_str(&audience).map_err(|_| corrupt("audience"))?,
        publisher: row.try_get("publisher")?,
        files: serde_json::from_str(&files).map_err(|_| corrupt("file list"))?,
    })
}

pub(crate) async fn part_by_id(
    pool: &SqlitePool,
    id: PartId,
) -> Result<Option<PartRecord>, CatalogueError> {
    let sql = format!("{PART_SELECT} WHERE p.id = ?1");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let mut record = part_from_row(&row)?;
    record.part.uses = uses_of(pool, record.part.id).await?;
    Ok(Some(record))
}

pub(crate) async fn uses_of(pool: &SqlitePool, id: PartId) -> Result<Vec<PartId>, CatalogueError> {
    let rows = sqlx::query(
        "SELECT uses_part_id FROM part_dependencies WHERE part_id = ?1 ORDER BY uses_part_id",
    )
    .bind(id.to_string())
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let text: String = row.try_get("uses_part_id")?;
            uuid::Uuid::from_str(&text)
                .map(PartId)
                .map_err(|_| corrupt("dependency"))
        })
        .collect()
}

/// Every visible head part of one repository (`at_head = 1` and `offered = 1`).
pub(crate) async fn visible_parts_of(
    pool: &SqlitePool,
    repository: AgentRepositoryId,
) -> Result<Vec<PartRecord>, CatalogueError> {
    let sql = format!(
        "{PART_SELECT} WHERE p.agent_repository_id = ?1 AND i.at_head = 1 AND i.offered = 1 \
         ORDER BY p.kind, p.identifier, p.version"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(repository.to_string())
        .fetch_all(pool)
        .await?;
    let mut records: Vec<PartRecord> = rows.iter().map(part_from_row).collect::<Result<_, _>>()?;
    for record in &mut records {
        record.part.uses = uses_of(pool, record.part.id).await?;
    }
    sort_parts(&mut records);
    Ok(records)
}

/// Sort by (kind in declaration order, identifier, semantic version).
pub(crate) fn sort_parts(records: &mut [PartRecord]) {
    records.sort_by(|a, b| {
        (a.part.kind, &a.part.identifier)
            .cmp(&(b.part.kind, &b.part.identifier))
            .then_with(|| version_order(&a.part.version, &b.part.version))
    });
}

pub(crate) fn version_order(a: &str, b: &str) -> std::cmp::Ordering {
    ordin_agent_model::compare_versions(a, b).unwrap_or_else(|_| a.cmp(b))
}

/// The visible head parts of `repository` with this kind and identifier (every version).
pub(crate) async fn visible_versions(
    pool: &SqlitePool,
    repository: AgentRepositoryId,
    kind: Option<PartKind>,
    identifier: &str,
) -> Result<Vec<PartRecord>, CatalogueError> {
    let mut sql = format!(
        "{PART_SELECT} WHERE p.agent_repository_id = ?1 AND p.identifier = ?2 \
         AND i.at_head = 1 AND i.offered = 1"
    );
    if kind.is_some() {
        sql.push_str(" AND p.kind = ?3");
    }
    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(repository.to_string())
        .bind(identifier.to_owned());
    if let Some(kind) = kind {
        query = query.bind(enum_text(&kind));
    }
    let rows = query.fetch_all(pool).await?;
    let mut records: Vec<PartRecord> = rows.iter().map(part_from_row).collect::<Result<_, _>>()?;
    for record in &mut records {
        record.part.uses = uses_of(pool, record.part.id).await?;
    }
    Ok(records)
}

/// The highest version of `records` whose status is not draft.
pub(crate) fn latest_non_draft(records: Vec<PartRecord>) -> Option<PartRecord> {
    records
        .into_iter()
        .filter(|r| r.part.status != PartStatus::Draft)
        .max_by(|a, b| version_order(&a.part.version, &b.part.version))
}

pub(crate) fn now_text() -> String {
    Timestamp::now().to_wire()
}
