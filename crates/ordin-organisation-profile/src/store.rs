//! The two tables this crate owns, `organisation_profiles` and `remote_agents` (*Data Model*), in the
//! journal database. Every query is fixed SQL with bound parameters (SA-DB-01).

use ordin_types::domain::agents::{RemoteAgent, RemoteAgentStatus, RemoteRoute};
use ordin_types::time::parse_rfc3339_ms_utc;
use ordin_types::time::Timestamp;
use ordin_types::{OrganisationProfileId, RemoteAgentId};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row as _, SqliteConnection, SqlitePool};

use crate::document::{OrganisationProfileDocument, RemoteAgentRoute};
use crate::error::ProfileError;
use crate::validate::{is_slack_channel_id, is_slack_member_id};

/// Rows in `replaced` or `failed_verification` kept at each activation, newest by `fetched_at`.
const KEPT_OLD_ROWS: i64 = 20;

fn unavailable<E>(_: E) -> ProfileError {
    ProfileError::DatastoreUnavailable
}

fn timestamp(raw: &str) -> Result<Timestamp, ProfileError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(unavailable)
}

/// The active row's columns the service needs.
#[derive(Debug, Clone)]
pub(crate) struct ActiveRow {
    pub(crate) id: OrganisationProfileId,
    pub(crate) profile_version: String,
    pub(crate) document_digest: String,
    pub(crate) cached_path: String,
    pub(crate) activated_at: Timestamp,
}

/// Every row in state `active` (the unique index keeps at most one; two is `PROF_MULTIPLE_ACTIVE`).
pub(crate) async fn active_row(pool: &SqlitePool) -> Result<Option<ActiveRow>, ProfileError> {
    let rows = sqlx::query(
        "SELECT id, profile_version, document_digest, cached_path, activated_at \
         FROM organisation_profiles WHERE state = 'active'",
    )
    .fetch_all(pool)
    .await
    .map_err(unavailable)?;
    if rows.len() > 1 {
        return Err(ProfileError::MultipleActive);
    }
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let id: String = row.try_get("id").map_err(unavailable)?;
    let activated_at: String = row.try_get("activated_at").map_err(unavailable)?;
    Ok(Some(ActiveRow {
        id: OrganisationProfileId::parse(&id).map_err(unavailable)?,
        profile_version: row.try_get("profile_version").map_err(unavailable)?,
        document_digest: row.try_get("document_digest").map_err(unavailable)?,
        cached_path: row.try_get("cached_path").map_err(unavailable)?,
        activated_at: timestamp(&activated_at)?,
    }))
}

/// A row already holding a digest: its id, state and refusal reason.
pub(crate) struct DigestRow {
    pub(crate) id: String,
    pub(crate) state: String,
    pub(crate) refusal_reason: Option<String>,
}

pub(crate) async fn row_by_digest(
    pool: &SqlitePool,
    digest: &str,
) -> Result<Option<DigestRow>, ProfileError> {
    let row = sqlx::query(
        "SELECT id, state, refusal_reason FROM organisation_profiles WHERE document_digest = ?",
    )
    .bind(digest)
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    row.map(|r| {
        Ok(DigestRow {
            id: r.try_get("id").map_err(unavailable)?,
            state: r.try_get("state").map_err(unavailable)?,
            refusal_reason: r.try_get("refusal_reason").map_err(unavailable)?,
        })
    })
    .transpose()
}

/// Removes a row left in `fetched` or `verified` by an attempt that never finished.
pub(crate) async fn delete_unfinished(pool: &SqlitePool, id: &str) -> Result<(), ProfileError> {
    sqlx::query(
        "DELETE FROM organisation_profiles WHERE id = ? AND state IN ('fetched', 'verified')",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(unavailable)?;
    Ok(())
}

/// Step 2.7: a new row in state `fetched`, `profile_version` `unknown` until parsed.
pub(crate) async fn insert_fetched(
    pool: &SqlitePool,
    id: OrganisationProfileId,
    signature: &str,
    digest: &str,
    cached_path: &str,
    fetched_at: Timestamp,
) -> Result<(), ProfileError> {
    sqlx::query(
        "INSERT INTO organisation_profiles \
         (id, profile_version, signature, document_digest, cached_path, state, fetched_at) \
         VALUES (?, 'unknown', ?, ?, ?, 'fetched', ?)",
    )
    .bind(id.to_string())
    .bind(signature)
    .bind(digest)
    .bind(cached_path)
    .bind(fetched_at.to_wire())
    .execute(pool)
    .await
    .map_err(unavailable)?;
    Ok(())
}

/// The version to record: the parsed one when it fits the column (1–64 characters), else `unknown`.
fn recordable(version: Option<&str>) -> &str {
    match version {
        Some(v) if (1..=64).contains(&v.chars().count()) => v,
        _ => "unknown",
    }
}

/// Step 2.8: the row becomes `failed_verification` with its reason.
pub(crate) async fn mark_failed(
    pool: &SqlitePool,
    id: OrganisationProfileId,
    version: Option<&str>,
    reason: &str,
) -> Result<(), ProfileError> {
    sqlx::query(
        "UPDATE organisation_profiles SET state = 'failed_verification', refusal_reason = ?, \
         profile_version = ? WHERE id = ? AND state = 'fetched'",
    )
    .bind(reason)
    .bind(recordable(version))
    .bind(id.to_string())
    .execute(pool)
    .await
    .map_err(unavailable)?;
    Ok(())
}

/// Step 3.8: the row becomes `verified`.
pub(crate) async fn mark_verified(
    pool: &SqlitePool,
    id: OrganisationProfileId,
    version: &str,
    at: Timestamp,
) -> Result<(), ProfileError> {
    sqlx::query(
        "UPDATE organisation_profiles SET state = 'verified', profile_version = ?, verified_at = ? \
         WHERE id = ? AND state = 'fetched'",
    )
    .bind(recordable(Some(version)))
    .bind(at.to_wire())
    .bind(id.to_string())
    .execute(pool)
    .await
    .map_err(unavailable)?;
    Ok(())
}

/// The counts `profile.remote_agents.synced` logs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SyncCounts {
    pub(crate) listed_new: u64,
    pub(crate) changed: u64,
    pub(crate) withdrawn: u64,
}

/// Step 4.2: one transaction — replace the active row, activate this one, sync the remote agents and
/// prune old rows.
pub(crate) async fn activate(
    pool: &SqlitePool,
    id: OrganisationProfileId,
    document: &OrganisationProfileDocument,
    now: Timestamp,
) -> Result<SyncCounts, ProfileError> {
    let mut tx = pool.begin().await.map_err(unavailable)?;
    sqlx::query("UPDATE organisation_profiles SET state = 'replaced' WHERE state = 'active'")
        .execute(&mut *tx)
        .await
        .map_err(unavailable)?;
    let changed = sqlx::query(
        "UPDATE organisation_profiles SET state = 'active', activated_at = ? \
         WHERE id = ? AND state = 'verified'",
    )
    .bind(now.to_wire())
    .bind(id.to_string())
    .execute(&mut *tx)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => ProfileError::MultipleActive,
        _ => ProfileError::DatastoreUnavailable,
    })?;
    if changed.rows_affected() == 0 {
        // Dropping `tx` rolls the replacement back.
        return Err(ProfileError::UnverifiedActivation);
    }
    let counts = sync_remote_agents(&mut tx, document, now).await?;
    sqlx::query(
        "DELETE FROM organisation_profiles WHERE state IN ('replaced', 'failed_verification') \
         AND id NOT IN (SELECT id FROM organisation_profiles \
         WHERE state IN ('replaced', 'failed_verification') ORDER BY fetched_at DESC LIMIT ?)",
    )
    .bind(KEPT_OLD_ROWS)
    .execute(&mut *tx)
    .await
    .map_err(unavailable)?;
    tx.commit().await.map_err(unavailable)?;
    Ok(counts)
}

/// The remote agent row an entry maps to (Internal Logic 5.1).
struct Target {
    route: RemoteRoute,
    deployment_address: Option<String>,
    slack_account: Option<String>,
    slack_channel: Option<String>,
    identity_provider_ref: Option<String>,
}

/// Step 5: the remote agent catalogue follows the profile, inside the activation transaction.
async fn sync_remote_agents(
    conn: &mut SqliteConnection,
    document: &OrganisationProfileDocument,
    now: Timestamp,
) -> Result<SyncCounts, ProfileError> {
    let live: Vec<RemoteAgent> = sqlx::query(
        "SELECT id, display_name, route, deployment_address, slack_account, slack_channel, \
         identity_provider_ref, status, listed_at FROM remote_agents WHERE status <> 'withdrawn'",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(unavailable)?
    .iter()
    .map(agent_from_row)
    .collect::<Result<_, _>>()?;
    let dedicated_channel = document
        .slack
        .as_ref()
        .and_then(|s| s.dedicated_channel.clone());
    let mut counts = SyncCounts::default();
    for entry in &document.remote_agents {
        let target = match entry.route {
            RemoteAgentRoute::HostedAgent => Target {
                route: RemoteRoute::Http,
                deployment_address: Some(entry.address.clone()),
                slack_account: None,
                slack_channel: None,
                identity_provider_ref: Some(entry.name.clone()),
            },
            RemoteAgentRoute::Slack => Target {
                route: RemoteRoute::Slack,
                deployment_address: None,
                slack_account: Some(entry.address.clone()),
                slack_channel: dedicated_channel.clone(),
                identity_provider_ref: None,
            },
        };
        let existing = live.iter().find(|a| a.display_name == entry.name);
        let mut agent = match existing {
            Some(a) => a.clone(),
            None => RemoteAgent {
                id: RemoteAgentId::new(),
                display_name: entry.name.clone(),
                route: target.route,
                deployment_address: None,
                slack_account: None,
                slack_channel: None,
                identity_provider_ref: None,
                status: RemoteAgentStatus::Listed,
                listed_at: now,
            },
        };
        let address_changed = existing.is_some_and(|a| {
            a.route != target.route
                || a.deployment_address != target.deployment_address
                || a.slack_account != target.slack_account
        });
        let fields_changed = existing.is_some_and(|a| {
            a.slack_channel != target.slack_channel
                || a.identity_provider_ref != target.identity_provider_ref
        });
        agent.route = target.route;
        agent.deployment_address = target.deployment_address;
        agent.slack_account = target.slack_account;
        agent.slack_channel = target.slack_channel;
        agent.identity_provider_ref = target.identity_provider_ref;
        if address_changed {
            // A changed address must be re-verified.
            agent.status = RemoteAgentStatus::Listed;
        }
        check_remote_agent(&agent)?;
        if existing.is_none() {
            insert_agent(conn, &agent).await?;
            counts.listed_new += 1;
        } else if address_changed || fields_changed {
            update_agent(conn, &agent).await?;
            if address_changed {
                counts.changed += 1;
            }
        }
    }
    for agent in &live {
        if !document
            .remote_agents
            .iter()
            .any(|e| e.name == agent.display_name)
        {
            sqlx::query("UPDATE remote_agents SET status = 'withdrawn' WHERE id = ?")
                .bind(agent.id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(unavailable)?;
            counts.withdrawn += 1;
        }
    }
    Ok(counts)
}

/// The 2C `RemoteAgent` rules (`RAG_NAME_INVALID`, `RAG_HTTP_FIELDS_INVALID`, `RAG_SLACK_FIELDS_INVALID`);
/// they cannot fail after the validation table and are checked before every write.
pub(crate) fn check_remote_agent(agent: &RemoteAgent) -> Result<(), ProfileError> {
    if !(1..=120).contains(&agent.display_name.chars().count()) {
        return Err(ProfileError::RemoteAgentNameInvalid);
    }
    match agent.route {
        RemoteRoute::Http => {
            let address_ok = agent
                .deployment_address
                .as_deref()
                .is_some_and(|a| a.len() <= 2048 && crate::installer::check_address(a).is_ok());
            if !address_ok
                || agent.identity_provider_ref.is_none()
                || agent.slack_account.is_some()
                || agent.slack_channel.is_some()
            {
                return Err(ProfileError::RemoteAgentHttpFieldsInvalid);
            }
        }
        RemoteRoute::Slack => {
            let account_ok = agent
                .slack_account
                .as_deref()
                .is_some_and(is_slack_member_id);
            let channel_ok = agent
                .slack_channel
                .as_deref()
                .is_some_and(is_slack_channel_id);
            if !account_ok || !channel_ok || agent.deployment_address.is_some() {
                return Err(ProfileError::RemoteAgentSlackFieldsInvalid);
            }
        }
    }
    Ok(())
}

async fn insert_agent(
    conn: &mut SqliteConnection,
    agent: &RemoteAgent,
) -> Result<(), ProfileError> {
    sqlx::query(
        "INSERT INTO remote_agents (id, display_name, route, deployment_address, slack_account, \
         slack_channel, identity_provider_ref, status, listed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(agent.id.to_string())
    .bind(&agent.display_name)
    .bind(route_text(agent.route))
    .bind(&agent.deployment_address)
    .bind(&agent.slack_account)
    .bind(&agent.slack_channel)
    .bind(&agent.identity_provider_ref)
    .bind(status_text(agent.status))
    .bind(agent.listed_at.to_wire())
    .execute(conn)
    .await
    .map_err(unavailable)?;
    Ok(())
}

async fn update_agent(
    conn: &mut SqliteConnection,
    agent: &RemoteAgent,
) -> Result<(), ProfileError> {
    sqlx::query(
        "UPDATE remote_agents SET route = ?, deployment_address = ?, slack_account = ?, \
         slack_channel = ?, identity_provider_ref = ?, status = ? WHERE id = ?",
    )
    .bind(route_text(agent.route))
    .bind(&agent.deployment_address)
    .bind(&agent.slack_account)
    .bind(&agent.slack_channel)
    .bind(&agent.identity_provider_ref)
    .bind(status_text(agent.status))
    .bind(agent.id.to_string())
    .execute(conn)
    .await
    .map_err(unavailable)?;
    Ok(())
}

fn route_text(route: RemoteRoute) -> &'static str {
    match route {
        RemoteRoute::Http => "http",
        RemoteRoute::Slack => "slack",
    }
}

fn status_text(status: RemoteAgentStatus) -> &'static str {
    match status {
        RemoteAgentStatus::Listed => "listed",
        RemoteAgentStatus::Enabled => "enabled",
        RemoteAgentStatus::Verified => "verified",
        RemoteAgentStatus::Withdrawn => "withdrawn",
    }
}

fn agent_from_row(row: &SqliteRow) -> Result<RemoteAgent, ProfileError> {
    let id: String = row.try_get("id").map_err(unavailable)?;
    let route: String = row.try_get("route").map_err(unavailable)?;
    let status: String = row.try_get("status").map_err(unavailable)?;
    let listed_at: String = row.try_get("listed_at").map_err(unavailable)?;
    Ok(RemoteAgent {
        id: RemoteAgentId::parse(&id).map_err(unavailable)?,
        display_name: row.try_get("display_name").map_err(unavailable)?,
        route: match route.as_str() {
            "http" => RemoteRoute::Http,
            "slack" => RemoteRoute::Slack,
            _ => return Err(ProfileError::DatastoreUnavailable),
        },
        deployment_address: row.try_get("deployment_address").map_err(unavailable)?,
        slack_account: row.try_get("slack_account").map_err(unavailable)?,
        slack_channel: row.try_get("slack_channel").map_err(unavailable)?,
        identity_provider_ref: row.try_get("identity_provider_ref").map_err(unavailable)?,
        status: match status.as_str() {
            "listed" => RemoteAgentStatus::Listed,
            "enabled" => RemoteAgentStatus::Enabled,
            "verified" => RemoteAgentStatus::Verified,
            "withdrawn" => RemoteAgentStatus::Withdrawn,
            _ => return Err(ProfileError::DatastoreUnavailable),
        },
        listed_at: timestamp(&listed_at)?,
    })
}

/// Every remote agent row, withdrawn ones included, oldest listing first.
pub(crate) async fn list_agents(pool: &SqlitePool) -> Result<Vec<RemoteAgent>, ProfileError> {
    sqlx::query(
        "SELECT id, display_name, route, deployment_address, slack_account, slack_channel, \
         identity_provider_ref, status, listed_at FROM remote_agents ORDER BY listed_at, id",
    )
    .fetch_all(pool)
    .await
    .map_err(unavailable)?
    .iter()
    .map(agent_from_row)
    .collect()
}

pub(crate) async fn get_agent(
    pool: &SqlitePool,
    id: RemoteAgentId,
) -> Result<RemoteAgent, ProfileError> {
    let row = sqlx::query(
        "SELECT id, display_name, route, deployment_address, slack_account, slack_channel, \
         identity_provider_ref, status, listed_at FROM remote_agents WHERE id = ?",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    match row {
        Some(row) => agent_from_row(&row),
        None => Err(ProfileError::RemoteAgentNotFound),
    }
}

pub(crate) async fn set_agent_status(
    pool: &SqlitePool,
    id: RemoteAgentId,
    status: RemoteAgentStatus,
) -> Result<(), ProfileError> {
    sqlx::query("UPDATE remote_agents SET status = ? WHERE id = ?")
        .bind(status_text(status))
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(unavailable)?;
    Ok(())
}
