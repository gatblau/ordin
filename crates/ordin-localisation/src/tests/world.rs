//! A started service over a temporary journal database, with the profile channel in the test's hands.

use std::sync::Arc;

use ordin_journal::Datastore;
use ordin_organisation_profile::ProfileSnapshot;
use tokio::sync::watch;

use super::support::{capture, open_plain, open_store, snapshot, Dir};
use crate::catalogue::complete_locales;
use crate::{DraftEnd, Localisation, LocalisationConfig, PreferenceStore, SAFETY_IDS};

pub const VERSION: &str = "1.4.0";

pub struct Opts {
    pub offered: Vec<&'static str>,
    pub fallback: &'static str,
    pub os: Vec<&'static str>,
    pub version: &'static str,
    /// Bundled locales counted as complete besides the ones that are (LOC-CMP-1). The bundled
    /// `fr-FR` and `de-DE` catalogues hold a few lines; most tests use them as a complete second
    /// shipped language, so the world declares them complete unless a test says otherwise.
    pub complete: Vec<&'static str>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            offered: vec!["en-GB", "fr-FR"],
            fallback: "en-GB",
            os: vec![],
            version: VERSION,
            complete: vec!["fr-FR", "de-DE"],
        }
    }
}

pub struct World {
    pub dir: Dir,
    pub loc: Arc<Localisation>,
    pub profile: watch::Sender<Arc<ProfileSnapshot>>,
    /// A second connection for tests that read or change the database directly.
    pub raw: Datastore,
}

impl World {
    pub async fn start(opts: Opts) -> World {
        // The recorder is global and installed before the first log line of any test (plan 01, FU-017).
        capture::install();
        let dir = Dir::new();
        let store = open_store(&dir).await;
        World::over(dir, store, opts).await
    }

    async fn over(dir: Dir, store: Datastore, opts: Opts) -> World {
        let initial = snapshot(&opts.offered, opts.fallback);
        let (profile, rx) = watch::channel(Arc::clone(&initial));
        let complete = complete_locales()
            .iter()
            .chain(opts.complete.iter())
            .map(|tag| (*tag).to_string())
            .collect();
        let loc = Localisation::start_with_catalogues(
            LocalisationConfig {
                store: PreferenceStore::Datastore(store),
                os_locales: opts.os.iter().map(|tag| (*tag).to_string()).collect(),
                app_version: opts.version.to_string(),
            },
            initial,
            rx,
            complete,
        )
        .await
        .expect("start");
        let raw = open_plain(&dir).await;
        World {
            dir,
            loc,
            profile,
            raw,
        }
    }

    /// Starts a second service over the same database (the next run of the program).
    pub async fn restart(self, opts: Opts) -> World {
        let World { dir, raw, .. } = self;
        raw.close().await;
        let store = open_plain(&dir).await;
        World::over(dir, store, opts).await
    }

    /// The agent profile: no store.
    pub async fn agent() -> Arc<Localisation> {
        capture::install();
        let initial = snapshot(&[], "en-GB");
        let (_profile, rx) = watch::channel(Arc::clone(&initial));
        Localisation::start_with_source(
            LocalisationConfig {
                store: PreferenceStore::None,
                os_locales: vec![],
                app_version: VERSION.to_string(),
            },
            initial,
            rx,
        )
        .await
        .expect("start agent")
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.raw.sqlite().expect("SQLite")
    }

    pub async fn sql(&self, statement: &str) {
        sqlx::query(sqlx::AssertSqlSafe(statement.to_string()))
            .execute(self.pool())
            .await
            .expect("statement");
    }

    pub async fn count(&self, statement: &str) -> i64 {
        let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(statement.to_string()))
            .fetch_one(self.pool())
            .await
            .expect("count");
        row.0
    }

    /// The stored preference as (locale, source).
    pub async fn preference(&self) -> Option<(String, String)> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT locale, source FROM locale_preferences WHERE id = 1")
                .fetch_optional(self.pool())
                .await
                .expect("read preference");
        row
    }
}

/// The Japanese entries the tests draft (Japanese has only the plural category `other`).
pub const JAPANESE: &[&str] = &[
    "locale-native-name = 日本語",
    "run-progress-documents-read = { NUMBER($count) } 件の文書を読み取りました",
    "approval-card-send-button = 送信",
    "approval-card-discard-button = 送信しない",
    "approval-card-title = 「{ $action }」を承認しますか？",
    "approval-card-who-acts = 承認後に { $actor } が実行します。",
    "who-acts-ordin-sends = Ordin が代わりに送信します。",
];

/// Drafts `tag` from `entries`, leaving it in state `needs_review`.
pub async fn draft(world: &World, tag: &str, entries: &[&str]) {
    world
        .loc
        .begin_draft(
            tag,
            "Test language",
            u32::try_from(entries.len()).unwrap_or(0),
        )
        .await
        .expect("begin draft");
    let outcome = world
        .loc
        .add_drafted(tag, &entries.join("\n"))
        .await
        .expect("add drafted");
    assert_eq!(outcome.left_out, 0);
    world
        .loc
        .end_draft(tag, DraftEnd::Finished)
        .await
        .expect("end draft");
}

/// Drafts `tag` from the Japanese entries, reviews its safety wording and confirms it.
pub async fn make_in_use(world: &World, tag: &str) {
    draft(world, tag, JAPANESE).await;
    let ids: Vec<String> = SAFETY_IDS.iter().map(|id| (*id).to_string()).collect();
    world.loc.mark_reviewed(tag, &ids).await.expect("review");
    world.loc.confirm_language(tag).await.expect("confirm");
}
