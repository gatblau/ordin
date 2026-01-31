//! The SQLite statements of this crate's tables (OrdinHost.md *Data Model*). Every statement is
//! written once with bound values; nothing here logs a message text, a summary or a workflow body.

use ordin_service_api::blocks::Block;
use ordin_service_api::commands::ordin::{ConversationEnd, MessageAuthor, MessageKind};
use ordin_types::domain::runs::{CrewCharter, CrewCharterState};
use ordin_types::domain::{WorkflowOrigin, WorkflowShare, WorkflowState};
use ordin_types::time::Timestamp;
use ordin_types::wire::WorkflowFile;
use ordin_types::{BlockId, CrewCharterId, CrewRunId, RunId, WorkflowId, WorkspaceId};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::{Row, SqliteConnection};

use crate::ids::ConversationId;

pub(crate) type Conn = SqliteConnection;

pub(crate) fn word<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

pub(crate) fn from_word<T: DeserializeOwned>(text: &str) -> Result<T, sqlx::Error> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

pub(crate) fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, sqlx::Error> {
    serde_json::from_str(text).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

pub(crate) fn to_json<T: Serialize>(value: &T) -> Result<String, sqlx::Error> {
    serde_json::to_string(value).map_err(|e| sqlx::Error::Encode(Box::new(e)))
}

fn ts(text: &str) -> Result<Timestamp, sqlx::Error> {
    from_word(text)
}

fn id<T: std::str::FromStr>(text: &str) -> Result<T, sqlx::Error>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    text.parse::<T>()
        .map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

// ---------------------------------------------------------------- conversations

#[derive(Debug, Clone)]
pub(crate) struct ConversationRow {
    pub id: ConversationId,
    pub workspace_id: WorkspaceId,
    pub run_id: RunId,
    pub current: bool,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub ended_by: Option<ConversationEnd>,
    pub seed_summary: Option<String>,
    pub end_summary: Option<String>,
    pub continues_id: Option<ConversationId>,
}

const CONVERSATION_COLUMNS: &str =
    "id, workspace_id, run_id, state, started_at, ended_at, ended_by, \
     seed_summary, end_summary, continues_id";

fn conversation_from(row: &sqlx::sqlite::SqliteRow) -> Result<ConversationRow, sqlx::Error> {
    let ended_at: Option<String> = row.try_get("ended_at")?;
    let ended_by: Option<String> = row.try_get("ended_by")?;
    let continues: Option<String> = row.try_get("continues_id")?;
    Ok(ConversationRow {
        id: id(&row.try_get::<String, _>("id")?)?,
        workspace_id: id(&row.try_get::<String, _>("workspace_id")?)?,
        run_id: id(&row.try_get::<String, _>("run_id")?)?,
        current: row.try_get::<String, _>("state")? == "current",
        started_at: ts(&row.try_get::<String, _>("started_at")?)?,
        ended_at: ended_at.as_deref().map(ts).transpose()?,
        ended_by: ended_by.as_deref().map(from_word).transpose()?,
        seed_summary: row.try_get("seed_summary")?,
        end_summary: row.try_get("end_summary")?,
        continues_id: continues.as_deref().map(id).transpose()?,
    })
}

pub(crate) async fn current_conversation(
    c: &mut Conn,
    workspace_id: WorkspaceId,
) -> Result<Option<ConversationRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {CONVERSATION_COLUMNS} FROM ordin_conversations WHERE workspace_id = ? AND state = 'current'"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *c)
        .await?
        .as_ref()
        .map(conversation_from)
        .transpose()
}

pub(crate) async fn conversation(
    c: &mut Conn,
    conversation_id: ConversationId,
) -> Result<Option<ConversationRow>, sqlx::Error> {
    let sql = format!("SELECT {CONVERSATION_COLUMNS} FROM ordin_conversations WHERE id = ?");
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(conversation_id.to_string())
        .fetch_optional(&mut *c)
        .await?
        .as_ref()
        .map(conversation_from)
        .transpose()
}

pub(crate) async fn is_continued(
    c: &mut Conn,
    conversation_id: ConversationId,
) -> Result<bool, sqlx::Error> {
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ordin_conversations WHERE continues_id = ?")
            .bind(conversation_id.to_string())
            .fetch_one(&mut *c)
            .await?;
    Ok(n > 0)
}

pub(crate) async fn insert_conversation(
    c: &mut Conn,
    row: &ConversationRow,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO ordin_conversations (id, workspace_id, run_id, state, started_at, ended_at, ended_by, \
         seed_summary, end_summary, continues_id) VALUES (?, ?, ?, 'current', ?, NULL, NULL, ?, NULL, ?)",
    )
    .bind(row.id.to_string())
    .bind(row.workspace_id.to_string())
    .bind(row.run_id.to_string())
    .bind(row.started_at.to_wire())
    .bind(row.seed_summary.as_deref())
    .bind(row.continues_id.map(|c| c.to_string()))
    .execute(&mut *c)
    .await?;
    Ok(())
}

pub(crate) async fn end_conversation(
    c: &mut Conn,
    conversation_id: ConversationId,
    ended_by: ConversationEnd,
    end_summary: Option<&str>,
    at: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE ordin_conversations SET state = 'earlier', ended_at = ?, ended_by = ?, end_summary = ? \
         WHERE id = ? AND state = 'current'",
    )
    .bind(at.to_wire())
    .bind(word(&ended_by))
    .bind(end_summary)
    .bind(conversation_id.to_string())
    .execute(&mut *c)
    .await?;
    Ok(())
}

pub(crate) async fn set_conversation_run(
    c: &mut Conn,
    conversation_id: ConversationId,
    run_id: RunId,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE ordin_conversations SET run_id = ? WHERE id = ?")
        .bind(run_id.to_string())
        .bind(conversation_id.to_string())
        .execute(&mut *c)
        .await?;
    Ok(())
}

/// Every conversation of the workspace, newest first (for `ordin.list_conversations`).
pub(crate) async fn conversations_of(
    c: &mut Conn,
    workspace_id: WorkspaceId,
) -> Result<Vec<ConversationRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {CONVERSATION_COLUMNS} FROM ordin_conversations WHERE workspace_id = ? ORDER BY started_at DESC, id DESC"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(workspace_id.to_string())
        .fetch_all(&mut *c)
        .await?
        .iter()
        .map(conversation_from)
        .collect()
}

// ---------------------------------------------------------------- messages

#[derive(Debug, Clone)]
pub(crate) struct MessageRow {
    pub seq: u32,
    pub author: MessageAuthor,
    pub kind: Option<String>,
    pub text: String,
    pub block_id: Option<BlockId>,
    pub at: Timestamp,
}

pub(crate) async fn insert_message(
    c: &mut Conn,
    conversation_id: ConversationId,
    author: MessageAuthor,
    kind: Option<&str>,
    text: &str,
    block_id: Option<BlockId>,
    at: Timestamp,
) -> Result<u32, sqlx::Error> {
    // One statement: the next `seq` is read and written atomically, so two writers of the same
    // conversation (a reply and the person's message) never take the same number.
    let next: i64 = sqlx::query_scalar(
        "INSERT INTO ordin_messages (conversation_id, seq, author, kind, text, block_id, at) \
         SELECT ?1, coalesce(max(seq), 0) + 1, ?2, ?3, ?4, ?5, ?6 FROM ordin_messages WHERE conversation_id = ?1 \
         RETURNING seq",
    )
    .bind(conversation_id.to_string())
    .bind(word(&author))
    .bind(kind)
    .bind(text)
    .bind(block_id.map(|b| b.to_string()))
    .bind(at.to_wire())
    .fetch_one(&mut *c)
    .await?;
    Ok(u32::try_from(next).unwrap_or(u32::MAX))
}

fn message_from(row: &sqlx::sqlite::SqliteRow) -> Result<MessageRow, sqlx::Error> {
    let block: Option<String> = row.try_get("block_id")?;
    Ok(MessageRow {
        seq: u32::try_from(row.try_get::<i64, _>("seq")?).unwrap_or(0),
        author: from_word(&row.try_get::<String, _>("author")?)?,
        kind: row.try_get("kind")?,
        text: row.try_get("text")?,
        block_id: block.as_deref().map(id).transpose()?,
        at: ts(&row.try_get::<String, _>("at")?)?,
    })
}

pub(crate) async fn messages(
    c: &mut Conn,
    conversation_id: ConversationId,
) -> Result<Vec<MessageRow>, sqlx::Error> {
    sqlx::query(
        "SELECT seq, author, kind, text, block_id, at FROM ordin_messages WHERE conversation_id = ? ORDER BY seq",
    )
    .bind(conversation_id.to_string())
    .fetch_all(&mut *c)
    .await?
    .iter()
    .map(message_from)
    .collect()
}

/// The message row that holds a block's reference (step 49: it stays where it was written).
pub(crate) async fn message_of_block(
    c: &mut Conn,
    block_id: BlockId,
) -> Result<Option<MessageRow>, sqlx::Error> {
    sqlx::query("SELECT seq, author, kind, text, block_id, at FROM ordin_messages WHERE block_id = ? ORDER BY seq LIMIT 1")
        .bind(block_id.to_string())
        .fetch_optional(&mut *c)
        .await?
        .as_ref()
        .map(message_from)
        .transpose()
}

pub(crate) async fn person_message_count(
    c: &mut Conn,
    conversation_id: ConversationId,
) -> Result<u32, sqlx::Error> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ordin_messages WHERE conversation_id = ? AND author IN ('person','ordin')",
    )
    .bind(conversation_id.to_string())
    .fetch_one(&mut *c)
    .await?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

// ---------------------------------------------------------------- blocks

#[derive(Debug, Clone)]
pub(crate) struct BlockRow {
    pub block_id: BlockId,
    pub conversation_id: ConversationId,
    pub workspace_id: WorkspaceId,
    pub author: MessageAuthor,
    pub block: Block,
    pub waiting: bool,
    pub shown_at: Timestamp,
}

fn block_from(row: &sqlx::sqlite::SqliteRow) -> Result<BlockRow, sqlx::Error> {
    Ok(BlockRow {
        block_id: id(&row.try_get::<String, _>("block_id")?)?,
        conversation_id: id(&row.try_get::<String, _>("conversation_id")?)?,
        workspace_id: id(&row.try_get::<String, _>("workspace_id")?)?,
        author: from_word(&row.try_get::<String, _>("author")?)?,
        block: from_json(&row.try_get::<String, _>("body")?)?,
        waiting: row.try_get::<i64, _>("waiting")? == 1,
        shown_at: ts(&row.try_get::<String, _>("shown_at")?)?,
    })
}

const BLOCK_COLUMNS: &str =
    "block_id, conversation_id, workspace_id, author, body, waiting, shown_at";

pub(crate) async fn insert_block(c: &mut Conn, row: &BlockRow) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO ordin_blocks (block_id, conversation_id, workspace_id, author, body, waiting, shown_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(row.block_id.to_string())
    .bind(row.conversation_id.to_string())
    .bind(row.workspace_id.to_string())
    .bind(word(&row.author))
    .bind(to_json(&row.block)?)
    .bind(i64::from(row.waiting))
    .bind(row.shown_at.to_wire())
    .execute(&mut *c)
    .await?;
    Ok(())
}

pub(crate) async fn block(
    c: &mut Conn,
    block_id: BlockId,
) -> Result<Option<BlockRow>, sqlx::Error> {
    let sql = format!("SELECT {BLOCK_COLUMNS} FROM ordin_blocks WHERE block_id = ?");
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(block_id.to_string())
        .fetch_optional(&mut *c)
        .await?
        .as_ref()
        .map(block_from)
        .transpose()
}

pub(crate) async fn set_block_waiting(
    c: &mut Conn,
    block_id: BlockId,
    waiting: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE ordin_blocks SET waiting = ? WHERE block_id = ?")
        .bind(i64::from(waiting))
        .bind(block_id.to_string())
        .execute(&mut *c)
        .await?;
    Ok(())
}

/// Waiting blocks, oldest first; one workspace's or every workspace's.
pub(crate) async fn waiting_blocks(
    c: &mut Conn,
    workspace_id: Option<WorkspaceId>,
    limit: i64,
) -> Result<Vec<BlockRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {BLOCK_COLUMNS} FROM ordin_blocks WHERE waiting = 1 AND (? IS NULL OR workspace_id = ?) \
         ORDER BY shown_at, block_id LIMIT ?"
    );
    let ws = workspace_id.map(|w| w.to_string());
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(ws.clone())
        .bind(ws)
        .bind(limit)
        .fetch_all(&mut *c)
        .await?
        .iter()
        .map(block_from)
        .collect()
}

/// Step 49 (OH-CC-10): every waiting block of the workspace moves to the new conversation.
pub(crate) async fn move_waiting(
    c: &mut Conn,
    workspace_id: WorkspaceId,
    to: ConversationId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE ordin_blocks SET conversation_id = ? WHERE workspace_id = ? AND waiting = 1",
    )
    .bind(to.to_string())
    .bind(workspace_id.to_string())
    .execute(&mut *c)
    .await?;
    Ok(())
}

/// Blocks that belong to a conversation but whose message rows are in another one (carried items).
pub(crate) async fn carried_blocks(
    c: &mut Conn,
    conversation_id: ConversationId,
) -> Result<Vec<BlockRow>, sqlx::Error> {
    let sql = format!(
        "SELECT {BLOCK_COLUMNS} FROM ordin_blocks b WHERE b.conversation_id = ? AND NOT EXISTS \
         (SELECT 1 FROM ordin_messages m WHERE m.block_id = b.block_id AND m.conversation_id = b.conversation_id) \
         ORDER BY shown_at, block_id"
    );
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(conversation_id.to_string())
        .fetch_all(&mut *c)
        .await?
        .iter()
        .map(block_from)
        .collect()
}

// ---------------------------------------------------------------- workflows

#[derive(Debug, Clone)]
pub(crate) struct WorkflowRow {
    pub id: WorkflowId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub state: WorkflowState,
    pub origin: WorkflowOrigin,
    pub shared: Option<WorkflowShare>,
    pub created_at: Timestamp,
}

fn workflow_from(row: &sqlx::sqlite::SqliteRow) -> Result<WorkflowRow, sqlx::Error> {
    let shared: Option<String> = row.try_get("shared")?;
    Ok(WorkflowRow {
        id: id(&row.try_get::<String, _>("id")?)?,
        workspace_id: id(&row.try_get::<String, _>("workspace_id")?)?,
        name: row.try_get("name")?,
        state: from_word(&row.try_get::<String, _>("state")?)?,
        origin: from_json(&row.try_get::<String, _>("origin")?)?,
        shared: shared.as_deref().map(from_json).transpose()?,
        created_at: ts(&row.try_get::<String, _>("created_at")?)?,
    })
}

pub(crate) async fn insert_workflow(c: &mut Conn, w: &WorkflowRow) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO workflows (id, workspace_id, name, state, origin, shared, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(w.id.to_string())
    .bind(w.workspace_id.to_string())
    .bind(&w.name)
    .bind(word(&w.state))
    .bind(to_json(&w.origin)?)
    .bind(w.shared.as_ref().map(to_json).transpose()?)
    .bind(w.created_at.to_wire())
    .execute(&mut *c)
    .await?;
    Ok(())
}

pub(crate) async fn workflow(
    c: &mut Conn,
    workflow_id: WorkflowId,
) -> Result<Option<WorkflowRow>, sqlx::Error> {
    sqlx::query("SELECT id, workspace_id, name, state, origin, shared, created_at FROM workflows WHERE id = ?")
        .bind(workflow_id.to_string())
        .fetch_optional(&mut *c)
        .await?
        .as_ref()
        .map(workflow_from)
        .transpose()
}

pub(crate) async fn workflows_of(
    c: &mut Conn,
    workspace_id: WorkspaceId,
) -> Result<Vec<WorkflowRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, workspace_id, name, state, origin, shared, created_at FROM workflows WHERE workspace_id = ? ORDER BY name, id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut *c)
    .await?
    .iter()
    .map(workflow_from)
    .collect()
}

pub(crate) async fn shared_workflows(c: &mut Conn) -> Result<Vec<WorkflowRow>, sqlx::Error> {
    sqlx::query("SELECT id, workspace_id, name, state, origin, shared, created_at FROM workflows WHERE shared IS NOT NULL")
        .fetch_all(&mut *c)
        .await?
        .iter()
        .map(workflow_from)
        .collect()
}

pub(crate) async fn name_taken(
    c: &mut Conn,
    workspace_id: WorkspaceId,
    name: &str,
    except: Option<WorkflowId>,
) -> Result<bool, sqlx::Error> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflows WHERE workspace_id = ? AND name = ? AND id IS NOT ?",
    )
    .bind(workspace_id.to_string())
    .bind(name)
    .bind(except.map(|w| w.to_string()))
    .fetch_one(&mut *c)
    .await?;
    Ok(n > 0)
}

pub(crate) async fn update_workflow(c: &mut Conn, w: &WorkflowRow) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE workflows SET name = ?, state = ?, shared = ? WHERE id = ?")
        .bind(&w.name)
        .bind(word(&w.state))
        .bind(w.shared.as_ref().map(to_json).transpose()?)
        .bind(w.id.to_string())
        .execute(&mut *c)
        .await?;
    Ok(())
}

pub(crate) async fn delete_workflow(
    c: &mut Conn,
    workflow_id: WorkflowId,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM workflows WHERE id = ?")
        .bind(workflow_id.to_string())
        .execute(&mut *c)
        .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct VersionRow {
    pub version: u32,
    pub change_note: String,
    pub file: WorkflowFile,
    pub restored_from_version: Option<u32>,
    pub saved_at: Timestamp,
}

fn version_from(row: &sqlx::sqlite::SqliteRow) -> Result<VersionRow, sqlx::Error> {
    let restored: Option<i64> = row.try_get("restored_from_version")?;
    Ok(VersionRow {
        version: u32::try_from(row.try_get::<i64, _>("version")?).unwrap_or(0),
        change_note: row.try_get("change_note")?,
        file: from_json(&row.try_get::<String, _>("content")?)?,
        restored_from_version: restored.and_then(|r| u32::try_from(r).ok()),
        saved_at: ts(&row.try_get::<String, _>("saved_at")?)?,
    })
}

pub(crate) async fn versions(
    c: &mut Conn,
    workflow_id: WorkflowId,
) -> Result<Vec<VersionRow>, sqlx::Error> {
    sqlx::query(
        "SELECT version, change_note, content, restored_from_version, saved_at FROM workflow_versions \
         WHERE workflow_id = ? ORDER BY version DESC",
    )
    .bind(workflow_id.to_string())
    .fetch_all(&mut *c)
    .await?
    .iter()
    .map(version_from)
    .collect()
}

pub(crate) async fn version(
    c: &mut Conn,
    workflow_id: WorkflowId,
    version: u32,
) -> Result<Option<VersionRow>, sqlx::Error> {
    sqlx::query(
        "SELECT version, change_note, content, restored_from_version, saved_at FROM workflow_versions \
         WHERE workflow_id = ? AND version = ?",
    )
    .bind(workflow_id.to_string())
    .bind(i64::from(version))
    .fetch_optional(&mut *c)
    .await?
    .as_ref()
    .map(version_from)
    .transpose()
}

pub(crate) async fn latest_version(
    c: &mut Conn,
    workflow_id: WorkflowId,
) -> Result<Option<u32>, sqlx::Error> {
    let v: Option<i64> =
        sqlx::query_scalar("SELECT max(version) FROM workflow_versions WHERE workflow_id = ?")
            .bind(workflow_id.to_string())
            .fetch_one(&mut *c)
            .await?;
    Ok(v.and_then(|v| u32::try_from(v).ok()))
}

pub(crate) async fn insert_version(
    c: &mut Conn,
    workflow_id: WorkflowId,
    version: u32,
    change_note: &str,
    file: &WorkflowFile,
    restored_from: Option<u32>,
    at: Timestamp,
) -> Result<(), sqlx::Error> {
    use sha2::Digest;
    let content = to_json(file)?;
    let digest = sha2::Sha256::digest(content.as_bytes());
    let sha: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query(
        "INSERT INTO workflow_versions (workflow_id, version, purpose, change_note, content, content_sha256, \
         restored_from_version, saved_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(workflow_id.to_string())
    .bind(i64::from(version))
    .bind(&file.purpose)
    .bind(change_note)
    .bind(content)
    .bind(sha)
    .bind(restored_from.map(i64::from))
    .bind(at.to_wire())
    .execute(&mut *c)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct DraftRow {
    pub file: WorkflowFile,
}

pub(crate) async fn set_draft(
    c: &mut Conn,
    workflow_id: WorkflowId,
    base_version: Option<u32>,
    file: &WorkflowFile,
    report: &ordin_service_api::commands::workflows::WorkflowCheckReport,
    drafted_by: &str,
    at: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO workflow_drafts (workflow_id, base_version, content, check_report, drafted_by, drafted_at) \
         VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT (workflow_id) DO UPDATE SET base_version = excluded.base_version, \
         content = excluded.content, check_report = excluded.check_report, drafted_by = excluded.drafted_by, \
         drafted_at = excluded.drafted_at",
    )
    .bind(workflow_id.to_string())
    .bind(base_version.map(i64::from))
    .bind(to_json(file)?)
    .bind(to_json(report)?)
    .bind(drafted_by)
    .bind(at.to_wire())
    .execute(&mut *c)
    .await?;
    Ok(())
}

pub(crate) async fn draft(
    c: &mut Conn,
    workflow_id: WorkflowId,
) -> Result<Option<DraftRow>, sqlx::Error> {
    let row = sqlx::query("SELECT content FROM workflow_drafts WHERE workflow_id = ?")
        .bind(workflow_id.to_string())
        .fetch_optional(&mut *c)
        .await?;
    let Some(row) = row else { return Ok(None) };
    Ok(Some(DraftRow {
        file: from_json(&row.try_get::<String, _>("content")?)?,
    }))
}

pub(crate) async fn delete_draft(c: &mut Conn, workflow_id: WorkflowId) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM workflow_drafts WHERE workflow_id = ?")
        .bind(workflow_id.to_string())
        .execute(&mut *c)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- crew charters

#[derive(Debug, Clone)]
pub(crate) struct CharterRow {
    pub charter: CrewCharter,
    pub run_inputs: Option<serde_json::Value>,
}

fn charter_from(row: &sqlx::sqlite::SqliteRow) -> Result<CharterRow, sqlx::Error> {
    let mut charter: CrewCharter = from_json(&row.try_get::<String, _>("body")?)?;
    charter.state = from_word(&row.try_get::<String, _>("state")?)?;
    let inputs: Option<String> = row.try_get("run_inputs")?;
    Ok(CharterRow {
        charter,
        run_inputs: inputs.as_deref().map(from_json).transpose()?,
    })
}

pub(crate) async fn insert_charter(
    c: &mut Conn,
    charter: &CrewCharter,
    started_by: Option<&serde_json::Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO crew_charters (id, workspace_id, body, state, workflow_id, workflow_version, started_by, \
         crew_run_id, run_inputs, written_at) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, ?)",
    )
    .bind(charter.id.to_string())
    .bind(charter.workspace_id.to_string())
    .bind(to_json(charter)?)
    .bind(word(&charter.state))
    .bind(charter.workflow_id.map(|w| w.to_string()))
    .bind(charter.workflow_version.map(i64::from))
    .bind(started_by.map(to_json).transpose()?)
    .bind(charter.written_at.to_wire())
    .execute(&mut *c)
    .await?;
    Ok(())
}

pub(crate) async fn charter(
    c: &mut Conn,
    charter_id: CrewCharterId,
) -> Result<Option<CharterRow>, sqlx::Error> {
    sqlx::query("SELECT body, state, run_inputs FROM crew_charters WHERE id = ?")
        .bind(charter_id.to_string())
        .fetch_optional(&mut *c)
        .await?
        .as_ref()
        .map(charter_from)
        .transpose()
}

pub(crate) async fn charter_by_run(
    c: &mut Conn,
    crew_run_id: CrewRunId,
) -> Result<Option<CharterRow>, sqlx::Error> {
    sqlx::query("SELECT body, state, run_inputs FROM crew_charters WHERE crew_run_id = ?")
        .bind(crew_run_id.to_string())
        .fetch_optional(&mut *c)
        .await?
        .as_ref()
        .map(charter_from)
        .transpose()
}

pub(crate) async fn set_charter_state(
    c: &mut Conn,
    charter_id: CrewCharterId,
    state: CrewCharterState,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE crew_charters SET state = ? WHERE id = ?")
        .bind(word(&state))
        .bind(charter_id.to_string())
        .execute(&mut *c)
        .await?;
    Ok(())
}

pub(crate) async fn set_charter_started(
    c: &mut Conn,
    charter_id: CrewCharterId,
    crew_run_id: CrewRunId,
    run_inputs: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE crew_charters SET state = 'started', crew_run_id = ?, run_inputs = ? WHERE id = ?",
    )
    .bind(crew_run_id.to_string())
    .bind(to_json(run_inputs)?)
    .bind(charter_id.to_string())
    .execute(&mut *c)
    .await?;
    Ok(())
}

/// A changed crew asked in words: the earlier written (or valid, unstarted) charters are discarded.
pub(crate) async fn discard_unstarted(
    c: &mut Conn,
    workspace_id: WorkspaceId,
) -> Result<Vec<CrewCharterId>, sqlx::Error> {
    // Written first: inside a transaction a read before the write could not take the write lock
    // once another connection has written (SQLite answers busy at once, without waiting).
    let ids: Vec<String> = sqlx::query_scalar(
        "UPDATE crew_charters SET state = 'discarded' WHERE workspace_id = ? AND state IN ('written','valid') AND workflow_id IS NULL RETURNING id",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut *c)
    .await?;
    ids.iter().map(|i| id(i)).collect()
}

// ---------------------------------------------------------------- voice

pub(crate) async fn voice(
    c: &mut Conn,
) -> Result<Option<ordin_types::domain::VoiceSettings>, sqlx::Error> {
    let body: Option<String> = sqlx::query_scalar("SELECT body FROM voice_settings WHERE id = 1")
        .fetch_optional(&mut *c)
        .await?;
    body.as_deref().map(from_json).transpose()
}

pub(crate) async fn put_voice(
    c: &mut Conn,
    v: &ordin_types::domain::VoiceSettings,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO voice_settings (id, body, updated_at) VALUES (1, ?, ?) \
         ON CONFLICT (id) DO UPDATE SET body = excluded.body, updated_at = excluded.updated_at",
    )
    .bind(to_json(v)?)
    .bind(v.updated_at.to_wire())
    .execute(&mut *c)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------- removal and pruning

/// Step 44: one workspace's rows (messages, blocks, versions and drafts cascade).
pub(crate) async fn delete_workspace(
    c: &mut Conn,
    workspace_id: WorkspaceId,
) -> Result<(), sqlx::Error> {
    let ws = workspace_id.to_string();
    // Blocks carried into another conversation still cascade with their own conversation row.
    sqlx::query("DELETE FROM ordin_blocks WHERE workspace_id = ?")
        .bind(&ws)
        .execute(&mut *c)
        .await?;
    sqlx::query("DELETE FROM ordin_conversations WHERE workspace_id = ?")
        .bind(&ws)
        .execute(&mut *c)
        .await?;
    sqlx::query("DELETE FROM workflows WHERE workspace_id = ?")
        .bind(&ws)
        .execute(&mut *c)
        .await?;
    sqlx::query("DELETE FROM crew_charters WHERE workspace_id = ?")
        .bind(&ws)
        .execute(&mut *c)
        .await?;
    Ok(())
}

/// Step 1: history before the cut-off. A waiting block is never removed (A-08): its conversation is
/// kept while it holds one.
pub(crate) async fn prune(
    c: &mut Conn,
    cutoff: Timestamp,
    runs_in_progress: &[RunId],
) -> Result<u64, sqlx::Error> {
    let conversations = sqlx::query(
        "DELETE FROM ordin_conversations WHERE state = 'earlier' AND ended_at < ? AND NOT EXISTS \
         (SELECT 1 FROM ordin_blocks b WHERE b.conversation_id = ordin_conversations.id AND b.waiting = 1)",
    )
    .bind(cutoff.to_wire())
    .execute(&mut *c)
    .await?
    .rows_affected();
    let keep: Vec<String> = runs_in_progress.iter().map(|r| r.to_string()).collect();
    let candidates: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, crew_run_id FROM crew_charters WHERE state IN ('archived','discarded') AND written_at < ?",
    )
    .bind(cutoff.to_wire())
    .fetch_all(&mut *c)
    .await?;
    let mut charters = 0;
    for (charter_id, run) in candidates {
        if run.as_ref().is_some_and(|r| keep.contains(r)) {
            continue;
        }
        charters += sqlx::query("DELETE FROM crew_charters WHERE id = ?")
            .bind(charter_id)
            .execute(&mut *c)
            .await?
            .rows_affected();
    }
    Ok(conversations + charters)
}

pub(crate) fn message_kind_word(kind: MessageKind) -> &'static str {
    match kind {
        MessageKind::SlackTurn => "slack_turn",
    }
}
