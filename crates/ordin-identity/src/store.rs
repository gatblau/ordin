//! The two tables of this crate in the journal database (Data Model) and their migration steps.
//! Every query is parameterised; no SQL text is built from a value (SA-DB-01).

use ordin_types::domain::identity::{Credential, CredentialKind, CredentialState};
use ordin_types::time::parse_rfc3339_ms_utc;
use ordin_types::time::Timestamp;
use ordin_types::CredentialId;
use sqlx::{Row as _, SqlitePool};

use crate::error::{code, CredError};

/// Step 1: the two tables, exactly as the Data Model states them.
// spec: ordin-desktop/components/IdentityAndCredentials#SA-RECON-01
pub(crate) const MIGRATION_1_SQL: &str = r#"CREATE TABLE credentials (
    id            TEXT PRIMARY KEY CHECK (length(id) = 36),
    kind          TEXT NOT NULL CHECK (kind IN ('github','oidc','slack','confluence','microsoft','google',
                     'mail_server_single_sign_on','mail_server_password','mail_server_app_password','gitlab')),
    account_label TEXT NOT NULL CHECK (length(account_label) BETWEEN 1 AND 254),
    keychain_ref  TEXT NOT NULL CHECK (length(keychain_ref) BETWEEN 1 AND 255),
    expires_at    TEXT,
    state         TEXT NOT NULL CHECK (state IN ('active','revoked')),
    created_at    TEXT NOT NULL,
    refreshed_at  TEXT
);
CREATE UNIQUE INDEX credentials_active_kind_label ON credentials(kind, account_label) WHERE state = 'active';

-- chosen here: non-secret sign-in health and granted scopes
CREATE TABLE credential_meta (
    credential_id   TEXT PRIMARY KEY REFERENCES credentials(id) ON DELETE CASCADE,
    provider_host   TEXT NOT NULL CHECK (length(provider_host) BETWEEN 1 AND 255),
    scopes          TEXT NOT NULL DEFAULT '[]',          -- JSON array of strings
    needs_sign_in   INTEGER NOT NULL DEFAULT 0 CHECK (needs_sign_in IN (0,1)),
    refused_at      TEXT,
    last_error_code TEXT CHECK (last_error_code IS NULL OR length(last_error_code) <= 64),
    display_name    TEXT CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 254)
);
"#;

/// Step 2 (amended January 2026, ADR-106): the kind `CHECK` widened by `jira`, a widening rebuild under
/// DatabaseMigrations rule 12a. `credential_meta` is copied and dropped before `credentials`, so no
/// `ON DELETE CASCADE` fires; the renames carry its reference to the new `credentials`.
pub(crate) const MIGRATION_2_SQL: &str = r#"CREATE TABLE credentials_v2 (
    id            TEXT PRIMARY KEY CHECK (length(id) = 36),
    kind          TEXT NOT NULL CHECK (kind IN ('github','oidc','slack','confluence','microsoft','google',
                     'mail_server_single_sign_on','mail_server_password','mail_server_app_password','gitlab','jira')),
    account_label TEXT NOT NULL CHECK (length(account_label) BETWEEN 1 AND 254),
    keychain_ref  TEXT NOT NULL CHECK (length(keychain_ref) BETWEEN 1 AND 255),
    expires_at    TEXT,
    state         TEXT NOT NULL CHECK (state IN ('active','revoked')),
    created_at    TEXT NOT NULL,
    refreshed_at  TEXT
);
CREATE TABLE credential_meta_v2 (
    credential_id   TEXT PRIMARY KEY REFERENCES credentials_v2(id) ON DELETE CASCADE,
    provider_host   TEXT NOT NULL CHECK (length(provider_host) BETWEEN 1 AND 255),
    scopes          TEXT NOT NULL DEFAULT '[]',
    needs_sign_in   INTEGER NOT NULL DEFAULT 0 CHECK (needs_sign_in IN (0,1)),
    refused_at      TEXT,
    last_error_code TEXT CHECK (last_error_code IS NULL OR length(last_error_code) <= 64),
    display_name    TEXT CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 254)
);
INSERT INTO credentials_v2 SELECT id, kind, account_label, keychain_ref, expires_at, state, created_at, refreshed_at FROM credentials;
INSERT INTO credential_meta_v2 SELECT credential_id, provider_host, scopes, needs_sign_in, refused_at, last_error_code, display_name FROM credential_meta;
DROP TABLE credential_meta;
DROP TABLE credentials;
ALTER TABLE credentials_v2 RENAME TO credentials;
ALTER TABLE credential_meta_v2 RENAME TO credential_meta;
CREATE UNIQUE INDEX credentials_active_kind_label ON credentials(kind, account_label) WHERE state = 'active';
"#;

/// The non-secret sign-in health of one credential (`credential_meta`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Meta {
    pub provider_host: String,
    pub scopes: Vec<String>,
    pub needs_sign_in: bool,
    pub refused_at: Option<Timestamp>,
    pub last_error_code: Option<String>,
    pub display_name: Option<String>,
}

/// One `credentials` row with its `credential_meta` row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub credential: Credential,
    pub meta: Meta,
}

pub(crate) fn kind_str(kind: CredentialKind) -> &'static str {
    match kind {
        CredentialKind::Github => "github",
        CredentialKind::Gitlab => "gitlab",
        CredentialKind::Oidc => "oidc",
        CredentialKind::Slack => "slack",
        CredentialKind::Confluence => "confluence",
        CredentialKind::Microsoft => "microsoft",
        CredentialKind::Google => "google",
        CredentialKind::MailServerSingleSignOn => "mail_server_single_sign_on",
        CredentialKind::MailServerPassword => "mail_server_password",
        CredentialKind::MailServerAppPassword => "mail_server_app_password",
        CredentialKind::Jira => "jira",
    }
}

fn kind_from(raw: &str) -> Option<CredentialKind> {
    serde_json::from_value(serde_json::Value::from(raw)).ok()
}

fn store_failed<E>(_: E) -> CredError {
    CredError::of(code::STORE_FAILED)
}

fn ts(raw: Option<String>) -> Result<Option<Timestamp>, CredError> {
    raw.map(|text| parse_rfc3339_ms_utc(&text).map(Timestamp))
        .transpose()
        .map_err(store_failed)
}

fn wire(t: Option<Timestamp>) -> Option<String> {
    t.map(|t| t.to_wire())
}

const SELECT: &str =
    "SELECT c.id, c.kind, c.account_label, c.keychain_ref, c.expires_at, c.state, \
     c.created_at, c.refreshed_at, m.provider_host, m.scopes, m.needs_sign_in, m.refused_at, \
     m.last_error_code, m.display_name \
     FROM credentials c JOIN credential_meta m ON m.credential_id = c.id WHERE c.state = 'active'";

/// A.3: every active row with its meta row.
pub(crate) async fn load_active(pool: &SqlitePool) -> Result<Vec<Row>, CredError> {
    let rows = sqlx::query(SELECT)
        .fetch_all(pool)
        .await
        .map_err(store_failed)?;
    rows.iter().map(read_row).collect()
}

fn read_row(row: &sqlx::sqlite::SqliteRow) -> Result<Row, CredError> {
    let get = |name: &str| row.try_get::<String, _>(name).map_err(store_failed);
    let opt = |name: &str| row.try_get::<Option<String>, _>(name).map_err(store_failed);
    let id = CredentialId::parse(&get("id")?).map_err(store_failed)?;
    let kind = kind_from(&get("kind")?).ok_or_else(|| CredError::of(code::STORE_FAILED))?;
    let scopes: Vec<String> = serde_json::from_str(&get("scopes")?).map_err(store_failed)?;
    let needs: i64 = row.try_get("needs_sign_in").map_err(store_failed)?;
    Ok(Row {
        credential: Credential {
            id,
            kind,
            account_label: get("account_label")?,
            keychain_ref: get("keychain_ref")?,
            expires_at: ts(opt("expires_at")?)?,
            state: CredentialState::Active,
            created_at: ts(Some(get("created_at")?))?
                .ok_or_else(|| CredError::of(code::STORE_FAILED))?,
            refreshed_at: ts(opt("refreshed_at")?)?,
        },
        meta: Meta {
            provider_host: get("provider_host")?,
            scopes,
            needs_sign_in: needs == 1,
            refused_at: ts(opt("refused_at")?)?,
            last_error_code: opt("last_error_code")?,
            display_name: opt("display_name")?,
        },
    })
}

/// C.10: inserts a new row or updates the row of the same id, with its meta row, in one transaction.
/// A unique-index violation on `(kind, account_label)` among active rows is `CRED_DUPLICATE`.
pub(crate) async fn upsert(pool: &SqlitePool, row: &Row, is_new: bool) -> Result<(), CredError> {
    let c = &row.credential;
    let m = &row.meta;
    let scopes = serde_json::to_string(&m.scopes).map_err(store_failed)?;
    let mut tx = pool.begin().await.map_err(store_failed)?;
    let written = if is_new {
        sqlx::query(
            "INSERT INTO credentials (id, kind, account_label, keychain_ref, expires_at, state, created_at, refreshed_at) \
             VALUES (?, ?, ?, ?, ?, 'active', ?, ?)",
        )
        .bind(c.id.to_string())
        .bind(kind_str(c.kind))
        .bind(&c.account_label)
        .bind(&c.keychain_ref)
        .bind(wire(c.expires_at))
        .bind(c.created_at.to_wire())
        .bind(wire(c.refreshed_at))
        .execute(&mut *tx)
        .await
    } else {
        sqlx::query(
            "UPDATE credentials SET kind = ?, account_label = ?, keychain_ref = ?, expires_at = ?, refreshed_at = ? \
             WHERE id = ?",
        )
        .bind(kind_str(c.kind))
        .bind(&c.account_label)
        .bind(&c.keychain_ref)
        .bind(wire(c.expires_at))
        .bind(wire(c.refreshed_at))
        .bind(c.id.to_string())
        .execute(&mut *tx)
        .await
    };
    if let Err(e) = written {
        let unique = e
            .as_database_error()
            .is_some_and(|d| d.is_unique_violation());
        return Err(CredError::of(if unique {
            code::DUPLICATE
        } else {
            code::STORE_FAILED
        }));
    }
    sqlx::query(
        "INSERT INTO credential_meta (credential_id, provider_host, scopes, needs_sign_in, refused_at, last_error_code, display_name) \
         VALUES (?, ?, ?, 0, NULL, ?, ?) \
         ON CONFLICT (credential_id) DO UPDATE SET provider_host = excluded.provider_host, scopes = excluded.scopes, \
         needs_sign_in = 0, refused_at = NULL, display_name = excluded.display_name",
    )
    .bind(c.id.to_string())
    .bind(&m.provider_host)
    .bind(scopes)
    .bind(&m.last_error_code)
    .bind(&m.display_name)
    .execute(&mut *tx)
    .await
    .map_err(store_failed)?;
    tx.commit().await.map_err(store_failed)
}

/// Marks a credential `needs_sign_in` with its reason (A.4, E.3, E.4, F.3).
pub(crate) async fn mark_needs_sign_in(
    pool: &SqlitePool,
    id: CredentialId,
    last_error_code: &str,
    refused_at: Option<Timestamp>,
) -> Result<(), CredError> {
    sqlx::query(
        "UPDATE credential_meta SET needs_sign_in = 1, last_error_code = ?, \
         refused_at = COALESCE(?, refused_at) WHERE credential_id = ?",
    )
    .bind(last_error_code)
    .bind(wire(refused_at))
    .bind(id.to_string())
    .execute(pool)
    .await
    .map_err(store_failed)?;
    Ok(())
}

/// E.4: a refresh sets `expires_at` and `refreshed_at`.
pub(crate) async fn record_refresh(
    pool: &SqlitePool,
    id: CredentialId,
    expires_at: Option<Timestamp>,
    refreshed_at: Timestamp,
) -> Result<(), CredError> {
    sqlx::query("UPDATE credentials SET expires_at = ?, refreshed_at = ? WHERE id = ?")
        .bind(wire(expires_at))
        .bind(refreshed_at.to_wire())
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(store_failed)?;
    Ok(())
}

/// G.2: `state='revoked'`.
pub(crate) async fn revoke(pool: &SqlitePool, id: CredentialId) -> Result<(), CredError> {
    sqlx::query("UPDATE credentials SET state = 'revoked' WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await
        .map_err(store_failed)?;
    Ok(())
}

/// The stored state of any row, active or revoked; None when the id is unknown.
pub(crate) async fn state_of(
    pool: &SqlitePool,
    id: CredentialId,
) -> Result<Option<String>, CredError> {
    let row = sqlx::query("SELECT state FROM credentials WHERE id = ?")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await
        .map_err(store_failed)?;
    row.map(|r| r.try_get::<String, _>("state").map_err(store_failed))
        .transpose()
}
