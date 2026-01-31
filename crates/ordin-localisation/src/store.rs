//! The SQLite statements of Localisation (Localisation.md *Data Model*). Every statement is a
//! parameterised query; nothing is built by string concatenation except the placeholder list of an
//! `IN (…)` clause, which holds only `?` characters.

use std::collections::BTreeMap;

use sqlx::{SqliteConnection, SqlitePool};

use crate::error::LocalisationError;
use crate::types::{LanguageKind, LanguageState};

/// The stored preference row, as read.
#[derive(Debug, Clone)]
pub(crate) struct PrefRow {
    pub(crate) locale: String,
    pub(crate) source: String,
    pub(crate) updated_at: String,
}

/// One `person_languages` row.
#[derive(Debug, Clone)]
pub(crate) struct LangRow {
    pub(crate) locale: String,
    pub(crate) kind: LanguageKind,
    pub(crate) state: LanguageState,
    pub(crate) name: Option<String>,
    pub(crate) made_for_version: String,
    pub(crate) checked_against: String,
    pub(crate) draft_total: Option<u32>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) confirmed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    Edited,
    Drafted,
    Imported,
}

impl Origin {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Origin::Edited => "edited",
            Origin::Drafted => "drafted",
            Origin::Imported => "imported",
        }
    }

    fn parse(raw: &str) -> Option<Origin> {
        match raw {
            "edited" => Some(Origin::Edited),
            "drafted" => Some(Origin::Drafted),
            "imported" => Some(Origin::Imported),
            _ => None,
        }
    }
}

/// One `person_messages` row (its locale is the key of the map that holds it).
#[derive(Debug, Clone)]
pub(crate) struct MsgRow {
    pub(crate) ftl: String,
    pub(crate) origin: Origin,
    pub(crate) reviewed: bool,
    pub(crate) usable: bool,
    pub(crate) updated_at: String,
}

pub(crate) fn kind_str(kind: LanguageKind) -> &'static str {
    match kind {
        LanguageKind::Shipped => "shipped",
        LanguageKind::Made => "made",
        LanguageKind::Imported => "imported",
    }
}

pub(crate) fn state_str(state: LanguageState) -> &'static str {
    match state {
        LanguageState::Drafting => "drafting",
        LanguageState::NeedsReview => "needs_review",
        LanguageState::InUse => "in_use",
    }
}

type LangTuple = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    Option<i64>,
    String,
    String,
    Option<String>,
);

fn lang_from(tuple: LangTuple) -> Result<LangRow, LocalisationError> {
    let kind = match tuple.1.as_str() {
        "shipped" => LanguageKind::Shipped,
        "made" => LanguageKind::Made,
        "imported" => LanguageKind::Imported,
        _ => return Err(LocalisationError::StoreCorrupt),
    };
    let state = match tuple.2.as_str() {
        "drafting" => LanguageState::Drafting,
        "needs_review" => LanguageState::NeedsReview,
        "in_use" => LanguageState::InUse,
        _ => return Err(LocalisationError::StoreCorrupt),
    };
    Ok(LangRow {
        locale: tuple.0,
        kind,
        state,
        name: tuple.3,
        made_for_version: tuple.4,
        checked_against: tuple.5,
        draft_total: tuple.6.and_then(|n| u32::try_from(n).ok()),
        created_at: tuple.7,
        updated_at: tuple.8,
        confirmed_at: tuple.9,
    })
}

type MsgTuple = (String, String, String, String, i64, i64, String);

fn msg_from(tuple: MsgTuple) -> Result<(String, String, MsgRow), LocalisationError> {
    let origin = Origin::parse(&tuple.3).ok_or(LocalisationError::StoreCorrupt)?;
    Ok((
        tuple.0,
        tuple.1,
        MsgRow {
            ftl: tuple.2,
            origin,
            reviewed: tuple.4 != 0,
            usable: tuple.5 != 0,
            updated_at: tuple.6,
        },
    ))
}

pub(crate) async fn load_pref(pool: &SqlitePool) -> Result<Option<PrefRow>, sqlx::Error> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT locale, source, updated_at FROM locale_preferences WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(locale, source, updated_at)| PrefRow {
        locale,
        source,
        updated_at,
    }))
}

pub(crate) async fn save_pref(
    pool: &SqlitePool,
    locale: &str,
    source: &str,
    now: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO locale_preferences (id, locale, source, updated_at) VALUES (1, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET locale = excluded.locale, source = excluded.source, \
         updated_at = excluded.updated_at",
    )
    .bind(locale)
    .bind(source)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every language row and every message of the person layer.
pub(crate) type Layer = BTreeMap<String, (LangRow, BTreeMap<String, MsgRow>)>;

pub(crate) async fn load_layer(pool: &SqlitePool) -> Result<Layer, LocalisationError> {
    let langs: Vec<LangTuple> = sqlx::query_as(
        "SELECT locale, kind, state, name, made_for_version, checked_against, draft_total, created_at, updated_at, confirmed_at FROM person_languages ORDER BY locale",
    )
    .fetch_all(pool)
    .await?;
    let mut layer = Layer::new();
    for tuple in langs {
        let row = lang_from(tuple)?;
        layer.insert(row.locale.clone(), (row, BTreeMap::new()));
    }
    let messages: Vec<MsgTuple> = sqlx::query_as(
        "SELECT locale, message_id, ftl, origin, reviewed, usable, updated_at FROM person_messages ORDER BY locale, message_id",
    )
    .fetch_all(pool)
    .await?;
    for tuple in messages {
        let (locale, id, row) = msg_from(tuple)?;
        if let Some((_, map)) = layer.get_mut(&locale) {
            map.insert(id, row);
        }
    }
    Ok(layer)
}

/// One language and its messages, or `None` when it has no row.
pub(crate) async fn load_language(
    pool: &SqlitePool,
    locale: &str,
) -> Result<Option<(LangRow, BTreeMap<String, MsgRow>)>, LocalisationError> {
    let tuple: Option<LangTuple> = sqlx::query_as(
        "SELECT locale, kind, state, name, made_for_version, checked_against, draft_total, created_at, updated_at, confirmed_at FROM person_languages WHERE locale = ?",
    )
    .bind(locale)
    .fetch_optional(pool)
    .await?;
    let Some(tuple) = tuple else {
        return Ok(None);
    };
    let row = lang_from(tuple)?;
    let messages: Vec<MsgTuple> = sqlx::query_as(
        "SELECT locale, message_id, ftl, origin, reviewed, usable, updated_at FROM person_messages WHERE locale = ? ORDER BY message_id",
    )
    .bind(locale)
    .fetch_all(pool)
    .await?;
    let mut map = BTreeMap::new();
    for tuple in messages {
        let (_, id, msg) = msg_from(tuple)?;
        map.insert(id, msg);
    }
    Ok(Some((row, map)))
}

pub(crate) async fn insert_language(
    conn: &mut SqliteConnection,
    row: &LangRow,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO person_languages (locale, kind, state, name, made_for_version, checked_against, draft_total, created_at, updated_at, confirmed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&row.locale)
    .bind(kind_str(row.kind))
    .bind(state_str(row.state))
    .bind(&row.name)
    .bind(&row.made_for_version)
    .bind(&row.checked_against)
    .bind(row.draft_total.map(i64::from))
    .bind(&row.created_at)
    .bind(&row.updated_at)
    .bind(&row.confirmed_at)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn set_state(
    conn: &mut SqliteConnection,
    locale: &str,
    state: LanguageState,
    draft_total: Option<u32>,
    confirmed_at: Option<&str>,
    now: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE person_languages SET state = ?, draft_total = ?, \
         confirmed_at = COALESCE(?, confirmed_at), updated_at = ? WHERE locale = ?",
    )
    .bind(state_str(state))
    .bind(draft_total.map(i64::from))
    .bind(confirmed_at)
    .bind(now)
    .bind(locale)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn set_draft_total(
    conn: &mut SqliteConnection,
    locale: &str,
    draft_total: Option<u32>,
    now: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE person_languages SET draft_total = ?, updated_at = ? WHERE locale = ?")
        .bind(draft_total.map(i64::from))
        .bind(now)
        .bind(locale)
        .execute(conn)
        .await?;
    Ok(())
}

pub(crate) async fn replace_language_meta(
    conn: &mut SqliteConnection,
    row: &LangRow,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE person_languages SET state = ?, made_for_version = ?, checked_against = ?, \
         draft_total = ?, updated_at = ?, confirmed_at = ? WHERE locale = ?",
    )
    .bind(state_str(row.state))
    .bind(&row.made_for_version)
    .bind(&row.checked_against)
    .bind(row.draft_total.map(i64::from))
    .bind(&row.updated_at)
    .bind(&row.confirmed_at)
    .bind(&row.locale)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn delete_language(
    conn: &mut SqliteConnection,
    locale: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM person_languages WHERE locale = ?")
        .bind(locale)
        .execute(conn)
        .await?;
    Ok(())
}

pub(crate) async fn delete_messages(
    conn: &mut SqliteConnection,
    locale: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM person_messages WHERE locale = ?")
        .bind(locale)
        .execute(conn)
        .await?;
    Ok(())
}

pub(crate) async fn delete_message(
    conn: &mut SqliteConnection,
    locale: &str,
    message_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM person_messages WHERE locale = ? AND message_id = ?")
        .bind(locale)
        .bind(message_id)
        .execute(conn)
        .await?;
    Ok(())
}

pub(crate) async fn upsert_message(
    conn: &mut SqliteConnection,
    locale: &str,
    message_id: &str,
    row: &MsgRow,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO person_messages (locale, message_id, ftl, origin, reviewed, usable, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(locale, message_id) DO UPDATE SET ftl = excluded.ftl, \
         origin = excluded.origin, reviewed = excluded.reviewed, usable = excluded.usable, \
         updated_at = excluded.updated_at",
    )
    .bind(locale)
    .bind(message_id)
    .bind(&row.ftl)
    .bind(row.origin.as_str())
    .bind(i64::from(row.reviewed))
    .bind(i64::from(row.usable))
    .bind(&row.updated_at)
    .execute(conn)
    .await?;
    Ok(())
}

pub(crate) async fn mark_reviewed(
    conn: &mut SqliteConnection,
    locale: &str,
    message_id: &str,
    now: &str,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE person_messages SET reviewed = 1, updated_at = ? \
         WHERE locale = ? AND message_id = ? AND reviewed = 0",
    )
    .bind(now)
    .bind(locale)
    .bind(message_id)
    .execute(conn)
    .await?;
    Ok(done.rows_affected())
}

pub(crate) async fn recheck_message(
    conn: &mut SqliteConnection,
    locale: &str,
    message_id: &str,
    usable: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE person_messages SET usable = ? WHERE locale = ? AND message_id = ?")
        .bind(i64::from(usable))
        .bind(locale)
        .bind(message_id)
        .execute(conn)
        .await?;
    Ok(())
}

pub(crate) async fn mark_checked(
    conn: &mut SqliteConnection,
    locale: &str,
    app_version: &str,
    now: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE person_languages SET checked_against = ?, updated_at = ? WHERE locale = ?")
        .bind(app_version)
        .bind(now)
        .bind(locale)
        .execute(conn)
        .await?;
    Ok(())
}

/// A language in state `drafting` found at start (ordin stopped during a draft) becomes `needs_review`.
pub(crate) async fn settle_interrupted_drafts(
    pool: &SqlitePool,
    now: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE person_languages SET state = 'needs_review', draft_total = NULL, updated_at = ? \
         WHERE state = 'drafting'",
    )
    .bind(now)
    .execute(pool)
    .await?;
    sqlx::query(
        "UPDATE person_languages SET draft_total = NULL, updated_at = ? \
         WHERE kind = 'shipped' AND draft_total IS NOT NULL",
    )
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}
