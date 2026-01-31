//! Moving a file inside a place, keeping its history (FileService.md, Amendment January 2026 —
//! Moving a file, ADR-104; Internal Logic 15 and 5a).

use std::path::Path;

use ordin_types::domain::places::{FileVersion, PlaceKind};
use ordin_types::time::Timestamp;
use ordin_types::PlaceId;

use crate::error::FileError;
use crate::log;
use crate::paths::{self, kind_of, name_of, normalise_file, parent_of};
use crate::service::FileService;
use crate::types::{PlaceChange, PlaceChangeKind};
use crate::versions::{blocking, is_unique_violation};

/// The outcome of carrying a path's history to a new path (Internal Logic 15.5).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Carry {
    Moved,
    /// Versions or a held edit already exist at the new path; nothing was changed.
    Collision,
}

impl FileService {
    /// In one transaction: the version rows, the index row and its full-text row, any held edit and
    /// the published copies' original key follow the file from `from` to `to` (Internal Logic 15.5).
    pub(crate) async fn carry_history(
        &self,
        place_id: PlaceId,
        from: &str,
        to: &str,
    ) -> Result<Carry, FileError> {
        let place = place_id.to_string();
        let mut tx = self.begin_write().await?;
        let step = async {
            sqlx::query("UPDATE file_versions SET path = ?3 WHERE place_id = ?1 AND path = ?2")
                .bind(&place)
                .bind(from)
                .bind(to)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE held_edits SET path = ?3 WHERE place_id = ?1 AND path = ?2")
                .bind(&place)
                .bind(from)
                .bind(to)
                .execute(&mut *tx)
                .await?;
            let old_key = format!("place:{place}:{from}");
            let new_key = format!("place:{place}:{to}");
            sqlx::query(
                "UPDATE published_copies SET original_key = ?2, \
                     original = json_set(original, '$.path', ?3) WHERE original_key = ?1",
            )
            .bind(&old_key)
            .bind(&new_key)
            .bind(to)
            .execute(&mut *tx)
            .await?;
            // A stale row at the new path (indexed before its file went) makes way.
            sqlx::query("DELETE FROM file_index_fts WHERE place_id = ?1 AND path = ?2")
                .bind(&place)
                .bind(to)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM file_index WHERE place_id = ?1 AND path = ?2")
                .bind(&place)
                .bind(to)
                .execute(&mut *tx)
                .await?;
            let name = name_of(to).to_owned();
            sqlx::query(
                "UPDATE file_index SET path = ?3, parent = ?4, name = ?5, kind = ?6 \
                 WHERE place_id = ?1 AND path = ?2",
            )
            .bind(&place)
            .bind(from)
            .bind(to)
            .bind(parent_of(to))
            .bind(&name)
            .bind(kind_of(&name))
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE file_index_fts SET path = ?3 WHERE place_id = ?1 AND path = ?2")
                .bind(&place)
                .bind(from)
                .bind(to)
                .execute(&mut *tx)
                .await?;
            Ok::<(), sqlx::Error>(())
        }
        .await;
        match step {
            Ok(()) => {
                tx.commit().await?;
                Ok(Carry::Moved)
            }
            Err(e) if is_unique_violation(&e) => {
                // Dropping the transaction rolls it back.
                drop(tx);
                Ok(Carry::Collision)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Moves `from` to `to` inside `place_id` (places of kind `workspace_folder` only); the version
    /// rows, the index row and any held edit follow the file. Returns the latest version, now at `to`.
    pub async fn move_file(
        &self,
        place_id: PlaceId,
        from: &str,
        to: &str,
    ) -> Result<FileVersion, FileError> {
        // 15.1
        let from = normalise_file(from)?;
        let to = normalise_file(to)?;
        let place = self.place_row(place_id).await?;
        if place.kind != PlaceKind::WorkspaceFolder {
            return Err(FileError::ProtectedLocation);
        }
        let abs_from = paths::confine(&place.root, &from)?;
        // The last name is kept as given: a volume that ignores letter case would otherwise hand
        // back the old spelling of a file that exists under another case.
        let abs_to = paths::confine(&place.root, paths::parent_of(&to))?.join(name_of(&to));
        let source = abs_from.clone();
        let present = blocking(move || source.is_file()).await?;
        if !present {
            return Err(FileError::NotFound);
        }
        // 15.3: the history starts before the move.
        if self.latest(place_id, &from).await?.is_none() {
            if let Some(bytes) = self.read_limited(&abs_from).await? {
                self.baseline_pub(place_id, &from, bytes).await?;
            }
        }
        if from == to {
            return self
                .latest(place_id, &from)
                .await?
                .ok_or(FileError::NotFound);
        }
        // 15.2: nothing is overwritten, and versions already kept at `to` are not mixed in.
        let (a, b) = (abs_from.clone(), abs_to.clone());
        let (target_exists, same_file) = blocking(move || {
            let exists = std::fs::symlink_metadata(&b).is_ok();
            (exists, exists && same_file(&a, &b))
        })
        .await?;
        if target_exists && !same_file {
            return Err(FileError::MoveTargetExists);
        }
        if self.latest(place_id, &to).await?.is_some() && !same_file {
            return Err(FileError::MoveTargetExists);
        }
        // 15.4: rename; a change of letter case only goes through a temporary name.
        self.mark_quiet(place_id, &from);
        self.mark_quiet(place_id, &to);
        let (a, b) = (abs_from.clone(), abs_to.clone());
        blocking(move || rename_into_place(&a, &b, same_file))
            .await?
            .map_err(|()| FileError::WriteFailed)?;
        // 15.5
        match self.carry_history(place_id, &from, &to).await {
            Ok(Carry::Moved) => {}
            Ok(Carry::Collision) => {
                self.undo_rename(&abs_from, &abs_to).await;
                return Err(FileError::MoveTargetExists);
            }
            Err(e) => {
                self.undo_rename(&abs_from, &abs_to).await;
                return Err(e);
            }
        }
        let latest = self
            .latest(place_id, &to)
            .await?
            .ok_or(FileError::NotFound)?;
        let count = self.versions(place_id, &to).await?.len();
        // The watcher's rename pair is dropped; the move is announced here once.
        self.emit(PlaceChange {
            place_id,
            path: to.clone(),
            change: PlaceChangeKind::Renamed { from: from.clone() },
            observed_at: Timestamp::now(),
            by_ordin: true,
            version: Some(latest.version),
            origin: Some(latest.origin.clone()),
        });
        // 15.6
        self.record(
            "file_moved",
            None,
            None,
            "A document was moved inside its folder.",
            serde_json::json!({ "place_id": place_id.to_string(), "versions": count }),
        )
        .await;
        log::file_moved(place_id);
        Ok(latest)
    }

    async fn undo_rename(&self, from: &Path, to: &Path) {
        let (a, b) = (from.to_path_buf(), to.to_path_buf());
        let _ = blocking(move || std::fs::rename(&b, &a)).await;
    }
}

/// True when both paths name the same file (a case-only rename on a case-insensitive volume).
#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

fn rename_into_place(from: &Path, to: &Path, case_only: bool) -> Result<(), ()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ())?;
    }
    if case_only {
        let folder = from.parent().ok_or(())?;
        let temp = folder.join(format!("{}{}", paths::TEMP_PREFIX, uuid::Uuid::now_v7()));
        std::fs::rename(from, &temp).map_err(|_| ())?;
        if std::fs::rename(&temp, to).is_err() {
            let _ = std::fs::rename(&temp, from);
            return Err(());
        }
        Ok(())
    } else {
        std::fs::rename(from, to).map_err(|_| ())
    }
}
