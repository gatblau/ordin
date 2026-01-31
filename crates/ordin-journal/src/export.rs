//! The support export (JournalAndArtefactStore.md Internal Logic 7.4; Logging.md E-1).

use ordin_types::time::Timestamp;
use ordin_types::RunId;
use serde_json::{json, Value};

use crate::codec;
use crate::db::{self, Arg};
use crate::error::JournalError;
use crate::journal::Journal;
use crate::plans::{step_from, STEP_COLUMNS};
use crate::records::{
    action_from, entry_from, startup_from, subject_key, tool_request_from, ACTION_COLUMNS,
    ENTRY_COLUMNS, STARTUP_COLUMNS, TOOL_REQUEST_COLUMNS,
};
use crate::types::SupportBundle;

/// At most 20 runs per export.
const MAX_EXPORT_RUNS: usize = 20;
/// The bundle is at most 10,000,000 bytes.
const MAX_BUNDLE_BYTES: usize = 10_000_000;
/// The newest entries with no run that are added to every bundle.
const UNLINKED_ENTRIES: i64 = 500;

/// One line with the field `record` naming what it is *(field name chosen here)*.
fn line(record: &str, value: impl serde::Serialize) -> Result<Value, JournalError> {
    let mut value = serde_json::to_value(value).map_err(|_| JournalError::Unavailable)?;
    if let Value::Object(map) = &mut value {
        map.insert("record".to_owned(), Value::String(record.to_owned()));
    }
    Ok(value)
}

impl Journal {
    /// The support bundle for at most 20 runs, as JSON Lines bytes; the program writes it
    /// where the person chose (step 7.4).
    pub async fn support_export(&self, run_ids: &[RunId]) -> Result<SupportBundle, JournalError> {
        if run_ids.is_empty() || run_ids.len() > MAX_EXPORT_RUNS {
            return Err(JournalError::ExportInvalid);
        }
        let store = &self.inner.store;
        let ids: Vec<Arg> = run_ids.iter().map(|r| Arg::Text(r.to_string())).collect();
        let in_list = db::placeholders(1, run_ids.len(), "{uuid}");
        // (time, line) pairs; sorted oldest first so the byte limit cuts the newest lines.
        let mut lines: Vec<(Timestamp, Value)> = Vec::new();

        let entries = db::fetch_all(
            store,
            &format!("SELECT {ENTRY_COLUMNS} FROM journal_entries WHERE run_id IN ({in_list})"),
            &ids,
        )
        .await?;
        for row in &entries {
            let entry = entry_from(row)?;
            lines.push((entry.recorded_at, line("journal_entry", &entry)?));
        }
        let unlinked = db::fetch_all(
            store,
            &format!(
                "SELECT {ENTRY_COLUMNS} FROM journal_entries WHERE run_id IS NULL \
                 ORDER BY recorded_at DESC, id DESC LIMIT $1"
            ),
            &[Arg::Int(UNLINKED_ENTRIES)],
        )
        .await?;
        for row in &unlinked {
            let entry = entry_from(row)?;
            lines.push((entry.recorded_at, line("journal_entry", &entry)?));
        }
        // Events: `seq`, `at` and `kind`, never `data`.
        let events = db::fetch_all(
            store,
            &format!("SELECT seq, {{t:at}}, kind FROM journal_events WHERE run_id IN ({in_list})"),
            &ids,
        )
        .await?;
        for row in &events {
            let at = codec::ts(&row.text(1)?)?;
            let event = json!({"seq": codec::u64_of(row.int(0)?)?, "at": at, "kind": row.text(2)?});
            lines.push((at, line("event", event)?));
        }
        // Tool requests: the stored row holds the arguments only as their digest.
        let requests = db::fetch_all(
            store,
            &format!(
                "SELECT {TOOL_REQUEST_COLUMNS} FROM tool_requests WHERE run_id IN ({in_list})"
            ),
            &ids,
        )
        .await?;
        for row in &requests {
            let request = tool_request_from(row)?;
            lines.push((request.requested_at, line("tool_request", &request)?));
        }
        // External actions without `destination`.
        let actions = db::fetch_all(
            store,
            &format!("SELECT {ACTION_COLUMNS} FROM external_actions WHERE run_id IN ({in_list})"),
            &ids,
        )
        .await?;
        for row in &actions {
            let action = action_from(row)?;
            let mut value = line("external_action", &action)?;
            if let Value::Object(map) = &mut value {
                map.remove("destination");
            }
            lines.push((action.sent_at, value));
        }
        let subjects: Vec<Arg> = run_ids
            .iter()
            .map(|run_id| {
                Arg::Text(subject_key(
                    &ordin_types::domain::runs::StartupSubject::Run { run_id: *run_id },
                ))
            })
            .collect();
        let reports = db::fetch_all(
            store,
            &format!(
                "SELECT {STARTUP_COLUMNS} FROM startup_reports WHERE subject_key IN ({})",
                db::placeholders(1, run_ids.len(), "")
            ),
            &subjects,
        )
        .await?;
        for row in &reports {
            let report = startup_from(row)?;
            lines.push((report.generated_at, line("startup_report", &report)?));
        }
        // Run steps without `line`.
        let steps = db::fetch_all(
            store,
            &format!("SELECT {STEP_COLUMNS} FROM run_steps WHERE run_id IN ({in_list})"),
            &ids,
        )
        .await?;
        for row in &steps {
            let step = step_from(row)?;
            let at = step.marks_set.first().map_or_else(|| self.now(), |m| m.at);
            let mut value = line("run_step", &step)?;
            if let Value::Object(map) = &mut value {
                map.remove("line");
            }
            lines.push((at, value));
        }
        lines.sort_by_key(|line| line.0);

        let made_at = self.now();
        let header = json!({
            "format": "ordin-support",
            "version": 1,
            "backend": self.backend_uuid(),
            "made_at": made_at,
        });
        let mut bytes = Vec::new();
        let mut written = 0usize;
        let mut truncated = false;
        for value in std::iter::once(header).chain(lines.into_iter().map(|(_, v)| v)) {
            let mut text = serde_json::to_vec(&value).map_err(|_| JournalError::Unavailable)?;
            text.push(b'\n');
            if bytes.len() + text.len() > MAX_BUNDLE_BYTES {
                truncated = true;
                break;
            }
            bytes.extend_from_slice(&text);
            written += 1;
        }
        tracing::info!(
            component = "journal",
            lines = written,
            truncated,
            "journal.support_export.done"
        );
        Ok(SupportBundle {
            suggested_file_name: format!(
                "ordin-support-{}.jsonl",
                made_at.0.format("%Y%m%dT%H%M%SZ")
            ),
            bytes,
            truncated,
        })
    }
}
