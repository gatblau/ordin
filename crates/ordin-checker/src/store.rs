//! The Checker's tables in the journal database (Checker.md *Data Model*): check runs and their
//! findings, executable-part decisions and what they were made from, and the executable-part list of
//! each instance's latest install check.

use ordin_agent_model::Timestamp;
use ordin_types::domain::agents::{DecisionSource, ExecutablePartDecision};
use ordin_types::time::parse_rfc3339_ms_utc;
use ordin_types::{AgentInstanceId, ExecutablePartDecisionId};
use sqlx::{Row, SqlitePool};

use crate::error::{from_sqlx, CheckError};
use crate::model::{CheckReport, CheckRunId, CheckSubject, Finding, Severity};
use crate::trust::{kind_from_text, kind_text, trust_from_text, trust_text};

/// Runs of one subject beyond its latest this many are deleted after each insert (*assumed here*).
const KEEP_RUNS_PER_SUBJECT: i64 = 20;

fn store_error(error: sqlx::Error) -> CheckError {
    from_sqlx(&error)
}

/// A stored value that no longer reads: the store cannot be trusted, and retrying will not help.
fn corrupt() -> CheckError {
    CheckError::DatastoreFailed { retryable: false }
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, CheckError> {
    serde_json::to_string(value).map_err(|_| corrupt())
}

fn parse_ts(raw: &str) -> Result<Timestamp, CheckError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| corrupt())
}

fn severity_text(severity: Severity) -> &'static str {
    match severity {
        Severity::Failure => "failure",
        Severity::Warning => "warning",
    }
}

/// One run to record.
pub(crate) struct RunRow<'a> {
    pub report: &'a CheckReport,
    pub started_at: Timestamp,
    pub failures: u32,
    pub warnings: u32,
}

/// The executable-part list of an instance's latest install check (SA-CLOSE-03).
pub(crate) struct CandidateRow {
    pub agent_instance_id: AgentInstanceId,
    pub plan_hash: String,
    pub parts_json: String,
}

/// Step 3: the run and its findings in one transaction, with the install candidate row when given;
/// then the subject's older runs beyond its latest 20.
pub(crate) async fn insert_run(
    pool: &SqlitePool,
    run: &RunRow<'_>,
    candidate: Option<&CandidateRow>,
) -> Result<(), CheckError> {
    let report = run.report;
    let id = report.check_run_id.to_string();
    let subject = json(&report.subject)?;
    let mut tx = pool.begin().await.map_err(store_error)?;
    sqlx::query(
        "INSERT INTO check_runs (id, subject_kind, subject, version_checked, passed, failures, \
         warnings, tidied, agents_that_stop, started_at, finished_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(&id)
    .bind(report.subject.kind_text())
    .bind(&subject)
    .bind(&report.version_checked)
    .bind(i64::from(report.passed))
    .bind(i64::from(run.failures))
    .bind(i64::from(run.warnings))
    .bind(json(&report.tidied)?)
    .bind(json(&report.agents_that_stop)?)
    .bind(run.started_at.to_wire())
    .bind(report.checked_at.to_wire())
    .execute(&mut *tx)
    .await
    .map_err(store_error)?;
    for (ordinal, finding) in report.findings.iter().enumerate() {
        sqlx::query(
            "INSERT INTO check_findings (check_run_id, ordinal, code, severity, part_identifier, \
             message_id, args, fixes, detail) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(&id)
        .bind(i64::try_from(ordinal).unwrap_or(i64::MAX))
        .bind(&finding.code)
        .bind(severity_text(finding.severity))
        .bind(finding.part_identifier.as_deref())
        .bind(&finding.message_id)
        .bind(json(&finding.args)?)
        .bind(json(&finding.fixes)?)
        .bind(&finding.detail)
        .execute(&mut *tx)
        .await
        .map_err(store_error)?;
    }
    if let Some(candidate) = candidate {
        sqlx::query(
            "INSERT INTO install_candidate_parts (agent_instance_id, plan_hash, parts, \
             check_run_id, recorded_at) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT (agent_instance_id) DO UPDATE SET plan_hash = excluded.plan_hash, \
             parts = excluded.parts, check_run_id = excluded.check_run_id, \
             recorded_at = excluded.recorded_at",
        )
        .bind(candidate.agent_instance_id.to_string())
        .bind(&candidate.plan_hash)
        .bind(&candidate.parts_json)
        .bind(&id)
        .bind(report.checked_at.to_wire())
        .execute(&mut *tx)
        .await
        .map_err(store_error)?;
    }
    // Retention: the subject's runs beyond its latest 20, findings first.
    let stale: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM check_runs WHERE subject = ?1 AND id NOT IN \
         (SELECT id FROM check_runs WHERE subject = ?1 ORDER BY finished_at DESC, id DESC LIMIT ?2)",
    )
    .bind(&subject)
    .bind(KEEP_RUNS_PER_SUBJECT)
    .fetch_all(&mut *tx)
    .await
    .map_err(store_error)?;
    for old in stale {
        sqlx::query("DELETE FROM check_findings WHERE check_run_id = ?1")
            .bind(&old)
            .execute(&mut *tx)
            .await
            .map_err(store_error)?;
        sqlx::query("DELETE FROM check_runs WHERE id = ?1")
            .bind(&old)
            .execute(&mut *tx)
            .await
            .map_err(store_error)?;
    }
    tx.commit().await.map_err(store_error)
}

/// One recorded run, rebuilt as its report; None when unknown or pruned.
pub(crate) async fn load_run(
    pool: &SqlitePool,
    id: CheckRunId,
) -> Result<Option<CheckReport>, CheckError> {
    let key = id.to_string();
    let Some(row) = sqlx::query(
        "SELECT subject, version_checked, passed, tidied, agents_that_stop, finished_at \
         FROM check_runs WHERE id = ?1",
    )
    .bind(&key)
    .fetch_optional(pool)
    .await
    .map_err(store_error)?
    else {
        return Ok(None);
    };
    let text = |column: &str| -> Result<String, CheckError> {
        row.try_get::<String, _>(column).map_err(|_| corrupt())
    };
    let subject: CheckSubject = serde_json::from_str(&text("subject")?).map_err(|_| corrupt())?;
    let passed: i64 = row.try_get("passed").map_err(|_| corrupt())?;
    let tidied = serde_json::from_str(&text("tidied")?).map_err(|_| corrupt())?;
    let agents_that_stop =
        serde_json::from_str(&text("agents_that_stop")?).map_err(|_| corrupt())?;
    let checked_at = parse_ts(&text("finished_at")?)?;
    let version_checked = text("version_checked")?;
    let rows = sqlx::query(
        "SELECT code, severity, part_identifier, message_id, args, fixes, detail \
         FROM check_findings WHERE check_run_id = ?1 ORDER BY ordinal",
    )
    .bind(&key)
    .fetch_all(pool)
    .await
    .map_err(store_error)?;
    let mut findings = Vec::with_capacity(rows.len());
    for row in rows {
        let get = |column: &str| -> Result<String, CheckError> {
            row.try_get::<String, _>(column).map_err(|_| corrupt())
        };
        let severity = match get("severity")?.as_str() {
            "failure" => Severity::Failure,
            "warning" => Severity::Warning,
            _ => return Err(corrupt()),
        };
        findings.push(Finding {
            code: get("code")?,
            severity,
            part_identifier: row
                .try_get::<Option<String>, _>("part_identifier")
                .map_err(|_| corrupt())?,
            message_id: get("message_id")?,
            args: serde_json::from_str(&get("args")?).map_err(|_| corrupt())?,
            fixes: serde_json::from_str(&get("fixes")?).map_err(|_| corrupt())?,
            detail: get("detail")?,
        });
    }
    let publish_gate_met = !findings.iter().any(|f| f.code == "publish_gate_not_met");
    Ok(Some(CheckReport {
        check_run_id: id,
        subject,
        passed: passed == 1,
        findings,
        tidied,
        checked_at,
        version_checked,
        agents_that_stop,
        publish_gate_met,
    }))
}

/// The stored executable-part list of an instance, as JSON; None when none is stored.
pub(crate) async fn load_candidate(
    pool: &SqlitePool,
    instance: AgentInstanceId,
) -> Result<Option<String>, CheckError> {
    sqlx::query_scalar("SELECT parts FROM install_candidate_parts WHERE agent_instance_id = ?1")
        .bind(instance.to_string())
        .fetch_optional(pool)
        .await
        .map_err(store_error)
}

/// One stored decision with what it was made from.
pub(crate) struct StoredDecision {
    pub decision: ExecutablePartDecision,
    pub declared_by: String,
}

/// Step 31: the instance's rows replaced in one transaction.
pub(crate) async fn replace_decisions(
    pool: &SqlitePool,
    instance: AgentInstanceId,
    rows: &[StoredDecision],
) -> Result<(), CheckError> {
    let mut tx = pool.begin().await.map_err(store_error)?;
    delete_decisions(&mut tx, instance).await?;
    for row in rows {
        let d = &row.decision;
        sqlx::query(
            "INSERT INTO executable_part_decisions (id, agent_instance_id, part_name, kind, \
             decision, source, decided_at) VALUES (?1, ?2, ?3, ?4, ?5, 'organisation_policy', ?6)",
        )
        .bind(d.id.to_string())
        .bind(instance.to_string())
        .bind(&d.part_name)
        .bind(kind_text(d.kind))
        .bind(trust_text(d.decision))
        .bind(d.decided_at.to_wire())
        .execute(&mut *tx)
        .await
        .map_err(store_error)?;
        sqlx::query(
            "INSERT INTO executable_part_subjects (decision_id, declared_by) VALUES (?1, ?2)",
        )
        .bind(d.id.to_string())
        .bind(&row.declared_by)
        .execute(&mut *tx)
        .await
        .map_err(store_error)?;
    }
    tx.commit().await.map_err(store_error)
}

async fn delete_decisions(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    instance: AgentInstanceId,
) -> Result<(), CheckError> {
    let key = instance.to_string();
    sqlx::query(
        "DELETE FROM executable_part_subjects WHERE decision_id IN \
         (SELECT id FROM executable_part_decisions WHERE agent_instance_id = ?1)",
    )
    .bind(&key)
    .execute(&mut **tx)
    .await
    .map_err(store_error)?;
    sqlx::query("DELETE FROM executable_part_decisions WHERE agent_instance_id = ?1")
        .bind(&key)
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
    Ok(())
}

/// The decisions of one instance, in name order.
pub(crate) async fn load_decisions(
    pool: &SqlitePool,
    instance: AgentInstanceId,
) -> Result<Vec<StoredDecision>, CheckError> {
    let rows = sqlx::query(
        "SELECT d.id, d.part_name, d.kind, d.decision, d.decided_at, s.declared_by \
         FROM executable_part_decisions d \
         LEFT JOIN executable_part_subjects s ON s.decision_id = d.id \
         WHERE d.agent_instance_id = ?1 ORDER BY d.kind, d.part_name",
    )
    .bind(instance.to_string())
    .fetch_all(pool)
    .await
    .map_err(store_error)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let get = |column: &str| -> Result<String, CheckError> {
            row.try_get::<String, _>(column).map_err(|_| corrupt())
        };
        out.push(StoredDecision {
            decision: ExecutablePartDecision {
                id: ExecutablePartDecisionId::parse(&get("id")?).map_err(|_| corrupt())?,
                agent_instance_id: instance,
                part_name: get("part_name")?,
                kind: kind_from_text(&get("kind")?).ok_or_else(corrupt)?,
                decision: trust_from_text(&get("decision")?).ok_or_else(corrupt)?,
                source: DecisionSource::OrganisationPolicy,
                decided_at: parse_ts(&get("decided_at")?)?,
            },
            declared_by: row
                .try_get::<Option<String>, _>("declared_by")
                .map_err(|_| corrupt())?
                .unwrap_or_default(),
        });
    }
    Ok(out)
}

/// Every instance with stored decisions.
pub(crate) async fn decided_instances(
    pool: &SqlitePool,
) -> Result<Vec<AgentInstanceId>, CheckError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT d.agent_instance_id FROM executable_part_decisions d \
         JOIN executable_part_subjects s ON s.decision_id = d.id ORDER BY d.agent_instance_id",
    )
    .fetch_all(pool)
    .await
    .map_err(store_error)?;
    ids.iter()
        .map(|id| AgentInstanceId::parse(id).map_err(|_| corrupt()))
        .collect()
}

/// One decision re-decided (step 32).
pub(crate) async fn update_decisions(
    pool: &SqlitePool,
    changed: &[ExecutablePartDecision],
) -> Result<(), CheckError> {
    let mut tx = pool.begin().await.map_err(store_error)?;
    for d in changed {
        sqlx::query(
            "UPDATE executable_part_decisions SET decision = ?1, decided_at = ?2 WHERE id = ?3",
        )
        .bind(trust_text(d.decision))
        .bind(d.decided_at.to_wire())
        .bind(d.id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(store_error)?;
    }
    tx.commit().await.map_err(store_error)
}

/// `forget_instance`: the decisions and the install candidate row.
pub(crate) async fn forget(pool: &SqlitePool, instance: AgentInstanceId) -> Result<(), CheckError> {
    let mut tx = pool.begin().await.map_err(store_error)?;
    delete_decisions(&mut tx, instance).await?;
    sqlx::query("DELETE FROM install_candidate_parts WHERE agent_instance_id = ?1")
        .bind(instance.to_string())
        .execute(&mut *tx)
        .await
        .map_err(store_error)?;
    tx.commit().await.map_err(store_error)
}
