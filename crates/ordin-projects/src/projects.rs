//! Projects (Internal Logic 2) and the engineer's git views, commit and push (Internal Logic 10).

use ordin_files::{FileError, WriteVersion};
use ordin_types::domain::{FileVersionOrigin, Project, ProjectState};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::{FileVersionId, PlaceId, ProjectId, WorkspaceId};
use serde_json::json;

use crate::error::{file_failed, ProjectError};
use crate::git;
use crate::service::ProjectService;
use crate::store::{self, ProjectRow};
use crate::types::{CommitSummary, PushOutcome, StatusEntry, StreamId, StreamPayload};

/// The marker files of Internal Logic 2.4, in language order.
const LANGUAGE_MARKERS: [(&str, &str); 6] = [
    ("go.mod", "go"),
    ("Cargo.toml", "rust"),
    ("pom.xml", "java"),
    ("build.gradle", "java"),
    ("build.gradle.kts", "java"),
    ("tsconfig.json", "typescript"),
];

/// The display name's limit (Data Model).
const MAX_DISPLAY_NAME_CHARS: usize = 120;

/// Internal Logic 2.6: trimmed, empty ones dropped, duplicates removed keeping the first; more than
/// 20, or one over 1000 characters, is `PROJ_CHECKS_INVALID`.
pub(crate) fn clean_check_commands(commands: Vec<String>) -> Result<Vec<String>, ProjectError> {
    let mut kept: Vec<String> = Vec::new();
    for command in commands {
        let command = command.trim();
        if !command.is_empty() && !kept.iter().any(|seen| seen == command) {
            kept.push(command.to_owned());
        }
    }
    if kept.len() > crate::MAX_CHECK_COMMANDS
        || kept
            .iter()
            .any(|c| c.chars().count() > crate::MAX_CHECK_COMMAND_CHARS)
    {
        return Err(ProjectError::Invalid("PROJ_CHECKS_INVALID"));
    }
    Ok(kept)
}

impl ProjectService {
    /// A project not `closed`; unknown or closed is `PROJ_NOT_FOUND`.
    pub(crate) async fn open_row(&self, id: ProjectId) -> Result<ProjectRow, ProjectError> {
        store::project(&self.pool, id)
            .await?
            .filter(|row| row.project.state != ProjectState::Closed)
            .ok_or(ProjectError::NotFound)
    }

    /// The folder of an open project that is a git repository.
    pub(crate) async fn repository_folder(
        &self,
        id: ProjectId,
    ) -> Result<(ProjectRow, std::path::PathBuf), ProjectError> {
        let row = self.open_row(id).await?;
        if !row.project.is_git_repository {
            return Err(ProjectError::NotARepository);
        }
        let folder = self.place_folder(row.project.place_id).await?;
        Ok((row, folder))
    }

    /// Internal Logic 2.1.
    pub async fn open_project(
        &self,
        place_id: PlaceId,
        workspace_id: WorkspaceId,
    ) -> Result<Project, ProjectError> {
        let place = self.files.place(place_id).await.map_err(file_failed)?;
        let folder = std::path::PathBuf::from(&place.path);
        let probe = folder.clone();
        let branch = git::blocking(move || git::detect(&probe)).await?;
        if store::open_project_on(&self.pool, place_id)
            .await?
            .is_some()
        {
            return Err(ProjectError::PlaceAlreadyOpen);
        }
        let now = Timestamp::now();
        let id = match store::closed_project_on(&self.pool, place_id, workspace_id).await? {
            Some(closed) => {
                store::reopen_project(
                    &self.pool,
                    closed.project.id,
                    branch.is_some(),
                    branch.as_deref(),
                    now,
                )
                .await?;
                closed.project.id
            }
            None => {
                let name: String = place
                    .friendly_name
                    .chars()
                    .take(MAX_DISPLAY_NAME_CHARS)
                    .collect();
                let row = ProjectRow {
                    project: Project {
                        id: ProjectId::new(),
                        place_id,
                        display_name: name,
                        is_git_repository: branch.is_some(),
                        current_branch: branch.clone(),
                        language_servers_in_use: Vec::new(),
                        check_commands: Vec::new(),
                        state: ProjectState::Opened,
                        opened_at: now,
                    },
                    workspace_id,
                };
                store::insert_project(&self.pool, &row).await?;
                row.project.id
            }
        };
        self.files.watch(&[place_id]).await.map_err(file_failed)?;
        crate::log::project_opened(id, branch.is_some());
        self.ensure_git_host_settings(id).await?;
        self.tell_registry(&["github", "gitlab"]);
        self.mark_indexed_when_covered(id, place_id).await?;
        self.project(id).await
    }

    /// Internal Logic 2.3.
    // `files.unwatch` runs "unless the place is held by a workspace", and this service has
    // no edge that can ask. A project's place is held by the workspace link that opened it, and the
    // Workspace Service unwatches a place no workspace holds (WorkspaceService.md, link removal), so
    // no call is made here.
    pub async fn close_project(&self, project_id: ProjectId) -> Result<Project, ProjectError> {
        let row = self.open_row(project_id).await?;
        store::set_project_state(&self.pool, project_id, ProjectState::Closed).await?;
        self.stop_project_instances(project_id).await;
        self.tell_registry(&["github", "gitlab"]);
        let mut project = row.project;
        project.state = ProjectState::Closed;
        Ok(project)
    }

    pub async fn project(&self, project_id: ProjectId) -> Result<Project, ProjectError> {
        store::project(&self.pool, project_id)
            .await?
            .map(|row| row.project)
            .ok_or(ProjectError::NotFound)
    }

    /// The project not `closed` on a place, if any (Internal Logic 2.5).
    pub async fn project_for_place(
        &self,
        place_id: PlaceId,
    ) -> Result<Option<Project>, ProjectError> {
        Ok(store::open_project_on(&self.pool, place_id)
            .await?
            .map(|row| row.project))
    }

    /// Internal Logic 2.6.
    // spec: ordin-desktop/components/ProjectService#SA-CLOSE-03
    pub async fn set_check_commands(
        &self,
        project_id: ProjectId,
        commands: Vec<String>,
    ) -> Result<Project, ProjectError> {
        let row = self.open_row(project_id).await?;
        let commands = clean_check_commands(commands)?;
        store::set_check_commands(&self.pool, project_id, &commands).await?;
        self.record(
            "check_commands_set",
            None,
            Some(row.workspace_id),
            "The engineer set the project's check commands.",
            json!({ "project_id": project_id.to_string(), "count": commands.len() }),
        )
        .await;
        self.project(project_id).await
    }

    /// Lower-case names of the languages of the workspace's open projects (Internal Logic 2.4).
    pub async fn workspace_languages(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<String>, ProjectError> {
        let mut languages: Vec<String> = Vec::new();
        for row in store::open_projects_in(&self.pool, workspace_id).await? {
            let folder = self.place_folder(row.project.place_id).await?;
            for (marker, language) in LANGUAGE_MARKERS {
                if folder.join(marker).is_file() {
                    languages.push(language.to_owned());
                }
            }
        }
        languages.sort();
        languages.dedup();
        Ok(languages)
    }

    // ---- the engineer's git (Internal Logic 10)

    pub async fn status(&self, project_id: ProjectId) -> Result<Vec<StatusEntry>, ProjectError> {
        let (_, folder) = self.repository_folder(project_id).await?;
        git::blocking(move || git::status(&folder)).await
    }

    pub async fn diff(
        &self,
        project_id: ProjectId,
        path: Option<&str>,
    ) -> Result<String, ProjectError> {
        let (_, folder) = self.repository_folder(project_id).await?;
        let path = path.map(str::to_owned);
        git::blocking(move || git::diff(&folder, path.as_deref())).await
    }

    pub async fn history(
        &self,
        project_id: ProjectId,
        path: Option<&str>,
        limit: u32,
    ) -> Result<Vec<CommitSummary>, ProjectError> {
        let (_, folder) = self.repository_folder(project_id).await?;
        let path = path.map(str::to_owned);
        git::blocking(move || git::history(&folder, path.as_deref(), limit)).await
    }

    pub async fn branches(&self, project_id: ProjectId) -> Result<Vec<String>, ProjectError> {
        let (_, folder) = self.repository_folder(project_id).await?;
        git::blocking(move || git::branches(&folder)).await
    }

    pub async fn create_branch(
        &self,
        project_id: ProjectId,
        name: &str,
    ) -> Result<(), ProjectError> {
        let (_, folder) = self.repository_folder(project_id).await?;
        let name = name.to_owned();
        git::blocking(move || git::create_branch(&folder, &name)).await
    }

    pub async fn switch_branch(
        &self,
        project_id: ProjectId,
        name: &str,
    ) -> Result<(), ProjectError> {
        let (_, folder) = self.repository_folder(project_id).await?;
        let target = name.to_owned();
        git::blocking(move || git::switch_branch(&folder, &target)).await?;
        store::set_current_branch(&self.pool, project_id, name).await
    }

    /// One line of what ordin did, written into the engineer's terminal and marked as ordin's
    /// (Internal Logic 10.3). No shell runs it.
    fn tell_terminal(&self, terminal: Option<StreamId>, lines: &[String]) {
        if let Some(stream) = terminal {
            for line in lines {
                let text = format!("[ordin] {line}\r\n");
                self.streams
                    .send(stream, StreamPayload::TerminalBytes(text.into_bytes()));
            }
        }
    }

    /// Internal Logic 10.2.
    // spec: ordin-desktop/components/ProjectService#SA-RECON-04
    pub async fn commit(
        &self,
        project_id: ProjectId,
        message: &str,
        terminal: Option<StreamId>,
    ) -> Result<CommitSummary, ProjectError> {
        let (row, folder) = self.repository_folder(project_id).await?;
        if store::pending_in_project(&self.pool, project_id).await? > 0 {
            return Err(ProjectError::UndecidedChanges);
        }
        let text = message.to_owned();
        let summary = git::blocking(move || git::commit_all(&folder, &text)).await?;
        self.record(
            "git_committed",
            None,
            Some(row.workspace_id),
            "The engineer committed changes.",
            json!({ "project_id": project_id.to_string(), "commit": summary.commit }),
        )
        .await;
        self.tell_terminal(
            terminal,
            &[
                format!("git commit -m {message:?}"),
                format!("committed {}", summary.commit),
            ],
        );
        Ok(summary)
    }

    /// Internal Logic 10.2. Without force; `ordin/*` branches stay local (*Security*).
    pub async fn push(
        &self,
        project_id: ProjectId,
        terminal: Option<StreamId>,
    ) -> Result<PushOutcome, ProjectError> {
        let (row, folder) = self.repository_folder(project_id).await?;
        let probe = folder.clone();
        let branch = git::blocking(move || git::current_branch(&git::open(&probe)?)).await?;
        if branch.starts_with("ordin/") {
            return Err(ProjectError::PushRefused);
        }
        let auth = self.remote_auth(&folder).await?;
        let limit = crate::service::lock(&self.tunables).network_timeout;
        let target = branch.clone();
        let commit =
            git::network(limit, move |stop| git::push(&folder, &target, &auth, stop)).await?;
        crate::log::git_pushed(row.project.place_id, &branch);
        self.record(
            "git_pushed",
            None,
            Some(row.workspace_id),
            "The engineer pushed a branch.",
            json!({ "project_id": project_id.to_string(), "commit": commit }),
        )
        .await;
        self.tell_terminal(
            terminal,
            &[
                format!("git push origin {branch}"),
                format!("pushed {commit}"),
            ],
        );
        Ok(PushOutcome { commit, branch })
    }

    /// Internal Logic 9.7: a new version through the File Service; the editor keeps the unsaved text
    /// on `PROJ_FILES_FAILED`.
    pub async fn save_file(
        &self,
        project_id: ProjectId,
        path: &str,
        text: String,
        expected_hash: Option<Sha256Hex>,
    ) -> Result<FileVersionId, ProjectError> {
        let row = self.open_row(project_id).await?;
        let version = self
            .files
            .write_version(WriteVersion {
                place_id: row.project.place_id,
                path: path.to_owned(),
                bytes: text.into_bytes(),
                origin: FileVersionOrigin::Person,
                expected_hash,
            })
            .await
            .map_err(|error: FileError| file_failed(error))?;
        Ok(version.id)
    }
}
