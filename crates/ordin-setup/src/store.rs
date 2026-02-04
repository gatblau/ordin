//! The setup tables (SetupAssistant.md *Data Model* and the ADR-114 amendment's), reached through
//! the Journal's SQLite datastore. Every statement binds its values.

use ordin_types::domain::identity::{HarnessAttachment, HarnessAttachmentState, HarnessKind};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::HarnessAttachmentId;
use sqlx::{Row, SqlitePool};

use crate::error::SetupError;
use crate::types::{CheckKind, CheckResult};

/// At most 5 kept release notes (Configuration amendment January 2026).
const WHATS_NEW_KEPT: i64 = 5;
/// A `running` server update older than this is taken as abandoned (SET-UP-15).
const ABANDONED_AFTER: chrono::Duration = chrono::Duration::minutes(30);

fn store_failed() -> SetupError {
    SetupError::of("SETUP_STORE_FAILED")
}

fn time(raw: &str) -> Result<Timestamp, SetupError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| store_failed())
}

pub(crate) fn harness_text(kind: HarnessKind) -> &'static str {
    match kind {
        HarnessKind::ClaudeCode => "claude_code",
        HarnessKind::CopilotCli => "copilot_cli",
    }
}

pub(crate) fn state_text(state: HarnessAttachmentState) -> &'static str {
    match state {
        HarnessAttachmentState::Found => "found",
        HarnessAttachmentState::Checked => "checked",
        HarnessAttachmentState::Ready => "ready",
        HarnessAttachmentState::NeedsSignIn => "needs_sign_in",
        HarnessAttachmentState::NeedsUpdate => "needs_update",
    }
}

fn state_of(raw: &str) -> Result<HarnessAttachmentState, SetupError> {
    Ok(match raw {
        "found" => HarnessAttachmentState::Found,
        "checked" => HarnessAttachmentState::Checked,
        "ready" => HarnessAttachmentState::Ready,
        "needs_sign_in" => HarnessAttachmentState::NeedsSignIn,
        "needs_update" => HarnessAttachmentState::NeedsUpdate,
        _ => return Err(store_failed()),
    })
}

/// The 2C rules of `HarnessAttachment`, checked before a write (`HARN_*`, emitter defects).
pub(crate) fn check_attachment(a: &HarnessAttachment) -> Result<(), SetupError> {
    let location_ok = !a.location.is_empty()
        && a.location.len() <= 4096
        && std::path::Path::new(&a.location).is_absolute();
    if !location_ok {
        return Err(SetupError::permanent("HARN_PATH_INVALID"));
    }
    if a.version.as_ref().is_some_and(|v| v.chars().count() > 64) {
        return Err(SetupError::permanent("HARN_VERSION_INVALID"));
    }
    if a.sign_in_method
        .as_ref()
        .is_some_and(|m| m.chars().count() > 120)
    {
        return Err(SetupError::permanent("HARN_SIGN_IN_METHOD_INVALID"));
    }
    if a.state == HarnessAttachmentState::Ready && !(a.tested && a.signed_in) {
        return Err(SetupError::permanent("HARN_STATE_INCONSISTENT"));
    }
    Ok(())
}

/// One attachment per harness (`HARN_DUPLICATE` is kept away by the upsert on `harness`).
pub(crate) async fn put_attachment(
    pool: &SqlitePool,
    a: &HarnessAttachment,
) -> Result<(), SetupError> {
    check_attachment(a)?;
    sqlx::query(
        "INSERT INTO harness_attachments \
         (id, harness, location, version, tested, signed_in, sign_in_method, state, last_checked_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
         ON CONFLICT (harness) DO UPDATE SET location = excluded.location, version = excluded.version, \
         tested = excluded.tested, signed_in = excluded.signed_in, \
         sign_in_method = excluded.sign_in_method, state = excluded.state, \
         last_checked_at = excluded.last_checked_at",
    )
    .bind(a.id.to_string())
    .bind(harness_text(a.harness))
    .bind(&a.location)
    .bind(&a.version)
    .bind(i64::from(a.tested))
    .bind(i64::from(a.signed_in))
    .bind(&a.sign_in_method)
    .bind(state_text(a.state))
    .bind(a.last_checked_at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn attachments(pool: &SqlitePool) -> Result<Vec<HarnessAttachment>, SetupError> {
    let rows = sqlx::query(
        "SELECT id, harness, location, version, tested, signed_in, sign_in_method, state, \
         last_checked_at FROM harness_attachments ORDER BY harness",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let id: String = row.get("id");
            let harness: String = row.get("harness");
            let state: String = row.get("state");
            let at: String = row.get("last_checked_at");
            Ok(HarnessAttachment {
                id: id
                    .parse::<HarnessAttachmentId>()
                    .map_err(|_| store_failed())?,
                harness: match harness.as_str() {
                    "claude_code" => HarnessKind::ClaudeCode,
                    "copilot_cli" => HarnessKind::CopilotCli,
                    _ => return Err(store_failed()),
                },
                location: row.get("location"),
                version: row.get("version"),
                tested: row.get::<i64, _>("tested") == 1,
                signed_in: row.get::<i64, _>("signed_in") == 1,
                sign_in_method: row.get("sign_in_method"),
                state: state_of(&state)?,
                last_checked_at: time(&at)?,
            })
        })
        .collect()
}

pub(crate) async fn attachment(
    pool: &SqlitePool,
    harness: HarnessKind,
) -> Result<Option<HarnessAttachment>, SetupError> {
    Ok(attachments(pool)
        .await?
        .into_iter()
        .find(|a| a.harness == harness))
}

/// Replaces the stored result of a check. `SecondInstall` and `HarnessChoice` are not stored.
// SetupAssistant.md, SET-UP-27: the result is returned in the report and not stored, because
// the `check_kind` constraint of migration step 1 has no `second_install` value.
// spec: ordin-desktop/components/SetupAssistant#SET-UP-27
pub(crate) async fn put_check(pool: &SqlitePool, result: &CheckResult) -> Result<(), SetupError> {
    // `HarnessChoice` likewise: the person's answer is kept by the program (SET-CHO-1), and the
    // constraint has no `harness_choice` value.
    // spec: ordin-desktop/components/SetupAssistant#SET-CHO-3
    if matches!(
        result.check,
        CheckKind::SecondInstall | CheckKind::HarnessChoice
    ) {
        return Ok(());
    }
    let json = serde_json::to_string(result).map_err(|_| store_failed())?;
    sqlx::query(
        "INSERT INTO setup_checks (check_kind, outcome, code, result_json, checked_at) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT (check_kind) DO UPDATE SET outcome = excluded.outcome, code = excluded.code, \
         result_json = excluded.result_json, checked_at = excluded.checked_at",
    )
    .bind(result.check.as_str())
    .bind(result.outcome.as_str())
    .bind(&result.code)
    .bind(json)
    .bind(result.checked_at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

/// The stored result of a check, if any.
#[cfg(test)]
pub(crate) async fn check(
    pool: &SqlitePool,
    kind: CheckKind,
) -> Result<Option<CheckResult>, SetupError> {
    let row = sqlx::query("SELECT result_json FROM setup_checks WHERE check_kind = ?1")
        .bind(kind.as_str())
        .fetch_optional(pool)
        .await?;
    match row {
        Some(row) => Ok(Some(
            serde_json::from_str(&row.get::<String, _>(0)).map_err(|_| store_failed())?,
        )),
        None => Ok(None),
    }
}

/// `(last_guided_completed_at, requirements_digest)` of the one row, if it exists.
pub(crate) async fn setup_state(
    pool: &SqlitePool,
) -> Result<Option<(Option<String>, Option<String>)>, SetupError> {
    let row = sqlx::query(
        "SELECT last_guided_completed_at, requirements_digest FROM setup_state WHERE singleton = 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get(0), r.get(1))))
}

pub(crate) async fn set_completed(
    pool: &SqlitePool,
    at: Timestamp,
    digest: &str,
) -> Result<(), SetupError> {
    sqlx::query(
        "INSERT INTO setup_state (singleton, last_guided_completed_at, requirements_digest) \
         VALUES (1, ?1, ?2) ON CONFLICT (singleton) DO UPDATE SET \
         last_guided_completed_at = excluded.last_guided_completed_at, \
         requirements_digest = excluded.requirements_digest",
    )
    .bind(at.to_wire())
    .bind(digest)
    .execute(pool)
    .await?;
    Ok(())
}

/// SET-UP-4, SET-UP-25: keeps the notes of `version` (replace on version) and the newest 5 only,
/// in one transaction.
pub(crate) async fn keep_whats_new(
    pool: &SqlitePool,
    version: &str,
    notes: &str,
    at: Timestamp,
) -> Result<(), SetupError> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO setup_whats_new (version, notes, kept_at, seen_at) VALUES (?1, ?2, ?3, NULL) \
         ON CONFLICT (version) DO UPDATE SET notes = excluded.notes, kept_at = excluded.kept_at",
    )
    .bind(version)
    .bind(notes)
    .bind(at.to_wire())
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM setup_whats_new WHERE version NOT IN \
         (SELECT version FROM setup_whats_new ORDER BY kept_at DESC, version DESC LIMIT ?1)",
    )
    .bind(WHATS_NEW_KEPT)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// The highest version recorded as started (SET-REC-2), as stored.
pub(crate) async fn highest_started(pool: &SqlitePool) -> Result<Option<String>, SetupError> {
    let row = sqlx::query("SELECT highest_version FROM setup_started WHERE singleton = 1")
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.get(0)))
}

/// Replaces the one row of `setup_started` (SET-REC-2.1).
pub(crate) async fn record_started(
    pool: &SqlitePool,
    version: &str,
    at: Timestamp,
) -> Result<(), SetupError> {
    sqlx::query(
        "INSERT INTO setup_started (singleton, highest_version, recorded_at) VALUES (1, ?1, ?2) \
         ON CONFLICT (singleton) DO UPDATE SET highest_version = excluded.highest_version, \
         recorded_at = excluded.recorded_at",
    )
    .bind(version)
    .bind(at.to_wire())
    .execute(pool)
    .await?;
    Ok(())
}

/// `(notes, seen)` of the running version's row.
pub(crate) async fn whats_new(
    pool: &SqlitePool,
    version: &str,
) -> Result<Option<(String, bool)>, SetupError> {
    let row = sqlx::query("SELECT notes, seen_at FROM setup_whats_new WHERE version = ?1")
        .bind(version)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| (r.get(0), r.get::<Option<String>, _>(1).is_some())))
}

pub(crate) async fn mark_seen(
    pool: &SqlitePool,
    version: &str,
    at: Timestamp,
) -> Result<(), SetupError> {
    sqlx::query("UPDATE setup_whats_new SET seen_at = ?2 WHERE version = ?1 AND seen_at IS NULL")
        .bind(version)
        .bind(at.to_wire())
        .execute(pool)
        .await?;
    Ok(())
}

/// `(version, state, started_at)` of the server update row.
pub(crate) async fn server_update_row(
    pool: &SqlitePool,
) -> Result<Option<(String, String, Timestamp)>, SetupError> {
    let row = sqlx::query(
        "SELECT version, state, started_at FROM setup_server_update WHERE singleton = 1",
    )
    .fetch_optional(pool)
    .await?;
    match row {
        Some(r) => Ok(Some((r.get(0), r.get(1), time(&r.get::<String, _>(2))?))),
        None => Ok(None),
    }
}

/// SET-UP-15: claims the row for `version` in one transaction, or `SETUP_UPDATE_IN_PROGRESS`.
pub(crate) async fn claim_server_update(
    pool: &SqlitePool,
    version: &str,
    now: Timestamp,
) -> Result<(), SetupError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query("SELECT state, started_at FROM setup_server_update WHERE singleton = 1")
        .fetch_optional(&mut *tx)
        .await?;
    if let Some(row) = row {
        let state: String = row.get(0);
        let started = time(&row.get::<String, _>(1))?;
        if state == "running" && now.0 - started.0 < ABANDONED_AFTER {
            return Err(SetupError::permanent("SETUP_UPDATE_IN_PROGRESS"));
        }
    }
    sqlx::query(
        "INSERT INTO setup_server_update (singleton, version, state, code, started_at, ended_at) \
         VALUES (1, ?1, 'running', NULL, ?2, NULL) ON CONFLICT (singleton) DO UPDATE SET \
         version = excluded.version, state = 'running', code = NULL, \
         started_at = excluded.started_at, ended_at = NULL",
    )
    .bind(version)
    .bind(now.to_wire())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Releases a claim that found nothing to do.
pub(crate) async fn release_server_update(pool: &SqlitePool) -> Result<(), SetupError> {
    sqlx::query("DELETE FROM setup_server_update WHERE singleton = 1 AND state = 'running'")
        .execute(pool)
        .await?;
    Ok(())
}

/// Moves the row to `state`; `done` and `failed` carry `ended_at`.
pub(crate) async fn set_server_update(
    pool: &SqlitePool,
    state: &str,
    code: Option<&str>,
    now: Timestamp,
) -> Result<(), SetupError> {
    let ended = matches!(state, "done" | "failed").then(|| now.to_wire());
    sqlx::query(
        "UPDATE setup_server_update SET state = ?1, code = ?2, ended_at = ?3 WHERE singleton = 1",
    )
    .bind(state)
    .bind(code)
    .bind(ended)
    .execute(pool)
    .await?;
    Ok(())
}
