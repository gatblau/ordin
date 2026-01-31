//! The local index of names and document text, and the walk that builds and refreshes it
//! (FileService.md Internal Logic 5.2–5.5; Data Model `file_index`, `file_index_fts`).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ordin_types::domain::places::FileVersionOrigin;
use ordin_types::time::Timestamp;
use ordin_types::PlaceId;
use sqlx::{Row, Sqlite, Transaction};

use crate::attribution::Claim;
use crate::error::FileError;
use crate::extract::{extract_text, ExtractFailure};
use crate::log;
use crate::paths::{is_temp_name, kind_of, name_of, parent_of, relative_to};
use crate::service::{FileService, PlaceRow};
use crate::types::{IndexCoverage, PlaceChange, PlaceChangeKind};
use crate::versions::{blocking, hash_of, is_unique_violation, mtime_ns, origin_kind, Stat};
use crate::{INDEX_MAX_SOURCE_BYTES, MAX_FOLDER_DEPTH, MAX_PLACE_FILES, READ_LIMIT_BYTES};

/// One `file_index` row to write.
pub(crate) struct IndexWrite<'a> {
    pub(crate) place_id: PlaceId,
    /// Normalised, without a trailing `/`; a folder is stored with one.
    pub(crate) path: &'a str,
    pub(crate) is_folder: bool,
    pub(crate) size: i64,
    pub(crate) mtime_ns: i64,
    pub(crate) hash: Option<&'a str>,
    pub(crate) changed_at: Timestamp,
    pub(crate) changed_by: Option<&'a str>,
    pub(crate) text: Option<&'a str>,
    /// `text` or `name_only` (`pending` is the column default for rows not yet read).
    pub(crate) text_state: &'static str,
}

/// Inserts or updates the row and replaces its full-text row. `replace_fts` is false only for a
/// path known to be new, so a large first build does not scan the full-text table once per file.
pub(crate) async fn upsert_tx(
    tx: &mut Transaction<'_, Sqlite>,
    w: &IndexWrite<'_>,
    replace_fts: bool,
) -> Result<(), sqlx::Error> {
    let stored = if w.is_folder {
        format!("{}/", w.path)
    } else {
        w.path.to_owned()
    };
    let name = name_of(w.path).to_owned();
    let kind = if w.is_folder {
        String::new()
    } else {
        kind_of(&name)
    };
    sqlx::query(
        "INSERT INTO file_index (place_id, path, parent, name, is_folder, kind, size_bytes, \
                                 mtime_ns, hash, changed_at, changed_by, text_state) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
         ON CONFLICT (place_id, path) DO UPDATE SET \
             size_bytes = excluded.size_bytes, mtime_ns = excluded.mtime_ns, hash = excluded.hash, \
             changed_at = excluded.changed_at, \
             changed_by = COALESCE(excluded.changed_by, file_index.changed_by), \
             text_state = excluded.text_state",
    )
    .bind(w.place_id.to_string())
    .bind(&stored)
    .bind(parent_of(w.path))
    .bind(&name)
    .bind(i64::from(w.is_folder))
    .bind(&kind)
    .bind(w.size)
    .bind(w.mtime_ns)
    .bind(w.hash)
    .bind(w.changed_at.to_wire())
    .bind(w.changed_by)
    .bind(w.text_state)
    .execute(&mut **tx)
    .await?;
    if replace_fts {
        sqlx::query("DELETE FROM file_index_fts WHERE place_id = ?1 AND path = ?2")
            .bind(w.place_id.to_string())
            .bind(&stored)
            .execute(&mut **tx)
            .await?;
    }
    if let Some(text) = w.text {
        sqlx::query("INSERT INTO file_index_fts (place_id, path, body) VALUES (?1, ?2, ?3)")
            .bind(w.place_id.to_string())
            .bind(&stored)
            .bind(text)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// What the index holds for a path.
#[derive(Debug, Clone)]
pub(crate) struct IndexRow {
    pub(crate) is_folder: bool,
    pub(crate) size: i64,
    pub(crate) mtime_ns: i64,
    pub(crate) hash: Option<String>,
}

impl FileService {
    /// Extraction runs on a blocking thread, two at a time, with a 30 s limit; a failure or a
    /// timeout indexes the name only and logs `index_extract_failed` (Internal Logic 5.4).
    pub(crate) async fn extract_indexed(
        &self,
        place_id: PlaceId,
        kind: &str,
        bytes: Vec<u8>,
    ) -> (Option<String>, &'static str) {
        let Ok(_permit) = self.extraction.clone().acquire_owned().await else {
            return (None, "name_only");
        };
        let kind_owned = kind.to_owned();
        let work = tokio::task::spawn_blocking(move || extract_text(&kind_owned, &bytes));
        match tokio::time::timeout(Duration::from_secs(30), work).await {
            Ok(Ok(Ok(Some(text)))) => (Some(text), "text"),
            Ok(Ok(Ok(None))) => (None, "name_only"),
            Ok(Ok(Err(failure))) => {
                log::index_extract_failed(place_id, kind, failure.class());
                (None, "name_only")
            }
            Ok(Err(_)) => {
                log::index_extract_failed(place_id, kind, ExtractFailure::Archive.class());
                (None, "name_only")
            }
            Err(_) => {
                log::index_extract_failed(place_id, kind, "timeout");
                (None, "name_only")
            }
        }
    }

    pub(crate) async fn index_rows(
        &self,
        place_id: PlaceId,
    ) -> Result<HashMap<String, IndexRow>, FileError> {
        let rows = sqlx::query(
            "SELECT path, is_folder, size_bytes, mtime_ns, hash FROM file_index WHERE place_id = ?1",
        )
        .bind(place_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut out = HashMap::with_capacity(rows.len());
        for row in &rows {
            let path: String = row.try_get("path")?;
            let is_folder: i64 = row.try_get("is_folder")?;
            out.insert(
                path,
                IndexRow {
                    is_folder: is_folder != 0,
                    size: row.try_get("size_bytes")?,
                    mtime_ns: row.try_get("mtime_ns")?,
                    hash: row.try_get("hash")?,
                },
            );
        }
        Ok(out)
    }

    pub(crate) async fn index_row(
        &self,
        place_id: PlaceId,
        stored_path: &str,
    ) -> Result<Option<IndexRow>, FileError> {
        let row = sqlx::query(
            "SELECT is_folder, size_bytes, mtime_ns, hash FROM file_index \
             WHERE place_id = ?1 AND path = ?2",
        )
        .bind(place_id.to_string())
        .bind(stored_path)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => {
                let is_folder: i64 = row.try_get("is_folder")?;
                Ok(Some(IndexRow {
                    is_folder: is_folder != 0,
                    size: row.try_get("size_bytes")?,
                    mtime_ns: row.try_get("mtime_ns")?,
                    hash: row.try_get("hash")?,
                }))
            }
            None => Ok(None),
        }
    }

    /// Deletes one file's index row and full-text row.
    pub(crate) async fn remove_index_path(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<(), FileError> {
        let mut tx = self.begin_write().await?;
        sqlx::query("DELETE FROM file_index_fts WHERE place_id = ?1 AND path = ?2")
            .bind(place_id.to_string())
            .bind(path)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM file_index WHERE place_id = ?1 AND path = ?2")
            .bind(place_id.to_string())
            .bind(path)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Deletes every index row under a folder (`prefix` ends with `/`); returns the files removed.
    pub(crate) async fn remove_index_prefix(
        &self,
        place_id: PlaceId,
        prefix: &str,
    ) -> Result<Vec<String>, FileError> {
        let rows = sqlx::query(
            "SELECT path, is_folder FROM file_index \
             WHERE place_id = ?1 AND (path = ?2 OR substr(path, 1, length(?2)) = ?2)",
        )
        .bind(place_id.to_string())
        .bind(prefix)
        .fetch_all(&self.pool)
        .await?;
        let mut files = Vec::new();
        for row in &rows {
            let path: String = row.try_get("path")?;
            let is_folder: i64 = row.try_get("is_folder")?;
            if is_folder == 0 {
                files.push(path.clone());
            }
            self.remove_index_path(place_id, &path).await?;
        }
        Ok(files)
    }

    /// Reads one changed or new file, writes its index row (and, for a path with version history
    /// whose hash moved on, a new version with origin Person; Internal Logic 5.5), and returns
    /// the change to announce. Does not emit.
    pub(crate) async fn index_changed_file(
        &self,
        place: &PlaceRow,
        rel: &str,
        abs: &Path,
        is_new: bool,
    ) -> Result<Option<PlaceChange>, FileError> {
        let size_limit = INDEX_MAX_SOURCE_BYTES;
        let target = abs.to_path_buf();
        let Some((stat, meta_len)) = blocking(move || {
            std::fs::metadata(&target)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| {
                    (
                        Stat {
                            size: i64::try_from(m.len()).unwrap_or(i64::MAX),
                            mtime_ns: mtime_ns(&m),
                        },
                        m.len(),
                    )
                })
        })
        .await?
        else {
            return Ok(None);
        };
        let latest = self.latest(place.id, rel).await?;
        // A change under a claim is the run's (Internal Logic 6a). A path claim also gives a new file
        // its first version; a wide claim does not, or a build would keep a copy of its every output.
        let claimed = self.claim_over(place.id, rel).await?;
        let keeps_new = claimed.as_ref().is_some_and(|claim| !claim.wide);
        let kind = kind_of(name_of(rel));
        let read_whole = meta_len <= size_limit
            || ((latest.is_some() || keeps_new) && meta_len <= READ_LIMIT_BYTES);
        let target = abs.to_path_buf();
        let (hash, bytes) = blocking(move || -> Option<(String, Option<Vec<u8>>)> {
            if read_whole {
                let bytes = std::fs::read(&target).ok()?;
                Some((hash_of(&bytes), Some(bytes)))
            } else {
                hash_streaming(&target).map(|h| (h, None))
            }
        })
        .await?
        .ok_or(FileError::NotFound)?;
        #[cfg(test)]
        self.after_read_for_test().await;
        // A claim opened while the bytes were read may cover them, so it is read again now.
        let claim = match claimed {
            Some(claim) => Some(claim),
            None => self.claim_over(place.id, rel).await?,
        };
        let keeps_new = keeps_new || claim.as_ref().is_some_and(|claim| !claim.wide);
        // Writes made here are announced by the writer (Internal Logic 5.6).
        if self.recent_matches(place.id, rel, &hash) {
            return Ok(None);
        }
        let now = Timestamp::now();
        let mut new_version = None;
        let moved_on = latest.as_ref().map_or(keeps_new, |l| l.hash.0 != hash);
        let needs_version =
            moved_on && !self.recent_age_under(place.id, rel, &hash, Duration::from_secs(2));
        if needs_version {
            if let Some(bytes) = &bytes {
                self.store_copy(&hash, bytes.clone()).await?;
                new_version = Some(());
            }
        }
        let (text, text_state) = match bytes {
            Some(bytes) => self.extract_indexed(place.id, &kind, bytes).await,
            None => (None, "name_only"),
        };
        let origin = claim
            .as_ref()
            .map_or(FileVersionOrigin::Person, Claim::origin);
        let mut tx = self.begin_write().await?;
        let mut written = None;
        if new_version.is_some() {
            written = Some(
                crate::versions::insert_version_pub(&mut tx, place.id, rel, &hash, &origin, now)
                    .await
                    .map_err(|e| {
                        if is_unique_violation(&e) {
                            FileError::Invalid("FVER_VERSION_GAP")
                        } else {
                            e.into()
                        }
                    })?,
            );
        }
        upsert_tx(
            &mut tx,
            &IndexWrite {
                place_id: place.id,
                path: rel,
                is_folder: false,
                size: stat.size,
                mtime_ns: stat.mtime_ns,
                hash: Some(&hash),
                changed_at: now,
                changed_by: Some(if claim.is_some() { "Ordin" } else { "you" }),
                text: text.as_deref(),
                text_state,
            },
            !is_new,
        )
        .await?;
        tx.commit().await?;
        let (version, origin) = match &written {
            Some(v) => (Some(v.version), Some(v.origin.clone())),
            None => (None, None),
        };
        let run_id = claim.as_ref().map(|claim| claim.run_id);
        if let Some(v) = &written {
            log::file_version_written(place.id, v.version, origin_kind(&v.origin), run_id);
            if run_id.is_some() {
                self.record(
                    "file_version_written",
                    None,
                    run_id,
                    "A new version of a document was kept.",
                    serde_json::json!({
                        "place_id": place.id.to_string(),
                        "version": v.version,
                        "origin_kind": origin_kind(&v.origin),
                        "path": rel,
                    }),
                )
                .await;
            }
        }
        Ok(Some(PlaceChange {
            place_id: place.id,
            path: rel.to_owned(),
            change: if is_new {
                PlaceChangeKind::Created
            } else {
                PlaceChangeKind::Modified
            },
            observed_at: now,
            by_ordin: claim.is_some(),
            version,
            origin,
        }))
    }

    /// Upserts a folder row.
    pub(crate) async fn index_folder(
        &self,
        place_id: PlaceId,
        rel: &str,
        mtime_ns_value: i64,
    ) -> Result<(), FileError> {
        let mut tx = self.begin_write().await?;
        upsert_tx(
            &mut tx,
            &IndexWrite {
                place_id,
                path: rel,
                is_folder: true,
                size: 0,
                mtime_ns: mtime_ns_value,
                hash: None,
                changed_at: Timestamp::now(),
                changed_by: None,
                text: None,
                text_state: "name_only",
            },
            false,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Start-up rescan and first index build (Internal Logic 5.3): walk the place breadth-first,
    /// compare `(size, mtime)` with the index, refresh what differs and drop what is gone.
    pub(crate) async fn rescan_place(&self, place_id: PlaceId) -> Result<(), FileError> {
        self.rescan_covering(place_id).await.map(|_| ())
    }

    /// `rescan_place`, saying whether the walk really covered the place: false when its folder is
    /// missing or the service stopped part way.
    pub(crate) async fn rescan_covering(&self, place_id: PlaceId) -> Result<bool, FileError> {
        let place = self.place_row(place_id).await?;
        let root = place.root.clone();
        let exists = blocking({
            let root = root.clone();
            move || root.is_dir()
        })
        .await?;
        if !exists {
            log::place_missing(place_id);
            return Ok(false);
        }
        let announce = place.index_state == "ready";
        let started = Instant::now();
        sqlx::query("UPDATE places SET index_state = 'building', index_indexed = 0 WHERE id = ?1")
            .bind(place_id.to_string())
            .execute(&self.pool)
            .await?;
        log::index_build_started(place_id);
        let existing = self.index_rows(place_id).await?;
        let stop = self.stopping.clone();
        let walk_root = root.clone();
        let (walked, skipped) = blocking(move || walk(&walk_root, &stop)).await?;
        if self.stopping.load(Ordering::SeqCst) {
            return Ok(false);
        }
        let mut seen: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(walked.len());
        let mut indexed: u64 = 0;
        let mut total: u64 = 0;
        for (n, entry) in walked.iter().enumerate() {
            if self.stopping.load(Ordering::SeqCst) {
                return Ok(false);
            }
            if n % 500 == 499 {
                tokio::task::yield_now().await;
            }
            let stored = if entry.is_folder {
                format!("{}/", entry.rel)
            } else {
                entry.rel.clone()
            };
            seen.insert(stored.clone());
            if entry.is_folder {
                if !existing.contains_key(&stored) {
                    self.index_folder(place_id, &entry.rel, entry.mtime_ns)
                        .await?;
                }
                continue;
            }
            total += 1;
            let known = existing.get(&stored);
            if known.is_some_and(|k| k.size == entry.size && k.mtime_ns == entry.mtime_ns) {
                indexed += 1;
            } else {
                let abs = root.join(&entry.rel);
                // A file that cannot be read or has just gone must not end the scan: a wide claim
                // waits on this rescan, and nothing can be published from a file that is unread.
                match self
                    .index_changed_file(&place, &entry.rel, &abs, known.is_none())
                    .await
                {
                    Ok(Some(change)) if announce => self.emit(change),
                    Ok(_) | Err(FileError::NotFound) => {}
                    Err(other) => return Err(other),
                }
                indexed += 1;
            }
            if indexed.is_multiple_of(500) {
                sqlx::query("UPDATE places SET index_indexed = ?2 WHERE id = ?1")
                    .bind(place_id.to_string())
                    .bind(i64::try_from(indexed).unwrap_or(i64::MAX))
                    .execute(&self.pool)
                    .await?;
            }
        }
        // Index rows whose file is gone.
        for (stored, row) in &existing {
            if seen.contains(stored) {
                continue;
            }
            if row.is_folder {
                self.remove_index_path(place_id, stored).await?;
            } else {
                self.remove_index_path(place_id, stored).await?;
                if announce {
                    self.emit(PlaceChange {
                        place_id,
                        path: stored.clone(),
                        change: PlaceChangeKind::Removed,
                        observed_at: Timestamp::now(),
                        by_ordin: false,
                        version: None,
                        origin: None,
                    });
                }
            }
        }
        sqlx::query(
            "UPDATE places SET index_state = 'ready', index_indexed = ?2, index_total = ?3, \
                               index_skipped = ?4 WHERE id = ?1",
        )
        .bind(place_id.to_string())
        .bind(i64::try_from(indexed).unwrap_or(i64::MAX))
        .bind(i64::try_from(total).unwrap_or(i64::MAX))
        .bind(i64::try_from(skipped).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await?;
        log::index_build_finished(
            place_id,
            indexed,
            total,
            skipped,
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        );
        Ok(true)
    }

    /// `indexed` of `total` files over the places (Internal Logic 8.4).
    pub async fn coverage(
        &self,
        place_ids: &[ordin_types::PlaceId],
    ) -> Result<IndexCoverage, FileError> {
        let mut indexed: u64 = 0;
        let mut total: u64 = 0;
        for id in place_ids {
            let row = sqlx::query("SELECT index_indexed, index_total FROM places WHERE id = ?1")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?;
            if let Some(row) = row {
                let i: i64 = row.try_get("index_indexed")?;
                let t: i64 = row.try_get("index_total")?;
                indexed += u64::try_from(i).unwrap_or(0);
                total += u64::try_from(t).unwrap_or(0);
            }
        }
        Ok(IndexCoverage { indexed, total })
    }
}

/// SHA-256 of a file without holding it in memory.
fn hash_streaming(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let digest: [u8; 32] = hasher.finalize().into();
    Some(ordin_types::num::Sha256Hex::from_digest(&digest).0)
}

pub(crate) struct Walked {
    pub(crate) rel: String,
    pub(crate) is_folder: bool,
    pub(crate) size: i64,
    pub(crate) mtime_ns: i64,
}

/// Breadth-first walk to `MAX_FOLDER_DEPTH`; symbolic links and this service's temporary files
/// are skipped; files beyond `MAX_PLACE_FILES` are counted, not returned.
pub(crate) fn walk(root: &Path, stop: &std::sync::atomic::AtomicBool) -> (Vec<Walked>, u64) {
    let mut out = Vec::new();
    let mut files: u64 = 0;
    let mut skipped: u64 = 0;
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));
    while let Some((dir, depth)) = queue.pop_front() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let abs = entry.path();
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if is_temp_name(name) {
                continue;
            }
            let Some(rel) = relative_to(root, &abs) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else { continue };
            if kind.is_dir() {
                if depth < MAX_FOLDER_DEPTH {
                    out.push(Walked {
                        rel,
                        is_folder: true,
                        size: 0,
                        mtime_ns: mtime_ns(&meta),
                    });
                    queue.push_back((abs, depth + 1));
                }
            } else if kind.is_file() && depth < MAX_FOLDER_DEPTH {
                if files >= MAX_PLACE_FILES {
                    skipped += 1;
                    continue;
                }
                files += 1;
                out.push(Walked {
                    rel,
                    is_folder: false,
                    size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
                    mtime_ns: mtime_ns(&meta),
                });
            }
        }
    }
    (out, skipped)
}
