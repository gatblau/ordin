//! The four tables of PackageCatalogue.md *Data Model* and the statements over them. Every
//! statement is a literal of this crate (2B-1 rule 5) and every value is bound, never spliced into
//! the text.

use ordin_types::domain::agents::{
    AgentInstance, AgentInstanceStatus, AuditResult, Authority, StoppedBy,
};
use ordin_types::domain::HarnessKind;
use ordin_types::time::Timestamp;
use ordin_types::{AgentDraftId, AgentInstanceId, AgentRepositoryId, WorkspaceId};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use crate::error::PackageError;

/// The text a serde enum is stored as (`snake_case`).
pub(crate) fn enum_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

pub(crate) fn enum_from<T: DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
}

fn corrupt(what: &str) -> PackageError {
    PackageError::store(false, format!("a stored {what} value is not readable"))
}

fn timestamp(text: &str) -> Result<Timestamp, PackageError> {
    ordin_types::time::parse_rfc3339_ms_utc(text)
        .map(Timestamp)
        .map_err(|_| corrupt("timestamp"))
}

fn opt_timestamp(text: Option<String>) -> Result<Option<Timestamp>, PackageError> {
    text.as_deref().map(timestamp).transpose()
}

fn json_list(text: &str) -> Result<Vec<String>, PackageError> {
    serde_json::from_str(text).map_err(|_| corrupt("list"))
}

/// Where a stored pack choice came from (`agent_instance_choices.source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChoiceSource {
    PersonChange,
    AgentAsked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredChoice {
    pub rule_index: u32,
    pub packs: Vec<String>,
    pub source: ChoiceSource,
}

/// One `agent_instances` row.
#[derive(Debug, Clone)]
pub(crate) struct InstanceRow {
    pub id: AgentInstanceId,
    pub display_name: String,
    pub package_name: String,
    pub package_version: String,
    pub agent_repository_id: Option<AgentRepositoryId>,
    pub commit: Option<String>,
    pub agent_folder_path: String,
    pub lock_file_path: String,
    pub audit_result: Option<AuditResult>,
    pub authority: Authority,
    pub parsed_declaration_ref: String,
    pub is_trial_of_own_draft: bool,
    pub status: AgentInstanceStatus,
    pub updated_between_runs_at: Option<Timestamp>,
    pub installed_at: Timestamp,
    pub agent_identifier: String,
    pub agent_draft_id: Option<AgentDraftId>,
    pub stopped_reason: Option<String>,
    pub stopped_by: Option<StoppedBy>,
    pub may_do: Option<String>,
    pub plan_hash: Option<String>,
    pub pending_choices: String,
    pub harness: HarnessKind,
    /// The languages the instance was added with.
    pub workspace_languages: Vec<String>,
    /// The locale the instance was added with.
    pub interface_locale: String,
}

impl InstanceRow {
    pub(crate) fn to_instance(&self) -> AgentInstance {
        AgentInstance {
            id: self.id,
            display_name: self.display_name.clone(),
            package_name: self.package_name.clone(),
            package_version: self.package_version.clone(),
            agent_repository_id: self.agent_repository_id,
            commit: self.commit.clone(),
            agent_folder_path: self.agent_folder_path.clone(),
            lock_file_path: self.lock_file_path.clone(),
            audit_result: self.audit_result,
            authority: self.authority,
            parsed_declaration_ref: self.parsed_declaration_ref.clone(),
            is_trial_of_own_draft: self.is_trial_of_own_draft,
            status: self.status,
            stopped_reason: self.stopped_reason.clone(),
            stopped_by: self.stopped_by,
            updated_between_runs_at: self.updated_between_runs_at,
            installed_at: self.installed_at,
        }
    }
}

macro_rules! columns {
    () => {
        "id, display_name, package_name, package_version, agent_repository_id, \"commit\", \
         agent_folder_path, lock_file_path, audit_result, authority, parsed_declaration_ref, \
         is_trial_of_own_draft, status, updated_between_runs_at, installed_at, agent_identifier, \
         agent_draft_id, stopped_reason, stopped_by, may_do, plan_hash, pending_choices, harness, \
         workspace_languages, interface_locale"
    };
}

fn row_from(row: &SqliteRow) -> Result<InstanceRow, PackageError> {
    let text = |name: &str| -> Result<String, PackageError> { Ok(row.try_get::<String, _>(name)?) };
    let opt = |name: &str| -> Result<Option<String>, PackageError> {
        Ok(row.try_get::<Option<String>, _>(name)?)
    };
    let id = AgentInstanceId::parse(&text("id")?).map_err(|_| corrupt("instance id"))?;
    let repository = opt("agent_repository_id")?
        .map(|t| AgentRepositoryId::parse(&t).map_err(|_| corrupt("repository id")))
        .transpose()?;
    let draft = opt("agent_draft_id")?
        .map(|t| AgentDraftId::parse(&t).map_err(|_| corrupt("draft id")))
        .transpose()?;
    let audit = opt("audit_result")?
        .map(|t| enum_from::<AuditResult>(&t).ok_or_else(|| corrupt("audit result")))
        .transpose()?;
    let stopped_by = opt("stopped_by")?
        .map(|t| enum_from::<StoppedBy>(&t).ok_or_else(|| corrupt("stopped by")))
        .transpose()?;
    Ok(InstanceRow {
        id,
        display_name: text("display_name")?,
        package_name: text("package_name")?,
        package_version: text("package_version")?,
        agent_repository_id: repository,
        commit: opt("commit")?,
        agent_folder_path: text("agent_folder_path")?,
        lock_file_path: text("lock_file_path")?,
        audit_result: audit,
        authority: enum_from(&text("authority")?).ok_or_else(|| corrupt("authority"))?,
        parsed_declaration_ref: text("parsed_declaration_ref")?,
        is_trial_of_own_draft: row.try_get::<i64, _>("is_trial_of_own_draft")? != 0,
        status: enum_from(&text("status")?).ok_or_else(|| corrupt("status"))?,
        updated_between_runs_at: opt_timestamp(opt("updated_between_runs_at")?)?,
        installed_at: timestamp(&text("installed_at")?)?,
        agent_identifier: text("agent_identifier")?,
        agent_draft_id: draft,
        stopped_reason: opt("stopped_reason")?,
        stopped_by,
        may_do: opt("may_do")?,
        plan_hash: opt("plan_hash")?,
        pending_choices: text("pending_choices")?,
        harness: enum_from(&text("harness")?).ok_or_else(|| corrupt("harness"))?,
        workspace_languages: json_list(&text("workspace_languages")?)?,
        interface_locale: text("interface_locale")?,
    })
}

fn rows_from(rows: &[SqliteRow]) -> Result<Vec<InstanceRow>, PackageError> {
    rows.iter().map(row_from).collect()
}

pub(crate) async fn insert_instance(
    conn: &mut SqliteConnection,
    row: &InstanceRow,
) -> Result<(), PackageError> {
    sqlx::query(
        "INSERT INTO agent_instances (id, display_name, package_name, package_version, \
         agent_repository_id, \"commit\", agent_folder_path, lock_file_path, audit_result, authority, \
         parsed_declaration_ref, is_trial_of_own_draft, status, updated_between_runs_at, installed_at, \
         agent_identifier, agent_draft_id, stopped_reason, stopped_by, may_do, plan_hash, \
         pending_choices, harness, workspace_languages, interface_locale) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, \
         ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
    )
    .bind(row.id.to_string())
    .bind(&row.display_name)
    .bind(&row.package_name)
    .bind(&row.package_version)
    .bind(row.agent_repository_id.map(|i| i.to_string()))
    .bind(&row.commit)
    .bind(&row.agent_folder_path)
    .bind(&row.lock_file_path)
    .bind(row.audit_result.map(|a| enum_text(&a)))
    .bind(enum_text(&row.authority))
    .bind(&row.parsed_declaration_ref)
    .bind(i64::from(row.is_trial_of_own_draft))
    .bind(enum_text(&row.status))
    .bind(row.updated_between_runs_at.map(|t| t.to_wire()))
    .bind(row.installed_at.to_wire())
    .bind(&row.agent_identifier)
    .bind(row.agent_draft_id.map(|i| i.to_string()))
    .bind(&row.stopped_reason)
    .bind(row.stopped_by.map(|s| enum_text(&s)))
    .bind(&row.may_do)
    .bind(&row.plan_hash)
    .bind(&row.pending_choices)
    .bind(enum_text(&row.harness))
    .bind(serde_json::to_string(&row.workspace_languages).unwrap_or_else(|_| "[]".to_owned()))
    .bind(&row.interface_locale)
    .execute(conn)
    .await?;
    Ok(())
}

/// Writes every column that changes after the insert.
pub(crate) async fn save_instance(
    conn: &mut SqliteConnection,
    row: &InstanceRow,
) -> Result<(), PackageError> {
    sqlx::query(
        "UPDATE agent_instances SET display_name = ?2, package_name = ?3, package_version = ?4, \
         \"commit\" = ?5, agent_folder_path = ?6, lock_file_path = ?7, audit_result = ?8, \
         authority = ?9, parsed_declaration_ref = ?10, status = ?11, \
         updated_between_runs_at = ?12, stopped_reason = ?13, stopped_by = ?14, may_do = ?15, \
         plan_hash = ?16, pending_choices = ?17 WHERE id = ?1",
    )
    .bind(row.id.to_string())
    .bind(&row.display_name)
    .bind(&row.package_name)
    .bind(&row.package_version)
    .bind(&row.commit)
    .bind(&row.agent_folder_path)
    .bind(&row.lock_file_path)
    .bind(row.audit_result.map(|a| enum_text(&a)))
    .bind(enum_text(&row.authority))
    .bind(&row.parsed_declaration_ref)
    .bind(enum_text(&row.status))
    .bind(row.updated_between_runs_at.map(|t| t.to_wire()))
    .bind(&row.stopped_reason)
    .bind(row.stopped_by.map(|s| enum_text(&s)))
    .bind(&row.may_do)
    .bind(&row.plan_hash)
    .bind(&row.pending_choices)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn get_instance(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
) -> Result<Option<InstanceRow>, PackageError> {
    let row = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_instances WHERE id = ?1"
    ))
    .bind(id.to_string())
    .fetch_optional(conn)
    .await?;
    row.as_ref().map(row_from).transpose()
}

/// Deletes an instance's row; its choices, memberships and pending update go with it
/// (`ON DELETE CASCADE`).
pub(crate) async fn delete_instance(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
) -> Result<(), PackageError> {
    sqlx::query("DELETE FROM agent_instances WHERE id = ?1")
        .bind(id.to_string())
        .execute(conn)
        .await?;
    Ok(())
}

/// Every instance that is not `removed`, ordered by `display_name` then id.
pub(crate) async fn live_instances(
    conn: &mut SqliteConnection,
) -> Result<Vec<InstanceRow>, PackageError> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_instances WHERE status <> 'removed' ORDER BY display_name, id"
    ))
    .fetch_all(conn)
    .await?;
    rows_from(&rows)
}

/// The instances serving one workspace that are not `removed` (§K, §J, `list`).
pub(crate) async fn instances_in_workspace(
    conn: &mut SqliteConnection,
    workspace: WorkspaceId,
) -> Result<Vec<InstanceRow>, PackageError> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_instances WHERE status <> 'removed' AND id IN \
         (SELECT agent_instance_id FROM agent_instance_workspaces WHERE workspace_id = ?1) \
         ORDER BY display_name, id"
    ))
    .bind(workspace.to_string())
    .fetch_all(conn)
    .await?;
    rows_from(&rows)
}

/// The instances §I verifies: `ready` or `updating`, in a fixed order.
pub(crate) async fn verifiable_instances(
    conn: &mut SqliteConnection,
) -> Result<Vec<InstanceRow>, PackageError> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_instances WHERE status IN ('ready', 'updating') ORDER BY display_name, id"
    ))
    .fetch_all(conn)
    .await?;
    rows_from(&rows)
}

/// `ready` instances, and `refused` ones that a withdrawal stopped, of one repository (§C1, §C5).
pub(crate) async fn updatable_instances(
    conn: &mut SqliteConnection,
    repository: AgentRepositoryId,
) -> Result<Vec<InstanceRow>, PackageError> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_instances WHERE agent_repository_id = ?1 AND (status = 'ready' OR \
         (status = 'refused' AND stopped_by = 'withdrawal')) ORDER BY display_name, id"
    ))
    .bind(repository.to_string())
    .fetch_all(conn)
    .await?;
    rows_from(&rows)
}

/// `ready` instances, any repository (§D looks for the withdrawn part in each lock file).
pub(crate) async fn ready_instances(
    conn: &mut SqliteConnection,
) -> Result<Vec<InstanceRow>, PackageError> {
    let rows = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_instances WHERE status = 'ready' ORDER BY display_name, id"
    ))
    .fetch_all(conn)
    .await?;
    rows_from(&rows)
}

/// The instance of an own draft's trial that is not `removed` (§G).
pub(crate) async fn trial_of_draft(
    conn: &mut SqliteConnection,
    draft: AgentDraftId,
) -> Result<Option<InstanceRow>, PackageError> {
    let row = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM agent_instances WHERE agent_draft_id = ?1 AND status <> 'removed' LIMIT 1"
    ))
    .bind(draft.to_string())
    .fetch_optional(conn)
    .await?;
    row.as_ref().map(row_from).transpose()
}

/// An instance of the same agent identifier serving the workspace that is not `removed` (§A2).
pub(crate) async fn same_agent_in_workspace(
    conn: &mut SqliteConnection,
    workspace: WorkspaceId,
    identifier: &str,
) -> Result<Option<AgentInstanceId>, PackageError> {
    let row = sqlx::query(
        "SELECT i.id FROM agent_instances i JOIN agent_instance_workspaces w \
         ON w.agent_instance_id = i.id WHERE w.workspace_id = ?1 AND i.agent_identifier = ?2 \
         AND i.status <> 'removed' LIMIT 1",
    )
    .bind(workspace.to_string())
    .bind(identifier)
    .fetch_optional(conn)
    .await?;
    row.map(|r| {
        let text: String = r.try_get("id")?;
        AgentInstanceId::parse(&text).map_err(|_| corrupt("instance id"))
    })
    .transpose()
}

pub(crate) async fn workspaces_of(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
) -> Result<Vec<WorkspaceId>, PackageError> {
    let rows = sqlx::query(
        "SELECT workspace_id FROM agent_instance_workspaces WHERE agent_instance_id = ?1 \
         ORDER BY added_at, workspace_id",
    )
    .bind(id.to_string())
    .fetch_all(conn)
    .await?;
    rows.iter()
        .map(|r| {
            let text: String = r.try_get("workspace_id")?;
            WorkspaceId::parse(&text).map_err(|_| corrupt("workspace id"))
        })
        .collect()
}

/// PKG-DOC-2 step 1: whether the starter agent `identifier` was offered to `workspace` already.
pub(crate) async fn starter_offered(
    conn: &mut SqliteConnection,
    workspace: WorkspaceId,
    identifier: &str,
) -> Result<bool, PackageError> {
    let row = sqlx::query(
        "SELECT 1 FROM starter_agent_offers WHERE workspace_id = ?1 AND agent_identifier = ?2",
    )
    .bind(workspace.to_string())
    .bind(identifier)
    .fetch_optional(conn)
    .await?;
    Ok(row.is_some())
}

/// PKG-DOC-2: records that the starter agent `identifier` was offered to `workspace`; a second
/// record of the same pair writes nothing.
pub(crate) async fn record_starter_offer(
    conn: &mut SqliteConnection,
    workspace: WorkspaceId,
    identifier: &str,
) -> Result<(), PackageError> {
    sqlx::query(
        "INSERT OR IGNORE INTO starter_agent_offers (workspace_id, agent_identifier, offered_at) \
         VALUES (?1, ?2, ?3)",
    )
    .bind(workspace.to_string())
    .bind(identifier)
    .bind(Timestamp::now().to_wire())
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn add_membership(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
    workspace: WorkspaceId,
) -> Result<(), PackageError> {
    sqlx::query(
        "INSERT OR IGNORE INTO agent_instance_workspaces (agent_instance_id, workspace_id, added_at) \
         VALUES (?1, ?2, ?3)",
    )
    .bind(id.to_string())
    .bind(workspace.to_string())
    .bind(Timestamp::now().to_wire())
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn remove_membership(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
    workspace: WorkspaceId,
) -> Result<bool, PackageError> {
    let done = sqlx::query(
        "DELETE FROM agent_instance_workspaces WHERE agent_instance_id = ?1 AND workspace_id = ?2",
    )
    .bind(id.to_string())
    .bind(workspace.to_string())
    .execute(conn)
    .await?;
    Ok(done.rows_affected() > 0)
}

pub(crate) async fn choices_of(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
) -> Result<Vec<StoredChoice>, PackageError> {
    let rows = sqlx::query(
        "SELECT rule_index, packs, source FROM agent_instance_choices \
         WHERE agent_instance_id = ?1 ORDER BY rule_index",
    )
    .bind(id.to_string())
    .fetch_all(conn)
    .await?;
    rows.iter()
        .map(|r| {
            let index: i64 = r.try_get("rule_index")?;
            let packs: String = r.try_get("packs")?;
            let source: String = r.try_get("source")?;
            Ok(StoredChoice {
                rule_index: u32::try_from(index).map_err(|_| corrupt("rule index"))?,
                packs: json_list(&packs)?,
                source: enum_from(&source).ok_or_else(|| corrupt("choice source"))?,
            })
        })
        .collect()
}

pub(crate) async fn put_choice(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
    choice: &StoredChoice,
) -> Result<(), PackageError> {
    sqlx::query(
        "INSERT OR REPLACE INTO agent_instance_choices \
         (agent_instance_id, rule_index, packs, source, chosen_at) VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(id.to_string())
    .bind(i64::from(choice.rule_index))
    .bind(serde_json::to_string(&choice.packs).unwrap_or_else(|_| "[]".to_owned()))
    .bind(enum_text(&choice.source))
    .bind(Timestamp::now().to_wire())
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn delete_choice(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
    rule_index: u32,
) -> Result<(), PackageError> {
    sqlx::query(
        "DELETE FROM agent_instance_choices WHERE agent_instance_id = ?1 AND rule_index = ?2",
    )
    .bind(id.to_string())
    .bind(i64::from(rule_index))
    .execute(conn)
    .await?;
    Ok(())
}

/// One `agent_pending_updates` row.
#[derive(Debug, Clone)]
pub(crate) struct PendingRow {
    pub to_commit: String,
    pub may_do_changes: String,
    pub candidate_plan_hash: String,
}

pub(crate) async fn pending_of(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
) -> Result<Option<PendingRow>, PackageError> {
    let row = sqlx::query(
        "SELECT to_commit, may_do_changes, candidate_plan_hash FROM agent_pending_updates \
         WHERE agent_instance_id = ?1",
    )
    .bind(id.to_string())
    .fetch_optional(conn)
    .await?;
    row.map(|r| {
        Ok(PendingRow {
            to_commit: r.try_get("to_commit")?,
            may_do_changes: r.try_get("may_do_changes")?,
            candidate_plan_hash: r.try_get("candidate_plan_hash")?,
        })
    })
    .transpose()
}

pub(crate) async fn put_pending(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
    pending: &PendingRow,
) -> Result<(), PackageError> {
    sqlx::query(
        "INSERT OR REPLACE INTO agent_pending_updates \
         (agent_instance_id, to_commit, may_do_changes, candidate_plan_hash, found_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )
    .bind(id.to_string())
    .bind(&pending.to_commit)
    .bind(&pending.may_do_changes)
    .bind(&pending.candidate_plan_hash)
    .bind(Timestamp::now().to_wire())
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn delete_pending(
    conn: &mut SqliteConnection,
    id: AgentInstanceId,
) -> Result<(), PackageError> {
    sqlx::query("DELETE FROM agent_pending_updates WHERE agent_instance_id = ?1")
        .bind(id.to_string())
        .execute(conn)
        .await?;
    Ok(())
}

/// The plan hash of every instance not `removed` plus every pending update's candidate (§A9).
pub(crate) async fn live_plan_hashes(
    conn: &mut SqliteConnection,
) -> Result<Vec<String>, PackageError> {
    let rows = sqlx::query(
        "SELECT plan_hash AS hash FROM agent_instances WHERE status <> 'removed' AND plan_hash IS NOT NULL \
         UNION SELECT candidate_plan_hash AS hash FROM agent_pending_updates",
    )
    .fetch_all(conn)
    .await?;
    rows.iter()
        .map(|r| Ok(r.try_get::<String, _>("hash")?))
        .collect()
}
