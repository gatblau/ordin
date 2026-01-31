//! C2 — Durable Work Queue over the SQL storage's `mem_work_job` and `mem_dead_letter` tables
//! (*Data Model* §B), plus the expired-lease sweep the backend-neutral `crate::queue::LeaseReaper`
//! drives.
//!
//! The protocol is the SurrealDB queue's: idempotent enqueue on `(scope, idempotency_key)`, a
//! single-winner claim, exponential backoff with jitter, then dead-letter (SA-QUEUE-02). The claim is
//! one conditional `UPDATE … WHERE id = (SELECT … LIMIT 1) … RETURNING`, whose outer `WHERE` re-checks
//! the claimable predicate so a row is leased to exactly one caller. SQLite serialises writers, so the
//! loser re-evaluates against the leased row and updates nothing; PostgreSQL picks the row with
//! `FOR UPDATE SKIP LOCKED` (§B), so concurrent instances claim different jobs rather than queueing on
//! one.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::Rng;

use super::exec::{is_unique_violation, json_col, map_queue_err, store_to_queue, SqlRow};
use super::{SqlDialect, SqlStorage};
use crate::types::job::{JobKind, JobStatus, WorkJob};
use crate::types::ports::{QueueError, QueueMaintenance, StoreError, WorkQueue};
use crate::types::scope::ScopeRef;

/// The `mem_work_job` select list.
fn job_cols(d: SqlDialect) -> String {
    format!(
        "id, tenant, kind, {}, scope_team, scope_user, idempotency_key, attempts, status, \
         not_before, created_at, leased_until",
        json_col(d, "payload", "payload")
    )
}

impl SqlStorage {
    /// The retry backoff for attempt `n`: `backoff_base_ms · 2^n` with a uniform jitter factor in
    /// `[0.8, 1.2]` (SA-QUEUE-02). Saturates so a high attempt count cannot overflow.
    fn backoff(&self, n: u32) -> Duration {
        let base = u64::from(self.backoff_base_ms);
        let factor = 1u64.checked_shl(n).unwrap_or(u64::MAX);
        let raw = base.saturating_mul(factor);
        let jitter: f64 = rand::thread_rng().gen_range(0.8..=1.2);
        Duration::from_millis((raw as f64 * jitter) as u64)
    }

    /// The id of the job already enqueued for `(scope, key)`, if any.
    async fn find_by_idem(
        &self,
        scope: &ScopeRef,
        key: &str,
    ) -> Result<Option<String>, QueueError> {
        let mut st = self.stmt();
        let (p_t, p_u, p_k) = (st.text(&scope.tenant), st.text(&scope.user), st.text(key));
        // A `None` team matches only a `None` team, as in the SurrealDB queue.
        let team = match &scope.team {
            Some(t) => format!("scope_team = {}", st.text(t)),
            None => "scope_team IS NULL".to_string(),
        };
        st.sql(&format!(
            "SELECT id FROM mem_work_job WHERE tenant = {p_t} AND scope_user = {p_u} \
             AND {team} AND idempotency_key = {p_k} LIMIT 1"
        ));
        let rows = self.pool.fetch(st).await.map_err(map_queue_err)?;
        rows.first()
            .map(|r| r.text("id"))
            .transpose()
            .map_err(store_to_queue)
    }

    /// Load one job of `tenant` by id.
    async fn load_job(&self, tenant: &str, job_id: &str) -> Result<Option<WorkJob>, QueueError> {
        let mut st = self.stmt();
        let cols = job_cols(self.dialect);
        let (p_id, p_t) = (st.text(job_id), st.text(tenant));
        st.sql(&format!(
            "SELECT {cols} FROM mem_work_job WHERE id = {p_id} AND tenant = {p_t}"
        ));
        let rows = self.pool.fetch(st).await.map_err(map_queue_err)?;
        rows.first()
            .map(row_to_job)
            .transpose()
            .map_err(store_to_queue)
    }
}

#[async_trait]
impl WorkQueue for SqlStorage {
    async fn enqueue(&self, job: WorkJob) -> Result<String, QueueError> {
        validate_job(&job)?;

        // Idempotent enqueue: a job already enqueued for (scope, key), in any status, is returned
        // without inserting (SA-IDEM-01 at the queue layer).
        if let Some(key) = job.idempotency_key.as_ref() {
            if let Some(existing) = self.find_by_idem(&job.scope, key).await? {
                tracing::debug!(target: "memory", job_kind = %kind_str(job.kind), dedup = true, "queue.enqueue");
                return Ok(existing);
            }
        }

        let now = Utc::now();
        let mut st = self.stmt();
        let vals = [
            st.text(&job.id),
            st.text(&job.scope.tenant),
            st.text(kind_str(job.kind)),
            st.json(&job.payload),
            st.opt_text(job.scope.team.clone()),
            st.text(&job.scope.user),
            st.opt_text(job.idempotency_key.clone()),
            st.ts(now),
            st.ts(now),
        ]
        .join(", ");
        st.sql(&format!(
            "INSERT INTO mem_work_job (id, tenant, kind, payload, scope_team, scope_user, \
             idempotency_key, not_before, created_at, attempts, status, leased_until) \
             VALUES ({vals}, 0, 'pending', NULL)"
        ));
        if let Err(e) = self.pool.execute(st).await {
            // `mem_work_job_idem` is the second-line guard against a concurrent duplicate racing past
            // the select above; on its violation, return the winning id.
            if is_unique_violation(&e) {
                if let Some(key) = job.idempotency_key.as_ref() {
                    if let Some(existing) = self.find_by_idem(&job.scope, key).await? {
                        return Ok(existing);
                    }
                }
            }
            return Err(map_queue_err(e));
        }
        tracing::info!(target: "memory", job_id = %job.id, kind = %kind_str(job.kind), "queue.enqueue");
        Ok(job.id)
    }

    async fn claim(
        &self,
        tenant: &str,
        kinds: &[JobKind],
        lease: Duration,
    ) -> Result<Option<WorkJob>, QueueError> {
        if kinds.is_empty() {
            return Ok(None);
        }
        let kind_strs: Vec<String> = kinds.iter().map(|k| kind_str(*k).to_string()).collect();
        let now = Utc::now();
        let lease_until =
            now + chrono::Duration::from_std(lease).unwrap_or(chrono::Duration::zero());

        let mut st = self.stmt();
        let cols = job_cols(self.dialect);
        let (p_lu, p_t, p_now) = (st.ts(lease_until), st.text(tenant), st.ts(now));
        let kinds_in = st.in_list("kind", &kind_strs);
        let lock = match self.dialect {
            SqlDialect::Sqlite => "",
            SqlDialect::Postgres => " FOR UPDATE SKIP LOCKED",
        };
        let claimable =
            format!("(status = 'pending' OR (status = 'leased' AND leased_until < {p_now}))");
        st.sql(&format!(
            "UPDATE mem_work_job SET status = 'leased', leased_until = {p_lu} \
             WHERE id = (SELECT id FROM mem_work_job WHERE tenant = {p_t} AND {kinds_in} \
                         AND not_before <= {p_now} AND {claimable} \
                         ORDER BY not_before LIMIT 1{lock}) \
               AND {claimable} \
             RETURNING {cols}"
        ));
        let rows = self.pool.fetch(st).await.map_err(map_queue_err)?;
        match rows.first() {
            Some(row) => {
                let job = row_to_job(row).map_err(store_to_queue)?;
                tracing::debug!(target: "memory", claimed = true, job_id = %job.id, "queue.claim");
                Ok(Some(job))
            }
            None => {
                tracing::debug!(target: "memory", claimed = false, "queue.claim");
                Ok(None)
            }
        }
    }

    async fn complete(&self, tenant: &str, job_id: &str) -> Result<(), QueueError> {
        // The row stays for dedupe; nothing reads a done job's payload, so it is cleared.
        let mut st = self.stmt();
        let empty = st.json(&serde_json::json!({}));
        let (p_id, p_t) = (st.text(job_id), st.text(tenant));
        st.sql(&format!(
            "UPDATE mem_work_job SET status = 'done', leased_until = NULL, payload = {empty} \
             WHERE id = {p_id} AND tenant = {p_t} AND status = 'leased'"
        ));
        if self.pool.execute(st).await.map_err(map_queue_err)? == 0 {
            // Updated nothing: absent, or present but not leased.
            return Err(match self.load_job(tenant, job_id).await? {
                Some(_) => QueueError::NotLeased(job_id.to_string()),
                None => QueueError::JobNotFound(job_id.to_string()),
            });
        }
        self.unlink_job_facts(tenant, job_id).await?;
        tracing::info!(target: "memory", job_id = %job_id, "queue.complete");
        Ok(())
    }

    async fn redact_job(
        &self,
        tenant: &str,
        job_id: &str,
        payload: &serde_json::Value,
    ) -> Result<(), QueueError> {
        let mut st = self.stmt();
        let (p_payload, p_id, p_t) = (st.json(payload), st.text(job_id), st.text(tenant));
        st.sql(&format!(
            "UPDATE mem_work_job SET payload = {p_payload} \
             WHERE id = {p_id} AND tenant = {p_t} AND status = 'leased'"
        ));
        if self.pool.execute(st).await.map_err(map_queue_err)? == 0 {
            return Err(QueueError::JobNotFound(job_id.to_string()));
        }
        Ok(())
    }

    async fn link_job_fact(
        &self,
        scope: &ScopeRef,
        job_id: &str,
        fact_id: &str,
    ) -> Result<(), QueueError> {
        let mut st = self.stmt();
        let vals = [
            st.text(job_id),
            st.text(fact_id),
            st.text(&scope.tenant),
            st.opt_text(scope.team.clone()),
        ]
        .join(", ");
        st.sql(&format!(
            "INSERT INTO mem_job_fact (job_id, fact_id, tenant, scope_team) VALUES ({vals}) \
             ON CONFLICT DO NOTHING"
        ));
        self.pool.execute(st).await.map_err(map_queue_err)?;
        Ok(())
    }

    async fn unlink_job_facts(&self, tenant: &str, job_id: &str) -> Result<(), QueueError> {
        let mut st = self.stmt();
        let (p_id, p_t) = (st.text(job_id), st.text(tenant));
        st.sql(&format!(
            "DELETE FROM mem_job_fact WHERE job_id = {p_id} AND tenant = {p_t}"
        ));
        self.pool.execute(st).await.map_err(map_queue_err)?;
        Ok(())
    }

    async fn job_is_live(&self, tenant: &str, job_id: &str) -> Result<bool, QueueError> {
        // PostgreSQL waits here for an erasure that holds the row, so the answer is the erasure's
        // outcome and not the moment before it.
        let lock = match self.dialect {
            SqlDialect::Sqlite => "",
            SqlDialect::Postgres => " FOR UPDATE",
        };
        let mut st = self.stmt();
        let (p_id, p_t) = (st.text(job_id), st.text(tenant));
        st.sql(&format!(
            "SELECT id FROM mem_work_job WHERE id = {p_id} AND tenant = {p_t}{lock}"
        ));
        let rows = self.pool.fetch(st).await.map_err(map_queue_err)?;
        Ok(!rows.is_empty())
    }

    async fn cancel_job(&self, scope: &ScopeRef, job_id: &str) -> Result<bool, QueueError> {
        let Some(job) = self.load_job(&scope.tenant, job_id).await? else {
            return Ok(false);
        };
        if job.scope != *scope {
            return Ok(false);
        }
        let mut removed = false;
        let mut tx = self.pool.begin().await.map_err(map_queue_err)?;
        if let Some(key) = &job.idempotency_key {
            let key = crate::store::scoped_quarantine_key(scope, key);
            let mut st = self.stmt();
            let (p_t, p_u, p_k) = (st.text(&scope.tenant), st.text(&scope.user), st.text(&key));
            st.sql(&format!(
                "DELETE FROM mem_quarantine WHERE tenant = {p_t} AND owner_user = {p_u} \
                 AND idempotency_key = {p_k}"
            ));
            removed |= tx.execute(st).await.map_err(map_queue_err)? > 0;
        }
        if job.status != JobStatus::Done {
            for (table, col) in [
                ("mem_job_fact", "job_id"),
                ("mem_dead_letter", "original_job_id"),
                ("mem_work_job", "id"),
            ] {
                let mut st = self.stmt();
                let (p_t, p_id) = (st.text(&scope.tenant), st.text(job_id));
                st.sql(&format!(
                    "DELETE FROM {table} WHERE tenant = {p_t} AND {col} = {p_id}"
                ));
                tx.execute(st).await.map_err(map_queue_err)?;
            }
            removed = true;
        }
        tx.commit().await.map_err(map_queue_err)?;
        tracing::info!(target: "memory", job_id = %job_id, removed, "queue.cancel");
        Ok(removed)
    }

    async fn fail(&self, tenant: &str, job_id: &str, retryable: bool) -> Result<(), QueueError> {
        let current = match self.load_job(tenant, job_id).await? {
            Some(j) => j,
            None => return Err(QueueError::JobNotFound(job_id.to_string())),
        };
        if current.status != JobStatus::Leased {
            return Err(QueueError::NotLeased(job_id.to_string()));
        }

        let now = Utc::now();
        if retryable && current.attempts < self.max_attempts {
            let next = current.attempts + 1;
            let delay = self.backoff(next);
            let not_before =
                now + chrono::Duration::from_std(delay).unwrap_or(chrono::Duration::zero());
            let mut st = self.stmt();
            let (p_n, p_nb, p_id, p_t) = (
                st.int(i64::from(next)),
                st.ts(not_before),
                st.text(job_id),
                st.text(tenant),
            );
            st.sql(&format!(
                "UPDATE mem_work_job SET attempts = {p_n}, status = 'pending', \
                 leased_until = NULL, not_before = {p_nb} \
                 WHERE id = {p_id} AND tenant = {p_t} AND status = 'leased'"
            ));
            if self.pool.execute(st).await.map_err(map_queue_err)? == 0 {
                return Err(QueueError::JobNotFound(job_id.to_string()));
            }
            tracing::warn!(
                target: "memory",
                job_id = %job_id,
                attempts = next,
                delay_ms = delay.as_millis() as u64,
                "queue.fail.retry"
            );
            return Ok(());
        }

        // Not retryable, or attempts exhausted: dead-letter and park a copy, together. Neither row
        // keeps text that was not redacted.
        // spec: phase-1-analysis#SA-QUEUE-03
        let parked = crate::queue::parked_payload(current.kind, &current.payload);
        let scope = serde_json::to_value(&current.scope)
            .map_err(|e| QueueError::BackendUnavailable(format!("encode scope: {e}")))?;
        let mut tx = self.pool.begin().await.map_err(map_queue_err)?;
        let mut st = self.stmt();
        let (p_payload, p_id, p_t) = (st.json(&parked), st.text(job_id), st.text(tenant));
        st.sql(&format!(
            "UPDATE mem_work_job SET status = 'dead_letter', leased_until = NULL, \
             payload = {p_payload} \
             WHERE id = {p_id} AND tenant = {p_t} AND status = 'leased'"
        ));
        // An erasure may have taken the job since it was read: its content is not parked again.
        if tx.execute(st).await.map_err(map_queue_err)? == 0 {
            return Err(QueueError::JobNotFound(job_id.to_string()));
        }
        let mut st = self.stmt();
        let vals = [
            st.text(format!("dead_letter:{}", uuid::Uuid::now_v7())),
            st.text(&current.scope.tenant),
            st.text(kind_str(current.kind)),
            st.json(&parked),
            st.json(&scope),
            st.opt_text(current.idempotency_key.clone()),
            st.int(i64::from(current.attempts)),
            st.text(&current.id),
            st.ts(current.created_at),
            st.ts(now),
        ]
        .join(", ");
        st.sql(&format!(
            "INSERT INTO mem_dead_letter (id, tenant, kind, payload, scope, idempotency_key, \
             attempts, original_job_id, created_at, dead_lettered_at) VALUES ({vals})"
        ));
        tx.execute(st).await.map_err(map_queue_err)?;
        tx.commit().await.map_err(map_queue_err)?;
        let reason = if retryable {
            "attempts_exhausted"
        } else {
            "non_retryable"
        };
        tracing::error!(
            target: "memory",
            job_id = %job_id,
            attempts = current.attempts,
            reason,
            "queue.fail.dead_letter"
        );
        Ok(())
    }
}

#[async_trait]
impl QueueMaintenance for SqlStorage {
    /// One sweep over every tenant's jobs: an expired lease returns to `pending`.
    async fn reap_expired_leases(&self, now: DateTime<Utc>) -> Result<u64, QueueError> {
        let mut st = self.stmt();
        let p_now = st.ts(now);
        st.sql(&format!(
            "UPDATE mem_work_job SET status = 'pending', leased_until = NULL \
             WHERE status = 'leased' AND leased_until < {p_now}"
        ));
        let n = self.pool.execute(st).await.map_err(map_queue_err)?;
        if n > 0 {
            tracing::info!(target: "memory", reclaimed = n, "queue.reap");
        }
        Ok(n)
    }

    async fn count_dead_letter(&self, tenant: &str) -> Result<u64, QueueError> {
        let mut st = self.stmt();
        let p_t = st.text(tenant);
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM mem_dead_letter WHERE tenant = {p_t}"
        ));
        let rows = self.pool.fetch(st).await.map_err(map_queue_err)?;
        super::count_of(&rows).map_err(store_to_queue)
    }

    async fn expire_dead_letters(
        &self,
        tenant: &str,
        before: DateTime<Utc>,
    ) -> Result<u64, QueueError> {
        let old = |st: &mut super::exec::Stmt| {
            let (p_t, p_b) = (st.text(tenant), st.ts(before));
            format!(
                "SELECT original_job_id FROM mem_dead_letter \
                 WHERE tenant = {p_t} AND dead_lettered_at < {p_b}"
            )
        };
        let mut tx = self.pool.begin().await.map_err(map_queue_err)?;
        let mut st = self.stmt();
        let jobs = old(&mut st);
        st.sql(&format!(
            "DELETE FROM mem_job_fact WHERE job_id IN ({jobs})"
        ));
        tx.execute(st).await.map_err(map_queue_err)?;
        let mut st = self.stmt();
        let jobs = old(&mut st);
        st.sql(&format!(
            "DELETE FROM mem_work_job WHERE status = 'dead_letter' AND id IN ({jobs})"
        ));
        tx.execute(st).await.map_err(map_queue_err)?;
        let mut st = self.stmt();
        let (p_t, p_b) = (st.text(tenant), st.ts(before));
        st.sql(&format!(
            "DELETE FROM mem_dead_letter WHERE tenant = {p_t} AND dead_lettered_at < {p_b}"
        ));
        let n = tx.execute(st).await.map_err(map_queue_err)?;
        tx.commit().await.map_err(map_queue_err)?;
        if n > 0 {
            tracing::info!(target: "memory", expired = n, "queue.dead_letters_expired");
        }
        Ok(n)
    }

    async fn open_jobs(&self, tenant: &str, team: Option<&str>) -> Result<u64, QueueError> {
        let mut st = self.stmt();
        let p_t = st.text(tenant);
        let in_team = match team {
            Some(team) => format!("scope_team = {}", st.text(team)),
            None => "scope_team IS NULL".to_owned(),
        };
        st.sql(&format!(
            "SELECT COUNT(*) AS n FROM mem_work_job WHERE tenant = {p_t} AND {in_team} \
             AND status IN ('pending', 'leased')"
        ));
        let rows = self.fetch(st).await.map_err(store_to_queue)?;
        let n = rows
            .first()
            .map(|row| row.int("n"))
            .transpose()
            .map_err(store_to_queue)?;
        Ok(n.unwrap_or(0).max(0) as u64)
    }

    async fn purge_done_jobs(
        &self,
        tenant: &str,
        before: DateTime<Utc>,
    ) -> Result<u64, QueueError> {
        let mut st = self.stmt();
        let (p_t, p_b) = (st.text(tenant), st.ts(before));
        st.sql(&format!(
            "DELETE FROM mem_work_job WHERE tenant = {p_t} AND status = 'done' AND created_at < {p_b}"
        ));
        let n = self.execute(st).await.map_err(store_to_queue)?;
        if n > 0 {
            tracing::info!(target: "memory", purged = n, "queue.done_jobs_purged");
        }
        Ok(n)
    }
}

/// Validate a job structurally before enqueue: `id` is `work_job:<key>`, and `scope.tenant` and
/// `scope.user` are non-empty (`kind` is a known `JobKind` by construction).
fn validate_job(job: &WorkJob) -> Result<(), QueueError> {
    let (table, key) = job
        .id
        .split_once(':')
        .ok_or_else(|| QueueError::InvalidJob(format!("malformed id: {}", job.id)))?;
    if table != "work_job" || key.is_empty() {
        return Err(QueueError::InvalidJob(format!(
            "id must be work_job:<key>: {}",
            job.id
        )));
    }
    if job.scope.tenant.trim().is_empty() {
        return Err(QueueError::InvalidJob("scope.tenant is empty".into()));
    }
    if job.scope.user.trim().is_empty() {
        return Err(QueueError::InvalidJob("scope.user is empty".into()));
    }
    Ok(())
}

/// The canonical snake_case string for a `JobKind` (the §B `CHECK` values).
fn kind_str(k: JobKind) -> &'static str {
    match k {
        JobKind::ExtractFact => "extract_fact",
        JobKind::ReEmbedFact => "re_embed_fact",
        JobKind::HardDelete => "hard_delete",
    }
}

fn parse_kind(s: &str) -> Result<JobKind, StoreError> {
    match s {
        "extract_fact" => Ok(JobKind::ExtractFact),
        "re_embed_fact" => Ok(JobKind::ReEmbedFact),
        "hard_delete" => Ok(JobKind::HardDelete),
        other => Err(StoreError::Internal(format!("decode kind: {other}"))),
    }
}

fn parse_status(s: &str) -> Result<JobStatus, StoreError> {
    match s {
        "pending" => Ok(JobStatus::Pending),
        "leased" => Ok(JobStatus::Leased),
        "done" => Ok(JobStatus::Done),
        "dead_letter" => Ok(JobStatus::DeadLetter),
        other => Err(StoreError::Internal(format!("decode status: {other}"))),
    }
}

fn row_to_job(r: &SqlRow) -> Result<WorkJob, StoreError> {
    Ok(WorkJob {
        id: r.text("id")?,
        kind: parse_kind(&r.text("kind")?)?,
        payload: r.json("payload")?,
        scope: ScopeRef {
            tenant: r.text("tenant")?,
            team: r.opt_text("scope_team")?,
            user: r.text("scope_user")?,
        },
        idempotency_key: r.opt_text("idempotency_key")?,
        attempts: u32::try_from(r.int("attempts")?)
            .map_err(|_| StoreError::Internal("decode attempts".into()))?,
        status: parse_status(&r.text("status")?)?,
        not_before: r.ts("not_before")?,
        created_at: r.ts("created_at")?,
        leased_until: r.opt_ts("leased_until")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, tenant: &str, user: &str) -> WorkJob {
        WorkJob {
            id: id.into(),
            kind: JobKind::ExtractFact,
            payload: serde_json::json!({}),
            scope: ScopeRef {
                tenant: tenant.into(),
                team: None,
                user: user.into(),
            },
            idempotency_key: None,
            attempts: 0,
            status: JobStatus::Pending,
            not_before: Utc::now(),
            created_at: Utc::now(),
            leased_until: None,
        }
    }

    #[test]
    fn validate_job_matches_the_surreal_queue_rules() {
        assert!(validate_job(&job("work_job:a", "t", "u")).is_ok());
        assert!(matches!(
            validate_job(&job("nojob", "t", "u")),
            Err(QueueError::InvalidJob(_))
        ));
        assert!(matches!(
            validate_job(&job("fact:a", "t", "u")),
            Err(QueueError::InvalidJob(_))
        ));
        assert!(matches!(
            validate_job(&job("work_job:a", " ", "u")),
            Err(QueueError::InvalidJob(_))
        ));
        assert!(matches!(
            validate_job(&job("work_job:a", "t", "")),
            Err(QueueError::InvalidJob(_))
        ));
    }

    #[test]
    fn every_job_kind_round_trips_through_its_stored_form() {
        for k in [
            JobKind::ExtractFact,
            JobKind::ReEmbedFact,
            JobKind::HardDelete,
        ] {
            assert_eq!(parse_kind(kind_str(k)).unwrap(), k);
        }
    }
}
