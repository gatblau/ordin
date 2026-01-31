//! Change review decisions (ADR-053, 11 §5.11; Internal Logic 11).

use std::path::{Path, PathBuf};

use ordin_files::{FileBytes, FileError, WriteVersion};
use ordin_types::domain::{ChangeEntryState, CodingTaskState, FileVersionOrigin};
use ordin_types::num::Sha256Hex;
use ordin_types::ChangeSetEntryId;

use crate::error::{file_failed, ProjectError};
use crate::git;
use crate::hunks;
use crate::service::{lock, ProjectService};
use crate::store::{self, EntryRow, TaskRow};
use crate::types::{DecisionOutcome, EntryDecision};

/// The engineer's working copy of a file: its bytes and hash, or nothing when it is not there.
struct Ours {
    bytes: Vec<u8>,
    hash: Option<Sha256Hex>,
}

/// What the agent's side of an entry holds.
enum Theirs {
    /// A whole file that cannot be merged line by line.
    Binary(Vec<u8>),
    Text {
        ancestor: Vec<u8>,
        theirs: Vec<u8>,
    },
}

/// An entry that stands for a whole file and holds none of its content.
pub(crate) fn is_whole_file(hunk: &str) -> bool {
    hunk == crate::BINARY_HUNK || hunk == crate::TOO_LARGE_HUNK
}

fn decision_word(decision: &EntryDecision) -> &'static str {
    match decision {
        EntryDecision::Accept => "accept",
        EntryDecision::Reject => "reject",
        EntryDecision::KeepYours => "keep_yours",
        EntryDecision::UseTheAgents => "use_the_agents",
    }
}

impl ProjectService {
    /// Internal Logic 11.1 to 11.4.
    pub async fn decide_entry(
        &self,
        entry: ChangeSetEntryId,
        decision: EntryDecision,
    ) -> Result<DecisionOutcome, ProjectError> {
        let row = store::entry(&self.pool, entry)
            .await?
            .ok_or(ProjectError::EntryNotFound)?;
        if row.entry.state != ChangeEntryState::Pending {
            return Err(ProjectError::DecisionInvalid);
        }
        let task = self.task_row(row.coding_task_id).await?;
        if task.task.state != CodingTaskState::ReadyForReview {
            return Err(ProjectError::TaskStateInvalid);
        }
        let outcome = match decision {
            EntryDecision::Reject | EntryDecision::KeepYours => {
                store::set_entry_state(&self.pool, entry, ChangeEntryState::Rejected).await?;
                DecisionOutcome::Dropped
            }
            EntryDecision::Accept => self.apply_entry(&row, &task, false).await?,
            EntryDecision::UseTheAgents => self.apply_entry(&row, &task, true).await?,
        };
        lock(&self.marked_entries).remove(&entry);
        crate::log::entry_decided(
            entry,
            decision_word(&decision),
            match outcome {
                DecisionOutcome::Applied { .. } => "applied",
                DecisionOutcome::Dropped => "dropped",
                DecisionOutcome::Conflict { .. } => "conflict",
            },
        );
        self.settle_when_decided(&row, &task).await?;
        Ok(outcome)
    }

    /// Internal Logic 11.4: with no entry pending, the change set and the task settle.
    async fn settle_when_decided(
        &self,
        row: &EntryRow,
        task: &TaskRow,
    ) -> Result<(), ProjectError> {
        if store::pending_in_set(&self.pool, row.change_set_id).await? > 0 {
            return Ok(());
        }
        store::settle_change_set(&self.pool, row.change_set_id).await?;
        store::set_task_state(&self.pool, task.task.id, CodingTaskState::Settled).await?;
        self.clean_up(task).await
    }

    async fn working_copy(
        &self,
        place: ordin_types::PlaceId,
        file: &str,
    ) -> Result<Ours, ProjectError> {
        match self.files.read_file(place, file).await {
            Ok(found) => Ok(Ours {
                bytes: found.bytes,
                hash: Some(found.hash),
            }),
            Err(FileError::NotFound) => Ok(Ours {
                bytes: Vec::new(),
                hash: None,
            }),
            Err(error) => Err(file_failed(error)),
        }
    }

    /// A whole-file entry's file in the worktree, read through the File Service so the read cannot
    /// leave the worktree. A symbolic link on the way is refused too: the File Service would follow
    /// one that stays inside, to bytes that are not the entry's file.
    pub(crate) async fn whole_file(
        &self,
        task: &TaskRow,
        file: &str,
    ) -> Result<FileBytes, ProjectError> {
        let (worktree, name) = (PathBuf::from(&task.task.worktree_path), file.to_owned());
        let plain = git::blocking(move || Ok(git::plain_file_in(&worktree, &name))).await?;
        if !plain {
            return Err(ProjectError::DecisionInvalid);
        }
        self.files
            .read_file(task.worktree_place_id, file)
            .await
            .map_err(|error| match error {
                FileError::NotFound
                | FileError::PathEscapesPlace
                | FileError::TooLarge { .. }
                | FileError::Invalid(_) => ProjectError::DecisionInvalid,
                other => file_failed(other),
            })
    }

    /// The bytes of a whole-file entry, only when they are the bytes captured with the change set.
    /// One read gives both the hash compared and the bytes applied, so nothing can change between.
    // spec: ordin-desktop/components/ProjectService#ADR-053
    async fn captured_bytes(
        &self,
        row: &EntryRow,
        task: &TaskRow,
    ) -> Result<Vec<u8>, ProjectError> {
        let found = self.whole_file(task, &row.entry.file).await?;
        if row.content_hash.as_deref() == Some(found.hash.0.as_str()) {
            Ok(found.bytes)
        } else {
            Err(ProjectError::DecisionInvalid)
        }
    }

    /// The ancestor and the agent's version of the entry's file.
    async fn agent_side(
        &self,
        folder: PathBuf,
        row: &EntryRow,
        task: &TaskRow,
    ) -> Result<Theirs, ProjectError> {
        let file = row.entry.file.clone();
        let (base, hunk) = (task.task.base_commit.clone(), row.entry.hunk.clone());
        let whole = if is_whole_file(&hunk) {
            Some(self.captured_bytes(row, task).await?)
        } else {
            None
        };
        git::blocking(move || {
            let ancestor = git::optional_file_at(&folder, &base, &file)?.unwrap_or_default();
            match (hunk.as_str(), whole) {
                (crate::BINARY_HUNK, Some(theirs)) => Ok(Theirs::Binary(theirs)),
                (_, Some(theirs)) => Ok(Theirs::Text { ancestor, theirs }),
                (text, None) => {
                    let theirs =
                        hunks::apply_hunk(&ancestor, text).ok_or(ProjectError::DecisionInvalid)?;
                    Ok(Theirs::Text { ancestor, theirs })
                }
            }
        })
        .await
    }

    /// Internal Logic 11.2 and 11.3. Never applied over the engineer's change on its own (ADR-052).
    // spec: ordin-desktop/components/ProjectService#ADR-052
    // spec: ordin-desktop/components/ProjectService#ADR-053
    async fn apply_entry(
        &self,
        row: &EntryRow,
        task: &TaskRow,
        use_the_agents: bool,
    ) -> Result<DecisionOutcome, ProjectError> {
        let project = store::project(&self.pool, task.task.project_id)
            .await?
            .ok_or(ProjectError::NotFound)?;
        let place = project.project.place_id;
        let folder = self.place_folder(place).await?;
        let file = &row.entry.file;
        let ours = self.working_copy(place, file).await?;
        let content = match self.agent_side(folder.clone(), row, task).await? {
            Theirs::Binary(theirs) => {
                let base = task.task.base_commit.clone();
                let (probe, name) = (folder, file.clone());
                let ancestor =
                    git::blocking(move || git::optional_file_at(Path::new(&probe), &base, &name))
                        .await?
                        .unwrap_or_default();
                if !use_the_agents && ours.bytes != ancestor {
                    return Ok(DecisionOutcome::Conflict {
                        yours: format!("{} bytes", ours.bytes.len()),
                        agents: format!("{} bytes", theirs.len()),
                    });
                }
                theirs
            }
            Theirs::Text { ancestor, theirs } => {
                let merged = git::merge(&ancestor, &ours.bytes, &theirs, use_the_agents)?;
                if !merged.clean && !use_the_agents {
                    let (yours, agents) = hunks::conflict_regions(&merged.content);
                    return Ok(DecisionOutcome::Conflict { yours, agents });
                }
                merged.content
            }
        };
        // An entry that deletes a whole file leaves an empty file, because the one step here is
        // `files.write_version` and it states no removal.
        let version = self
            .files
            .write_version(WriteVersion {
                place_id: place,
                path: file.clone(),
                bytes: content,
                origin: FileVersionOrigin::Run {
                    run_id: row.run_id,
                    tool: "change_review.accept".to_owned(),
                },
                expected_hash: ours.hash,
            })
            .await
            .map_err(file_failed)?;
        store::set_entry_state(&self.pool, row.entry.id, ChangeEntryState::Accepted).await?;
        Ok(DecisionOutcome::Applied {
            file_version_id: version.id,
        })
    }
}
