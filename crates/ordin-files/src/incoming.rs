//! Import of dropped files, fetched working-copy bytes, and the incoming-file safety check
//! (FileService.md Internal Logic 6.10, 7.1, 11).

use std::path::{Path, PathBuf};

use ordin_types::domain::places::{FileVersionOrigin, PlaceKind};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::PlaceId;
use sqlx::Row;

use crate::error::FileError;
use crate::extract::{extract_text, has_hidden_text, holds_macros, is_program_header};
use crate::index::IndexWrite;
use crate::paths::{
    self, is_markdown_kind, is_office_zip_kind, is_text_kind, kind_of, name_of, normalise_file,
    normalise_relative,
};
use crate::service::{ts, FileService};
use crate::types::{FileEntry, SavedFile};
use crate::versions::{
    atomic_write, blocking, hash_of, is_unique_violation, stat_of, writable_kind, WriteArgs,
};
use crate::{INCOMING_REFUSED_EXTENSIONS, READ_LIMIT_BYTES};

const PROGRAM_REFUSED: &str = "This file can run as a program, so ordin didn't save it.";
const MACRO_REFUSED: &str = "This document holds macros, so ordin didn't save it.";
const NAME_REFUSED: &str = "The file's name can't be used.";

/// `name`, then `stem (2).ext`, `stem (3).ext` … up to `(999)` while `taken` says the name is used.
pub(crate) fn numbered_name(name: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    if !taken(name) {
        return Some(name.to_owned());
    }
    let path = Path::new(name);
    let stem = path.file_stem()?.to_str()?;
    let extension = path.extension().and_then(|e| e.to_str());
    for n in 2..=999 {
        let candidate = match extension {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        if !taken(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn clean_file_name(raw: &str) -> Result<String, FileError> {
    let refuse = || FileError::IncomingRefused {
        reason: NAME_REFUSED.to_owned(),
    };
    let last = raw.rsplit('/').next().unwrap_or_default().trim();
    if last.is_empty()
        || last.starts_with('.')
        || last.len() > 255
        || last.contains('\\')
        || last.chars().any(char::is_control)
    {
        return Err(refuse());
    }
    Ok(last.to_owned())
}

impl FileService {
    /// Copies dropped files into a place folder; returns the new entries in drop order.
    pub async fn import_files(
        &self,
        place_id: PlaceId,
        folder: &str,
        sources: Vec<PathBuf>,
    ) -> Result<Vec<FileEntry>, FileError> {
        let folder = normalise_relative(folder)?;
        let place = self.place_row(place_id).await?;
        if !writable_kind(place.kind) {
            return Err(FileError::ProtectedLocation);
        }
        // Every source is checked before the first is copied.
        for source in &sources {
            if self.rules.is_protected_location(source) {
                return Err(FileError::ProtectedLocation);
            }
            let probe = source.clone();
            let meta = blocking(move || std::fs::metadata(&probe).ok()).await?;
            match meta {
                None => return Err(FileError::NotFound),
                Some(meta) if meta.is_dir() => return Err(FileError::QueryInvalid),
                Some(_) => {}
            }
        }
        let target_dir = paths::confine(&place.root, &folder)?;
        let mut entries = Vec::with_capacity(sources.len());
        for source in sources {
            let Some(file_name) = source
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_owned)
            else {
                return Err(FileError::NotFound);
            };
            let dir = target_dir.clone();
            let (src, name) = (source.clone(), file_name.clone());
            let chosen = blocking(move || -> Result<String, FileError> {
                std::fs::create_dir_all(&dir).map_err(|_| FileError::WriteFailed)?;
                let chosen =
                    numbered_name(&name, |n| std::fs::symlink_metadata(dir.join(n)).is_ok())
                        .ok_or(FileError::WriteFailed)?;
                copy_atomic(&src, &dir.join(&chosen)).map_err(|_| FileError::WriteFailed)?;
                Ok(chosen)
            })
            .await??;
            let rel = if folder.is_empty() {
                chosen
            } else {
                format!("{folder}/{chosen}")
            };
            let abs = place.root.join(&rel);
            // The copy is `Created` with `by_ordin = false` (the person dropped it).
            if let Some(change) = self.index_changed_file(&place, &rel, &abs, true).await? {
                self.emit(change);
            }
            entries.push(self.entry_from_index(place_id, &rel).await?);
        }
        self.record(
            "files_imported",
            None,
            None,
            "Dropped files were copied into a folder.",
            serde_json::json!({ "place_id": place_id.to_string(), "count": entries.len() }),
        )
        .await;
        Ok(entries)
    }

    async fn entry_from_index(&self, place_id: PlaceId, rel: &str) -> Result<FileEntry, FileError> {
        let row = sqlx::query(
            "SELECT path, name, is_folder, kind, size_bytes, changed_at, changed_by \
             FROM file_index WHERE place_id = ?1 AND path = ?2",
        )
        .bind(place_id.to_string())
        .bind(rel)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(FileError::NotFound)?;
        let is_folder: i64 = row.try_get("is_folder")?;
        let size: i64 = row.try_get("size_bytes")?;
        let changed_at: String = row.try_get("changed_at")?;
        let path: String = row.try_get("path")?;
        let has_versions = self.latest(place_id, &path).await?.is_some();
        Ok(FileEntry {
            place_id,
            name: row.try_get("name")?,
            is_folder: is_folder != 0,
            kind: row.try_get("kind")?,
            size_bytes: u64::try_from(size).unwrap_or(0),
            changed_at: ts(&changed_at)?,
            changed_by: row.try_get("changed_by")?,
            has_versions,
            path,
        })
    }

    /// Writes bytes read from an origin into a `working_copy` place. No version row for a path
    /// without history and no `PlaceChange`: fetching is nobody's change. Returns the hash.
    // spec: ordin-desktop/components/FileService#SA-RECON-04
    pub async fn store_fetched(
        &self,
        place_id: PlaceId,
        path: &str,
        bytes: Vec<u8>,
        changed_by: Option<String>,
        changed_at: Timestamp,
    ) -> Result<Sha256Hex, FileError> {
        let place = self.place_row(place_id).await?;
        if place.kind != PlaceKind::WorkingCopy {
            return Err(FileError::ProtectedLocation);
        }
        let path = normalise_file(path)?;
        if bytes.len() as u64 > READ_LIMIT_BYTES {
            return Err(FileError::TooLarge {
                limit_bytes: READ_LIMIT_BYTES,
            });
        }
        let abs = paths::confine(&place.root, &path)?;
        let hash = hash_of(&bytes);
        // The watcher event that follows is dropped.
        self.mark_quiet(place_id, &path);
        let (target, data) = (abs.clone(), bytes.clone());
        blocking(move || atomic_write(&target, &data))
            .await?
            .map_err(|_| FileError::WriteFailed)?;
        let stat = stat_of(&abs).await?;
        let latest = self.latest(place_id, &path).await?;
        // FS-FETCH-1: the first fetch is version 1, as the working copy item counts it
        // (WorkspaceService.md `local_version: 1`; PushService.md reads that number).
        // spec: ordin-desktop/components/FileService#FS-FETCH-1
        let new_version = latest.as_ref().is_none_or(|l| l.hash.0 != hash);
        if new_version {
            self.store_copy(&hash, bytes.clone()).await?;
        }
        let kind = kind_of(name_of(&path));
        let (text, text_state) = self.extract_indexed(place_id, &kind, bytes).await;
        let mut tx = self.begin_write().await?;
        if new_version {
            crate::versions::insert_version_pub(
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
        }
        crate::index::upsert_tx(
            &mut tx,
            &IndexWrite {
                place_id,
                path: &path,
                is_folder: false,
                size: stat.size,
                mtime_ns: stat.mtime_ns,
                hash: Some(&hash),
                changed_at,
                changed_by: changed_by.as_deref(),
                text: text.as_deref(),
                text_state,
            },
            true,
        )
        .await?;
        tx.commit().await?;
        Ok(Sha256Hex(hash))
    }

    /// Runs the incoming-file safety check, then writes a new file in a `workspace_folder` place.
    /// Never overwrites: a taken name gets a number (Internal Logic 11).
    pub async fn save_incoming(
        &self,
        place_id: PlaceId,
        file_name: &str,
        bytes: Vec<u8>,
        origin_note: &str,
    ) -> Result<SavedFile, FileError> {
        // 11.1
        let name = clean_file_name(file_name)?;
        if bytes.len() as u64 > READ_LIMIT_BYTES {
            return Err(FileError::TooLarge {
                limit_bytes: READ_LIMIT_BYTES,
            });
        }
        let place = self.place_row(place_id).await?;
        if place.kind != PlaceKind::WorkspaceFolder {
            return Err(FileError::ProtectedLocation);
        }
        // 11.2
        let kind = kind_of(&name);
        let program =
            INCOMING_REFUSED_EXTENSIONS.contains(&kind.as_str()) || is_program_header(&bytes);
        if program {
            return Err(FileError::IncomingRefused {
                reason: PROGRAM_REFUSED.to_owned(),
            });
        }
        if is_office_zip_kind(&kind) && holds_macros(&bytes) {
            return Err(FileError::IncomingRefused {
                reason: MACRO_REFUSED.to_owned(),
            });
        }
        // 11.3
        let hidden_content_found = if is_markdown_kind(&kind) || is_text_kind(&kind) {
            has_hidden_text(&String::from_utf8_lossy(&bytes))
        } else if is_office_zip_kind(&kind) {
            matches!(extract_text(&kind, &bytes), Ok(Some(text)) if has_hidden_text(&text))
        } else {
            false
        };
        // 11.4
        let root = place.root.clone();
        let chosen = blocking(move || {
            numbered_name(&name, |n| std::fs::symlink_metadata(root.join(n)).is_ok())
        })
        .await?
        .ok_or(FileError::WriteFailed)?;
        // 11.5
        let size = bytes.len();
        let (version, _) = self
            .write_inner(WriteArgs {
                place_id,
                path: chosen.clone(),
                bytes,
                origin: FileVersionOrigin::Person,
                expected_hash: None,
                by_ordin: true,
            })
            .await?;
        let note: String = origin_note.chars().take(200).collect();
        self.record(
            "file_received",
            None,
            None,
            "A document was received and kept.",
            serde_json::json!({
                "place_id": place_id.to_string(),
                "path": version.path,
                "bytes": size,
                "hidden_content_found": hidden_content_found,
                "origin_note": note,
            }),
        )
        .await;
        Ok(SavedFile {
            place_id,
            path: chosen,
            hidden_content_found,
        })
    }
}

/// Copies to a temporary file in the target folder, then renames over the name.
fn copy_atomic(source: &Path, target: &Path) -> std::io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent folder"))?;
    let temp = parent.join(format!("{}{}", paths::TEMP_PREFIX, uuid::Uuid::now_v7()));
    let result = std::fs::copy(source, &temp).and_then(|_| {
        // Opened for writing: Windows refuses to flush a handle opened only for reading.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&temp)?
            .sync_all()?;
        std::fs::rename(&temp, target)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_numbered_before_the_extension() {
        let taken = |n: &str| n == "quote.pdf" || n == "quote (2).pdf";
        assert_eq!(numbered_name("quote.pdf", taken).unwrap(), "quote (3).pdf");
        assert_eq!(numbered_name("new.pdf", taken).unwrap(), "new.pdf");
        assert_eq!(
            numbered_name("README", |n| n == "README").unwrap(),
            "README (2)"
        );
        assert!(numbered_name("a.txt", |_| true).is_none());
    }

    #[test]
    fn file_names_are_cleaned_or_refused() {
        assert_eq!(clean_file_name(" dir/quote.pdf ").unwrap(), "quote.pdf");
        for bad in [
            "",
            ".hidden",
            "a\\b.txt",
            "bad\u{7}name",
            "dir/",
            &"x".repeat(256),
        ] {
            assert_eq!(
                clean_file_name(bad).unwrap_err().code(),
                "FILE_INCOMING_REFUSED",
                "{bad:?}"
            );
        }
    }
}
