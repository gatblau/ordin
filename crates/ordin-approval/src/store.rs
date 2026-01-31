//! The SQLite statements of the Approval Service (ApprovalService.md *Internal Logic* and
//! *Data Model*). Every write transaction opens with a write statement, so SQLite waits on the
//! busy timeout instead of failing a read-to-write upgrade.

use ordin_types::domain::runs::ApprovalState;
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::wire::approval::{AffectedItem, ApprovalAsker, ApprovalCard};
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{ApprovalId, Cursor, WorkspaceId};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};

use crate::error::ApprovalError;
use crate::model::{ApprovalRecord, ApprovalRequest, Ids};

/// Rows are read in pages of this size (Pagination.md; SA-PAGE-01).
pub(crate) const PAGE_SIZE: usize = ordin_types::paging::PAGE_SIZE;

/// The columns of one `ApprovalRecord`, then `created_at` (the paging key), then the caller's tail.
macro_rules! select_record {
    ($tail:literal) => {
        concat!(
            "SELECT id, workspace_id, action, destination, affected_records, consequence, card, \
             state, decided_by, decided_on, decided_at, created_at FROM approvals ",
            $tail
        )
    };
}

pub(crate) fn state_text(state: ApprovalState) -> &'static str {
    match state {
        ApprovalState::Pending => "pending",
        ApprovalState::Approved => "approved",
        ApprovalState::Rejected => "rejected",
        ApprovalState::Expired => "expired",
    }
}

pub(crate) fn parse_state(raw: &str) -> Result<ApprovalState, ApprovalError> {
    match raw {
        "pending" => Ok(ApprovalState::Pending),
        "approved" => Ok(ApprovalState::Approved),
        "rejected" => Ok(ApprovalState::Rejected),
        "expired" => Ok(ApprovalState::Expired),
        _ => Err(ApprovalError::corrupt("approval state")),
    }
}

fn parse_ts(raw: &str) -> Result<Timestamp, ApprovalError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| ApprovalError::corrupt("approval timestamp"))
}

fn parse_id(raw: &str) -> Result<ApprovalId, ApprovalError> {
    ApprovalId::parse(raw).map_err(|_| ApprovalError::corrupt("approval id"))
}

fn parse_workspace(raw: &str) -> Result<WorkspaceId, ApprovalError> {
    WorkspaceId::parse(raw).map_err(|_| ApprovalError::corrupt("workspace id"))
}

/// The label a person reads for an affected item (`Approval.affected_records` holds these).
pub(crate) fn affected_label(item: &AffectedItem) -> String {
    match item {
        AffectedItem::File { label, .. } | AffectedItem::Record { label } => label.clone(),
        AffectedItem::Recipient { name, .. } => name.clone(),
    }
}

/// A card button, as the `button` column names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Button {
    Approve,
    Reject,
    ShowDetails,
}

impl Button {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Button::Approve => "approve",
            Button::Reject => "reject",
            Button::ShowDetails => "show_details",
        }
    }

    fn parse(raw: &str) -> Result<Button, ApprovalError> {
        match raw {
            "approve" => Ok(Button::Approve),
            "reject" => Ok(Button::Reject),
            "show_details" => Ok(Button::ShowDetails),
            _ => Err(ApprovalError::corrupt("button")),
        }
    }
}

/// What C.1 and C.2 read about the pressed reference and its approval.
pub(crate) struct ButtonLookup {
    pub ids: Ids,
    pub button: Button,
    pub button_open: bool,
    pub state: ApprovalState,
    pub decided_on: Option<String>,
    pub asker: Option<String>,
    pub leaves_company: bool,
}

/// What a decided or expired approval says about itself.
pub(crate) struct StateRow {
    pub ids: Ids,
    pub state: ApprovalState,
    pub decided_on: Option<String>,
}

/// B.4: the approval row and one button row per reference, in one transaction.
pub(crate) async fn insert(
    pool: &SqlitePool,
    card: &ApprovalCard,
    req: &ApprovalRequest,
    refs: &[(Button, String)],
) -> Result<(), ApprovalError> {
    let (kind, run_id, crew_run_id, round) = match &card.asked_by {
        ApprovalAsker::Run { run_id } => ("run", Some(run_id.to_string()), None, None),
        ApprovalAsker::CrewRound { crew_run_id, round } => (
            "crew_round",
            None,
            Some(crew_run_id.to_string()),
            Some(i64::from(*round)),
        ),
        ApprovalAsker::Person => ("person", None, None, None),
    };
    // The proposal's affected items are kept whole, one JSON text per array element, so `get`
    // returns the proposal as it was asked.
    let records: Vec<String> = req
        .proposal
        .affects
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<_, _>>()
        .map_err(|_| ApprovalError::corrupt("affected records"))?;
    let records =
        serde_json::to_string(&records).map_err(|_| ApprovalError::corrupt("affected records"))?;
    let card_json =
        serde_json::to_string(card).map_err(|_| ApprovalError::corrupt("approval card"))?;
    let created_at = card.created_at.to_wire();

    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO approvals (id, workspace_id, asked_by_kind, asked_by_run_id, \
         asked_by_crew_run_id, asked_by_round, action, destination, affected_records, \
         consequence, card, asker, leaves_company, state, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?)",
    )
    .bind(card.approval_id.to_string())
    .bind(card.workspace_id.to_string())
    .bind(kind)
    .bind(run_id)
    .bind(crew_run_id)
    .bind(round)
    .bind(&req.proposal.action)
    .bind(&req.proposal.destination)
    .bind(records)
    .bind(&req.proposal.consequence)
    .bind(card_json)
    .bind(req.asker.as_deref())
    .bind(i64::from(req.leaves_company))
    .bind(&created_at)
    .execute(&mut *tx)
    .await?;
    for (button, card_ref) in refs {
        sqlx::query(
            "INSERT INTO approval_buttons (card_ref, approval_id, button, state) \
             VALUES (?, ?, ?, 'open')",
        )
        .bind(card_ref)
        .bind(card.approval_id.to_string())
        .bind(button.text())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// A.2: every pending approval's identifier.
pub(crate) async fn pending_ids(pool: &SqlitePool) -> Result<Vec<ApprovalId>, ApprovalError> {
    let rows = sqlx::query("SELECT id FROM approvals WHERE state = 'pending'")
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|row| parse_id(&row.try_get::<String, _>("id")?))
        .collect()
}

/// C.1: the pressed reference with its approval.
pub(crate) async fn button_lookup(
    pool: &SqlitePool,
    card_ref: &str,
) -> Result<Option<ButtonLookup>, ApprovalError> {
    let row = sqlx::query(
        "SELECT b.approval_id, b.button, b.state AS button_state, a.workspace_id, \
         a.state AS approval_state, a.decided_on, a.asker, a.leaves_company \
         FROM approval_buttons b JOIN approvals a ON a.id = b.approval_id WHERE b.card_ref = ?",
    )
    .bind(card_ref)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(ButtonLookup {
        ids: Ids {
            approval: parse_id(&row.try_get::<String, _>("approval_id")?)?,
            workspace: parse_workspace(&row.try_get::<String, _>("workspace_id")?)?,
        },
        button: Button::parse(&row.try_get::<String, _>("button")?)?,
        button_open: row.try_get::<String, _>("button_state")? == "open",
        state: parse_state(&row.try_get::<String, _>("approval_state")?)?,
        decided_on: row.try_get("decided_on")?,
        asker: row.try_get("asker")?,
        leaves_company: row.try_get::<i64, _>("leaves_company")? != 0,
    }))
}

/// The state, decision place and workspace of one approval.
pub(crate) async fn state_row(
    pool: &SqlitePool,
    id: ApprovalId,
) -> Result<Option<StateRow>, ApprovalError> {
    let row = sqlx::query("SELECT workspace_id, state, decided_on FROM approvals WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(StateRow {
        ids: Ids {
            approval: id,
            workspace: parse_workspace(&row.try_get::<String, _>("workspace_id")?)?,
        },
        state: parse_state(&row.try_get::<String, _>("state")?)?,
        decided_on: row.try_get("decided_on")?,
    }))
}

/// C.6: one statement decides once. Returns `false` when the approval was no longer pending
/// (another press won, or it expired); nothing is changed then.
pub(crate) async fn decide(
    pool: &SqlitePool,
    id: ApprovalId,
    state: ApprovalState,
    decided_by: &str,
    decided_on: &str,
    button: Button,
) -> Result<bool, ApprovalError> {
    let now = Timestamp::now().to_wire();
    let mut tx = pool.begin().await?;
    let won = sqlx::query(
        "UPDATE approvals SET state = ?, decided_by = ?, decided_on = ?, decided_at = ? \
         WHERE id = ? AND state = 'pending'",
    )
    .bind(state_text(state))
    .bind(decided_by)
    .bind(decided_on)
    .bind(&now)
    .bind(id.to_string())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if won == 0 {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE approval_buttons SET state = 'used', used_at = ? WHERE approval_id = ? AND button = ?",
    )
    .bind(&now)
    .bind(id.to_string())
    .bind(button.text())
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE approval_buttons SET state = 'closed' WHERE approval_id = ? AND button <> ?",
    )
    .bind(id.to_string())
    .bind(button.text())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// K.3: pending → `expired` for one approval, buttons closed. `false` when it was not pending.
pub(crate) async fn withdraw(pool: &SqlitePool, id: ApprovalId) -> Result<bool, ApprovalError> {
    let now = Timestamp::now().to_wire();
    let mut tx = pool.begin().await?;
    let changed = sqlx::query(
        "UPDATE approvals SET state = 'expired', decided_at = ? WHERE id = ? AND state = 'pending'",
    )
    .bind(&now)
    .bind(id.to_string())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Ok(false);
    }
    sqlx::query("UPDATE approval_buttons SET state = 'closed' WHERE approval_id = ?")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// Which asker column F.1 matches.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Asker {
    Run,
    CrewRun,
}

/// F.1: every pending approval of a run (or crew run) → `expired`, their buttons closed; returns
/// the identifiers with their workspaces.
pub(crate) async fn expire(
    pool: &SqlitePool,
    asker: Asker,
    asker_id: &str,
) -> Result<Vec<Ids>, ApprovalError> {
    let sql = match asker {
        Asker::Run => {
            "UPDATE approvals SET state = 'expired', decided_at = ? \
             WHERE state = 'pending' AND asked_by_run_id = ? RETURNING id, workspace_id"
        }
        Asker::CrewRun => {
            "UPDATE approvals SET state = 'expired', decided_at = ? \
             WHERE state = 'pending' AND asked_by_crew_run_id = ? RETURNING id, workspace_id"
        }
    };
    let now = Timestamp::now().to_wire();
    let mut tx = pool.begin().await?;
    let rows = sqlx::query(sql)
        .bind(&now)
        .bind(asker_id)
        .fetch_all(&mut *tx)
        .await?;
    let mut expired = Vec::with_capacity(rows.len());
    for row in &rows {
        let ids = Ids {
            approval: parse_id(&row.try_get::<String, _>("id")?)?,
            workspace: parse_workspace(&row.try_get::<String, _>("workspace_id")?)?,
        };
        sqlx::query("UPDATE approval_buttons SET state = 'closed' WHERE approval_id = ?")
            .bind(ids.approval.to_string())
            .execute(&mut *tx)
            .await?;
        expired.push(ids);
    }
    tx.commit().await?;
    Ok(expired)
}

fn record_from_row(row: &SqliteRow) -> Result<ApprovalRecord, ApprovalError> {
    let card: ApprovalCard = serde_json::from_str(&row.try_get::<String, _>("card")?)
        .map_err(|_| ApprovalError::corrupt("approval card"))?;
    let texts: Vec<String> = serde_json::from_str(&row.try_get::<String, _>("affected_records")?)
        .map_err(|_| ApprovalError::corrupt("affected records"))?;
    let affects = texts
        .iter()
        .map(|text| serde_json::from_str::<AffectedItem>(text))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ApprovalError::corrupt("affected records"))?;
    let decided_at: Option<String> = row.try_get("decided_at")?;
    Ok(ApprovalRecord {
        card,
        proposal: ActionProposal {
            action: row.try_get("action")?,
            destination: row.try_get("destination")?,
            affects,
            consequence: row.try_get("consequence")?,
        },
        state: parse_state(&row.try_get::<String, _>("state")?)?,
        decided_by: row.try_get("decided_by")?,
        decided_on: row.try_get("decided_on")?,
        decided_at: decided_at.as_deref().map(parse_ts).transpose()?,
    })
}

pub(crate) async fn get(
    pool: &SqlitePool,
    id: ApprovalId,
) -> Result<Option<ApprovalRecord>, ApprovalError> {
    let row = sqlx::query(select_record!("WHERE id = ?"))
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(record_from_row).transpose()
}

/// H.1 to H.3: one keyset page of pending approvals, oldest first.
pub(crate) async fn list_pending(
    pool: &SqlitePool,
    workspace: Option<WorkspaceId>,
    cursor: Option<Cursor>,
) -> Result<ordin_types::Page<ApprovalRecord>, ApprovalError> {
    let after = match cursor {
        Some(cursor) => Some(
            cursor
                .decode::<(String, ApprovalId)>()
                .map_err(|_| ApprovalError::BadCursor)?,
        ),
        None => None,
    };
    // Four fixed statements, so no SQL text is built at run time. The index `approvals_pending`
    // serves the first two; `LIMIT 101` reads one row past the page (H.1).
    let sql: &'static str = match (workspace.is_some(), after.is_some()) {
        (false, false) => select_record!(
            "WHERE state = 'pending' ORDER BY created_at ASC, id ASC LIMIT ?"
        ),
        (true, false) => select_record!(
            "WHERE state = 'pending' AND workspace_id = ? ORDER BY created_at ASC, id ASC LIMIT ?"
        ),
        (false, true) => select_record!(
            "WHERE state = 'pending' AND (created_at, id) > (?, ?) ORDER BY created_at ASC, id ASC LIMIT ?"
        ),
        (true, true) => select_record!(
            "WHERE state = 'pending' AND workspace_id = ? AND (created_at, id) > (?, ?) ORDER BY created_at ASC, id ASC LIMIT ?"
        ),
    };
    let mut query = sqlx::query(sql);
    if let Some(workspace) = workspace {
        query = query.bind(workspace.to_string());
    }
    if let Some((created_at, id)) = &after {
        query = query.bind(created_at.clone()).bind(id.to_string());
    }
    let rows = query.bind((PAGE_SIZE + 1) as i64).fetch_all(pool).await?;
    let mut items = Vec::with_capacity(rows.len().min(PAGE_SIZE));
    let mut last_key: Option<(String, ApprovalId)> = None;
    for row in rows.iter().take(PAGE_SIZE) {
        let record = record_from_row(row)?;
        last_key = Some((row.try_get("created_at")?, record.card.approval_id));
        items.push(record);
    }
    let next_cursor = if rows.len() > PAGE_SIZE {
        last_key.as_ref().map(Cursor::encode)
    } else {
        None
    };
    Ok(ordin_types::Page { items, next_cursor })
}

/// The journal's pruning of decided approvals (Data Model, *Pruning*): never a pending row, and
/// never a row asked by a run still in progress. Buttons go by cascade.
pub(crate) async fn prune(
    pool: &SqlitePool,
    cutoff: Timestamp,
    runs_in_progress: &[String],
) -> Result<u64, sqlx::Error> {
    let runs = serde_json::to_string(runs_in_progress).unwrap_or_else(|_| "[]".to_owned());
    let result = sqlx::query(
        "DELETE FROM approvals WHERE state <> 'pending' AND decided_at < ? \
         AND (asked_by_run_id IS NULL OR asked_by_run_id NOT IN (SELECT value FROM json_each(?)))",
    )
    .bind(cutoff.to_wire())
    .bind(runs)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
