//! Coding tasks and worktrees (ADR-053; Internal Logic 5).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ordin_files::AddPlace;
use ordin_types::domain::{
    ChangeEntryState, ChangeSet, ChangeSetEntry, ChangeSetState, CodingTask, CodingTaskState,
    PlaceKind, RunOrigin, TestOutcome,
};
use ordin_types::time::Timestamp;
use ordin_types::{ChangeSetEntryId, ChangeSetId, CodingTaskId, RunId};
use serde_json::json;

use crate::error::{file_failed, ProjectError};
use crate::git;
use crate::service::ProjectService;
use crate::store::{self, TaskRow};
use crate::types::{ReviewTask, StartCodingTask, StartedTask};

/// `[a-z0-9-]{1,100}` of `CodingTask.branch`.
const MAX_SLUG_CHARS: usize = 100;

/// Lower-case, other characters to `-`, collapsed (Internal Logic 5.1).
pub(crate) fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(MAX_SLUG_CHARS);
    out
}

/// `ordin/` + `[a-z0-9-]{1,100}`, a valid git ref (2C).
pub(crate) fn branch_valid(branch: &str) -> bool {
    branch.strip_prefix("ordin/").is_some_and(|slug| {
        (1..=MAX_SLUG_CHARS).contains(&slug.len())
            && slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }) && git::valid_branch_name(branch)
}

impl ProjectService {
    pub(crate) async fn task_row(&self, id: CodingTaskId) -> Result<TaskRow, ProjectError> {
        store::task(&self.pool, id)
            .await?
            .ok_or(ProjectError::TaskNotFound)
    }

    fn worktree_path(&self, project: ordin_types::ProjectId, task: CodingTaskId) -> PathBuf {
        self.data_root()
            .join("worktrees")
            .join(project.to_string())
            .join(task.to_string())
    }

    /// The first free branch name: `ordin/<slug>`, then `-2`, `-3` … (Internal Logic 5.1).
    async fn free_branch(&self, folder: &Path, slug: &str) -> Result<String, ProjectError> {
        let mut suffix = 1u32;
        loop {
            let branch = if suffix == 1 {
                format!("ordin/{slug}")
            } else {
                format!("ordin/{slug}-{suffix}")
            };
            if !branch_valid(&branch) {
                return Err(ProjectError::Invalid("CODE_BRANCH_INVALID"));
            }
            let (probe, name) = (folder.to_path_buf(), branch.clone());
            if !git::blocking(move || git::branch_exists(&probe, &name)).await? {
                return Ok(branch);
            }
            suffix += 1;
        }
    }

    /// Internal Logic 5.1 to 5.5.
    // spec: ordin-desktop/components/ProjectService#ADR-053
    // spec: ordin-desktop/components/ProjectService#SA-RECON-08
    pub async fn start_coding_task(
        &self,
        req: StartCodingTask,
    ) -> Result<StartedTask, ProjectError> {
        let (row, folder) = self.repository_folder(req.project_id).await?;
        let chars = req.request.chars().count();
        if !(1..=crate::MAX_REQUEST_CHARS).contains(&chars) {
            return Err(ProjectError::Invalid("CODE_REQUEST_INVALID"));
        }
        let branch = self.free_branch(&folder, &slug(&req.task_name)).await?;
        if let Some(group) = req.attempt_group_id {
            let started = store::group_tasks(&self.pool, group).await?.len();
            if u32::try_from(started).unwrap_or(u32::MAX) >= self.current_settings().max_attempts {
                return Err(ProjectError::TooManyAttempts);
            }
        }
        let probe = folder.clone();
        let (base_branch, base_commit) = git::blocking(move || git::base_of(&probe)).await?;
        let id = CodingTaskId::new();
        let worktree = self.worktree_path(req.project_id, id);
        if !git::under_root(&worktree, self.data_root()) {
            return Err(ProjectError::Invalid("CODE_WORKTREE_OUTSIDE_ROOT"));
        }
        let worktree_text = worktree
            .to_str()
            .ok_or(ProjectError::Invalid("CODE_WORKTREE_OUTSIDE_ROOT"))?
            .to_owned();
        let (source, name, target, commit, path) = (
            folder.clone(),
            branch.clone(),
            id.to_string(),
            base_commit.clone(),
            worktree.clone(),
        );
        git::blocking(move || {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|_| ProjectError::WorktreeFailed)?;
            }
            git::add_worktree(&source, &target, &name, &commit, &path)
        })
        .await?;
        let place = match self
            .files
            .add_place(AddPlace {
                path: worktree.clone(),
                friendly_name: Some(branch.clone()),
                kind: PlaceKind::CodingTaskWorktree,
            })
            .await
        {
            Ok(place) => place,
            Err(error) => {
                self.drop_worktree(&folder, id, &worktree, Some(&branch))
                    .await?;
                return Err(file_failed(error));
            }
        };
        let task = CodingTask {
            id,
            origin: req.origin,
            project_id: req.project_id,
            agent_instance_id: req.agent_instance_id,
            request: req.request,
            worktree_path: worktree_text,
            branch: branch.clone(),
            base_branch,
            base_commit,
            attempt_group_id: req.attempt_group_id,
            test_outcome: None,
            new_problems: 0,
            problems_unchecked: false,
            state: CodingTaskState::Working,
        };
        store::insert_task(&self.pool, &task, place.id).await?;
        crate::log::coding_task_started(id);
        self.record(
            "coding_task_started",
            None,
            Some(row.workspace_id),
            "A coding task started in its own worktree.",
            json!({
                "task_id": id.to_string(),
                "project_id": req.project_id.to_string(),
                "branch": branch,
            }),
        )
        .await;
        Ok(StartedTask {
            task,
            worktree_place_id: place.id,
        })
    }

    async fn drop_worktree(
        &self,
        folder: &Path,
        task: CodingTaskId,
        worktree: &Path,
        branch: Option<&str>,
    ) -> Result<(), ProjectError> {
        let (source, name, path, branch) = (
            folder.to_path_buf(),
            task.to_string(),
            worktree.to_path_buf(),
            branch.map(str::to_owned),
        );
        git::blocking(move || git::remove_worktree(&source, &name, &path, branch.as_deref())).await
    }

    /// Internal Logic 5.6 and 5.7.
    // spec: ordin-desktop/components/ProjectService#ADR-053
    // spec: ordin-desktop/components/ProjectService#SA-CLOSE-03
    pub async fn end_coding_run(
        &self,
        task: CodingTaskId,
        run_id: RunId,
    ) -> Result<ChangeSet, ProjectError> {
        let row = self.task_row(task).await?;
        if row.task.state != CodingTaskState::Working {
            return Err(ProjectError::TaskStateInvalid);
        }
        let (worktree, base) = (
            PathBuf::from(&row.task.worktree_path),
            row.task.base_commit.clone(),
        );
        let changes = git::blocking(move || git::worktree_changes(&worktree, &base)).await?;
        let now = Timestamp::now();
        if changes.is_empty() {
            store::set_task_state(&self.pool, task, CodingTaskState::Settled).await?;
            self.clean_up(&row).await?;
            // The method returns `ChangeSet`: for an unchanged task the answer is an empty, settled
            // value that is never stored.
            return Ok(ChangeSet {
                id: ChangeSetId::new(),
                coding_task_id: task,
                run_id,
                project_id: row.task.project_id,
                entries: Vec::new(),
                state: ChangeSetState::Settled,
                created_at: now,
            });
        }
        let set = ChangeSet {
            id: ChangeSetId::new(),
            coding_task_id: task,
            run_id,
            project_id: row.task.project_id,
            entries: changes
                .iter()
                .map(|change| ChangeSetEntry {
                    id: ChangeSetEntryId::new(),
                    file: change.file.clone(),
                    hunk: change.hunk.clone(),
                    state: ChangeEntryState::Pending,
                })
                .collect(),
            state: ChangeSetState::InReview,
            created_at: now,
        };
        let hashes = self.capture_whole_files(&row, &set).await?;
        let mut tx = self.pool.begin().await?;
        store::insert_change_set(&mut tx, &set, &hashes).await?;
        tx.commit().await?;
        store::set_task_state(&self.pool, task, CodingTaskState::ReadyForReview).await?;
        let files: Vec<String> = changes.into_iter().map(|change| change.file).collect();
        let new_problems = self.find_new_problems(&row, &files).await;
        let outcome = self.run_task_checks(&row, run_id).await?;
        crate::log::change_set_ready(
            task,
            set.entries.len(),
            new_problems,
            outcome.map(|outcome| match outcome {
                TestOutcome::Passed => "passed",
                TestOutcome::Failed => "failed",
            }),
        );
        Ok(set)
    }

    /// Internal Logic 5.6: the content hash of each whole-file entry. A file that cannot be read now
    /// (removed, a symbolic link, too large) gets none, and the entry can then only be rejected.
    async fn capture_whole_files(
        &self,
        row: &TaskRow,
        set: &ChangeSet,
    ) -> Result<HashMap<ChangeSetEntryId, String>, ProjectError> {
        let mut hashes = HashMap::new();
        for entry in &set.entries {
            if !crate::review::is_whole_file(&entry.hunk) {
                continue;
            }
            match self.whole_file(row, &entry.file).await {
                Ok(found) => {
                    hashes.insert(entry.id, found.hash.0);
                }
                Err(ProjectError::DecisionInvalid) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(hashes)
    }

    /// Internal Logic 5.7: every check command in the worktree, with origin `Run`.
    async fn run_task_checks(
        &self,
        row: &TaskRow,
        run_id: RunId,
    ) -> Result<Option<TestOutcome>, ProjectError> {
        let project = store::project(&self.pool, row.task.project_id)
            .await?
            .ok_or(ProjectError::NotFound)?;
        let commands = project.project.check_commands;
        if commands.is_empty() {
            return Ok(None);
        }
        let origin = ordin_rules::CommandOrigin::Run {
            run_id,
            agent_instance_id: row.task.agent_instance_id,
        };
        let mut outcome = TestOutcome::Passed;
        for command in commands {
            let passed = match self
                .run_check(
                    row.task.project_id,
                    Some(row.task.id),
                    &command,
                    origin.clone(),
                )
                .await
            {
                Ok(result) => result.exit_code == Some(0),
                Err(ProjectError::DatastoreUnavailable) => {
                    return Err(ProjectError::DatastoreUnavailable)
                }
                Err(error) => {
                    crate::log::background_failed("task_check", error.code());
                    false
                }
            };
            if !passed {
                outcome = TestOutcome::Failed;
            }
        }
        store::set_test_outcome(&self.pool, row.task.id, outcome).await?;
        Ok(Some(outcome))
    }

    /// Internal Logic 5.8. Journals `coding_task_removed`.
    pub(crate) async fn clean_up(&self, row: &TaskRow) -> Result<(), ProjectError> {
        self.stop_task_instances(row.task.id).await;
        // The place may be gone already when a crash interrupted an earlier clean-up.
        match self.files.remove_place(row.worktree_place_id).await {
            Ok(()) | Err(ordin_files::FileError::PlaceNotFound) => {}
            Err(error) => return Err(file_failed(error)),
        }
        let project = store::project(&self.pool, row.task.project_id)
            .await?
            .ok_or(ProjectError::NotFound)?;
        let folder = self.place_folder(project.project.place_id).await?;
        self.drop_worktree(
            &folder,
            row.task.id,
            Path::new(&row.task.worktree_path),
            Some(&row.task.branch),
        )
        .await?;
        self.remove_base_worktree(&folder, row).await?;
        crate::log::coding_task_removed(row.task.id);
        self.record(
            "coding_task_removed",
            None,
            Some(project.workspace_id),
            "A coding task's worktree and branch were removed.",
            json!({ "task_id": row.task.id.to_string() }),
        )
        .await;
        Ok(())
    }

    /// Step 1.3: tasks `settled` or `set_aside` whose worktree folder still exists.
    pub(crate) async fn recover_worktrees(&self) {
        let rows = match store::finished_tasks(&self.pool).await {
            Ok(rows) => rows,
            Err(error) => {
                crate::log::background_failed("recover_worktrees", error.code());
                return;
            }
        };
        for row in rows {
            if Path::new(&row.task.worktree_path).exists() {
                if let Err(error) = self.clean_up(&row).await {
                    crate::log::background_failed("recover_worktrees", error.code());
                }
            }
        }
    }

    /// Tasks waiting for the engineer, each with its change set and new problems.
    pub async fn review(
        &self,
        project_id: ordin_types::ProjectId,
    ) -> Result<Vec<ReviewTask>, ProjectError> {
        let row = self.open_row(project_id).await?;
        let tasks =
            store::tasks_of_project_in(&self.pool, row.project.id, CodingTaskState::ReadyForReview)
                .await?;
        let mut out = Vec::with_capacity(tasks.len());
        for task in tasks {
            let change_set = store::change_set_of(&self.pool, task.task.id).await?;
            out.push(ReviewTask {
                task: task.task,
                change_set,
                new_problems: task.problems,
            });
        }
        Ok(out)
    }

    /// PROJ-ORIG-1: true when a coding task was started for `origin` (the run's origin as
    /// `StartCodingTask` gave it), so a caller can tell a coding task's run from any other.
    // spec: ordin-desktop/components/ProjectService#PROJ-ORIG-1
    pub async fn is_coding_task_origin(&self, origin: RunOrigin) -> Result<bool, ProjectError> {
        store::has_task_with_origin(&self.pool, &origin).await
    }

    /// Internal Logic 11.5.
    pub async fn keep_attempt(&self, task: CodingTaskId) -> Result<(), ProjectError> {
        let row = self.task_row(task).await?;
        if !matches!(
            row.task.state,
            CodingTaskState::Working | CodingTaskState::ReadyForReview
        ) {
            return Err(ProjectError::TaskStateInvalid);
        }
        let group = row
            .task
            .attempt_group_id
            .ok_or(ProjectError::Invalid("CODE_GROUP_TOO_SMALL"))?;
        let members = store::group_tasks(&self.pool, group).await?;
        if members.len() < 2 {
            return Err(ProjectError::Invalid("CODE_GROUP_TOO_SMALL"));
        }
        for other in members {
            let live = matches!(
                other.task.state,
                CodingTaskState::Working | CodingTaskState::ReadyForReview
            );
            if other.task.id != task && live {
                store::set_task_state(&self.pool, other.task.id, CodingTaskState::SetAside).await?;
                self.clean_up(&other).await?;
            }
        }
        let workspace = store::project(&self.pool, row.task.project_id)
            .await?
            .map(|project| project.workspace_id);
        self.record(
            "attempt_kept",
            None,
            workspace,
            "The engineer kept one attempt.",
            json!({ "group": group.to_string(), "task_id": task.to_string() }),
        )
        .await;
        Ok(())
    }
}
