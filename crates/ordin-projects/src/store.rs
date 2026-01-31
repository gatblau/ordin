//! The module's tables in the Journal's datastore (Data Model), SQLite only. Every query is
//! parameterised; no SQL text is built from a value. Each read decodes one row into its 2C type.

use std::collections::HashMap;

use ordin_types::domain::{
    ChangeEntryState, ChangeSet, ChangeSetEntry, ChangeSetState, CodingTask, CodingTaskState,
    GithubOperationSetting, GithubSettingSource, LanguagePlugin, LanguagePluginSource,
    LanguagePluginStatus, Project, ProjectState, RunOrigin, TestOutcome,
};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{
    AgentInstanceId, AttemptGroupId, ChangeSetEntryId, ChangeSetId, CodingTaskId,
    GithubOperationSettingId, LanguagePluginId, PlaceId, ProjectId, RunId, WorkspaceId,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::error::ProjectError;
use crate::types::Problem;

/// A stored value this module wrote cannot be read back: the record is unusable.
fn bad() -> ProjectError {
    ProjectError::DatastoreUnavailable
}

fn id_of<T>(
    row: &SqliteRow,
    column: &str,
    parse: fn(&str) -> Result<T, ordin_types::TypesError>,
) -> Result<T, ProjectError> {
    let raw: String = row.try_get(column)?;
    parse(&raw).map_err(|_| bad())
}

fn time_of(raw: &str) -> Result<Timestamp, ProjectError> {
    parse_rfc3339_ms_utc(raw).map(Timestamp).map_err(|_| bad())
}

fn json_of<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, ProjectError> {
    serde_json::from_str(raw).map_err(|_| bad())
}

fn text_of<T: serde::Serialize>(value: &T) -> Result<String, ProjectError> {
    serde_json::to_string(value).map_err(|_| bad())
}

/// True when the failure is a unique-constraint violation.
fn is_unique(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.is_unique_violation())
}

// ---- projects

/// A project and the workspace it was opened in.
#[derive(Debug, Clone)]
pub(crate) struct ProjectRow {
    pub project: Project,
    pub workspace_id: WorkspaceId,
}

fn project_state(raw: &str) -> Result<ProjectState, ProjectError> {
    match raw {
        "opened" => Ok(ProjectState::Opened),
        "indexed" => Ok(ProjectState::Indexed),
        "closed" => Ok(ProjectState::Closed),
        _ => Err(bad()),
    }
}

pub(crate) fn project_state_text(state: ProjectState) -> &'static str {
    match state {
        ProjectState::Opened => "opened",
        ProjectState::Indexed => "indexed",
        ProjectState::Closed => "closed",
    }
}

fn project_of(row: &SqliteRow) -> Result<ProjectRow, ProjectError> {
    let servers: String = row.try_get("language_servers_in_use")?;
    let checks: String = row.try_get("check_commands")?;
    let state: String = row.try_get("state")?;
    let opened_at: String = row.try_get("opened_at")?;
    let is_git: i64 = row.try_get("is_git_repository")?;
    Ok(ProjectRow {
        project: Project {
            id: id_of(row, "id", ProjectId::parse)?,
            place_id: id_of(row, "place_id", PlaceId::parse)?,
            display_name: row.try_get("display_name")?,
            is_git_repository: is_git == 1,
            current_branch: row.try_get("current_branch")?,
            language_servers_in_use: json_of(&servers)?,
            check_commands: json_of(&checks)?,
            state: project_state(&state)?,
            opened_at: time_of(&opened_at)?,
        },
        workspace_id: id_of(row, "workspace_id", WorkspaceId::parse)?,
    })
}

pub(crate) async fn project(
    pool: &SqlitePool,
    id: ProjectId,
) -> Result<Option<ProjectRow>, ProjectError> {
    let row = sqlx::query("SELECT * FROM projects WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(project_of).transpose()
}

pub(crate) async fn open_project_on(
    pool: &SqlitePool,
    place: PlaceId,
) -> Result<Option<ProjectRow>, ProjectError> {
    let row = sqlx::query("SELECT * FROM projects WHERE place_id = ?1 AND state != 'closed'")
        .bind(place.to_string())
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(project_of).transpose()
}

/// The newest closed project of this place in this workspace, reopened by `open_project`.
pub(crate) async fn closed_project_on(
    pool: &SqlitePool,
    place: PlaceId,
    workspace: WorkspaceId,
) -> Result<Option<ProjectRow>, ProjectError> {
    let row = sqlx::query(
        "SELECT * FROM projects WHERE place_id = ?1 AND workspace_id = ?2 AND state = 'closed' \
         ORDER BY opened_at DESC LIMIT 1",
    )
    .bind(place.to_string())
    .bind(workspace.to_string())
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(project_of).transpose()
}

pub(crate) async fn open_projects(pool: &SqlitePool) -> Result<Vec<ProjectRow>, ProjectError> {
    let rows = sqlx::query("SELECT * FROM projects WHERE state != 'closed' ORDER BY opened_at")
        .fetch_all(pool)
        .await?;
    rows.iter().map(project_of).collect()
}

pub(crate) async fn open_projects_in(
    pool: &SqlitePool,
    workspace: WorkspaceId,
) -> Result<Vec<ProjectRow>, ProjectError> {
    let rows = sqlx::query(
        "SELECT * FROM projects WHERE workspace_id = ?1 AND state != 'closed' ORDER BY opened_at",
    )
    .bind(workspace.to_string())
    .fetch_all(pool)
    .await?;
    rows.iter().map(project_of).collect()
}

pub(crate) async fn insert_project(
    pool: &SqlitePool,
    row: &ProjectRow,
) -> Result<(), ProjectError> {
    let p = &row.project;
    if p.is_git_repository != p.current_branch.is_some() {
        return Err(ProjectError::Invalid("PROJ_BRANCH_MISMATCH"));
    }
    let done = sqlx::query(
        "INSERT INTO projects (id, place_id, workspace_id, display_name, is_git_repository, \
         current_branch, state, opened_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(p.id.to_string())
    .bind(p.place_id.to_string())
    .bind(row.workspace_id.to_string())
    .bind(&p.display_name)
    .bind(i64::from(p.is_git_repository))
    .bind(&p.current_branch)
    .bind(project_state_text(p.state))
    .bind(p.opened_at.to_wire())
    .execute(pool)
    .await;
    match done {
        Err(error) if is_unique(&error) => Err(ProjectError::PlaceAlreadyOpen),
        other => other.map(|_| ()).map_err(Into::into),
    }
}

/// `closed → opened`: the same record, with what the folder holds now.
pub(crate) async fn reopen_project(
    pool: &SqlitePool,
    id: ProjectId,
    is_git: bool,
    branch: Option<&str>,
    opened_at: Timestamp,
) -> Result<(), ProjectError> {
    if is_git != branch.is_some() {
        return Err(ProjectError::Invalid("PROJ_BRANCH_MISMATCH"));
    }
    let done = sqlx::query(
        "UPDATE projects SET state = 'opened', is_git_repository = ?2, current_branch = ?3, \
         opened_at = ?4 WHERE id = ?1",
    )
    .bind(id.to_string())
    .bind(i64::from(is_git))
    .bind(branch)
    .bind(opened_at.to_wire())
    .execute(pool)
    .await;
    match done {
        Err(error) if is_unique(&error) => Err(ProjectError::PlaceAlreadyOpen),
        other => other.map(|_| ()).map_err(Into::into),
    }
}

pub(crate) async fn set_project_state(
    pool: &SqlitePool,
    id: ProjectId,
    state: ProjectState,
) -> Result<(), ProjectError> {
    sqlx::query("UPDATE projects SET state = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(project_state_text(state))
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_current_branch(
    pool: &SqlitePool,
    id: ProjectId,
    branch: &str,
) -> Result<(), ProjectError> {
    sqlx::query("UPDATE projects SET current_branch = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(branch)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_check_commands(
    pool: &SqlitePool,
    id: ProjectId,
    commands: &[String],
) -> Result<(), ProjectError> {
    sqlx::query("UPDATE projects SET check_commands = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(text_of(&commands)?)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_servers_in_use(
    pool: &SqlitePool,
    id: ProjectId,
    servers: &[String],
) -> Result<(), ProjectError> {
    sqlx::query("UPDATE projects SET language_servers_in_use = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(text_of(&servers)?)
        .execute(pool)
        .await?;
    Ok(())
}

// ---- GitHub and GitLab approval settings

fn setting_of(row: &SqliteRow) -> Result<GithubOperationSetting, ProjectError> {
    let is_write: i64 = row.try_get("is_write")?;
    let approval: i64 = row.try_get("approval_required")?;
    let set_by: String = row.try_get("set_by")?;
    let set_at: String = row.try_get("set_at")?;
    Ok(GithubOperationSetting {
        id: id_of(row, "id", GithubOperationSettingId::parse)?,
        project_id: id_of(row, "project_id", ProjectId::parse)?,
        operation_kind: row.try_get("operation_kind")?,
        is_write: is_write == 1,
        approval_required: approval == 1,
        set_by: match set_by.as_str() {
            "organisation_profile" => GithubSettingSource::OrganisationProfile,
            "engineer" => GithubSettingSource::Engineer,
            _ => return Err(bad()),
        },
        set_at: time_of(&set_at)?,
    })
}

fn set_by_text(source: GithubSettingSource) -> &'static str {
    match source {
        GithubSettingSource::OrganisationProfile => "organisation_profile",
        GithubSettingSource::Engineer => "engineer",
    }
}

pub(crate) async fn settings_of(
    pool: &SqlitePool,
    project: ProjectId,
) -> Result<Vec<GithubOperationSetting>, ProjectError> {
    let rows = sqlx::query(
        "SELECT * FROM github_operation_settings WHERE project_id = ?1 ORDER BY operation_kind",
    )
    .bind(project.to_string())
    .fetch_all(pool)
    .await?;
    rows.iter().map(setting_of).collect()
}

pub(crate) async fn insert_setting(
    pool: &SqlitePool,
    setting: &GithubOperationSetting,
) -> Result<(), ProjectError> {
    let done = sqlx::query(
        "INSERT INTO github_operation_settings (id, project_id, operation_kind, is_write, \
         approval_required, set_by, set_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(setting.id.to_string())
    .bind(setting.project_id.to_string())
    .bind(&setting.operation_kind)
    .bind(i64::from(setting.is_write))
    .bind(i64::from(setting.approval_required))
    .bind(set_by_text(setting.set_by))
    .bind(setting.set_at.to_wire())
    .execute(pool)
    .await;
    match done {
        Err(error) if is_unique(&error) => Err(ProjectError::Invalid("GHOP_DUPLICATE")),
        other => other.map(|_| ()).map_err(Into::into),
    }
}

pub(crate) async fn update_setting(
    pool: &SqlitePool,
    setting: &GithubOperationSetting,
) -> Result<(), ProjectError> {
    sqlx::query(
        "UPDATE github_operation_settings SET approval_required = ?2, set_by = ?3, set_at = ?4 \
         WHERE id = ?1",
    )
    .bind(setting.id.to_string())
    .bind(i64::from(setting.approval_required))
    .bind(set_by_text(setting.set_by))
    .bind(setting.set_at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

// ---- language plug-ins

fn plugin_of(row: &SqliteRow) -> Result<LanguagePlugin, ProjectError> {
    let approved: String = row.try_get("approved_servers")?;
    let source: String = row.try_get("source")?;
    let enabled: i64 = row.try_get("enabled")?;
    let settings: String = row.try_get("server_settings")?;
    let status: String = row.try_get("status")?;
    Ok(LanguagePlugin {
        id: id_of(row, "id", LanguagePluginId::parse)?,
        language: row.try_get("language")?,
        approved_servers: json_of(&approved)?,
        default_server: row.try_get("default_server")?,
        chosen_server: row.try_get("chosen_server")?,
        pinned_version: row.try_get("pinned_version")?,
        source: match source.as_str() {
            "agent_package" => LanguagePluginSource::AgentPackage,
            "organisation_profile" => LanguagePluginSource::OrganisationProfile,
            _ => return Err(bad()),
        },
        enabled: enabled == 1,
        server_settings: json_of(&settings)?,
        status: match status.as_str() {
            "available" => LanguagePluginStatus::Available,
            "added" => LanguagePluginStatus::Added,
            "installed" => LanguagePluginStatus::Installed,
            "removed" => LanguagePluginStatus::Removed,
            _ => return Err(bad()),
        },
    })
}

fn plugin_status_text(status: LanguagePluginStatus) -> &'static str {
    match status {
        LanguagePluginStatus::Available => "available",
        LanguagePluginStatus::Added => "added",
        LanguagePluginStatus::Installed => "installed",
        LanguagePluginStatus::Removed => "removed",
    }
}

pub(crate) async fn plugins(pool: &SqlitePool) -> Result<Vec<LanguagePlugin>, ProjectError> {
    let rows = sqlx::query("SELECT * FROM language_plugins ORDER BY language")
        .fetch_all(pool)
        .await?;
    rows.iter().map(plugin_of).collect()
}

pub(crate) async fn plugin(
    pool: &SqlitePool,
    language: &str,
) -> Result<Option<LanguagePlugin>, ProjectError> {
    let row = sqlx::query("SELECT * FROM language_plugins WHERE language = ?1")
        .bind(language)
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(plugin_of).transpose()
}

pub(crate) async fn insert_plugin(
    pool: &SqlitePool,
    plugin: &LanguagePlugin,
) -> Result<(), ProjectError> {
    let done = sqlx::query(
        "INSERT INTO language_plugins (id, language, approved_servers, default_server, \
         chosen_server, pinned_version, source, enabled, server_settings, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(plugin.id.to_string())
    .bind(&plugin.language)
    .bind(text_of(&plugin.approved_servers)?)
    .bind(&plugin.default_server)
    .bind(&plugin.chosen_server)
    .bind(&plugin.pinned_version)
    .bind(match plugin.source {
        LanguagePluginSource::AgentPackage => "agent_package",
        LanguagePluginSource::OrganisationProfile => "organisation_profile",
    })
    .bind(i64::from(plugin.enabled))
    .bind(text_of(&plugin.server_settings)?)
    .bind(plugin_status_text(plugin.status))
    .execute(pool)
    .await;
    match done {
        Err(error) if is_unique(&error) => Err(ProjectError::Invalid("LANG_DUPLICATE")),
        other => other.map(|_| ()).map_err(Into::into),
    }
}

pub(crate) async fn update_plugin(
    pool: &SqlitePool,
    plugin: &LanguagePlugin,
) -> Result<(), ProjectError> {
    sqlx::query(
        "UPDATE language_plugins SET approved_servers = ?2, default_server = ?3, \
         chosen_server = ?4, pinned_version = ?5, enabled = ?6, server_settings = ?7, \
         status = ?8 WHERE id = ?1",
    )
    .bind(plugin.id.to_string())
    .bind(text_of(&plugin.approved_servers)?)
    .bind(&plugin.default_server)
    .bind(&plugin.chosen_server)
    .bind(&plugin.pinned_version)
    .bind(i64::from(plugin.enabled))
    .bind(text_of(&plugin.server_settings)?)
    .bind(plugin_status_text(plugin.status))
    .execute(pool)
    .await?;
    Ok(())
}

// ---- coding tasks

/// A coding task with the columns 2C's type does not carry.
#[derive(Debug, Clone)]
pub(crate) struct TaskRow {
    pub task: CodingTask,
    pub worktree_place_id: PlaceId,
    pub problems: Vec<Problem>,
}

fn task_state(raw: &str) -> Result<CodingTaskState, ProjectError> {
    match raw {
        "working" => Ok(CodingTaskState::Working),
        "ready_for_review" => Ok(CodingTaskState::ReadyForReview),
        "settled" => Ok(CodingTaskState::Settled),
        "set_aside" => Ok(CodingTaskState::SetAside),
        _ => Err(bad()),
    }
}

fn task_state_text(state: CodingTaskState) -> &'static str {
    match state {
        CodingTaskState::Working => "working",
        CodingTaskState::ReadyForReview => "ready_for_review",
        CodingTaskState::Settled => "settled",
        CodingTaskState::SetAside => "set_aside",
    }
}

fn task_of(row: &SqliteRow) -> Result<TaskRow, ProjectError> {
    let origin: String = row.try_get("origin")?;
    let group: Option<String> = row.try_get("attempt_group_id")?;
    let outcome: Option<String> = row.try_get("test_outcome")?;
    let problems: i64 = row.try_get("new_problems")?;
    let unchecked: i64 = row.try_get("problems_unchecked")?;
    let list: String = row.try_get("new_problem_list")?;
    let state: String = row.try_get("state")?;
    Ok(TaskRow {
        task: CodingTask {
            id: id_of(row, "id", CodingTaskId::parse)?,
            origin: json_of::<RunOrigin>(&origin)?,
            project_id: id_of(row, "project_id", ProjectId::parse)?,
            agent_instance_id: id_of(row, "agent_instance_id", AgentInstanceId::parse)?,
            request: row.try_get("request")?,
            worktree_path: row.try_get("worktree_path")?,
            branch: row.try_get("branch")?,
            base_branch: row.try_get("base_branch")?,
            base_commit: row.try_get("base_commit")?,
            attempt_group_id: group
                .map(|g| AttemptGroupId::parse(&g).map_err(|_| bad()))
                .transpose()?,
            test_outcome: match outcome.as_deref() {
                None => None,
                Some("passed") => Some(TestOutcome::Passed),
                Some("failed") => Some(TestOutcome::Failed),
                Some(_) => return Err(bad()),
            },
            new_problems: u32::try_from(problems).map_err(|_| bad())?,
            problems_unchecked: unchecked != 0,
            state: task_state(&state)?,
        },
        worktree_place_id: id_of(row, "worktree_place_id", PlaceId::parse)?,
        problems: json_of(&list)?,
    })
}

pub(crate) async fn task(
    pool: &SqlitePool,
    id: CodingTaskId,
) -> Result<Option<TaskRow>, ProjectError> {
    let row = sqlx::query("SELECT * FROM coding_tasks WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(task_of).transpose()
}

/// True when a coding task was started for `origin` (PROJ-ORIG-1).
pub(crate) async fn has_task_with_origin(
    pool: &SqlitePool,
    origin: &RunOrigin,
) -> Result<bool, ProjectError> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM coding_tasks WHERE origin = ?1 LIMIT 1")
            .bind(text_of(origin)?)
            .fetch_optional(pool)
            .await?;
    Ok(found.is_some())
}

pub(crate) async fn insert_task(
    pool: &SqlitePool,
    task: &CodingTask,
    place: PlaceId,
) -> Result<(), ProjectError> {
    if task.base_commit.len() != 40
        || !task
            .base_commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ProjectError::Invalid("CODE_COMMIT_INVALID"));
    }
    sqlx::query(
        "INSERT INTO coding_tasks (id, origin, project_id, agent_instance_id, request, \
         worktree_path, worktree_place_id, branch, base_branch, base_commit, attempt_group_id, \
         state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    )
    .bind(task.id.to_string())
    .bind(text_of(&task.origin)?)
    .bind(task.project_id.to_string())
    .bind(task.agent_instance_id.to_string())
    .bind(&task.request)
    .bind(&task.worktree_path)
    .bind(place.to_string())
    .bind(&task.branch)
    .bind(&task.base_branch)
    .bind(&task.base_commit)
    .bind(task.attempt_group_id.map(|g| g.to_string()))
    .bind(task_state_text(task.state))
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn set_task_state(
    pool: &SqlitePool,
    id: CodingTaskId,
    state: CodingTaskState,
) -> Result<(), ProjectError> {
    sqlx::query("UPDATE coding_tasks SET state = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(task_state_text(state))
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_test_outcome(
    pool: &SqlitePool,
    id: CodingTaskId,
    outcome: TestOutcome,
) -> Result<(), ProjectError> {
    sqlx::query("UPDATE coding_tasks SET test_outcome = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(match outcome {
            TestOutcome::Passed => "passed",
            TestOutcome::Failed => "failed",
        })
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_new_problems(
    pool: &SqlitePool,
    id: CodingTaskId,
    problems: &[Problem],
) -> Result<(), ProjectError> {
    sqlx::query("UPDATE coding_tasks SET new_problems = ?2, new_problem_list = ?3 WHERE id = ?1")
        .bind(id.to_string())
        .bind(i64::try_from(problems.len()).map_err(|_| bad())?)
        .bind(text_of(&problems)?)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_problems_unchecked(
    pool: &SqlitePool,
    id: CodingTaskId,
    unchecked: bool,
) -> Result<(), ProjectError> {
    sqlx::query("UPDATE coding_tasks SET problems_unchecked = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(i64::from(unchecked))
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn group_tasks(
    pool: &SqlitePool,
    group: AttemptGroupId,
) -> Result<Vec<TaskRow>, ProjectError> {
    let rows = sqlx::query("SELECT * FROM coding_tasks WHERE attempt_group_id = ?1 ORDER BY id")
        .bind(group.to_string())
        .fetch_all(pool)
        .await?;
    rows.iter().map(task_of).collect()
}

pub(crate) async fn tasks_of_project_in(
    pool: &SqlitePool,
    project: ProjectId,
    state: CodingTaskState,
) -> Result<Vec<TaskRow>, ProjectError> {
    let rows =
        sqlx::query("SELECT * FROM coding_tasks WHERE project_id = ?1 AND state = ?2 ORDER BY id")
            .bind(project.to_string())
            .bind(task_state_text(state))
            .fetch_all(pool)
            .await?;
    rows.iter().map(task_of).collect()
}

pub(crate) async fn finished_tasks(pool: &SqlitePool) -> Result<Vec<TaskRow>, ProjectError> {
    let rows = sqlx::query("SELECT * FROM coding_tasks WHERE state IN ('settled','set_aside')")
        .fetch_all(pool)
        .await?;
    rows.iter().map(task_of).collect()
}

// ---- change sets

fn entry_state(raw: &str) -> Result<ChangeEntryState, ProjectError> {
    match raw {
        "pending" => Ok(ChangeEntryState::Pending),
        "accepted" => Ok(ChangeEntryState::Accepted),
        "rejected" => Ok(ChangeEntryState::Rejected),
        _ => Err(bad()),
    }
}

fn entry_state_text(state: ChangeEntryState) -> &'static str {
    match state {
        ChangeEntryState::Pending => "pending",
        ChangeEntryState::Accepted => "accepted",
        ChangeEntryState::Rejected => "rejected",
    }
}

fn set_state_text(state: ChangeSetState) -> &'static str {
    match state {
        ChangeSetState::Open => "open",
        ChangeSetState::InReview => "in_review",
        ChangeSetState::Settled => "settled",
    }
}

fn entry_of(row: &SqliteRow) -> Result<ChangeSetEntry, ProjectError> {
    let state: String = row.try_get("state")?;
    Ok(ChangeSetEntry {
        id: id_of(row, "id", ChangeSetEntryId::parse)?,
        file: row.try_get("file")?,
        hunk: row.try_get("hunk")?,
        state: entry_state(&state)?,
    })
}

/// Writes the change set and its entries in one transaction; a second one for the task is refused.
/// `hashes` holds the content hash of each whole-file entry that has one.
pub(crate) async fn insert_change_set(
    tx: &mut Transaction<'_, Sqlite>,
    set: &ChangeSet,
    hashes: &HashMap<ChangeSetEntryId, String>,
) -> Result<(), ProjectError> {
    let done = sqlx::query(
        "INSERT INTO change_sets (id, coding_task_id, run_id, project_id, state, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(set.id.to_string())
    .bind(set.coding_task_id.to_string())
    .bind(set.run_id.to_string())
    .bind(set.project_id.to_string())
    .bind(set_state_text(set.state))
    .bind(set.created_at.to_wire())
    .execute(&mut **tx)
    .await;
    match done {
        Err(error) if is_unique(&error) => return Err(ProjectError::Invalid("CSET_DUPLICATE")),
        other => {
            other?;
        }
    }
    for (position, entry) in set.entries.iter().enumerate() {
        if entry.hunk.len() > crate::MAX_HUNK_BYTES {
            return Err(ProjectError::Invalid("CSET_HUNK_TOO_LARGE"));
        }
        sqlx::query(
            "INSERT INTO change_set_entries \
             (id, change_set_id, position, file, hunk, state, content_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(entry.id.to_string())
        .bind(set.id.to_string())
        .bind(i64::try_from(position).map_err(|_| bad())?)
        .bind(&entry.file)
        .bind(&entry.hunk)
        .bind(entry_state_text(entry.state))
        .bind(hashes.get(&entry.id))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub(crate) async fn change_set_of(
    pool: &SqlitePool,
    task: CodingTaskId,
) -> Result<Option<ChangeSet>, ProjectError> {
    let row = sqlx::query("SELECT * FROM change_sets WHERE coding_task_id = ?1")
        .bind(task.to_string())
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: ChangeSetId = id_of(&row, "id", ChangeSetId::parse)?;
    let entries = sqlx::query(
        "SELECT id, file, hunk, state FROM change_set_entries WHERE change_set_id = ?1 \
         ORDER BY position",
    )
    .bind(id.to_string())
    .fetch_all(pool)
    .await?;
    let state: String = row.try_get("state")?;
    let created_at: String = row.try_get("created_at")?;
    Ok(Some(ChangeSet {
        id,
        coding_task_id: id_of(&row, "coding_task_id", CodingTaskId::parse)?,
        run_id: id_of(&row, "run_id", RunId::parse)?,
        project_id: id_of(&row, "project_id", ProjectId::parse)?,
        entries: entries
            .iter()
            .map(entry_of)
            .collect::<Result<Vec<_>, _>>()?,
        state: match state.as_str() {
            "open" => ChangeSetState::Open,
            "in_review" => ChangeSetState::InReview,
            "settled" => ChangeSetState::Settled,
            _ => return Err(bad()),
        },
        created_at: time_of(&created_at)?,
    }))
}

/// An entry and the ids around it.
#[derive(Debug, Clone)]
pub(crate) struct EntryRow {
    pub entry: ChangeSetEntry,
    pub change_set_id: ChangeSetId,
    pub coding_task_id: CodingTaskId,
    pub run_id: RunId,
    /// The content hash recorded at capture, for a whole-file entry.
    pub content_hash: Option<String>,
}

pub(crate) async fn entry(
    pool: &SqlitePool,
    id: ChangeSetEntryId,
) -> Result<Option<EntryRow>, ProjectError> {
    let row = sqlx::query(
        "SELECT e.id, e.file, e.hunk, e.state, e.content_hash, e.change_set_id, \
         s.coding_task_id, s.run_id \
         FROM change_set_entries e JOIN change_sets s ON s.id = e.change_set_id \
         WHERE e.id = ?1",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;
    row.as_ref()
        .map(|row| {
            Ok(EntryRow {
                entry: entry_of(row)?,
                change_set_id: id_of(row, "change_set_id", ChangeSetId::parse)?,
                coding_task_id: id_of(row, "coding_task_id", CodingTaskId::parse)?,
                run_id: id_of(row, "run_id", RunId::parse)?,
                content_hash: row.try_get("content_hash")?,
            })
        })
        .transpose()
}

pub(crate) async fn set_entry_state(
    pool: &SqlitePool,
    id: ChangeSetEntryId,
    state: ChangeEntryState,
) -> Result<(), ProjectError> {
    sqlx::query("UPDATE change_set_entries SET state = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(entry_state_text(state))
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn pending_in_set(
    pool: &SqlitePool,
    set: ChangeSetId,
) -> Result<i64, ProjectError> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM change_set_entries WHERE change_set_id = ?1 AND state = 'pending'",
    )
    .bind(set.to_string())
    .fetch_one(pool)
    .await?)
}

/// `settled`, refused while an entry is pending (`CSET_UNDECIDED_ENTRIES`).
pub(crate) async fn settle_change_set(
    pool: &SqlitePool,
    set: ChangeSetId,
) -> Result<(), ProjectError> {
    if pending_in_set(pool, set).await? > 0 {
        return Err(ProjectError::Invalid("CSET_UNDECIDED_ENTRIES"));
    }
    sqlx::query("UPDATE change_sets SET state = 'settled' WHERE id = ?1")
        .bind(set.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// Pending entries of the project's tasks still in review; a task set aside no longer counts.
pub(crate) async fn pending_in_project(
    pool: &SqlitePool,
    project: ProjectId,
) -> Result<i64, ProjectError> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM change_set_entries e \
         JOIN change_sets s ON s.id = e.change_set_id \
         JOIN coding_tasks t ON t.id = s.coding_task_id \
         WHERE s.project_id = ?1 AND e.state = 'pending' \
         AND t.state NOT IN ('settled','set_aside')",
    )
    .bind(project.to_string())
    .fetch_one(pool)
    .await?)
}

/// Pending entries of one file in the project (Internal Logic 2.2).
pub(crate) async fn pending_entries_for_file(
    pool: &SqlitePool,
    project: ProjectId,
    file: &str,
) -> Result<Vec<ChangeSetEntryId>, ProjectError> {
    let rows = sqlx::query(
        "SELECT e.id FROM change_set_entries e JOIN change_sets s ON s.id = e.change_set_id \
         WHERE s.project_id = ?1 AND e.file = ?2 AND e.state = 'pending'",
    )
    .bind(project.to_string())
    .bind(file)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| id_of(row, "id", ChangeSetEntryId::parse))
        .collect()
}

// ---- followed originals and fetch state

#[derive(Debug, Clone)]
pub(crate) struct FollowedRow {
    pub path: String,
    pub last_blob: Option<String>,
}

pub(crate) async fn followed_count_of_places(
    pool: &SqlitePool,
    other_than: PlaceId,
) -> Result<i64, ProjectError> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(DISTINCT place_id) FROM followed_originals WHERE place_id != ?1",
    )
    .bind(other_than.to_string())
    .fetch_one(pool)
    .await?)
}

pub(crate) async fn follow(
    pool: &SqlitePool,
    place: PlaceId,
    path: &str,
    branch: &str,
) -> Result<(), ProjectError> {
    sqlx::query(
        "INSERT INTO followed_originals (place_id, path, branch) VALUES (?1, ?2, ?3) \
         ON CONFLICT (place_id, path, branch) DO NOTHING",
    )
    .bind(place.to_string())
    .bind(path)
    .bind(branch)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn unfollow(
    pool: &SqlitePool,
    place: PlaceId,
    path: &str,
    branch: &str,
) -> Result<(), ProjectError> {
    sqlx::query("DELETE FROM followed_originals WHERE place_id = ?1 AND path = ?2 AND branch = ?3")
        .bind(place.to_string())
        .bind(path)
        .bind(branch)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn followed_paths(
    pool: &SqlitePool,
    place: PlaceId,
    branch: &str,
) -> Result<Vec<FollowedRow>, ProjectError> {
    let rows = sqlx::query(
        "SELECT path, last_blob FROM followed_originals WHERE place_id = ?1 AND branch = ?2 \
         ORDER BY path",
    )
    .bind(place.to_string())
    .bind(branch)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(FollowedRow {
                path: row.try_get("path")?,
                last_blob: row.try_get("last_blob")?,
            })
        })
        .collect()
}

/// The distinct (place, branch) pairs followed, with their fetch timing.
pub(crate) async fn followed_branches(
    pool: &SqlitePool,
) -> Result<Vec<FollowedBranch>, ProjectError> {
    let rows = sqlx::query(
        "SELECT DISTINCT f.place_id, f.branch, s.last_checked_at, s.interval_s \
         FROM followed_originals f LEFT JOIN repository_fetch_state s \
         ON s.place_id = f.place_id AND s.branch = f.branch",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let checked: Option<String> = row.try_get("last_checked_at")?;
            let interval: Option<i64> = row.try_get("interval_s")?;
            Ok(FollowedBranch {
                place_id: id_of(row, "place_id", PlaceId::parse)?,
                branch: row.try_get("branch")?,
                last_checked_at: checked.as_deref().map(time_of).transpose()?,
                interval_s: interval.unwrap_or(900),
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
pub(crate) struct FollowedBranch {
    pub place_id: PlaceId,
    pub branch: String,
    pub last_checked_at: Option<Timestamp>,
    pub interval_s: i64,
}

pub(crate) async fn set_followed_blob(
    pool: &SqlitePool,
    place: PlaceId,
    path: &str,
    branch: &str,
    blob: Option<&str>,
    commit: Option<&str>,
) -> Result<(), ProjectError> {
    sqlx::query(
        "UPDATE followed_originals SET last_blob = ?4, last_commit = COALESCE(?5, last_commit) \
         WHERE place_id = ?1 AND path = ?2 AND branch = ?3",
    )
    .bind(place.to_string())
    .bind(path)
    .bind(branch)
    .bind(blob)
    .bind(commit)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn remote_commit(
    pool: &SqlitePool,
    place: PlaceId,
    branch: &str,
) -> Result<Option<String>, ProjectError> {
    let row = sqlx::query(
        "SELECT remote_commit FROM repository_fetch_state WHERE place_id = ?1 AND branch = ?2",
    )
    .bind(place.to_string())
    .bind(branch)
    .fetch_optional(pool)
    .await?;
    match row {
        Some(row) => Ok(row.try_get("remote_commit")?),
        None => Ok(None),
    }
}

pub(crate) async fn fetch_succeeded(
    pool: &SqlitePool,
    place: PlaceId,
    branch: &str,
    commit: &str,
    at: Timestamp,
) -> Result<(), ProjectError> {
    sqlx::query(
        "INSERT INTO repository_fetch_state (place_id, branch, remote_commit, last_checked_at, \
         last_error, interval_s) VALUES (?1, ?2, ?3, ?4, NULL, ?5) \
         ON CONFLICT (place_id, branch) DO UPDATE SET remote_commit = ?3, last_checked_at = ?4, \
         last_error = NULL, interval_s = ?5",
    )
    .bind(place.to_string())
    .bind(branch)
    .bind(commit)
    .bind(at.to_wire())
    .bind(i64::try_from(crate::FETCH_INTERVAL.as_secs()).map_err(|_| bad())?)
    .execute(pool)
    .await?;
    Ok(())
}

/// Records a failed fetch; a rate limit doubles the interval up to the two-hour limit.
pub(crate) async fn fetch_failed(
    pool: &SqlitePool,
    place: PlaceId,
    branch: &str,
    code: &str,
    at: Timestamp,
    rate_limited: bool,
) -> Result<(), ProjectError> {
    let maximum = i64::try_from(crate::FETCH_INTERVAL_MAX.as_secs()).map_err(|_| bad())?;
    let start = i64::try_from(crate::FETCH_INTERVAL.as_secs()).map_err(|_| bad())?;
    sqlx::query(
        "INSERT INTO repository_fetch_state (place_id, branch, last_checked_at, last_error, \
         interval_s) VALUES (?1, ?2, ?3, ?4, CASE WHEN ?5 = 1 THEN MIN(?6 * 2, ?7) ELSE ?6 END) \
         ON CONFLICT (place_id, branch) DO UPDATE SET last_checked_at = ?3, last_error = ?4, \
         interval_s = CASE WHEN ?5 = 1 THEN MIN(interval_s * 2, ?7) ELSE interval_s END",
    )
    .bind(place.to_string())
    .bind(branch)
    .bind(at.to_wire())
    .bind(code)
    .bind(i64::from(rate_limited))
    .bind(start)
    .bind(maximum)
    .execute(pool)
    .await?;
    Ok(())
}

// ---- IDE choices

/// The person's answer for this repository at a place; None when they have not given one for it.
pub(crate) async fn language_server_answer(
    pool: &SqlitePool,
    place: PlaceId,
    repository: &str,
) -> Result<Option<bool>, ProjectError> {
    let row = sqlx::query(
        "SELECT allowed FROM language_server_answers WHERE place_id = ?1 AND repository = ?2",
    )
    .bind(place.to_string())
    .bind(repository)
    .fetch_optional(pool)
    .await?;
    match row {
        Some(row) => {
            let allowed: i64 = row.try_get("allowed")?;
            Ok(Some(allowed == 1))
        }
        None => Ok(None),
    }
}

pub(crate) async fn set_language_server_answer(
    pool: &SqlitePool,
    place: PlaceId,
    allowed: bool,
    repository: &str,
    at: Timestamp,
) -> Result<(), ProjectError> {
    sqlx::query(
        "INSERT INTO language_server_answers (place_id, allowed, repository, answered_at) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT (place_id) DO UPDATE SET \
         allowed = ?2, repository = ?3, answered_at = ?4",
    )
    .bind(place.to_string())
    .bind(i64::from(allowed))
    .bind(repository)
    .bind(at.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn ide_choice(pool: &SqlitePool, ide: &str) -> Result<Option<bool>, ProjectError> {
    let row = sqlx::query("SELECT enabled FROM ide_choices WHERE ide_id = ?1")
        .bind(ide)
        .fetch_optional(pool)
        .await?;
    match row {
        Some(row) => {
            let enabled: i64 = row.try_get("enabled")?;
            Ok(Some(enabled == 1))
        }
        None => Ok(None),
    }
}

pub(crate) async fn set_ide_choice(
    pool: &SqlitePool,
    ide: &str,
    enabled: bool,
) -> Result<(), ProjectError> {
    sqlx::query(
        "INSERT INTO ide_choices (ide_id, enabled) VALUES (?1, ?2) \
         ON CONFLICT (ide_id) DO UPDATE SET enabled = ?2",
    )
    .bind(ide)
    .bind(i64::from(enabled))
    .execute(pool)
    .await?;
    Ok(())
}
