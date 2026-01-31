//! The SQLite statements of the Craft Toolkit (*Data Model*). The tables are reached through the
//! Journal's `Datastore` (SA-RECON-06).

use ordin_types::domain::engineering::{
    CraftCliMode, CraftInstallation, CraftInstallationStatus, FileTypeMapEntry, VerificationResult,
};
use ordin_types::time::Timestamp;
use ordin_types::{CraftInstallationId, FileTypeMapEntryId};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row as _, SqlitePool};

use crate::error::CraftError;

/// One `craft_installations` row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub(crate) id: CraftInstallationId,
    pub(crate) app_name: String,
    pub(crate) pinned_version: String,
    pub(crate) installed_version: Option<String>,
    /// The hash of the archive `installed_version` came from.
    pub(crate) installed_sha256: Option<String>,
    pub(crate) verification_result: Option<VerificationResult>,
    pub(crate) cli_modes: Vec<CraftCliMode>,
    pub(crate) status: CraftInstallationStatus,
    pub(crate) signature_check: Option<String>,
}

impl Row {
    pub(crate) fn approved(app_name: &str, pinned_version: &str) -> Row {
        Row {
            id: CraftInstallationId::new(),
            app_name: app_name.to_owned(),
            pinned_version: pinned_version.to_owned(),
            installed_version: None,
            installed_sha256: None,
            verification_result: None,
            cli_modes: Vec::new(),
            status: CraftInstallationStatus::Approved,
            signature_check: None,
        }
    }

    pub(crate) fn to_public(&self) -> CraftInstallation {
        CraftInstallation {
            id: self.id,
            app_name: self.app_name.clone(),
            pinned_version: self.pinned_version.clone(),
            installed_version: self.installed_version.clone(),
            verification_result: self.verification_result,
            cli_modes: self.cli_modes.clone(),
            status: self.status,
        }
    }

    pub(crate) fn has_mcp(&self) -> bool {
        self.cli_modes.contains(&CraftCliMode::Mcp)
    }

    pub(crate) fn is_installed(&self) -> bool {
        self.status == CraftInstallationStatus::Installed
    }
}

fn status_text(status: CraftInstallationStatus) -> &'static str {
    match status {
        CraftInstallationStatus::Approved => "approved",
        CraftInstallationStatus::Downloaded => "downloaded",
        CraftInstallationStatus::Installed => "installed",
        CraftInstallationStatus::VerificationFailed => "verification_failed",
        CraftInstallationStatus::Removed => "removed",
    }
}

fn parse_status(raw: &str) -> Result<CraftInstallationStatus, CraftError> {
    match raw {
        "approved" => Ok(CraftInstallationStatus::Approved),
        "downloaded" => Ok(CraftInstallationStatus::Downloaded),
        "installed" => Ok(CraftInstallationStatus::Installed),
        "verification_failed" => Ok(CraftInstallationStatus::VerificationFailed),
        "removed" => Ok(CraftInstallationStatus::Removed),
        _ => Err(CraftError::DatastoreUnavailable),
    }
}

fn result_text(result: VerificationResult) -> &'static str {
    match result {
        VerificationResult::Verified => "verified",
        VerificationResult::Failed => "failed",
    }
}

fn parse_result(raw: &str) -> Result<VerificationResult, CraftError> {
    match raw {
        "verified" => Ok(VerificationResult::Verified),
        "failed" => Ok(VerificationResult::Failed),
        _ => Err(CraftError::DatastoreUnavailable),
    }
}

fn row_of(row: &SqliteRow) -> Result<Row, CraftError> {
    let id: String = row.try_get("id")?;
    let modes: String = row.try_get("cli_modes")?;
    let status: String = row.try_get("status")?;
    let result: Option<String> = row.try_get("verification_result")?;
    Ok(Row {
        id: CraftInstallationId::parse(&id).map_err(|_| CraftError::DatastoreUnavailable)?,
        app_name: row.try_get("app_name")?,
        pinned_version: row.try_get("pinned_version")?,
        installed_version: row.try_get("installed_version")?,
        installed_sha256: row.try_get("installed_sha256")?,
        verification_result: result.as_deref().map(parse_result).transpose()?,
        cli_modes: serde_json::from_str(&modes).map_err(|_| CraftError::DatastoreUnavailable)?,
        status: parse_status(&status)?,
        signature_check: row.try_get("signature_check")?,
    })
}

/// Every row, by app name.
pub(crate) async fn load_rows(pool: &SqlitePool) -> Result<Vec<Row>, CraftError> {
    let rows = sqlx::query(
        "SELECT id, app_name, pinned_version, installed_version, installed_sha256, \
         verification_result, cli_modes, status, signature_check FROM craft_installations \
         ORDER BY app_name",
    )
    .fetch_all(pool)
    .await?;
    rows.iter().map(row_of).collect()
}

fn modes_json(row: &Row) -> Result<String, CraftError> {
    serde_json::to_string(&row.cli_modes).map_err(|_| CraftError::DatastoreUnavailable)
}

/// `CRAFT_DUPLICATE` guards the unique name; the app-name length rule is the column's.
pub(crate) async fn insert_row(pool: &SqlitePool, row: &Row) -> Result<(), CraftError> {
    let inserted = sqlx::query(
        "INSERT INTO craft_installations (id, app_name, pinned_version, installed_version, \
         installed_sha256, verification_result, cli_modes, status, signature_check, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(row.id.to_string())
    .bind(&row.app_name)
    .bind(&row.pinned_version)
    .bind(&row.installed_version)
    .bind(&row.installed_sha256)
    .bind(row.verification_result.map(result_text))
    .bind(modes_json(row)?)
    .bind(status_text(row.status))
    .bind(&row.signature_check)
    .bind(Timestamp::now().to_string())
    .execute(pool)
    .await;
    match inserted {
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            Err(CraftError::Invalid("CRAFT_DUPLICATE"))
        }
        Err(e) => Err(e.into()),
    }
}

/// `CraftInstallation.status = installed` requires `verification_result = verified`.
pub(crate) async fn update_row(pool: &SqlitePool, row: &Row) -> Result<(), CraftError> {
    if row.status == CraftInstallationStatus::Installed
        && row.verification_result != Some(VerificationResult::Verified)
    {
        return Err(CraftError::Invalid("CRAFT_UNVERIFIED_INSTALL"));
    }
    sqlx::query(
        "UPDATE craft_installations SET pinned_version = ?2, installed_version = ?3, \
         installed_sha256 = ?4, verification_result = ?5, cli_modes = ?6, status = ?7, \
         signature_check = ?8, updated_at = ?9 WHERE id = ?1",
    )
    .bind(row.id.to_string())
    .bind(&row.pinned_version)
    .bind(&row.installed_version)
    .bind(&row.installed_sha256)
    .bind(row.verification_result.map(result_text))
    .bind(modes_json(row)?)
    .bind(status_text(row.status))
    .bind(&row.signature_check)
    .bind(Timestamp::now().to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Every `file_type_map_entries` row.
pub(crate) async fn load_entries(pool: &SqlitePool) -> Result<Vec<FileTypeMapEntry>, CraftError> {
    let rows = sqlx::query(
        "SELECT id, extensions, media_types, open_with_app, craft_tools, profile_version \
         FROM file_type_map_entries ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    rows.iter().map(entry_of).collect()
}

fn list_of(raw: &str) -> Result<Vec<String>, CraftError> {
    serde_json::from_str(raw).map_err(|_| CraftError::DatastoreUnavailable)
}

fn entry_of(row: &SqliteRow) -> Result<FileTypeMapEntry, CraftError> {
    let id: String = row.try_get("id")?;
    let extensions: String = row.try_get("extensions")?;
    let media_types: String = row.try_get("media_types")?;
    let craft_tools: String = row.try_get("craft_tools")?;
    Ok(FileTypeMapEntry {
        id: FileTypeMapEntryId::parse(&id).map_err(|_| CraftError::DatastoreUnavailable)?,
        extensions: list_of(&extensions)?,
        media_types: list_of(&media_types)?,
        open_with_app: row.try_get("open_with_app")?,
        craft_tools: list_of(&craft_tools)?,
        profile_version: row.try_get("profile_version")?,
    })
}

fn json_of(list: &[String]) -> Result<String, CraftError> {
    serde_json::to_string(list).map_err(|_| CraftError::DatastoreUnavailable)
}

/// Internal Logic 3.3: replaces all rows in one transaction. An extension stored twice breaks the
/// primary key of `file_type_extensions` (`FTM_EXTENSION_DUPLICATE`) and the old rows stay.
pub(crate) async fn replace_entries(
    pool: &SqlitePool,
    entries: &[FileTypeMapEntry],
) -> Result<(), CraftError> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM file_type_extensions")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM file_type_map_entries")
        .execute(&mut *tx)
        .await?;
    for entry in entries {
        sqlx::query(
            "INSERT INTO file_type_map_entries (id, extensions, media_types, open_with_app, \
             craft_tools, profile_version) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(entry.id.to_string())
        .bind(json_of(&entry.extensions)?)
        .bind(json_of(&entry.media_types)?)
        .bind(&entry.open_with_app)
        .bind(json_of(&entry.craft_tools)?)
        .bind(&entry.profile_version)
        .execute(&mut *tx)
        .await?;
        for extension in &entry.extensions {
            let inserted = sqlx::query(
                "INSERT INTO file_type_extensions (extension, entry_id) VALUES (?1, ?2)",
            )
            .bind(extension)
            .bind(entry.id.to_string())
            .execute(&mut *tx)
            .await;
            match inserted {
                Ok(_) => {}
                Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                    return Err(CraftError::Invalid("FTM_EXTENSION_DUPLICATE"));
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    tx.commit().await?;
    Ok(())
}
