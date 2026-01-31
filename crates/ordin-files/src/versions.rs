//! Versions, writes, restore, the bin and pruning (FileService.md Internal Logic 6, 7.2, 7.3, 14).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordin_journal::{PruneFailure, PruneRequest};
use ordin_types::domain::places::{FileVersion, FileVersionOrigin, PlaceKind};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::{FileVersionId, PlaceId};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, Transaction};

use crate::error::FileError;
use crate::index::IndexWrite;
use crate::log;
use crate::paths::{self, kind_of, name_of, normalise_file};
use crate::service::{lock, ts, FileService};
use crate::types::{BinTicket, FileBytes, PlaceChange, PlaceChangeKind, PruneReport, WriteVersion};
use crate::READ_LIMIT_BYTES;

/// Runs blocking file-system work off the async threads.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, FileError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| FileError::WriteFailed)
}

pub(crate) fn hash_of(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    Sha256Hex::from_digest(&digest).0
}

/// `<versions>/<hash[0..2]>/<hash>`.
pub(crate) fn copy_path(versions_dir: &Path, hash: &str) -> PathBuf {
    versions_dir.join(&hash[..2]).join(hash)
}

/// Temporary file in the same folder, `fsync`, rename over the target (Internal Logic 6.5, 6.6).
pub(crate) fn atomic_write(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent folder"))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!("{}{}", paths::TEMP_PREFIX, uuid::Uuid::now_v7()));
    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, target)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub(crate) fn origin_kind(origin: &FileVersionOrigin) -> &'static str {
    match origin {
        FileVersionOrigin::Person => "person",
        FileVersionOrigin::Run { .. } => "run",
        FileVersionOrigin::ArtCraftEdit { .. } => "art_craft_edit",
    }
}

/// The "changed by" word recorded in the index for a write with this origin.
fn author_of(origin: &FileVersionOrigin) -> &'static str {
    match origin {
        FileVersionOrigin::Person => "you",
        _ => "Ordin",
    }
}

pub(crate) fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(d) if d.is_unique_violation())
}

/// Places a file may be written into through this service (Internal Logic 6.2).
pub(crate) fn writable_kind(kind: PlaceKind) -> bool {
    matches!(
        kind,
        PlaceKind::PersonsFolder
            | PlaceKind::WorkspaceFolder
            | PlaceKind::WorkingCopy
            | PlaceKind::CodingTaskWorktree
    )
}

pub(crate) fn version_from_row(row: &SqliteRow) -> Result<FileVersion, FileError> {
    let bad = || FileError::DatastoreUnavailable;
    let id: String = row.try_get("id")?;
    let place_id: String = row.try_get("place_id")?;
    let version: i64 = row.try_get("version")?;
    let hash: String = row.try_get("hash")?;
    let origin: String = row.try_get("origin")?;
    let created_at: String = row.try_get("created_at")?;
    Ok(FileVersion {
        id: FileVersionId::parse(&id).map_err(|_| bad())?,
        place_id: PlaceId::parse(&place_id).map_err(|_| bad())?,
        path: row.try_get("path")?,
        version: u32::try_from(version).map_err(|_| bad())?,
        hash: Sha256Hex::parse(&hash).map_err(|_| bad())?,
        origin: serde_json::from_str(&origin).map_err(|_| bad())?,
        created_at: ts(&created_at)?,
    })
}

const VERSION_COLUMNS: &str = "id, place_id, path, version, hash, origin, created_at";

/// Inserts the next version of a path inside `tx` (Internal Logic 6.7).
async fn insert_version_tx(
    tx: &mut Transaction<'_, Sqlite>,
    place_id: PlaceId,
    path: &str,
    hash: &str,
    origin: &FileVersionOrigin,
    created_at: Timestamp,
) -> Result<FileVersion, sqlx::Error> {
    let next: i64 = sqlx::query(
        "SELECT COALESCE(MAX(version), 0) + 1 AS next FROM file_versions \
         WHERE place_id = ?1 AND path = ?2",
    )
    .bind(place_id.to_string())
    .bind(path)
    .fetch_one(&mut **tx)
    .await?
    .try_get("next")?;
    let id = FileVersionId::new();
    let origin_json =
        serde_json::to_string(origin).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
    sqlx::query(
        "INSERT INTO file_versions (id, place_id, path, version, hash, origin, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(id.to_string())
    .bind(place_id.to_string())
    .bind(path)
    .bind(next)
    .bind(hash)
    .bind(&origin_json)
    .bind(created_at.to_wire())
    .execute(&mut **tx)
    .await?;
    Ok(FileVersion {
        id,
        place_id,
        path: path.to_owned(),
        version: u32::try_from(next).unwrap_or(u32::MAX),
        hash: Sha256Hex(hash.to_owned()),
        origin: origin.clone(),
        created_at,
    })
}

pub(crate) async fn insert_version_pub(
    tx: &mut Transaction<'_, Sqlite>,
    place_id: PlaceId,
    path: &str,
    hash: &str,
    origin: &FileVersionOrigin,
    created_at: Timestamp,
) -> Result<FileVersion, sqlx::Error> {
    insert_version_tx(tx, place_id, path, hash, origin, created_at).await
}

/// What `write_inner` takes: the public `WriteVersion` plus who the change is announced as.
pub(crate) struct WriteArgs {
    pub(crate) place_id: PlaceId,
    pub(crate) path: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) origin: FileVersionOrigin,
    pub(crate) expected_hash: Option<Sha256Hex>,
    pub(crate) by_ordin: bool,
}

impl FileService {
    pub(crate) async fn latest(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<Option<FileVersion>, FileError> {
        let sql = format!(
            "SELECT {VERSION_COLUMNS} FROM file_versions WHERE place_id = ?1 AND path = ?2 \
             ORDER BY version DESC LIMIT 1"
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(place_id.to_string())
            .bind(path)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(version_from_row).transpose()
    }

    /// Stores `bytes` under their hash in the version store if not already there (Internal Logic 6.5).
    pub(crate) async fn store_copy(&self, hash: &str, bytes: Vec<u8>) -> Result<(), FileError> {
        let target = copy_path(&self.versions_dir, hash);
        blocking(move || {
            if target.exists() {
                return Ok(());
            }
            atomic_write(&target, &bytes).map_err(|_| FileError::WriteFailed)
        })
        .await?
    }

    /// Internal Logic 6.4: a file with no version row keeps its current bytes as version 1.
    async fn ensure_baseline(
        &self,
        place_id: PlaceId,
        path: &str,
        current: Vec<u8>,
    ) -> Result<(), FileError> {
        let hash = hash_of(&current);
        self.store_copy(&hash, current).await?;
        let mut tx = self.begin_write().await?;
        insert_version_tx(
            &mut tx,
            place_id,
            path,
            &hash,
            &FileVersionOrigin::Person,
            Timestamp::now(),
        )
        .await
        .map_err(|e| {
            if is_unique_violation(&e) {
                FileError::Invalid("FVER_VERSION_GAP")
            } else {
                e.into()
            }
        })?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn baseline_pub(
        &self,
        place_id: PlaceId,
        path: &str,
        current: Vec<u8>,
    ) -> Result<(), FileError> {
        self.ensure_baseline(place_id, path, current).await
    }

    /// A file just published keeps those bytes as version 1 when it has no version yet, so that a
    /// later change to it is recorded and has an author (Internal Logic 6a).
    pub async fn keep_published_version(
        &self,
        place_id: PlaceId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<(), FileError> {
        let path = normalise_file(path)?;
        if self.latest(place_id, &path).await?.is_some() {
            return Ok(());
        }
        self.ensure_baseline(place_id, &path, bytes).await
    }

    /// Reads a file for hashing; larger than the limit is `FILE_TOO_LARGE`.
    pub(crate) async fn read_limited(&self, abs: &Path) -> Result<Option<Vec<u8>>, FileError> {
        let abs = abs.to_path_buf();
        blocking(move || -> Result<Option<Vec<u8>>, FileError> {
            let meta = match std::fs::metadata(&abs) {
                Ok(meta) => meta,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(FileError::NotFound),
            };
            if meta.is_dir() {
                return Err(FileError::Invalid("FVER_PATH_INVALID"));
            }
            if meta.len() > READ_LIMIT_BYTES {
                return Err(FileError::TooLarge {
                    limit_bytes: READ_LIMIT_BYTES,
                });
            }
            std::fs::read(&abs)
                .map(Some)
                .map_err(|_| FileError::NotFound)
        })
        .await?
    }

    /// Steps 6.1–6.8; `restore_*`, `save_*` and `write_version` all arrive here.
    pub(crate) async fn write_inner(&self, a: WriteArgs) -> Result<(FileVersion, bool), FileError> {
        // 6.1
        let path = normalise_file(&a.path)?;
        if a.bytes.len() as u64 > READ_LIMIT_BYTES {
            return Err(FileError::TooLarge {
                limit_bytes: READ_LIMIT_BYTES,
            });
        }
        let place = self.place_row(a.place_id).await?;
        // 6.2
        if !writable_kind(place.kind) {
            return Err(FileError::ProtectedLocation);
        }
        let abs = paths::confine(&place.root, &path)?;
        // 6.3 and 6.4
        let has_rows = self.latest(a.place_id, &path).await?.is_some();
        let current = if a.expected_hash.is_some() || !has_rows {
            self.read_limited(&abs).await?
        } else {
            None
        };
        if let Some(expected) = &a.expected_hash {
            let Some(bytes) = &current else {
                return Err(FileError::NotFound);
            };
            let actual = hash_of(bytes);
            if actual != expected.0 {
                return Err(FileError::ChangedMeanwhile {
                    current: Sha256Hex(actual),
                });
            }
        }
        let existed = if has_rows {
            blocking({
                let abs = abs.clone();
                move || abs.is_file()
            })
            .await?
        } else {
            current.is_some()
        };
        if !has_rows {
            if let Some(bytes) = current {
                self.ensure_baseline(a.place_id, &path, bytes).await?;
            }
        }
        // 6.5
        let hash = hash_of(&a.bytes);
        self.store_copy(&hash, a.bytes.clone()).await?;
        // 6.6: the watcher's event for this write is dropped by the recent-writes map.
        self.note_recent(a.place_id, &path, &hash);
        let write = {
            let (abs, bytes) = (abs.clone(), a.bytes.clone());
            blocking(move || atomic_write(&abs, &bytes)).await?
        };
        if write.is_err() {
            lock(&self.recent).remove(&(a.place_id, path.clone()));
            return Err(FileError::WriteFailed);
        }
        let stat = stat_of(&abs).await?;
        let name = name_of(&path).to_owned();
        let kind = kind_of(&name);
        let (text, text_state) = self.extract_indexed(a.place_id, &kind, a.bytes).await;
        // 6.7: one retry of the transaction on a unique violation.
        let now = Timestamp::now();
        let mut attempt = 0;
        let version = loop {
            attempt += 1;
            let tried = self
                .insert_version_and_index(
                    &a.place_id,
                    &path,
                    &hash,
                    &a.origin,
                    now,
                    &stat,
                    text.as_deref(),
                    text_state,
                )
                .await;
            match tried {
                Ok(version) => break version,
                Err(e) if is_unique_violation(&e) && attempt == 1 => continue,
                Err(e) if is_unique_violation(&e) => {
                    return Err(FileError::Invalid("FVER_VERSION_GAP"))
                }
                Err(e) => return Err(e.into()),
            }
        };
        // 6.8
        let run_id = match &a.origin {
            FileVersionOrigin::Run { run_id, .. } => Some(*run_id),
            _ => None,
        };
        self.record(
            "file_version_written",
            None,
            run_id,
            "A new version of a document was kept.",
            serde_json::json!({
                "place_id": a.place_id.to_string(),
                "version": version.version,
                "origin_kind": origin_kind(&a.origin),
                "path": path,
            }),
        )
        .await;
        log::file_version_written(a.place_id, version.version, origin_kind(&a.origin), run_id);
        self.emit(PlaceChange {
            place_id: a.place_id,
            path: path.clone(),
            change: if existed {
                PlaceChangeKind::Modified
            } else {
                PlaceChangeKind::Created
            },
            observed_at: now,
            by_ordin: a.by_ordin,
            version: Some(version.version),
            origin: Some(a.origin),
        });
        Ok((version, existed))
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_version_and_index(
        &self,
        place_id: &PlaceId,
        path: &str,
        hash: &str,
        origin: &FileVersionOrigin,
        now: Timestamp,
        stat: &Stat,
        text: Option<&str>,
        text_state: &'static str,
    ) -> Result<FileVersion, sqlx::Error> {
        let mut tx = self.begin_write().await?;
        let version = insert_version_tx(&mut tx, *place_id, path, hash, origin, now).await?;
        crate::index::upsert_tx(
            &mut tx,
            &IndexWrite {
                place_id: *place_id,
                path,
                is_folder: false,
                size: stat.size,
                mtime_ns: stat.mtime_ns,
                hash: Some(hash),
                changed_at: now,
                changed_by: Some(author_of(origin)),
                text,
                text_state,
            },
            true,
        )
        .await?;
        tx.commit().await?;
        Ok(version)
    }

    // ---- public surface

    /// A person's edit: a new version with origin `Person`, announced with `by_ordin = false`
    /// (FileService.md FS-R1-1). The refusals are `write_version`'s.
    // spec: ordin-desktop/components/FileService#FS-R1-1
    pub async fn write_person_edit(
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
        Ok(version)
    }

    pub async fn write_version(&self, w: WriteVersion) -> Result<FileVersion, FileError> {
        let (version, _) = self
            .write_inner(WriteArgs {
                place_id: w.place_id,
                path: w.path,
                bytes: w.bytes,
                origin: w.origin,
                expected_hash: w.expected_hash,
                by_ordin: true,
            })
            .await?;
        Ok(version)
    }

    pub async fn versions(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<Vec<FileVersion>, FileError> {
        let path = normalise_file(path)?;
        let sql = format!(
            "SELECT {VERSION_COLUMNS} FROM file_versions WHERE place_id = ?1 AND path = ?2 \
             ORDER BY version"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(place_id.to_string())
            .bind(&path)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(version_from_row).collect()
    }

    async fn version_row(&self, id: FileVersionId) -> Result<FileVersion, FileError> {
        let sql = format!("SELECT {VERSION_COLUMNS} FROM file_versions WHERE id = ?1");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(row) => version_from_row(&row),
            None => Err(FileError::VersionNotFound),
        }
    }

    async fn read_copy(&self, hash: &str) -> Result<Vec<u8>, FileError> {
        let target = copy_path(&self.versions_dir, hash);
        blocking(move || std::fs::read(&target).map_err(|_| FileError::VersionCopyMissing)).await?
    }

    pub async fn read_version(&self, version_id: FileVersionId) -> Result<FileBytes, FileError> {
        let version = self.version_row(version_id).await?;
        let bytes = self.read_copy(&version.hash.0).await?;
        let media_type = paths::media_type(&kind_of(name_of(&version.path))).to_owned();
        Ok(FileBytes {
            place_id: version.place_id,
            path: version.path,
            bytes,
            hash: version.hash,
            media_type,
        })
    }

    /// Writes an earlier version's bytes back as a new version with origin Person.
    pub async fn restore_version(
        &self,
        version_id: FileVersionId,
    ) -> Result<FileVersion, FileError> {
        let version = self.version_row(version_id).await?;
        let bytes = self.read_copy(&version.hash.0).await?;
        let (written, _) = self
            .write_inner(WriteArgs {
                place_id: version.place_id,
                path: version.path,
                bytes,
                origin: FileVersionOrigin::Person,
                expected_hash: None,
                by_ordin: true,
            })
            .await?;
        Ok(written)
    }

    /// The latest version row of a path; `None` when the path has no version history.
    pub async fn current_version(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<Option<FileVersion>, FileError> {
        let path = normalise_file(path)?;
        self.latest(place_id, &path).await
    }

    /// Keeps the file's bytes as a version, then moves the file to the system bin (Internal Logic 7.2).
    pub async fn move_to_bin(&self, place_id: PlaceId, path: &str) -> Result<BinTicket, FileError> {
        let path = normalise_file(path)?;
        let place = self.place_row(place_id).await?;
        if !writable_kind(place.kind) {
            return Err(FileError::ProtectedLocation);
        }
        let abs = paths::confine(&place.root, &path)?;
        let bytes = self.read_limited(&abs).await?.ok_or(FileError::NotFound)?;
        let hash = hash_of(&bytes);
        // Reuse the latest version when it already holds these bytes.
        let kept = match self.latest(place_id, &path).await? {
            Some(latest) if latest.hash.0 == hash => latest,
            _ => {
                self.store_copy(&hash, bytes).await?;
                let mut tx = self.begin_write().await?;
                let version = insert_version_tx(
                    &mut tx,
                    place_id,
                    &path,
                    &hash,
                    &FileVersionOrigin::Person,
                    Timestamp::now(),
                )
                .await
                .map_err(|e| {
                    if is_unique_violation(&e) {
                        FileError::Invalid("FVER_VERSION_GAP")
                    } else {
                        e.into()
                    }
                })?;
                tx.commit().await?;
                version
            }
        };
        let bin = self.bin.clone();
        let target = abs.clone();
        let moved = blocking(move || bin.delete(&target)).await?;
        if moved.is_err() {
            return Err(FileError::BinUnavailable);
        }
        self.mark_quiet(place_id, &path);
        self.remove_index_path(place_id, &path).await?;
        self.emit(PlaceChange {
            place_id,
            path: path.clone(),
            change: PlaceChangeKind::Removed,
            observed_at: Timestamp::now(),
            by_ordin: true,
            version: Some(kept.version),
            origin: Some(kept.origin.clone()),
        });
        self.record(
            "file_binned",
            None,
            None,
            "A document was moved to the Bin.",
            serde_json::json!({
                "place_id": place_id.to_string(),
                "file_version_id": kept.id.to_string(),
            }),
        )
        .await;
        Ok(BinTicket {
            place_id,
            path,
            file_version_id: kept.id,
        })
    }

    /// Writes the kept bytes back at the ticket's path as a new version with origin Person (Internal Logic 7.3).
    pub async fn restore_from_bin(&self, ticket: &BinTicket) -> Result<FileVersion, FileError> {
        let place = self.place_row(ticket.place_id).await?;
        let path = normalise_file(&ticket.path)?;
        let abs = paths::confine(&place.root, &path)?;
        let taken = blocking({
            let abs = abs.clone();
            move || std::fs::symlink_metadata(&abs).is_ok()
        })
        .await?;
        if taken {
            return Err(FileError::RestoreTargetExists);
        }
        let kept = self.version_row(ticket.file_version_id).await?;
        let bytes = self.read_copy(&kept.hash.0).await?;
        let (written, _) = self
            .write_inner(WriteArgs {
                place_id: ticket.place_id,
                path,
                bytes,
                origin: FileVersionOrigin::Person,
                expected_hash: None,
                by_ordin: true,
            })
            .await?;
        self.record(
            "file_restored",
            None,
            None,
            "A document was put back from the Bin.",
            serde_json::json!({
                "place_id": ticket.place_id.to_string(),
                "file_version_id": ticket.file_version_id.to_string(),
            }),
        )
        .await;
        Ok(written)
    }

    /// Deletes version copies older than `before`, keeping the latest version of every path.
    pub async fn prune_versions(&self, before: Timestamp) -> Result<PruneReport, FileError> {
        // A version a held edit points at stays until the edit is sent.
        let rows = sqlx::query(
            "SELECT v.id AS id, v.place_id AS place_id, v.path AS path, v.version AS version, \
                    v.hash AS hash \
             FROM file_versions v \
             WHERE v.created_at < ?1 \
               AND v.id NOT IN (SELECT file_version_id FROM held_edits) \
               AND NOT EXISTS (SELECT 1 FROM published_copies c \
                               WHERE c.state <> 'removed' AND c.original_key = 'place:' || v.place_id || ':' || v.path \
                                 AND v.version >= COALESCE( \
                                     (SELECT MAX(x.version) FROM file_versions x \
                                      WHERE x.place_id = v.place_id AND x.path = v.path \
                                        AND x.hash = c.original_version_published), 0)) \
               AND v.version < (SELECT MAX(x.version) FROM file_versions x \
                                WHERE x.place_id = v.place_id AND x.path = v.path)",
        )
        .bind(before.to_wire())
        .fetch_all(&self.pool)
        .await?;
        let mut candidates: Vec<Candidate> = Vec::with_capacity(rows.len());
        for row in &rows {
            candidates.push(Candidate {
                id: row.try_get("id")?,
                place_id: row.try_get("place_id")?,
                path: row.try_get("path")?,
                version: row.try_get("version")?,
                hash: row.try_get("hash")?,
            });
        }
        // The defect guard: no candidate is the latest version of its path.
        let latest = sqlx::query(
            "SELECT place_id, path, MAX(version) AS latest FROM file_versions GROUP BY place_id, path",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut latest_map = std::collections::HashMap::new();
        for row in &latest {
            latest_map.insert(
                (
                    row.try_get::<String, _>("place_id")?,
                    row.try_get::<String, _>("path")?,
                ),
                row.try_get::<i64, _>("latest")?,
            );
        }
        ensure_not_latest(&candidates, &latest_map)?;

        let mut tx = self.begin_write().await?;
        for c in &candidates {
            sqlx::query("DELETE FROM file_versions WHERE id = ?1")
                .bind(&c.id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        // A content-addressed copy goes only when no row points at it any more.
        let mut hashes: Vec<&str> = candidates.iter().map(|c| c.hash.as_str()).collect();
        hashes.sort_unstable();
        hashes.dedup();
        let mut bytes_freed = 0u64;
        for hash in hashes {
            let left: i64 = sqlx::query("SELECT COUNT(*) AS n FROM file_versions WHERE hash = ?1")
                .bind(hash)
                .fetch_one(&self.pool)
                .await?
                .try_get("n")?;
            if left == 0 {
                let target = copy_path(&self.versions_dir, hash);
                bytes_freed += blocking(move || {
                    let size = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
                    std::fs::remove_file(&target).map(|()| size).unwrap_or(0)
                })
                .await?;
            }
        }
        let report = PruneReport {
            versions_removed: candidates.len() as u64,
            bytes_freed,
        };
        log::versions_pruned(report.versions_removed, report.bytes_freed);
        Ok(report)
    }

    /// The Journal's prune participant for module "files" (Internal Logic 14).
    // spec: ordin-desktop/components/FileService#SA-RECON-03
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn ordin_journal::PruneParticipant> {
        Arc::new(FilesPruneParticipant {
            service: self.clone(),
        })
    }
}

/// A version row selected for removal.
pub(crate) struct Candidate {
    pub(crate) id: String,
    pub(crate) place_id: String,
    pub(crate) path: String,
    pub(crate) version: i64,
    pub(crate) hash: String,
}

/// `FVER_LATEST_PRUNE`: a pruning pass must never remove the newest version of a path.
pub(crate) fn ensure_not_latest(
    candidates: &[Candidate],
    latest: &std::collections::HashMap<(String, String), i64>,
) -> Result<(), FileError> {
    for c in candidates {
        if latest.get(&(c.place_id.clone(), c.path.clone())) == Some(&c.version) {
            return Err(FileError::Invalid("FVER_LATEST_PRUNE"));
        }
    }
    Ok(())
}

pub(crate) struct Stat {
    pub(crate) size: i64,
    pub(crate) mtime_ns: i64,
}

pub(crate) async fn stat_of(abs: &Path) -> Result<Stat, FileError> {
    let abs = abs.to_path_buf();
    blocking(move || {
        let meta = std::fs::metadata(&abs).map_err(|_| FileError::WriteFailed)?;
        Ok(Stat {
            size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
            mtime_ns: mtime_ns(&meta),
        })
    })
    .await?
}

pub(crate) fn mtime_ns(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

struct FilesPruneParticipant {
    service: Arc<FileService>,
}

#[async_trait::async_trait]
impl ordin_journal::PruneParticipant for FilesPruneParticipant {
    fn module(&self) -> &'static str {
        crate::MODULE
    }

    async fn prune(
        &self,
        _store: &ordin_journal::Datastore,
        request: &PruneRequest,
    ) -> Result<u64, PruneFailure> {
        // The latest version per path is always kept, so `runs_in_progress` needs no test here.
        self.service
            .prune_versions(request.cutoff)
            .await
            .map(|report| report.versions_removed)
            .map_err(|error| PruneFailure { code: error.code() })
    }
}
