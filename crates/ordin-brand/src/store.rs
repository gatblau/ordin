//! The `brand_packs` table (Brand.md *Data Model*): one row per received pack, as 2C `BrandPack`.

use std::collections::BTreeMap;

use ordin_types::domain::identity::{BrandPack, BrandPackSource, BrandPackState, BrandStrings};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::BrandPackId;
use sqlx::{Row as _, SqlitePool};

use crate::error::BrandError;

/// The columns in 2C field order, as one literal so every statement stays a `&'static str`.
macro_rules! columns {
    () => {
        "id, brand_identifier, version, source, strings, logo_light_path, logo_dark_path, \
         icon_set_light_path, icon_set_dark_path, design_tokens, font_file_paths, help_url, \
         support_url, privacy_url, state, refusal_reason, received_at, activated_at"
    };
}

/// `SELECT <columns> FROM brand_packs <tail>`.
macro_rules! select {
    ($tail:literal) => {
        concat!("SELECT ", columns!(), " FROM brand_packs ", $tail)
    };
}

pub(crate) fn source_text(source: BrandPackSource) -> &'static str {
    match source {
        BrandPackSource::Build => "build",
        BrandPackSource::OrganisationProfile => "organisation_profile",
    }
}

fn state_text(state: BrandPackState) -> &'static str {
    match state {
        BrandPackState::Received => "received",
        BrandPackState::ContrastChecked => "contrast_checked",
        BrandPackState::Active => "active",
        BrandPackState::Refused => "refused",
    }
}

fn parse_source(raw: &str) -> Result<BrandPackSource, BrandError> {
    match raw {
        "build" => Ok(BrandPackSource::Build),
        "organisation_profile" => Ok(BrandPackSource::OrganisationProfile),
        _ => Err(BrandError::corrupt("brand_packs.source")),
    }
}

fn parse_state(raw: &str) -> Result<BrandPackState, BrandError> {
    match raw {
        "received" => Ok(BrandPackState::Received),
        "contrast_checked" => Ok(BrandPackState::ContrastChecked),
        "active" => Ok(BrandPackState::Active),
        "refused" => Ok(BrandPackState::Refused),
        _ => Err(BrandError::corrupt("brand_packs.state")),
    }
}

fn parse_time(raw: &str, what: &'static str) -> Result<Timestamp, BrandError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| BrandError::corrupt(what))
}

fn json<T: serde::Serialize>(value: &T) -> String {
    // The three JSON columns hold plain strings, vectors and maps, which always serialise.
    serde_json::to_string(value).unwrap_or_default()
}

fn from_row(row: &sqlx::sqlite::SqliteRow) -> Result<BrandPack, BrandError> {
    let text = |name: &str| -> Result<String, BrandError> { Ok(row.try_get::<String, _>(name)?) };
    let id = BrandPackId::parse(&text("id")?).map_err(|_| BrandError::corrupt("brand_packs.id"))?;
    let strings: Vec<BrandStrings> = serde_json::from_str(&text("strings")?)
        .map_err(|_| BrandError::corrupt("brand_packs.strings"))?;
    let design_tokens: BTreeMap<String, String> = serde_json::from_str(&text("design_tokens")?)
        .map_err(|_| BrandError::corrupt("brand_packs.design_tokens"))?;
    let font_file_paths: Vec<String> = serde_json::from_str(&text("font_file_paths")?)
        .map_err(|_| BrandError::corrupt("brand_packs.font_file_paths"))?;
    let activated_at: Option<String> = row.try_get("activated_at")?;
    Ok(BrandPack {
        id,
        brand_identifier: text("brand_identifier")?,
        version: text("version")?,
        source: parse_source(&text("source")?)?,
        strings,
        logo_light_path: text("logo_light_path")?,
        logo_dark_path: text("logo_dark_path")?,
        icon_set_light_path: text("icon_set_light_path")?,
        icon_set_dark_path: text("icon_set_dark_path")?,
        design_tokens,
        font_file_paths,
        help_url: text("help_url")?,
        support_url: text("support_url")?,
        privacy_url: text("privacy_url")?,
        state: parse_state(&text("state")?)?,
        refusal_reason: row.try_get("refusal_reason")?,
        received_at: parse_time(&text("received_at")?, "brand_packs.received_at")?,
        activated_at: activated_at
            .map(|raw| parse_time(&raw, "brand_packs.activated_at"))
            .transpose()?,
    })
}

async fn fetch_all(pool: &SqlitePool, sql: &'static str) -> Result<Vec<BrandPack>, BrandError> {
    sqlx::query(sql)
        .fetch_all(pool)
        .await?
        .iter()
        .map(from_row)
        .collect()
}

/// Inserts a whole row.
pub(crate) async fn insert(pool: &SqlitePool, pack: &BrandPack) -> Result<(), BrandError> {
    let sql = concat!(
        "INSERT INTO brand_packs (",
        columns!(),
        ") VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    sqlx::query(sql)
        .bind(pack.id.to_string())
        .bind(&pack.brand_identifier)
        .bind(&pack.version)
        .bind(source_text(pack.source))
        .bind(json(&pack.strings))
        .bind(&pack.logo_light_path)
        .bind(&pack.logo_dark_path)
        .bind(&pack.icon_set_light_path)
        .bind(&pack.icon_set_dark_path)
        .bind(json(&pack.design_tokens))
        .bind(json(&pack.font_file_paths))
        .bind(&pack.help_url)
        .bind(&pack.support_url)
        .bind(&pack.privacy_url)
        .bind(state_text(pack.state))
        .bind(&pack.refusal_reason)
        .bind(pack.received_at.to_wire())
        .bind(pack.activated_at.map(|t| t.to_wire()))
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn by_id(pool: &SqlitePool, id: BrandPackId) -> Result<BrandPack, BrandError> {
    let sql = select!("WHERE id = ?");
    let row = sqlx::query(sql)
        .bind(id.to_string())
        .fetch_one(pool)
        .await?;
    from_row(&row)
}

/// The row of one `(brand_identifier, version, source)`, if any.
pub(crate) async fn by_key(
    pool: &SqlitePool,
    brand_identifier: &str,
    version: &str,
    source: BrandPackSource,
) -> Result<Option<BrandPack>, BrandError> {
    let sql = select!("WHERE brand_identifier = ? AND version = ? AND source = ?");
    let row = sqlx::query(sql)
        .bind(brand_identifier)
        .bind(version)
        .bind(source_text(source))
        .fetch_optional(pool)
        .await?;
    row.as_ref().map(from_row).transpose()
}

pub(crate) async fn active(pool: &SqlitePool) -> Result<Option<BrandPack>, BrandError> {
    let sql = select!("WHERE state = 'active'");
    let row = sqlx::query(sql).fetch_optional(pool).await?;
    row.as_ref().map(from_row).transpose()
}

/// Rows in `received`, newest first.
pub(crate) async fn received(pool: &SqlitePool) -> Result<Vec<BrandPack>, BrandError> {
    fetch_all(
        pool,
        select!(
            "WHERE state = 'received' \
             ORDER BY received_at DESC, id DESC"
        ),
    )
    .await
}

/// The newest refused row received from a profile.
pub(crate) async fn newest_refusal(pool: &SqlitePool) -> Result<Option<BrandPack>, BrandError> {
    let sql = select!(
        "WHERE state = 'refused' \
         AND source = 'organisation_profile' ORDER BY received_at DESC, id DESC LIMIT 1"
    );
    let row = sqlx::query(sql).fetch_optional(pool).await?;
    row.as_ref().map(from_row).transpose()
}

/// The newest build row (by `received_at`).
pub(crate) async fn newest_build(pool: &SqlitePool) -> Result<Option<BrandPack>, BrandError> {
    let sql = select!(
        "WHERE source = 'build' \
         ORDER BY received_at DESC, id DESC LIMIT 1"
    );
    let row = sqlx::query(sql).fetch_optional(pool).await?;
    row.as_ref().map(from_row).transpose()
}

/// Every row, newest first.
pub(crate) async fn all_newest_first(pool: &SqlitePool) -> Result<Vec<BrandPack>, BrandError> {
    fetch_all(pool, select!("ORDER BY received_at DESC, id DESC")).await
}

/// C7, refusal: `state = 'refused'` with its reason; paths are left as they were.
pub(crate) async fn mark_refused(
    pool: &SqlitePool,
    id: BrandPackId,
    reason: &str,
) -> Result<(), BrandError> {
    sqlx::query("UPDATE brand_packs SET state = 'refused', refusal_reason = ? WHERE id = ?")
        .bind(reason)
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

/// C7, pass: `contrast_checked` with the paths, tokens, strings and links of the checked pack.
pub(crate) async fn mark_checked(pool: &SqlitePool, pack: &BrandPack) -> Result<(), BrandError> {
    sqlx::query(
        "UPDATE brand_packs SET state = 'contrast_checked', strings = ?, logo_light_path = ?, \
         logo_dark_path = ?, icon_set_light_path = ?, icon_set_dark_path = ?, design_tokens = ?, \
         font_file_paths = ?, help_url = ?, support_url = ?, privacy_url = ? WHERE id = ?",
    )
    .bind(json(&pack.strings))
    .bind(&pack.logo_light_path)
    .bind(&pack.logo_dark_path)
    .bind(&pack.icon_set_light_path)
    .bind(&pack.icon_set_dark_path)
    .bind(json(&pack.design_tokens))
    .bind(json(&pack.font_file_paths))
    .bind(&pack.help_url)
    .bind(&pack.support_url)
    .bind(&pack.privacy_url)
    .bind(pack.id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// E: in one transaction, the current `active` row becomes `contrast_checked` and `id` becomes
/// `active` with `activated_at = now`.
pub(crate) async fn activate(
    pool: &SqlitePool,
    id: BrandPackId,
    now: Timestamp,
) -> Result<(), BrandError> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE brand_packs SET state = 'contrast_checked' WHERE state = 'active'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE brand_packs SET state = 'active', activated_at = ? WHERE id = ?")
        .bind(now.to_wire())
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn delete(pool: &SqlitePool, id: BrandPackId) -> Result<(), BrandError> {
    sqlx::query("DELETE FROM brand_packs WHERE id = ?")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}
