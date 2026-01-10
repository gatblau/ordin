//! History retention, pruning, erasure and compaction (JournalAndArtefactStore.md Internal Logic 14;
//! A-08, UX-A58; Configuration.md `history.*` clamp).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use ordin_types::time::Timestamp;
use ordin_types::RunId;

use crate::codec;
use crate::db::{self, Arg, Tx};
use crate::error::JournalError;
use crate::journal::Journal;
use crate::records::subject_key;
use crate::types::{PruneParticipant, PruneRequest, PruneSummary, RetentionSetting};

/// The journal keeps at most this many runs before pruning by count (2D-4; 07 — *Scale*).
const MAX_RUNS: i64 = 100_000;
/// Rows deleted per transaction while pruning (Internal Logic 14.3.4; chosen there).
const PRUNE_BATCH: i64 = 1_000;
/// `count_for_runs` and `delete_for_runs` take at most this many run ids (Internal Logic 14.4).
const MAX_ERASE_RUNS: usize = 500;

/// `NOT EXISTS` guard: the row's run is not in progress according to the run index.
const NOT_IN_PROGRESS: &str =
    "NOT EXISTS (SELECT 1 FROM journal_runs r WHERE r.run_id = x.run_id AND r.ended_at IS NULL)";

/// The tables pruned by age, in the order of Internal Logic 14.3.4, as
/// (table, key column, key marker, condition on alias `x` with `#1` the cut-off).
fn prune_targets(shown_reports: usize) -> Vec<(&'static str, &'static str, &'static str, String)> {
    let reports_guard = if shown_reports == 0 {
        String::new()
    } else {
        format!(
            " AND x.run_id NOT IN ({})",
            db::placeholders(2, shown_reports, "{uuid}")
        )
    };
    vec![
        (
            "journal_events",
            "seq",
            "",
            format!("x.at < $1{{ts}} AND {NOT_IN_PROGRESS}"),
        ),
        (
            "journal_entries",
            "id",
            "",
            format!("x.recorded_at < $1{{ts}} AND {NOT_IN_PROGRESS}"),
        ),
        (
            "tool_requests",
            "id",
            "",
            format!("x.requested_at < $1{{ts}} AND {NOT_IN_PROGRESS}"),
        ),
        (
            "external_actions",
            "action_key",
            "",
            format!("x.sent_at < $1{{ts}} AND x.state <> 'unknown_outcome' AND {NOT_IN_PROGRESS}"),
        ),
        (
            "startup_reports",
            "id",
            "",
            // A start-up report names its run only through `subject_key` (`run:<id>`).
            "x.generated_at < $1{ts} AND NOT EXISTS (SELECT 1 FROM journal_runs r \
             WHERE r.ended_at IS NULL AND x.subject_key = 'run:' || {s:r.run_id})"
                .to_owned(),
        ),
        (
            "run_steps",
            "id",
            "",
            "EXISTS (SELECT 1 FROM journal_runs r WHERE r.run_id = x.run_id AND r.ended_at < $1{ts})"
                .to_owned(),
        ),
        (
            "reports",
            "id",
            "",
            format!("x.recorded_at < $1{{ts}} AND {NOT_IN_PROGRESS}{reports_guard}"),
        ),
        (
            "artefacts",
            "id",
            "",
            "x.created_at < $1{ts} AND x.version < (SELECT MAX(a.version) FROM artefacts a \
             WHERE a.location_key = x.location_key AND a.path = x.path)"
                .to_owned(),
        ),
        ("journal_runs", "run_id", "", "x.ended_at < $1{ts}".to_owned()),
    ]
}

impl Journal {
    pub async fn retention(&self) -> Result<RetentionSetting, JournalError> {
        let row = db::fetch_optional(
            &self.inner.store,
            "SELECT retention_days, company_max_days, {t:last_pruned_at} FROM journal_meta WHERE id = 1",
            &[],
        )
        .await?
        .ok_or(JournalError::Unavailable)?;
        Ok(RetentionSetting {
            days: codec::u32_of(row.int(0)?)?,
            company_max_days: row.opt_int(1)?.map(codec::u32_of).transpose()?,
            last_pruned_at: codec::opt_ts(row.opt_text(2)?)?,
        })
    }

    pub async fn set_retention_days(&self, days: u32) -> Result<RetentionSetting, JournalError> {
        if days < 1 {
            return Err(JournalError::RetentionInvalid);
        }
        let _writer = self.inner.write_lock.lock().await;
        let current = self.retention().await?;
        if let Some(max_days) = current.company_max_days.filter(|max| days > *max) {
            return Err(JournalError::RetentionAboveCompanyMax { max_days });
        }
        db::execute(
            &self.inner.store,
            "UPDATE journal_meta SET retention_days = $1 WHERE id = 1",
            &[Arg::Int(i64::from(days))],
        )
        .await?;
        tracing::info!(
            component = "journal",
            days,
            company_max_days = current.company_max_days,
            "journal.retention.changed"
        );
        Ok(RetentionSetting { days, ..current })
    }

    /// The program calls this at start and whenever the active profile changes.
    pub async fn set_company_max_days(
        &self,
        max_days: Option<u32>,
    ) -> Result<RetentionSetting, JournalError> {
        // SPEC-GAP-7: Internal Logic 14.2 names no code for a company maximum of 0 (the profile's
        // rule is ≥ 1, Configuration.md `history.max_days`); it is refused as an invalid day count.
        if max_days == Some(0) {
            return Err(JournalError::RetentionInvalid);
        }
        let _writer = self.inner.write_lock.lock().await;
        let current = self.retention().await?;
        let days = match max_days {
            Some(max) if current.days > max => max,
            _ => current.days,
        };
        db::execute(
            &self.inner.store,
            "UPDATE journal_meta SET company_max_days = $1, retention_days = $2 WHERE id = 1",
            &[
                Arg::OptInt(max_days.map(i64::from)),
                Arg::Int(i64::from(days)),
            ],
        )
        .await?;
        if days != current.days || max_days != current.company_max_days {
            tracing::info!(
                component = "journal",
                days,
                company_max_days = max_days,
                "journal.retention.changed"
            );
        }
        Ok(RetentionSetting {
            days,
            company_max_days: max_days,
            last_pruned_at: current.last_pruned_at,
        })
    }

    pub fn register_prune_participant(&self, participant: std::sync::Arc<dyn PruneParticipant>) {
        if let Ok(mut participants) = self.inner.participants.lock() {
            participants.push(participant);
        }
    }

    pub async fn prune(
        &self,
        cutoff_override: Option<Timestamp>,
    ) -> Result<PruneSummary, JournalError> {
        // Step 14.3.7: a call made while a prune runs waits for it and returns its summary.
        let generation = self.inner.prune_generation.load(Ordering::SeqCst);
        let mut slot = self.inner.prune_slot.lock().await;
        if self.inner.prune_generation.load(Ordering::SeqCst) != generation {
            if let Some(summary) = slot.as_ref() {
                return Ok(summary.clone());
            }
        }
        let summary = self.prune_now(cutoff_override).await;
        if let Ok(summary) = &summary {
            *slot = Some(summary.clone());
            self.inner.prune_generation.fetch_add(1, Ordering::SeqCst);
        }
        summary
    }

    async fn prune_now(
        &self,
        cutoff_override: Option<Timestamp>,
    ) -> Result<PruneSummary, JournalError> {
        let store = &self.inner.store;
        // 14.3.1
        let age_cutoff = match cutoff_override {
            Some(cutoff) => cutoff,
            None => {
                let days = self.retention().await?.days;
                Timestamp(self.now().0 - chrono::Duration::days(i64::from(days)))
            }
        };
        // 14.3.2
        let count = db::fetch_optional(store, "SELECT COUNT(*) FROM journal_runs", &[])
            .await?
            .map(|row| row.int(0))
            .transpose()?
            .unwrap_or(0);
        let mut cutoff = age_cutoff;
        if count > MAX_RUNS {
            let cap = db::fetch_optional(
                store,
                "SELECT {t:first_seen_at} FROM journal_runs ORDER BY first_seen_at DESC LIMIT 1 OFFSET $1",
                &[Arg::Int(MAX_RUNS - 1)],
            )
            .await?
            .map(|row| row.text(0))
            .transpose()?
            .map(|raw| codec::ts(&raw))
            .transpose()?;
            if let Some(cap) = cap {
                cutoff = cutoff.max(cap);
            }
        }
        // 14.3.3
        let runs_in_progress = self.runs_in_progress().await?;
        // 14.3.4
        let shown = self.shown_report_runs().await?;
        let mut removed_own: u64 = 0;
        'tables: for (table, key, key_marker, condition) in prune_targets(shown.len()) {
            let sql =
                format!(
                "DELETE FROM {table} WHERE {key} IN (SELECT x.{key}{key_marker} FROM {table} x \
                 WHERE {condition} LIMIT ${})",
                if table == "reports" { shown.len() + 2 } else { 2 }
            );
            let mut args = vec![Arg::Text(cutoff.to_wire())];
            if table == "reports" {
                args.extend(shown.iter().cloned().map(Arg::Text));
            }
            args.push(Arg::Int(PRUNE_BATCH));
            loop {
                if self.inner.stopping.load(Ordering::SeqCst) {
                    break 'tables;
                }
                let _writer = self.inner.write_lock.lock().await;
                let mut tx = Tx::begin(store).await?;
                let removed = tx.execute(&sql, &args).await?;
                tx.commit().await?;
                removed_own += removed;
                if removed < PRUNE_BATCH as u64 {
                    break;
                }
            }
        }
        let mut removed = BTreeMap::new();
        removed.insert("journal".to_owned(), removed_own);
        let mut failed = BTreeMap::new();
        // 14.3.5
        let participants: Vec<Arc<dyn PruneParticipant>> = self
            .inner
            .participants
            .lock()
            .map(|p| p.clone())
            .unwrap_or_default();
        let request = PruneRequest {
            cutoff,
            runs_in_progress,
        };
        for participant in participants {
            match participant.prune(store, &request).await {
                Ok(count) => {
                    removed.insert(participant.module().to_owned(), count);
                }
                Err(failure) => {
                    failed.insert(participant.module().to_owned(), failure.code.to_owned());
                }
            }
        }
        // 14.3.6
        let now = self.now();
        {
            let _writer = self.inner.write_lock.lock().await;
            db::execute(
                store,
                "UPDATE journal_meta SET last_pruned_at = $1{ts} WHERE id = 1",
                &[Arg::Text(now.to_wire())],
            )
            .await?;
        }
        let removed_text = serde_json::to_string(&removed).unwrap_or_default();
        let failed_names = failed.keys().cloned().collect::<Vec<_>>().join(",");
        tracing::info!(
            component = "journal",
            cutoff = cutoff.to_wire().as_str(),
            removed = removed_text.as_str(),
            failed = failed_names.as_str(),
            "journal.prune.done"
        );
        Ok(PruneSummary {
            cutoff,
            removed,
            failed,
        })
    }

    /// Rows of this crate's tables linked to any of `run_ids` (step 14.4); nothing is removed.
    pub async fn count_for_runs(&self, run_ids: &[RunId]) -> Result<u64, JournalError> {
        if run_ids.len() > MAX_ERASE_RUNS {
            return Err(JournalError::EntryInvalid);
        }
        if run_ids.is_empty() {
            return Ok(0);
        }
        let mut total = 0u64;
        for (sql, args) in erase_statements(run_ids, "SELECT COUNT(*) FROM") {
            let row = db::fetch_optional(&self.inner.store, &sql, &args).await?;
            if let Some(row) = row {
                total += codec::u64_of(row.int(0)?)?;
            }
        }
        Ok(total)
    }

    /// Removes every row of this crate's tables linked to any of `run_ids`, at most 500 ids
    /// per call (step 14.4). Returns the number of rows removed.
    pub async fn delete_for_runs(&self, run_ids: &[RunId]) -> Result<u64, JournalError> {
        if run_ids.len() > MAX_ERASE_RUNS {
            return Err(JournalError::EntryInvalid);
        }
        if run_ids.is_empty() {
            return Ok(0);
        }
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        let mut rows = 0u64;
        for (sql, args) in erase_statements(run_ids, "DELETE FROM") {
            rows += tx.execute(&sql, &args).await?;
        }
        self.drop_refresh_runs(&mut tx, run_ids).await?;
        tx.commit().await?;
        tracing::info!(component = "journal", rows, "journal.erase.done");
        Ok(rows)
    }

    /// SQLite: `VACUUM` then `PRAGMA optimize`. PostgreSQL: `VACUUM (ANALYZE)` on the journal's tables.
    pub async fn compact(&self) -> Result<(), JournalError> {
        let started = Instant::now();
        let _writer = self.inner.write_lock.lock().await;
        let compacted = match &self.inner.store {
            crate::datastore::Datastore::Sqlite(pool) => {
                match sqlx::raw_sql("VACUUM").execute(pool).await {
                    Ok(_) => sqlx::raw_sql("PRAGMA optimize")
                        .execute(pool)
                        .await
                        .map(|_| ()),
                    Err(e) => Err(e),
                }
            }
            crate::datastore::Datastore::Postgres(pool) => sqlx::raw_sql(
                "VACUUM (ANALYZE) journal_meta, journal_events, journal_entries, journal_runs, \
                 artefacts, tool_requests, external_actions, startup_reports, reports, dashboards, \
                 run_steps",
            )
            .execute(pool)
            .await
            .map(|_| ()),
        };
        match compacted {
            Ok(()) => {
                let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                tracing::info!(
                    component = "journal",
                    engine = self.inner.engine.as_str(),
                    duration_ms,
                    "journal.compact.done"
                );
                Ok(())
            }
            Err(_) => {
                tracing::warn!(
                    component = "journal",
                    engine = self.inner.engine.as_str(),
                    code = "JRNL_DATASTORE_UNAVAILABLE",
                    "journal.compact.failed"
                );
                Err(JournalError::Unavailable)
            }
        }
    }
}

/// One statement per table for the rows linked to `run_ids` (Internal Logic 14.4), each
/// `<verb> <table> WHERE …`.
fn erase_statements(run_ids: &[RunId], verb: &str) -> Vec<(String, Vec<Arg>)> {
    let ids: Vec<Arg> = run_ids.iter().map(|r| Arg::Text(r.to_string())).collect();
    let in_list = db::placeholders(1, run_ids.len(), "{uuid}");
    let subjects: Vec<Arg> = run_ids
        .iter()
        .map(|run_id| {
            Arg::Text(subject_key(
                &ordin_types::domain::runs::StartupSubject::Run { run_id: *run_id },
            ))
        })
        .collect();
    let subject_list = db::placeholders(1, run_ids.len(), "");
    let mut statements: Vec<(String, Vec<Arg>)> = [
        ("journal_events", "run_id"),
        ("journal_entries", "run_id"),
        ("tool_requests", "run_id"),
        ("external_actions", "run_id"),
        ("run_steps", "run_id"),
        ("reports", "run_id"),
        ("artefacts", "producing_run_id"),
        ("journal_runs", "run_id"),
    ]
    .iter()
    .map(|(table, column)| {
        (
            format!("{verb} {table} WHERE {column} IN ({in_list})"),
            ids.clone(),
        )
    })
    .collect();
    statements.push((
        format!("{verb} startup_reports WHERE subject_key IN ({subject_list})"),
        subjects,
    ));
    statements
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erase_statements_cover_every_table_of_the_crate() {
        let run = RunId::new();
        let statements = erase_statements(&[run], "DELETE FROM");
        assert_eq!(statements.len(), 9);
        assert!(statements
            .iter()
            .any(|(sql, args)| sql.contains("startup_reports")
                && matches!(&args[0], Arg::Text(k) if k.starts_with("run:"))));
        assert_eq!(prune_targets(0).len(), 9);
        assert!(prune_targets(2)[6].3.contains("$2{uuid}, $3{uuid}"));
    }
}
