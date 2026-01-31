//! The SQLite statements of the Push Service (PushService.md *Data Model*). Every row is written
//! before the step it records leaves the computer, so a restart resumes from the table.

use ordin_types::domain::agents::PullRequestRef;
use ordin_types::domain::places::{PendingPush, PendingPushItem, PendingPushState};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{
    ApprovalId, ArtefactId, ExternalActionKey, KnowledgePageId, PendingPushId, PlaceId,
    PublishedCopyId, RunId, SourceLinkId, TeamShareId, WorkingCopyItemId, WorkspaceId,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::error::PushError;
use crate::model::{PushProposal, SubmissionId};

/// Statements built only from this module's constant column lists and fixed state words.
fn dynamic(
    sql: String,
) -> sqlx::query::Query<'static, sqlx::Sqlite, sqlx::sqlite::SqliteArguments> {
    sqlx::query(sqlx::AssertSqlSafe(sql))
}

fn corrupt(what: &'static str) -> PushError {
    PushError::store(true, format!("a stored {what} no longer parses"))
}

fn ts(raw: &str) -> Result<Timestamp, PushError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| corrupt("timestamp"))
}

pub(crate) fn push_state_text(state: PendingPushState) -> &'static str {
    match state {
        PendingPushState::Proposed => "proposed",
        PendingPushState::Held => "held",
        PendingPushState::Approved => "approved",
        PendingPushState::Rejected => "rejected",
        PendingPushState::Closed => "closed",
    }
}

fn push_state(raw: &str) -> Result<PendingPushState, PushError> {
    match raw {
        "proposed" => Ok(PendingPushState::Proposed),
        "held" => Ok(PendingPushState::Held),
        "approved" => Ok(PendingPushState::Approved),
        "rejected" => Ok(PendingPushState::Rejected),
        "closed" => Ok(PendingPushState::Closed),
        _ => Err(corrupt("push state")),
    }
}

// ---- pending pushes ----

/// One item with the columns this crate adds to 2C's `PendingPushItem`.
#[derive(Debug, Clone)]
pub(crate) struct ItemRow {
    pub id: WorkingCopyItemId,
    pub summary: String,
    pub change_view: Option<ArtefactId>,
    pub ordins_local_version: u32,
    pub pending_action_key: Option<ExternalActionKey>,
    pub external_action_key: Option<ExternalActionKey>,
    pub result_code: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PushRow {
    pub push: PendingPush,
    pub run_id: Option<RunId>,
    pub items: Vec<ItemRow>,
}

fn item_from(row: &SqliteRow) -> Result<ItemRow, PushError> {
    let id: String = row.try_get("working_copy_item_id")?;
    let change_view: Option<String> = row.try_get("change_view")?;
    let pending: Option<String> = row.try_get("pending_action_key")?;
    let external: Option<String> = row.try_get("external_action_key")?;
    let local: i64 = row.try_get("ordins_local_version")?;
    Ok(ItemRow {
        id: WorkingCopyItemId::parse(&id).map_err(|_| corrupt("item id"))?,
        summary: row.try_get("summary")?,
        change_view: change_view
            .map(|v| ArtefactId::parse(&v).map_err(|_| corrupt("artefact id")))
            .transpose()?,
        ordins_local_version: u32::try_from(local).map_err(|_| corrupt("local version"))?,
        pending_action_key: pending
            .map(|v| ExternalActionKey::parse(&v).map_err(|_| corrupt("action key")))
            .transpose()?,
        external_action_key: external
            .map(|v| ExternalActionKey::parse(&v).map_err(|_| corrupt("action key")))
            .transpose()?,
        result_code: row.try_get("result_code")?,
    })
}

const PUSH_COLUMNS: &str =
    "id, workspace_id, run_id, request, origins, approval_id, state, created_at";

async fn push_from(pool: &SqlitePool, row: &SqliteRow) -> Result<PushRow, PushError> {
    let id_text: String = row.try_get("id")?;
    let id = PendingPushId::parse(&id_text).map_err(|_| corrupt("push id"))?;
    let workspace: String = row.try_get("workspace_id")?;
    let run: Option<String> = row.try_get("run_id")?;
    let origins: String = row.try_get("origins")?;
    let approval: Option<String> = row.try_get("approval_id")?;
    let state: String = row.try_get("state")?;
    let created: String = row.try_get("created_at")?;
    let item_rows = sqlx::query(
        "SELECT working_copy_item_id, summary, change_view, ordins_local_version, pending_action_key, \
         external_action_key, result_code FROM pending_push_items WHERE pending_push_id = ?1 ORDER BY position",
    )
    .bind(&id_text)
    .fetch_all(pool)
    .await?;
    let items: Vec<ItemRow> = item_rows.iter().map(item_from).collect::<Result<_, _>>()?;
    let origins: Vec<SourceLinkId> =
        serde_json::from_str(&origins).map_err(|_| corrupt("origins"))?;
    Ok(PushRow {
        push: PendingPush {
            id,
            workspace_id: WorkspaceId::parse(&workspace).map_err(|_| corrupt("workspace id"))?,
            request: row.try_get("request")?,
            items: items
                .iter()
                .map(|i| PendingPushItem {
                    working_copy_item_id: i.id,
                    summary: i.summary.clone(),
                    external_action_key: i.external_action_key,
                })
                .collect(),
            origins,
            approval_id: approval
                .map(|v| ApprovalId::parse(&v).map_err(|_| corrupt("approval id")))
                .transpose()?,
            state: push_state(&state)?,
            created_at: ts(&created)?,
        },
        run_id: run
            .map(|v| RunId::parse(&v).map_err(|_| corrupt("run id")))
            .transpose()?,
        items,
    })
}

/// A.7: the push and its items in one transaction.
pub(crate) async fn insert_push(
    pool: &SqlitePool,
    proposal: &PushProposal,
    id: PendingPushId,
    origins: &[SourceLinkId],
    local_versions: &[u32],
) -> Result<Timestamp, PushError> {
    let created = Timestamp::now();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO pending_pushes (id, workspace_id, run_id, request, origins, state, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'proposed', ?6)",
    )
    .bind(id.to_string())
    .bind(proposal.workspace_id.to_string())
    .bind(proposal.run_id.map(|r| r.to_string()))
    .bind(&proposal.request)
    .bind(serde_json::to_string(origins).map_err(|_| corrupt("origins"))?)
    .bind(created.to_wire())
    .execute(&mut *tx)
    .await?;
    for (position, (item, local)) in proposal.items.iter().zip(local_versions).enumerate() {
        sqlx::query(
            "INSERT INTO pending_push_items (pending_push_id, working_copy_item_id, position, summary, \
             change_view, ordins_local_version) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(id.to_string())
        .bind(item.working_copy_item_id.to_string())
        .bind(position as i64)
        .bind(&item.summary)
        .bind(item.change_view.map(|v| v.to_string()))
        .bind(i64::from(*local))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(created)
}

pub(crate) async fn delete_push(pool: &SqlitePool, id: PendingPushId) -> Result<(), PushError> {
    sqlx::query("DELETE FROM pending_pushes WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn get_push(
    pool: &SqlitePool,
    id: PendingPushId,
) -> Result<Option<PushRow>, PushError> {
    let row = dynamic(format!(
        "SELECT {PUSH_COLUMNS} FROM pending_pushes WHERE id = ?1"
    ))
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;
    match row {
        Some(row) => Ok(Some(push_from(pool, &row).await?)),
        None => Ok(None),
    }
}

pub(crate) async fn push_by_approval(
    pool: &SqlitePool,
    approval: ApprovalId,
) -> Result<Option<PushRow>, PushError> {
    let row = dynamic(format!(
        "SELECT {PUSH_COLUMNS} FROM pending_pushes WHERE approval_id = ?1"
    ))
    .bind(approval.to_string())
    .fetch_optional(pool)
    .await?;
    match row {
        Some(row) => Ok(Some(push_from(pool, &row).await?)),
        None => Ok(None),
    }
}

pub(crate) async fn list_pushes(
    pool: &SqlitePool,
    workspace: WorkspaceId,
) -> Result<Vec<PushRow>, PushError> {
    let rows = dynamic(format!(
        "SELECT {PUSH_COLUMNS} FROM pending_pushes WHERE workspace_id = ?1 \
         AND state IN ('proposed','held','approved') ORDER BY created_at, id"
    ))
    .bind(workspace.to_string())
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(push_from(pool, row).await?);
    }
    Ok(out)
}

/// The push in one of `states` that holds `item`.
pub(crate) async fn push_holding(
    pool: &SqlitePool,
    item: WorkingCopyItemId,
    states: &[&str],
) -> Result<Option<PendingPushId>, PushError> {
    let list = states
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(",");
    let row = dynamic(format!(
        "SELECT p.id FROM pending_pushes p JOIN pending_push_items i ON i.pending_push_id = p.id \
         WHERE i.working_copy_item_id = ?1 AND p.state IN ({list}) ORDER BY p.created_at DESC LIMIT 1"
    ))
    .bind(item.to_string())
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        let id: String = r.try_get("id")?;
        PendingPushId::parse(&id).map_err(|_| corrupt("push id"))
    })
    .transpose()
}

pub(crate) async fn set_push_state(
    pool: &SqlitePool,
    id: PendingPushId,
    state: PendingPushState,
) -> Result<(), PushError> {
    sqlx::query("UPDATE pending_pushes SET state = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(push_state_text(state))
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_push_approval(
    pool: &SqlitePool,
    id: PendingPushId,
    approval: ApprovalId,
) -> Result<(), PushError> {
    sqlx::query("UPDATE pending_pushes SET approval_id = ?2 WHERE id = ?1")
        .bind(id.to_string())
        .bind(approval.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn set_item_pending_key(
    pool: &SqlitePool,
    push: PendingPushId,
    item: WorkingCopyItemId,
    key: ExternalActionKey,
) -> Result<(), PushError> {
    sqlx::query(
        "UPDATE pending_push_items SET pending_action_key = ?3, result_code = NULL \
         WHERE pending_push_id = ?1 AND working_copy_item_id = ?2",
    )
    .bind(push.to_string())
    .bind(item.to_string())
    .bind(key.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn set_item_result(
    pool: &SqlitePool,
    push: PendingPushId,
    item: WorkingCopyItemId,
    code: &str,
) -> Result<(), PushError> {
    sqlx::query(
        "UPDATE pending_push_items SET result_code = ?3 WHERE pending_push_id = ?1 AND working_copy_item_id = ?2",
    )
    .bind(push.to_string())
    .bind(item.to_string())
    .bind(code)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn set_item_pushed(
    pool: &SqlitePool,
    push: PendingPushId,
    item: WorkingCopyItemId,
    key: Option<ExternalActionKey>,
) -> Result<(), PushError> {
    sqlx::query(
        "UPDATE pending_push_items SET external_action_key = ?3, result_code = NULL \
         WHERE pending_push_id = ?1 AND working_copy_item_id = ?2",
    )
    .bind(push.to_string())
    .bind(item.to_string())
    .bind(key.map(|k| k.to_string()))
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_item(
    pool: &SqlitePool,
    push: PendingPushId,
    item: WorkingCopyItemId,
) -> Result<(), PushError> {
    sqlx::query(
        "DELETE FROM pending_push_items WHERE pending_push_id = ?1 AND working_copy_item_id = ?2",
    )
    .bind(push.to_string())
    .bind(item.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// D (FU-113): the re-raised card counts for the version now on it.
pub(crate) async fn set_item_local_version(
    pool: &SqlitePool,
    push: PendingPushId,
    item: WorkingCopyItemId,
    version: u32,
) -> Result<(), PushError> {
    sqlx::query(
        "UPDATE pending_push_items SET ordins_local_version = ?3 \
         WHERE pending_push_id = ?1 AND working_copy_item_id = ?2",
    )
    .bind(push.to_string())
    .bind(item.to_string())
    .bind(i64::from(version))
    .execute(pool)
    .await?;
    Ok(())
}

/// G.1: the item whose `pending_action_key` is `key`.
pub(crate) async fn item_by_pending_key(
    pool: &SqlitePool,
    key: ExternalActionKey,
) -> Result<Option<(PendingPushId, ItemRow)>, PushError> {
    let row = sqlx::query(
        "SELECT pending_push_id, working_copy_item_id, summary, change_view, ordins_local_version, \
         pending_action_key, external_action_key, result_code FROM pending_push_items WHERE pending_action_key = ?1",
    )
    .bind(key.to_string())
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        let push: String = r.try_get("pending_push_id")?;
        Ok((
            PendingPushId::parse(&push).map_err(|_| corrupt("push id"))?,
            item_from(&r)?,
        ))
    })
    .transpose()
}

// ---- pull request submissions ----

#[derive(Debug, Clone)]
pub(crate) struct SubmissionRow {
    pub id: SubmissionId,
    pub origin: String,
    pub proposal: String,
    pub action_key: Option<ExternalActionKey>,
    pub pull_request: Option<PullRequestRef>,
    pub state: String,
}

fn submission_from(row: &SqliteRow) -> Result<SubmissionRow, PushError> {
    let id: String = row.try_get("id")?;
    let key: Option<String> = row.try_get("action_key")?;
    let pr: Option<String> = row.try_get("pull_request")?;
    Ok(SubmissionRow {
        id: SubmissionId(uuid::Uuid::parse_str(&id).map_err(|_| corrupt("submission id"))?),
        origin: row.try_get("origin")?,
        proposal: row.try_get("proposal")?,
        action_key: key
            .map(|v| ExternalActionKey::parse(&v).map_err(|_| corrupt("action key")))
            .transpose()?,
        pull_request: pr
            .map(|v| serde_json::from_str(&v).map_err(|_| corrupt("pull request")))
            .transpose()?,
        state: row.try_get("state")?,
    })
}

const SUBMISSION_COLUMNS: &str = "id, origin, proposal, action_key, pull_request, state";

pub(crate) async fn insert_submission(
    pool: &SqlitePool,
    id: SubmissionId,
    workspace: WorkspaceId,
    origin: &str,
    proposal: &str,
) -> Result<(), PushError> {
    let now = Timestamp::now().to_wire();
    sqlx::query(
        "INSERT INTO pull_request_submissions (id, workspace_id, origin, proposal, state, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, 'proposed', ?5, ?5)",
    )
    .bind(id.to_string())
    .bind(workspace.to_string())
    .bind(origin)
    .bind(proposal)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_submission(
    pool: &SqlitePool,
    id: SubmissionId,
) -> Result<(), PushError> {
    sqlx::query("DELETE FROM pull_request_submissions WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn submission_by_approval(
    pool: &SqlitePool,
    approval: ApprovalId,
) -> Result<Option<SubmissionRow>, PushError> {
    let row = dynamic(format!(
        "SELECT {SUBMISSION_COLUMNS} FROM pull_request_submissions WHERE approval_id = ?1"
    ))
    .bind(approval.to_string())
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(submission_from).transpose()
}

/// G.3: submissions approved whose pull request is not recorded.
pub(crate) async fn submissions_in_state(
    pool: &SqlitePool,
    state: &str,
) -> Result<Vec<SubmissionRow>, PushError> {
    let rows = dynamic(format!(
        "SELECT {SUBMISSION_COLUMNS} FROM pull_request_submissions WHERE state = ?1 ORDER BY created_at"
    ))
    .bind(state)
    .fetch_all(pool)
    .await?;
    rows.iter().map(submission_from).collect()
}

pub(crate) async fn update_submission(
    pool: &SqlitePool,
    id: SubmissionId,
    state: &str,
    approval: Option<ApprovalId>,
    key: Option<ExternalActionKey>,
    pull_request: Option<&PullRequestRef>,
    failure: Option<&str>,
) -> Result<(), PushError> {
    sqlx::query(
        "UPDATE pull_request_submissions SET state = ?2, approval_id = COALESCE(?3, approval_id), \
         action_key = COALESCE(?4, action_key), pull_request = COALESCE(?5, pull_request), \
         failure_code = ?6, updated_at = ?7 WHERE id = ?1",
    )
    .bind(id.to_string())
    .bind(state)
    .bind(approval.map(|a| a.to_string()))
    .bind(key.map(|k| k.to_string()))
    .bind(
        pull_request
            .map(|p| serde_json::to_string(p).map_err(|_| corrupt("pull request")))
            .transpose()?,
    )
    .bind(failure)
    .bind(Timestamp::now().to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

// ---- copy operations and audiences ----

#[derive(Debug, Clone)]
pub(crate) struct CopyOpRow {
    pub id: String,
    pub published_copy_id: PublishedCopyId,
    pub kind: String,
    pub action_key: Option<ExternalActionKey>,
    pub original_version: String,
    pub state: String,
}

fn copy_op_from(row: &SqliteRow) -> Result<CopyOpRow, PushError> {
    let copy: String = row.try_get("published_copy_id")?;
    let key: Option<String> = row.try_get("action_key")?;
    Ok(CopyOpRow {
        id: row.try_get("id")?,
        published_copy_id: PublishedCopyId::parse(&copy).map_err(|_| corrupt("copy id"))?,
        kind: row.try_get("kind")?,
        action_key: key
            .map(|v| ExternalActionKey::parse(&v).map_err(|_| corrupt("action key")))
            .transpose()?,
        original_version: row.try_get("original_version")?,
        state: row.try_get("state")?,
    })
}

const COPY_OP_COLUMNS: &str = "id, published_copy_id, kind, action_key, original_version, state";

pub(crate) async fn insert_copy_op(
    pool: &SqlitePool,
    copy: PublishedCopyId,
    kind: &str,
    approval: Option<ApprovalId>,
    original_version: &str,
    state: &str,
) -> Result<String, PushError> {
    let id = uuid::Uuid::now_v7().hyphenated().to_string();
    sqlx::query(
        "INSERT INTO copy_operations (id, published_copy_id, kind, approval_id, original_version, state, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(&id)
    .bind(copy.to_string())
    .bind(kind)
    .bind(approval.map(|a| a.to_string()))
    .bind(original_version)
    .bind(state)
    .bind(Timestamp::now().to_wire())
    .execute(pool)
    .await?;
    Ok(id)
}

pub(crate) async fn copy_ops_by_approval(
    pool: &SqlitePool,
    approval: ApprovalId,
) -> Result<Vec<CopyOpRow>, PushError> {
    let rows = dynamic(format!(
        "SELECT {COPY_OP_COLUMNS} FROM copy_operations WHERE approval_id = ?1 ORDER BY created_at, id"
    ))
    .bind(approval.to_string())
    .fetch_all(pool)
    .await?;
    rows.iter().map(copy_op_from).collect()
}

pub(crate) async fn copy_op_by_key(
    pool: &SqlitePool,
    key: ExternalActionKey,
) -> Result<Option<CopyOpRow>, PushError> {
    let row = dynamic(format!(
        "SELECT {COPY_OP_COLUMNS} FROM copy_operations WHERE action_key = ?1"
    ))
    .bind(key.to_string())
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(copy_op_from).transpose()
}

/// The copies that have at least one operation (the 15-minute comparison's candidates).
pub(crate) async fn copy_ids(pool: &SqlitePool) -> Result<Vec<PublishedCopyId>, PushError> {
    let rows = sqlx::query("SELECT DISTINCT published_copy_id FROM copy_operations")
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|r| {
            let id: String = r.try_get("published_copy_id")?;
            PublishedCopyId::parse(&id).map_err(|_| corrupt("copy id"))
        })
        .collect()
}

pub(crate) async fn set_copy_op(
    pool: &SqlitePool,
    id: &str,
    state: &str,
    key: Option<ExternalActionKey>,
) -> Result<(), PushError> {
    sqlx::query("UPDATE copy_operations SET state = ?2, action_key = COALESCE(?3, action_key) WHERE id = ?1")
        .bind(id)
        .bind(state)
        .bind(key.map(|k| k.to_string()))
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudienceRow {
    pub line: String,
    pub people: Option<u32>,
    pub everyone_who_can_open: bool,
    pub principals_digest: String,
}

pub(crate) async fn put_audience(
    pool: &SqlitePool,
    copy: PublishedCopyId,
    audience: &AudienceRow,
) -> Result<(), PushError> {
    sqlx::query(
        "INSERT INTO copy_audiences (published_copy_id, line, people, everyone_who_can_open, principals_digest, recorded_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(published_copy_id) DO UPDATE SET line = excluded.line, \
         people = excluded.people, everyone_who_can_open = excluded.everyone_who_can_open, \
         principals_digest = excluded.principals_digest, recorded_at = excluded.recorded_at",
    )
    .bind(copy.to_string())
    .bind(&audience.line)
    .bind(audience.people.map(i64::from))
    .bind(i64::from(audience.everyone_who_can_open))
    .bind(&audience.principals_digest)
    .bind(Timestamp::now().to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn get_audience(
    pool: &SqlitePool,
    copy: PublishedCopyId,
) -> Result<Option<AudienceRow>, PushError> {
    let row = sqlx::query(
        "SELECT line, people, everyone_who_can_open, principals_digest FROM copy_audiences WHERE published_copy_id = ?1",
    )
    .bind(copy.to_string())
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        let people: Option<i64> = r.try_get("people")?;
        let everyone: i64 = r.try_get("everyone_who_can_open")?;
        Ok(AudienceRow {
            line: r.try_get("line")?,
            people: people.map(|p| u32::try_from(p).unwrap_or(u32::MAX)),
            everyone_who_can_open: everyone != 0,
            principals_digest: r.try_get("principals_digest")?,
        })
    })
    .transpose()
}

// ---- team share submissions ----

#[derive(Debug, Clone)]
pub(crate) struct TeamShareRow {
    pub share_id: TeamShareId,
    pub workspace_id: WorkspaceId,
    pub approval_id: Option<ApprovalId>,
    pub state: String,
    pub clone_path: String,
    pub commit_id: String,
    pub host: String,
    pub repository: String,
    pub branch: String,
    pub review_branch: Option<String>,
    pub title: String,
    pub body: String,
    pub action_key: Option<ExternalActionKey>,
    pub refused_keys: Vec<ExternalActionKey>,
    pub moves: u32,
}

fn team_from(row: &SqliteRow) -> Result<TeamShareRow, PushError> {
    let share: String = row.try_get("share_id")?;
    let workspace: String = row.try_get("workspace_id")?;
    let approval: Option<String> = row.try_get("approval_id")?;
    let key: Option<String> = row.try_get("action_key")?;
    let refused: String = row.try_get("refused_keys")?;
    let moves: i64 = row.try_get("moves")?;
    Ok(TeamShareRow {
        share_id: TeamShareId::parse(&share).map_err(|_| corrupt("share id"))?,
        workspace_id: WorkspaceId::parse(&workspace).map_err(|_| corrupt("workspace id"))?,
        approval_id: approval
            .map(|v| ApprovalId::parse(&v).map_err(|_| corrupt("approval id")))
            .transpose()?,
        state: row.try_get("state")?,
        clone_path: row.try_get("clone_path")?,
        commit_id: row.try_get("commit_id")?,
        host: row.try_get("host")?,
        repository: row.try_get("repository")?,
        branch: row.try_get("branch")?,
        review_branch: row.try_get("review_branch")?,
        title: row.try_get("title")?,
        body: row.try_get("body")?,
        action_key: key
            .map(|v| ExternalActionKey::parse(&v).map_err(|_| corrupt("action key")))
            .transpose()?,
        refused_keys: serde_json::from_str(&refused).map_err(|_| corrupt("refused keys"))?,
        moves: u32::try_from(moves).unwrap_or(u32::MAX),
    })
}

const TEAM_COLUMNS: &str =
    "share_id, workspace_id, approval_id, state, clone_path, commit_id, host, \
     repository, branch, review_branch, title, body, action_key, refused_keys, moves";

pub(crate) async fn insert_team_share(
    pool: &SqlitePool,
    p: &crate::model::TeamShareProposal,
) -> Result<(), PushError> {
    let now = Timestamp::now().to_wire();
    sqlx::query(
        "INSERT INTO team_share_submissions (share_id, workspace_id, state, clone_path, commit_id, host, \
         repository, branch, review_branch, title, body, created_at, updated_at) \
         VALUES (?1, ?2, 'proposed', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
    )
    .bind(p.share_id.to_string())
    .bind(p.workspace_id.to_string())
    .bind(p.clone_path.to_string_lossy().into_owned())
    .bind(&p.commit)
    .bind(&p.host)
    .bind(&p.repository)
    .bind(&p.branch)
    .bind(&p.review_branch)
    .bind(&p.title)
    .bind(&p.body)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn delete_team_share(pool: &SqlitePool, id: TeamShareId) -> Result<(), PushError> {
    sqlx::query("DELETE FROM team_share_submissions WHERE share_id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn get_team_share(
    pool: &SqlitePool,
    id: TeamShareId,
) -> Result<Option<TeamShareRow>, PushError> {
    let row = dynamic(format!(
        "SELECT {TEAM_COLUMNS} FROM team_share_submissions WHERE share_id = ?1"
    ))
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(team_from).transpose()
}

pub(crate) async fn team_share_by_approval(
    pool: &SqlitePool,
    approval: ApprovalId,
) -> Result<Option<TeamShareRow>, PushError> {
    let row = dynamic(format!(
        "SELECT {TEAM_COLUMNS} FROM team_share_submissions WHERE approval_id = ?1"
    ))
    .bind(approval.to_string())
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(team_from).transpose()
}

pub(crate) async fn team_shares_in_state(
    pool: &SqlitePool,
    state: &str,
) -> Result<Vec<TeamShareRow>, PushError> {
    let rows = dynamic(format!(
        "SELECT {TEAM_COLUMNS} FROM team_share_submissions WHERE state = ?1 ORDER BY created_at"
    ))
    .bind(state)
    .fetch_all(pool)
    .await?;
    rows.iter().map(team_from).collect()
}

/// One statement for every column a transition may change; `None` leaves a column as it is.
pub(crate) struct TeamUpdate<'a> {
    pub state: &'a str,
    pub approval: Option<ApprovalId>,
    pub commit: Option<&'a str>,
    pub key: Option<ExternalActionKey>,
    pub pull_request: Option<&'a PullRequestRef>,
    pub refused_keys: Option<&'a [ExternalActionKey]>,
    pub moves: Option<u32>,
    pub result_code: Option<&'a str>,
}

pub(crate) async fn update_team_share(
    pool: &SqlitePool,
    id: TeamShareId,
    u: TeamUpdate<'_>,
) -> Result<(), PushError> {
    sqlx::query(
        "UPDATE team_share_submissions SET state = ?2, approval_id = COALESCE(?3, approval_id), \
         commit_id = COALESCE(?4, commit_id), action_key = COALESCE(?5, action_key), \
         pull_request = COALESCE(?6, pull_request), refused_keys = COALESCE(?7, refused_keys), \
         moves = COALESCE(?8, moves), result_code = COALESCE(?9, result_code), updated_at = ?10 \
         WHERE share_id = ?1",
    )
    .bind(id.to_string())
    .bind(u.state)
    .bind(u.approval.map(|a| a.to_string()))
    .bind(u.commit)
    .bind(u.key.map(|k| k.to_string()))
    .bind(
        u.pull_request
            .map(|p| serde_json::to_string(p).map_err(|_| corrupt("pull request")))
            .transpose()?,
    )
    .bind(
        u.refused_keys
            .map(|k| serde_json::to_string(k).map_err(|_| corrupt("refused keys")))
            .transpose()?,
    )
    .bind(u.moves.map(i64::from))
    .bind(u.result_code)
    .bind(Timestamp::now().to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

// ---- facts a first publish and a pull request used to keep in memory only (PushService.md Data Model, step 3) ----

/// A first publish's request facts: the copy's workspace and names.
#[derive(Debug, Clone)]
pub(crate) struct CopyRequestRow {
    pub workspace_id: WorkspaceId,
    pub original_name: String,
    pub file_name: String,
}

pub(crate) async fn put_copy_request(
    pool: &SqlitePool,
    copy: PublishedCopyId,
    row: &CopyRequestRow,
) -> Result<(), PushError> {
    sqlx::query(
        "INSERT INTO copy_requests (published_copy_id, workspace_id, original_name, file_name) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT(published_copy_id) DO UPDATE SET \
         workspace_id = excluded.workspace_id, original_name = excluded.original_name, \
         file_name = excluded.file_name",
    )
    .bind(copy.to_string())
    .bind(row.workspace_id.to_string())
    .bind(&row.original_name)
    .bind(&row.file_name)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn get_copy_request(
    pool: &SqlitePool,
    copy: PublishedCopyId,
) -> Result<Option<CopyRequestRow>, PushError> {
    let row = sqlx::query(
        "SELECT workspace_id, original_name, file_name FROM copy_requests WHERE published_copy_id = ?1",
    )
    .bind(copy.to_string())
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        let workspace: String = row.get(0);
        Ok(CopyRequestRow {
            workspace_id: WorkspaceId::parse(&workspace).map_err(|_| corrupt("workspace id"))?,
            original_name: row.get(1),
            file_name: row.get(2),
        })
    })
    .transpose()
}

pub(crate) async fn put_page_file(
    pool: &SqlitePool,
    page: KnowledgePageId,
    place: PlaceId,
    path: &str,
) -> Result<(), PushError> {
    sqlx::query(
        "INSERT INTO knowledge_page_files (knowledge_page_id, place_id, path) VALUES (?1, ?2, ?3) \
         ON CONFLICT(knowledge_page_id) DO UPDATE SET place_id = excluded.place_id, path = excluded.path",
    )
    .bind(page.to_string())
    .bind(place.to_string())
    .bind(path)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn get_page_file(
    pool: &SqlitePool,
    page: KnowledgePageId,
) -> Result<Option<(PlaceId, String)>, PushError> {
    let row =
        sqlx::query("SELECT place_id, path FROM knowledge_page_files WHERE knowledge_page_id = ?1")
            .bind(page.to_string())
            .fetch_optional(pool)
            .await?;
    row.map(|row| {
        let place: String = row.get(0);
        Ok((
            PlaceId::parse(&place).map_err(|_| corrupt("place id"))?,
            row.get(1),
        ))
    })
    .transpose()
}

/// The knowledge pages whose copies are made from the file `path` of `place`.
pub(crate) async fn pages_made_from(
    pool: &SqlitePool,
    place: PlaceId,
    path: &str,
) -> Result<Vec<KnowledgePageId>, PushError> {
    sqlx::query(
        "SELECT knowledge_page_id FROM knowledge_page_files WHERE place_id = ?1 AND path = ?2",
    )
    .bind(place.to_string())
    .bind(path)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        let id: String = row.get(0);
        KnowledgePageId::parse(&id).map_err(|_| corrupt("knowledge page id"))
    })
    .collect()
}

pub(crate) async fn put_pull_request_files(
    pool: &SqlitePool,
    id: SubmissionId,
    files: &[(String, Vec<u8>)],
) -> Result<(), PushError> {
    for (position, (path, content)) in files.iter().enumerate() {
        sqlx::query(
            "INSERT INTO pull_request_files (submission_id, position, path, content) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(id.to_string())
        .bind(position as i64)
        .bind(path)
        .bind(content)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub(crate) async fn get_pull_request_files(
    pool: &SqlitePool,
    id: SubmissionId,
) -> Result<Vec<(String, Vec<u8>)>, PushError> {
    Ok(sqlx::query(
        "SELECT path, content FROM pull_request_files WHERE submission_id = ?1 ORDER BY position",
    )
    .bind(id.to_string())
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| (row.get(0), row.get(1)))
    .collect())
}

pub(crate) async fn delete_pull_request_files(
    pool: &SqlitePool,
    id: SubmissionId,
) -> Result<(), PushError> {
    sqlx::query("DELETE FROM pull_request_files WHERE submission_id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}
