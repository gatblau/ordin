//! Listing, reading and attribution (FileService.md Internal Logic 4.1–4.4).

use std::collections::{HashMap, HashSet};

use ordin_types::paging::{Cursor, Page, PAGE_SIZE};
use ordin_types::time::Timestamp;
use ordin_types::PlaceId;
use sqlx::Row;

use crate::error::FileError;
use crate::paths::{self, kind_of, media_type, name_of, normalise_file, normalise_relative};
use crate::service::{ts, FileService, PlaceRow};
use crate::types::{FileBytes, FileEntry, FolderSort};
use crate::versions::{blocking, hash_of, mtime_ns};

/// The three-part keyset of one row: two sort values and the path.
type Key = (String, String, String);

/// `(first, second)` SQL expressions of the sort key, and whether the order is descending.
fn sort_parts(sort: FolderSort) -> (&'static str, &'static str, bool) {
    match sort {
        FolderSort::NameAsc => ("name", "''", false),
        FolderSort::NameDesc => ("name", "''", true),
        FolderSort::KindAsc => ("kind", "name", false),
        FolderSort::ChangedDesc => ("changed_at", "''", true),
        FolderSort::ChangedAsc => ("changed_at", "''", false),
        FolderSort::ChangedByAsc => ("COALESCE(changed_by, '')", "name", false),
    }
}

fn key_of(sort: FolderSort, entry: &FileEntry) -> Key {
    let (k1, k2) = match sort {
        FolderSort::NameAsc | FolderSort::NameDesc => (entry.name.clone(), String::new()),
        FolderSort::KindAsc => (entry.kind.clone(), entry.name.clone()),
        FolderSort::ChangedDesc | FolderSort::ChangedAsc => {
            (entry.changed_at.to_wire(), String::new())
        }
        FolderSort::ChangedByAsc => (
            entry.changed_by.clone().unwrap_or_default(),
            entry.name.clone(),
        ),
    };
    (k1, k2, entry.path.clone())
}

impl FileService {
    // spec: ordin-desktop/components/FileService#SA-PAGE-01
    pub async fn list_folder(
        &self,
        place_id: PlaceId,
        folder: &str,
        sort: FolderSort,
        cursor: Option<Cursor>,
    ) -> Result<Page<FileEntry>, FileError> {
        let folder = normalise_relative(folder)?;
        let place = self.place_row(place_id).await?;
        let after: Option<Key> = match cursor {
            Some(cursor) => Some(cursor.decode::<Key>()?),
            None => None,
        };
        let abs = paths::confine(&place.root, &folder)?;
        let is_dir = blocking(move || abs.is_dir()).await?;
        if !is_dir {
            return Err(FileError::NotFound);
        }
        let mut rows = if place.index_state == "ready" {
            self.list_from_index(&place, &folder, sort, after.as_ref())
                .await?
        } else {
            self.list_from_disk(&place, &folder, sort, after.as_ref())
                .await?
        };
        // The 101st row only sets `next_cursor`.
        let next_cursor = if rows.len() > PAGE_SIZE {
            rows.truncate(PAGE_SIZE);
            rows.last().map(|last| Cursor::encode(&key_of(sort, last)))
        } else {
            None
        };
        self.mark_versions(place_id, &mut rows).await?;
        Ok(Page {
            items: rows,
            next_cursor,
        })
    }

    async fn list_from_index(
        &self,
        place: &PlaceRow,
        folder: &str,
        sort: FolderSort,
        after: Option<&Key>,
    ) -> Result<Vec<FileEntry>, FileError> {
        let (k1, k2, descending) = sort_parts(sort);
        let (cmp, order) = if descending {
            ("<", "DESC")
        } else {
            (">", "ASC")
        };
        let cursor_clause = if after.is_some() {
            format!("AND ({k1}, {k2}, path) {cmp} (?3, ?4, ?5)")
        } else {
            String::new()
        };
        let sql = format!(
            "SELECT path, name, is_folder, kind, size_bytes, changed_at, changed_by \
             FROM file_index WHERE place_id = ?1 AND parent = ?2 {cursor_clause} \
             ORDER BY {k1} {order}, {k2} {order}, path {order} LIMIT {limit}",
            limit = PAGE_SIZE + 1
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(place.id.to_string())
            .bind(folder);
        if let Some((a, b, c)) = after {
            query = query.bind(a).bind(b).bind(c);
        }
        let rows = query.fetch_all(&self.pool).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let is_folder: i64 = row.try_get("is_folder")?;
            let size: i64 = row.try_get("size_bytes")?;
            let changed_at: String = row.try_get("changed_at")?;
            out.push(FileEntry {
                place_id: place.id,
                path: row.try_get("path")?,
                name: row.try_get("name")?,
                is_folder: is_folder != 0,
                kind: row.try_get("kind")?,
                size_bytes: u64::try_from(size).unwrap_or(0),
                changed_at: ts(&changed_at)?,
                changed_by: row.try_get("changed_by")?,
                has_versions: false,
            });
        }
        Ok(out)
    }

    /// For a place whose index is still being built: read the folder, sort in memory, page the same way.
    async fn list_from_disk(
        &self,
        place: &PlaceRow,
        folder: &str,
        sort: FolderSort,
        after: Option<&Key>,
    ) -> Result<Vec<FileEntry>, FileError> {
        let dir = place.root.join(folder);
        let listed = blocking(
            move || -> Result<Vec<(String, bool, u64, i64)>, FileError> {
                let mut out = Vec::new();
                for entry in std::fs::read_dir(&dir)
                    .map_err(|_| FileError::FolderUnreadable)?
                    .flatten()
                {
                    let Ok(kind) = entry.file_type() else {
                        continue;
                    };
                    if kind.is_symlink() {
                        continue;
                    }
                    let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    if paths::is_temp_name(&name) {
                        continue;
                    }
                    let Ok(meta) = entry.metadata() else { continue };
                    out.push((name, kind.is_dir(), meta.len(), mtime_ns(&meta)));
                }
                Ok(out)
            },
        )
        .await??;
        let authors = self.authors_in(place.id, folder).await?;
        let mut entries: Vec<FileEntry> = Vec::with_capacity(listed.len());
        for (name, is_folder, size, mtime) in listed {
            let path = join_entry(folder, &name, is_folder);
            let changed_at =
                Timestamp::from_unix_millis(mtime / 1_000_000).unwrap_or_else(|_| Timestamp::now());
            entries.push(FileEntry {
                place_id: place.id,
                kind: if is_folder {
                    String::new()
                } else {
                    kind_of(&name)
                },
                changed_by: authors.get(&path).cloned(),
                path,
                name,
                is_folder,
                size_bytes: if is_folder { 0 } else { size },
                changed_at,
                has_versions: false,
            });
        }
        let (_, _, descending) = sort_parts(sort);
        entries.sort_by_key(|a| key_of(sort, a));
        if descending {
            entries.reverse();
        }
        if let Some(after) = after {
            entries.retain(|e| {
                let key = key_of(sort, e);
                if descending {
                    key < *after
                } else {
                    key > *after
                }
            });
        }
        entries.truncate(PAGE_SIZE + 1);
        Ok(entries)
    }

    /// `changed_by` of the indexed rows of a folder, by stored path.
    async fn authors_in(
        &self,
        place_id: PlaceId,
        folder: &str,
    ) -> Result<HashMap<String, String>, FileError> {
        let rows = sqlx::query(
            "SELECT path, changed_by FROM file_index \
             WHERE place_id = ?1 AND parent = ?2 AND changed_by IS NOT NULL",
        )
        .bind(place_id.to_string())
        .bind(folder)
        .fetch_all(&self.pool)
        .await?;
        let mut out = HashMap::new();
        for row in &rows {
            out.insert(row.try_get("path")?, row.try_get("changed_by")?);
        }
        Ok(out)
    }

    async fn mark_versions(
        &self,
        place_id: PlaceId,
        rows: &mut [FileEntry],
    ) -> Result<(), FileError> {
        let files: Vec<&str> = rows
            .iter()
            .filter(|e| !e.is_folder)
            .map(|e| e.path.as_str())
            .collect();
        if files.is_empty() {
            return Ok(());
        }
        let marks = vec!["?"; files.len()].join(",");
        let sql = format!(
            "SELECT DISTINCT path FROM file_versions WHERE place_id = ?1 AND path IN ({marks})"
        );
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(place_id.to_string());
        for path in &files {
            query = query.bind(*path);
        }
        let found = query.fetch_all(&self.pool).await?;
        let mut kept: HashSet<String> = HashSet::new();
        for row in &found {
            kept.insert(row.try_get("path")?);
        }
        for entry in rows.iter_mut() {
            entry.has_versions = kept.contains(&entry.path);
        }
        Ok(())
    }

    pub async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError> {
        let path = normalise_file(path)?;
        let place = self.place_row(place_id).await?;
        let abs = paths::confine(&place.root, &path)?;
        let bytes = self.read_limited(&abs).await?.ok_or(FileError::NotFound)?;
        let hash = hash_of(&bytes);
        let media = media_type(&kind_of(name_of(&path))).to_owned();
        Ok(FileBytes {
            place_id,
            path,
            bytes,
            hash: ordin_types::num::Sha256Hex(hash),
            media_type: media,
        })
    }

    /// Records who changed a file at its origin, for the "changed by" column.
    pub async fn set_attribution(
        &self,
        place_id: PlaceId,
        path: &str,
        changed_by: String,
    ) -> Result<(), FileError> {
        let path = normalise_file(path)?;
        let place = self.place_row(place_id).await?;
        let abs = paths::confine(&place.root, &path)?;
        let present = blocking({
            let abs = abs.clone();
            move || abs.is_file()
        })
        .await?;
        if !present {
            return Err(FileError::NotFound);
        }
        let update = || async {
            sqlx::query("UPDATE file_index SET changed_by = ?3 WHERE place_id = ?1 AND path = ?2")
                .bind(place_id.to_string())
                .bind(&path)
                .bind(&changed_by)
                .execute(&self.pool)
                .await
        };
        if update().await?.rows_affected() == 0 {
            // Not indexed yet: index the file quietly, then record the author.
            self.index_changed_file(&place, &path, &abs, true).await?;
            update().await?;
        }
        Ok(())
    }

    /// "<friendly name> › <folder> › <file>" for the person; no absolute path.
    pub async fn display_path(&self, place_id: PlaceId, path: &str) -> Result<String, FileError> {
        let path = normalise_relative(path)?;
        let place = self.place_row(place_id).await?;
        let mut parts = vec![place.friendly_name];
        parts.extend(path.split('/').filter(|s| !s.is_empty()).map(str::to_owned));
        Ok(parts.join(" › "))
    }
}

fn join_entry(folder: &str, name: &str, is_folder: bool) -> String {
    let mut path = if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    };
    if is_folder {
        path.push('/');
    }
    path
}
