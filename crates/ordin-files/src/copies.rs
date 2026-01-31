//! Published copies and held edits (FileService.md Internal Logic 9; Shared Context, *PublishedCopy
//! state machine*).

use ordin_types::domain::places::{
    CopyDestination, CopyFormat, CopyMaker, CopyOriginal, FileVersion, FileVersionOrigin,
    PlaceKind, PublishedCopy, PublishedCopyState, PublishedVia,
};
use ordin_types::time::Timestamp;
use ordin_types::{ConfluenceLinkId, PlaceId, PublishedCopyId};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::error::FileError;
use crate::service::{ts, FileService};
use crate::types::HeldEdit;
use crate::versions::WriteArgs;

/// The snake_case word serde writes for a unit enum value.
fn word<T: Serialize>(value: &T) -> Result<String, FileError> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        _ => Err(FileError::DatastoreUnavailable),
    }
}

fn from_word<T: DeserializeOwned>(raw: &str) -> Result<T, FileError> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| FileError::DatastoreUnavailable)
}

fn json<T: Serialize>(value: &T) -> Result<String, FileError> {
    serde_json::to_string(value).map_err(|_| FileError::DatastoreUnavailable)
}

fn from_json<T: DeserializeOwned>(raw: &str) -> Result<T, FileError> {
    serde_json::from_str(raw).map_err(|_| FileError::DatastoreUnavailable)
}

/// `place:<place_id>:<path>` or `page:<knowledge_page_id>` (Data Model `original_key`).
pub(crate) fn original_key(original: &CopyOriginal) -> String {
    match original {
        CopyOriginal::PlaceFile { place_id, path } => format!("place:{place_id}:{path}"),
        CopyOriginal::KnowledgePage { knowledge_page_id } => format!("page:{knowledge_page_id}"),
    }
}

/// The 2C rules the File Service checks on a published copy.
pub(crate) fn validate_copy(c: &PublishedCopy) -> Result<(), FileError> {
    if c.format == CopyFormat::ExactFile
        && (!matches!(c.destination, CopyDestination::ConfluenceAttachment { .. })
            || c.maker != CopyMaker::None)
    {
        return Err(FileError::Invalid("PUB_FORMAT_MAKER_MISMATCH"));
    }
    if c.format == CopyFormat::Page
        && !matches!(c.destination, CopyDestination::ConfluencePage { .. })
    {
        return Err(FileError::Invalid("PUB_FORMAT_DESTINATION_MISMATCH"));
    }
    // 2C allows `followed_branch` "only for an original in a repository", which a row of this
    // component cannot tell; a knowledge page is never in a repository, so only that original is
    // refused.
    if c.followed_branch.is_some() && matches!(c.original, CopyOriginal::KnowledgePage { .. }) {
        return Err(FileError::Invalid("PUB_BRANCH_UNEXPECTED"));
    }
    let audience = c.audience.chars().count();
    if !(1..=500).contains(&audience) {
        return Err(FileError::Invalid("PUB_AUDIENCE_INVALID"));
    }
    let reason_ok = match &c.failure_reason {
        Some(reason) => c.state == PublishedCopyState::Failed && reason.chars().count() <= 500,
        None => c.state != PublishedCopyState::Failed,
    };
    if !reason_ok {
        return Err(FileError::Invalid("PUB_FAILURE_REASON_MISMATCH"));
    }
    Ok(())
}

/// The `PublishedCopy` state machine (Shared Context). Staying in a state is not a transition.
pub(crate) fn transition_allowed(from: PublishedCopyState, to: PublishedCopyState) -> bool {
    use PublishedCopyState::*;
    if from == to {
        return true;
    }
    match (from, to) {
        (ReadyToPublish, AwaitingApproval)
        | (AwaitingApproval, Current)
        | (AwaitingApproval, Failed)
        | (Current, OutOfDate)
        | (OutOfDate, Updating)
        | (Updating, Current)
        | (Updating, Failed)
        | (Failed, Updating)
        | (Current | OutOfDate, ChangedAtDestination)
        | (ChangedAtDestination, Current)
        | (NotKeptUpToDate, OutOfDate) => true,
        (from, NotKeptUpToDate | Removed) => from != Removed,
        _ => false,
    }
}

const COPY_COLUMNS: &str = "id, original, destination, destination_item_id, format, maker, \
    published_via, followed_branch, keep_up_to_date, audience, original_version_published, \
    destination_version_pushed, last_updated_at, state, failure_reason";

fn copy_from_row(row: &SqliteRow) -> Result<PublishedCopy, FileError> {
    let id: String = row.try_get("id")?;
    let original: String = row.try_get("original")?;
    let destination: String = row.try_get("destination")?;
    let format: String = row.try_get("format")?;
    let maker: String = row.try_get("maker")?;
    let via: String = row.try_get("published_via")?;
    let keep: i64 = row.try_get("keep_up_to_date")?;
    let state: String = row.try_get("state")?;
    let last: Option<String> = row.try_get("last_updated_at")?;
    Ok(PublishedCopy {
        id: PublishedCopyId::parse(&id).map_err(|_| FileError::DatastoreUnavailable)?,
        original: from_json(&original)?,
        destination: from_json(&destination)?,
        destination_item_id: row.try_get("destination_item_id")?,
        format: from_word(&format)?,
        maker: from_word(&maker)?,
        published_via: from_json(&via)?,
        followed_branch: row.try_get("followed_branch")?,
        keep_up_to_date: keep != 0,
        audience: row.try_get("audience")?,
        original_version_published: row.try_get("original_version_published")?,
        destination_version_pushed: row.try_get("destination_version_pushed")?,
        last_updated_at: last.as_deref().map(ts).transpose()?,
        state: from_word(&state)?,
        failure_reason: row.try_get("failure_reason")?,
    })
}

impl FileService {
    async fn copy_row(&self, id: PublishedCopyId) -> Result<PublishedCopy, FileError> {
        let sql = format!("SELECT {COPY_COLUMNS} FROM published_copies WHERE id = ?1");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => copy_from_row(&row),
            None => Err(FileError::CopyNotFound),
        }
    }

    /// Inserts, or replaces the row with the same id; checks the 2C rules and the state transition.
    pub async fn record_published_copy(
        &self,
        c: PublishedCopy,
    ) -> Result<PublishedCopy, FileError> {
        validate_copy(&c)?;
        // A row with a new id has no earlier state, so any state may start it (the Confluence
        // Adapter records copies it finds already current); only a replacement is
        // checked against the state table.
        match self.copy_row(c.id).await {
            Ok(existing) => {
                if !transition_allowed(existing.state, c.state) {
                    return Err(FileError::CopyTransitionInvalid {
                        from: existing.state,
                        to: c.state,
                    });
                }
            }
            Err(FileError::CopyNotFound) => {}
            Err(other) => return Err(other),
        }
        let link = match &c.published_via {
            PublishedVia::ConfluenceLink { confluence_link_id } => {
                Some(confluence_link_id.to_string())
            }
            PublishedVia::PublishACopy => None,
        };
        sqlx::query(
            "INSERT INTO published_copies (id, original, original_key, destination, \
                 destination_item_id, format, maker, published_via, confluence_link_id, \
                 followed_branch, keep_up_to_date, audience, original_version_published, \
                 destination_version_pushed, last_updated_at, state, failure_reason) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17) \
             ON CONFLICT (id) DO UPDATE SET original = excluded.original, \
                 original_key = excluded.original_key, destination = excluded.destination, \
                 destination_item_id = excluded.destination_item_id, format = excluded.format, \
                 maker = excluded.maker, published_via = excluded.published_via, \
                 confluence_link_id = excluded.confluence_link_id, \
                 followed_branch = excluded.followed_branch, \
                 keep_up_to_date = excluded.keep_up_to_date, audience = excluded.audience, \
                 original_version_published = excluded.original_version_published, \
                 destination_version_pushed = excluded.destination_version_pushed, \
                 last_updated_at = excluded.last_updated_at, state = excluded.state, \
                 failure_reason = excluded.failure_reason",
        )
        .bind(c.id.to_string())
        .bind(json(&c.original)?)
        .bind(original_key(&c.original))
        .bind(json(&c.destination)?)
        .bind(&c.destination_item_id)
        .bind(word(&c.format)?)
        .bind(word(&c.maker)?)
        .bind(json(&c.published_via)?)
        .bind(link)
        .bind(&c.followed_branch)
        .bind(i64::from(c.keep_up_to_date))
        .bind(&c.audience)
        .bind(&c.original_version_published)
        .bind(&c.destination_version_pushed)
        .bind(c.last_updated_at.map(|t| t.to_wire()))
        .bind(word(&c.state)?)
        .bind(&c.failure_reason)
        .execute(&self.pool)
        .await?;
        self.record(
            "published_copy_recorded",
            None,
            None,
            "A published copy was recorded.",
            serde_json::json!({ "id": c.id.to_string(), "state": word(&c.state)? }),
        )
        .await;
        self.copy_row(c.id).await
    }

    pub async fn set_published_copy_state(
        &self,
        id: PublishedCopyId,
        state: PublishedCopyState,
        failure_reason: Option<String>,
    ) -> Result<PublishedCopy, FileError> {
        let existing = self.copy_row(id).await?;
        if !transition_allowed(existing.state, state) {
            return Err(FileError::CopyTransitionInvalid {
                from: existing.state,
                to: state,
            });
        }
        let reason_ok = match &failure_reason {
            Some(reason) => state == PublishedCopyState::Failed && reason.chars().count() <= 500,
            None => state != PublishedCopyState::Failed,
        };
        if !reason_ok {
            return Err(FileError::Invalid("PUB_FAILURE_REASON_MISMATCH"));
        }
        let updated = if state == PublishedCopyState::Current {
            Some(Timestamp::now().to_wire())
        } else {
            existing.last_updated_at.map(|t| t.to_wire())
        };
        sqlx::query(
            "UPDATE published_copies SET state = ?2, failure_reason = ?3, last_updated_at = ?4 \
             WHERE id = ?1",
        )
        .bind(id.to_string())
        .bind(word(&state)?)
        .bind(&failure_reason)
        .bind(updated)
        .execute(&self.pool)
        .await?;
        self.copy_row(id).await
    }

    pub async fn published_copy(&self, id: PublishedCopyId) -> Result<PublishedCopy, FileError> {
        self.copy_row(id).await
    }

    pub async fn published_copies_of(
        &self,
        original: &CopyOriginal,
    ) -> Result<Vec<PublishedCopy>, FileError> {
        let sql = format!(
            "SELECT {COPY_COLUMNS} FROM published_copies WHERE original_key = ?1 ORDER BY id"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(original_key(original))
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(copy_from_row).collect()
    }

    pub async fn published_copies_via_link(
        &self,
        link: ConfluenceLinkId,
    ) -> Result<Vec<PublishedCopy>, FileError> {
        let sql = format!(
            "SELECT {COPY_COLUMNS} FROM published_copies WHERE confluence_link_id = ?1 ORDER BY id"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(link.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(copy_from_row).collect()
    }

    /// The person's Save on a Markdown document (ADR-048): a new version with origin Person, held
    /// until it goes back. For a persons or workspace folder the saved file is the origin itself,
    /// so no edit is held (Internal Logic 9.4).
    pub async fn save_person_edit(
        &self,
        place_id: PlaceId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<FileVersion, FileError> {
        let (version, _) = self
            .write_inner(WriteArgs {
                place_id,
                path: path.to_owned(),
                bytes,
                origin: FileVersionOrigin::Person,
                expected_hash: None,
                by_ordin: false,
            })
            .await?;
        let place = self.place_row(place_id).await?;
        if !matches!(
            place.kind,
            PlaceKind::PersonsFolder | PlaceKind::WorkspaceFolder
        ) {
            sqlx::query(
                "INSERT INTO held_edits (place_id, path, file_version_id, saved_at) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT (place_id, path) DO UPDATE SET \
                     file_version_id = excluded.file_version_id, saved_at = excluded.saved_at",
            )
            .bind(place_id.to_string())
            .bind(&version.path)
            .bind(version.id.to_string())
            .bind(version.created_at.to_wire())
            .execute(&self.pool)
            .await?;
        }
        Ok(version)
    }

    pub async fn held_edits(&self, place_ids: &[PlaceId]) -> Result<Vec<HeldEdit>, FileError> {
        let mut out = Vec::new();
        for id in place_ids {
            let rows = sqlx::query(
                "SELECT place_id, path, file_version_id, saved_at FROM held_edits \
                 WHERE place_id = ?1 ORDER BY saved_at, path",
            )
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await?;
            for row in &rows {
                let version: String = row.try_get("file_version_id")?;
                let saved: String = row.try_get("saved_at")?;
                out.push(HeldEdit {
                    place_id: *id,
                    path: row.try_get("path")?,
                    file_version_id: ordin_types::FileVersionId::parse(&version)
                        .map_err(|_| FileError::DatastoreUnavailable)?,
                    saved_at: ts(&saved)?,
                });
            }
        }
        Ok(out)
    }

    /// The Push Service calls it after the push; the edit is no longer held.
    pub async fn mark_edit_sent(&self, place_id: PlaceId, path: &str) -> Result<(), FileError> {
        let path = crate::paths::normalise_file(path)?;
        sqlx::query("DELETE FROM held_edits WHERE place_id = ?1 AND path = ?2")
            .bind(place_id.to_string())
            .bind(&path)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
