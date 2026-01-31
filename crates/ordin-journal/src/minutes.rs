//! Minutes kept by Ordin (JournalAndArtefactStore.md JA-MIN-1 to JA-MIN-3). Each change runs in one
//! transaction with the `workspace_changed` event for its workspace, so the tree reloads.
//!
//! Erasure (JA-MIN-3) is `forget_workspace`, beside the dashboards, and `remove_workspace_minutes`.
//! Export is the desktop program's `minutes.export` (DSK-MIN-4), not the Journal's.

use ordin_types::wire::minutes::{Minutes, MinutesSummary};
use ordin_types::{Cursor, MinutesId, Page, RecordingId, RunId, WorkspaceId};

use crate::codec;
use crate::db::{self, Arg, Conditions, DbRow, Tx};
use crate::error::JournalError;
use crate::journal::{check_event, Journal};
use crate::records::add_keyset;
use crate::types::{MinutesUpdate, NewMinutes};
use crate::validate;

/// `list_minutes` answers 50 per page (JA-MIN-2).
const MINUTES_PAGE: usize = 50;

const MINUTES_COLUMNS: &str = "{s:id}, {s:workspace_id}, title, source, source_name, body, \
     {s:recording_id}, {s:run_id}, revision, {t:kept_at}, {t:edited_at}";

fn minutes_from(row: &DbRow) -> Result<Minutes, JournalError> {
    Ok(Minutes {
        id: MinutesId(codec::uuid(&row.text(0)?)?),
        workspace_id: WorkspaceId(codec::uuid(&row.text(1)?)?),
        title: row.text(2)?,
        source: codec::from_name(&row.text(3)?)?,
        source_name: row.text(4)?,
        body: row.text(5)?,
        recording: codec::opt_uuid(row.opt_text(6)?)?.map(RecordingId),
        run_id: codec::opt_uuid(row.opt_text(7)?)?.map(RunId),
        revision: codec::u32_of(row.int(8)?)?,
        kept_at: codec::ts(&row.text(9)?)?,
        edited_at: codec::opt_ts(row.opt_text(10)?)?,
    })
}

async fn minutes_in_tx(tx: &mut Tx, id: MinutesId) -> Result<Option<Minutes>, JournalError> {
    tx.fetch_optional(
        &format!("SELECT {MINUTES_COLUMNS} FROM minutes WHERE id = $1{{uuid}}"),
        &[Arg::Text(id.to_string())],
    )
    .await?
    .map(|row| minutes_from(&row))
    .transpose()
}

impl Journal {
    /// Appends `workspace_changed` inside `tx`, commits, then publishes the event.
    async fn commit_with_workspace_changed(
        &self,
        mut tx: Tx,
        workspace_id: WorkspaceId,
    ) -> Result<(), JournalError> {
        const KIND: &str = "workspace_changed";
        let data = serde_json::json!({ "workspace_id": workspace_id });
        let text = check_event(KIND, &data)?;
        let event = match self.append_in_tx(&mut tx, KIND, data, text).await {
            Ok(event) => event,
            Err(error) => return Err(self.append_failed(KIND, error)),
        };
        tx.commit().await?;
        self.published(event).await;
        Ok(())
    }

    /// Keeps new minutes with a UUIDv7 id at revision 1.
    // spec: ordin-desktop/components/JournalAndArtefactStore#JA-MIN-2
    pub async fn keep_minutes(&self, new: NewMinutes) -> Result<Minutes, JournalError> {
        validate::minutes_title(&new.title)?;
        validate::minutes_body(&new.body)?;
        validate::minutes_source_name(&new.source_name)?;
        let minutes = Minutes {
            id: MinutesId(uuid::Uuid::now_v7()),
            workspace_id: new.workspace_id,
            title: new.title,
            source: new.source,
            source_name: new.source_name,
            body: new.body,
            recording: new.recording,
            run_id: new.run_id,
            revision: 1,
            kept_at: self.now(),
            edited_at: None,
        };
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        tx.execute(
            "INSERT INTO minutes (id, workspace_id, title, source, source_name, body, recording_id, \
             run_id, revision, kept_at, edited_at) \
             VALUES ($1{uuid}, $2{uuid}, $3, $4, $5, $6, $7{uuid}, $8{uuid}, $9, $10{ts}, NULL)",
            &[
                Arg::Text(minutes.id.to_string()),
                Arg::Text(minutes.workspace_id.to_string()),
                Arg::Text(minutes.title.clone()),
                Arg::Text(codec::name_of(&minutes.source)?),
                Arg::Text(minutes.source_name.clone()),
                Arg::Text(minutes.body.clone()),
                Arg::OptText(minutes.recording.map(|r| r.to_string())),
                Arg::OptText(minutes.run_id.map(|r| r.to_string())),
                Arg::Int(1),
                Arg::Text(minutes.kept_at.to_wire()),
            ],
        )
        .await?;
        self.commit_with_workspace_changed(tx, minutes.workspace_id)
            .await?;
        Ok(minutes)
    }

    /// One minutes by id; `Ok(None)` when absent.
    pub async fn minutes(&self, id: MinutesId) -> Result<Option<Minutes>, JournalError> {
        db::fetch_optional(
            &self.inner.store,
            &format!("SELECT {MINUTES_COLUMNS} FROM minutes WHERE id = $1{{uuid}}"),
            &[Arg::Text(id.to_string())],
        )
        .await?
        .map(|row| minutes_from(&row))
        .transpose()
    }

    /// Newest first by `(kept_at DESC, id DESC)`, the order of index `minutes_workspace`; 50 a page.
    pub async fn list_minutes(
        &self,
        workspace_id: WorkspaceId,
        cursor: Option<Cursor>,
    ) -> Result<Page<MinutesSummary>, JournalError> {
        let mut conditions = Conditions::default();
        conditions.add(
            "workspace_id = #1{uuid}",
            vec![Arg::Text(workspace_id.to_string())],
        );
        add_keyset(&mut conditions, cursor.as_ref(), "kept_at", "id", "{uuid}")?;
        let sql = format!(
            "SELECT {{s:id}}, title, source, {{t:kept_at}} FROM minutes{} \
             ORDER BY kept_at DESC, id DESC LIMIT ${}",
            conditions.sql(),
            conditions.next()
        );
        let mut args = conditions.args;
        args.push(Arg::Int(i64::try_from(MINUTES_PAGE + 1).unwrap_or(51)));
        let rows = db::fetch_all(&self.inner.store, &sql, &args).await?;
        let mut items = rows
            .iter()
            .map(|row| {
                Ok(MinutesSummary {
                    id: MinutesId(codec::uuid(&row.text(0)?)?),
                    title: row.text(1)?,
                    source: codec::from_name(&row.text(2)?)?,
                    kept_at: codec::ts(&row.text(3)?)?,
                })
            })
            .collect::<Result<Vec<_>, JournalError>>()?;
        let next_cursor = if items.len() > MINUTES_PAGE {
            items.truncate(MINUTES_PAGE);
            items
                .last()
                .map(|last| Cursor::encode(&(last.kept_at, last.id.0)))
        } else {
            None
        };
        Ok(Page { items, next_cursor })
    }

    /// Replaces the body when `base_revision` is the stored one: revision + 1, `edited_at` now.
    pub async fn update_minutes_body(
        &self,
        id: MinutesId,
        body: String,
        base_revision: u32,
    ) -> Result<MinutesUpdate, JournalError> {
        validate::minutes_body(&body)?;
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        let Some(mut minutes) = minutes_in_tx(&mut tx, id).await? else {
            return Ok(MinutesUpdate::NotFound);
        };
        if minutes.revision != base_revision {
            return Ok(MinutesUpdate::Stale {
                current_revision: minutes.revision,
            });
        }
        minutes.revision = minutes.revision.saturating_add(1);
        minutes.body = body;
        minutes.edited_at = Some(self.now());
        tx.execute(
            "UPDATE minutes SET body = $1, revision = $2, edited_at = $3{ts} WHERE id = $4{uuid}",
            &[
                Arg::Text(minutes.body.clone()),
                Arg::Int(i64::from(minutes.revision)),
                Arg::OptText(minutes.edited_at.map(|t| t.to_wire())),
                Arg::Text(id.to_string()),
            ],
        )
        .await?;
        self.commit_with_workspace_changed(tx, minutes.workspace_id)
            .await?;
        Ok(MinutesUpdate::Updated(minutes))
    }

    /// Sets or clears the recording; an unknown id is `JRNL_NOT_FOUND`.
    pub async fn set_minutes_recording(
        &self,
        id: MinutesId,
        recording: Option<RecordingId>,
    ) -> Result<Minutes, JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        let mut minutes = minutes_in_tx(&mut tx, id)
            .await?
            .ok_or(JournalError::NotFound)?;
        minutes.recording = recording;
        tx.execute(
            "UPDATE minutes SET recording_id = $1{uuid} WHERE id = $2{uuid}",
            &[
                Arg::OptText(recording.map(|r| r.to_string())),
                Arg::Text(id.to_string()),
            ],
        )
        .await?;
        self.commit_with_workspace_changed(tx, minutes.workspace_id)
            .await?;
        Ok(minutes)
    }

    /// Removes one minutes; `Ok` when absent.
    pub async fn remove_minutes(&self, id: MinutesId) -> Result<(), JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        let Some(minutes) = minutes_in_tx(&mut tx, id).await? else {
            return Ok(());
        };
        tx.execute(
            "DELETE FROM minutes WHERE id = $1{uuid}",
            &[Arg::Text(id.to_string())],
        )
        .await?;
        self.commit_with_workspace_changed(tx, minutes.workspace_id)
            .await
    }

    /// Removes every minutes of a workspace; answers how many were removed.
    // spec: ordin-desktop/components/JournalAndArtefactStore#JA-MIN-3
    pub async fn remove_workspace_minutes(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<u32, JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        let removed = tx
            .execute(
                "DELETE FROM minutes WHERE workspace_id = $1{uuid}",
                &[Arg::Text(workspace_id.to_string())],
            )
            .await?;
        let removed = u32::try_from(removed).map_err(|_| JournalError::Unavailable)?;
        if removed == 0 {
            tx.commit().await?;
            return Ok(0);
        }
        self.commit_with_workspace_changed(tx, workspace_id).await?;
        Ok(removed)
    }
}
