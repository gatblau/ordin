//! Log lines of Localisation (Localisation.md *Observability*). These tests record `tracing` events
//! through one global subscriber, so they run alone in this binary and one at a time (plan 01, FU-017).

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_journal::Datastore;
use ordin_localisation::{
    id, ArgValue, DraftEnd, Localisation, LocalisationConfig, MessageArgs, PreferenceStore,
    SAFETY_IDS,
};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_types::time::Timestamp;
use support::{capture, open_plain, open_store, plain, Dir};

/// One test at a time: they share the recorder.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

async fn defaults_profile() -> Arc<OrganisationProfileService> {
    let service = OrganisationProfileService::start(
        ProfileMode::NotUsed,
        ProfileOptions {
            refresh_interval: Duration::from_secs(3600),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .expect("profile defaults");
    Arc::new(service)
}

async fn start_over(store: Datastore, os: &[&str], version: &str) -> Arc<Localisation> {
    Localisation::start(
        LocalisationConfig {
            store: PreferenceStore::Datastore(store),
            os_locales: os.iter().map(|tag| (*tag).to_string()).collect(),
            app_version: version.to_string(),
        },
        defaults_profile().await,
    )
    .await
    .expect("start")
}

const READ: &str = "run-progress-documents-read";
const JAPANESE: &[&str] = &[
    "locale-native-name = 日本語",
    "run-progress-documents-read = { NUMBER($count) } 件の文書を読み取りました",
    "approval-card-send-button = 送信",
    "approval-card-title = 「{ $action }」を承認しますか？",
    "who-acts-ordin-sends = Ordin が代わりに送信します。",
];

#[tokio::test]
async fn edge_message_missing_in_the_active_locale_falls_back() {
    let _serial = SERIAL.lock().await;
    capture::install();
    let dir = Dir::new();
    let loc = start_over(open_store(&dir).await, &["fr-FR"], "1.4.0").await;
    // The bundled fr-FR catalogue is short, so it is offered only once the person has taken it up
    // (LOC-CMP-1): one edited message, then the person's choice.
    loc.edit_message(
        "fr-FR",
        READ,
        "run-progress-documents-read = { $count } documents lus",
    )
    .await
    .expect("edit");
    loc.set_locale(Some("fr-FR")).await.expect("chosen");
    assert_eq!(loc.active().locale, "fr-FR");
    capture::clear();
    // "approval-card-discard-button" exists in en-GB but not in fr-FR.
    let text = loc.message(id::APPROVAL_CARD_DISCARD_BUTTON, &MessageArgs::new());
    assert_eq!(text, "Don't send");
    let lines = capture::event("localisation.message_fallback");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].level, tracing::Level::DEBUG);
    assert_eq!(lines[0].field("message_id"), "approval-card-discard-button");
    assert_eq!(lines[0].field("locale"), "fr-FR");
    assert_eq!(lines[0].field("served_by"), "en-GB");
}

#[tokio::test]
async fn performance_formatting_with_a_person_language_does_no_i_o() {
    let _serial = SERIAL.lock().await;
    capture::install();
    let dir = Dir::new();
    let loc = start_over(open_store(&dir).await, &[], "1.4.0").await;
    loc.begin_draft("ja-JP", "Japanese", 5)
        .await
        .expect("begin");
    loc.add_drafted("ja-JP", &JAPANESE.join("\n"))
        .await
        .expect("batch");
    loc.end_draft("ja-JP", DraftEnd::Finished)
        .await
        .expect("end");
    let ids: Vec<String> = SAFETY_IDS.iter().map(|s| (*s).to_string()).collect();
    loc.mark_reviewed("ja-JP", &ids).await.expect("review");
    loc.confirm_language("ja-JP").await.expect("confirm");
    loc.set_locale(Some("ja-JP")).await.expect("set");

    capture::clear();
    let args = MessageArgs::new()
        .with("count", ArgValue::Count(3))
        .with("action", ArgValue::Text("送る".into()))
        .with("actor", ArgValue::Text("Ordin".into()));
    let messages = [
        id::RUN_PROGRESS_DOCUMENTS_READ,
        id::APPROVAL_CARD_TITLE,
        id::APPROVAL_CARD_WHO_ACTS,
        id::APPROVAL_CARD_SEND_BUTTON,
    ];
    let mut elapsed = Vec::with_capacity(10_000);
    let mut last = String::new();
    for n in 0..10_000 {
        let started = Instant::now();
        last = loc.message(messages[n % messages.len()], &args);
        elapsed.push(started.elapsed());
    }
    assert!(!last.is_empty());
    elapsed.sort();
    let p95 = elapsed[elapsed.len() * 95 / 100];
    assert!(
        p95 <= Duration::from_micros(500),
        "p95 per message was {p95:?}"
    );

    // No database query ran, and no model was called (the crate has no client for one).
    let queries: Vec<_> = capture::lines()
        .into_iter()
        .filter(|line| line.target.starts_with("sqlx"))
        .collect();
    assert!(queries.is_empty(), "queries during formatting: {queries:?}");

    // No file is opened outside the journal database: the run-time sources use no file API at all.
    // (build.rs reads the catalogues at build time; nothing else in the crate may.)
    for (name, source) in [
        ("catalogue.rs", include_str!("../src/catalogue.rs")),
        ("check.rs", include_str!("../src/check.rs")),
        ("error.rs", include_str!("../src/error.rs")),
        ("fmt.rs", include_str!("../src/fmt.rs")),
        ("lib.rs", include_str!("../src/lib.rs")),
        ("migrations.rs", include_str!("../src/migrations.rs")),
        ("person.rs", include_str!("../src/person.rs")),
        ("service.rs", include_str!("../src/service.rs")),
        ("store.rs", include_str!("../src/store.rs")),
        ("types.rs", include_str!("../src/types.rs")),
    ] {
        for needle in [
            "std::fs",
            "File::open",
            "read_to_string",
            "tokio::fs",
            "std::net",
            "reqwest",
        ] {
            assert!(!source.contains(needle), "{name} uses {needle}");
        }
    }
}

#[tokio::test]
async fn every_event_names_the_component_keeps_its_fields_and_logs_no_message_text() {
    let _serial = SERIAL.lock().await;
    capture::install();
    capture::clear();
    let dir = Dir::new();
    let loc = start_over(open_store(&dir).await, &["fr-FR"], "1.4.0").await;
    loc.set_locale(Some("en-GB")).await.expect("set");

    // A refused edit, an accepted edit, a restore.
    loc.edit_message(
        "fr-FR",
        READ,
        "run-progress-documents-read = SECRET-WORDING",
    )
    .await
    .expect_err("refused");
    loc.edit_message(
        "fr-FR",
        READ,
        "run-progress-documents-read = SECRET-WORDING { $count }",
    )
    .await
    .expect("edit");
    loc.restore_message("fr-FR", READ).await.expect("restore");
    // Make a language: draft with one failing entry, review, confirm, export, import, remove.
    loc.begin_draft("ja-JP", "Japanese", 6)
        .await
        .expect("begin");
    loc.add_drafted(
        "ja-JP",
        &format!(
            "{}\n\napproval-card-title = SECRET-DRAFT",
            JAPANESE.join("\n")
        ),
    )
    .await
    .expect("batch");
    loc.end_draft("ja-JP", DraftEnd::Finished)
        .await
        .expect("end");
    let ids: Vec<String> = SAFETY_IDS.iter().map(|s| (*s).to_string()).collect();
    loc.mark_reviewed("ja-JP", &ids).await.expect("review");
    loc.confirm_language("ja-JP").await.expect("confirm");
    loc.export_language("ja-JP").expect("export");
    let file = "### ordin language file\n### locale: pt-BR\napproval-card-send-button = Enviar\n\nrun-progress-documents-read = SECRET-IMPORT\n\nunknown-here = x\n";
    loc.import_language("pt.ftl", file, false)
        .await
        .expect("import");
    loc.remove_language("pt-BR").await.expect("remove");
    // A formatting error (missing argument).
    loc.message(id::APPROVAL_CARD_TITLE, &MessageArgs::new());

    // A corrupt preference row, then a new version of the program.
    drop(loc);
    let raw = open_plain(&dir).await;
    sqlx::query("UPDATE locale_preferences SET updated_at = 'garbage'")
        .execute(raw.sqlite().expect("SQLite"))
        .await
        .expect("corrupt");
    sqlx::query("UPDATE person_messages SET ftl = 'approval-card-title = SECRET-STALE' WHERE locale = 'ja-JP' AND message_id = 'approval-card-title'")
        .execute(raw.sqlite().expect("SQLite"))
        .await
        .expect("stale");
    let _loc = start_over(open_store(&dir).await, &["fr-FR"], "1.5.0").await;

    let all = capture::ours();
    let expected: &[(&str, &[&str])] = &[
        ("localisation.active", &["locale", "source", "direction"]),
        ("localisation.locale_set", &["locale", "source"]),
        (
            "localisation.entry_refused",
            &["locale", "message_id", "problem", "origin"],
        ),
        (
            "localisation.message_edited",
            &["locale", "message_id", "change"],
        ),
        ("localisation.language_reviewed", &["locale", "count"]),
        ("localisation.language_confirmed", &["locale"]),
        ("localisation.language_removed", &["locale", "kind"]),
        ("localisation.language_exported", &["locale", "messages"]),
        (
            "localisation.language_imported",
            &[
                "locale",
                "stored",
                "dropped_unknown",
                "in_english",
                "left_out",
            ],
        ),
        (
            "localisation.draft_batch",
            &["locale", "stored", "left_out"],
        ),
        (
            "localisation.draft_ended",
            &["locale", "end", "done", "total"],
        ),
        (
            "localisation.format_error",
            &["message_id", "locale", "error_kind"],
        ),
        ("localisation.preference_reset", &["reason"]),
        (
            "localisation.person_layer_rechecked",
            &["locale", "need_changing", "removed"],
        ),
    ];
    for (event, fields) in expected {
        let found: Vec<_> = all.iter().filter(|line| line.event == *event).collect();
        assert!(!found.is_empty(), "{event} was written");
        for line in found {
            for field in *fields {
                assert!(
                    line.fields.contains_key(*field),
                    "{event} lacks {field}: {line:?}"
                );
            }
        }
    }
    let rechecks = capture::event("localisation.person_layer_rechecked");
    let japanese = rechecks
        .iter()
        .find(|l| l.field("locale") == "ja-JP")
        .expect("ja-JP re-checked");
    assert_eq!(japanese.field("need_changing"), "1");
    let refused = capture::event("localisation.entry_refused");
    assert!(refused
        .iter()
        .any(|l| l.field("origin") == "edited" && l.field("problem") == "variable_missing"));
    assert!(refused.iter().any(|l| l.field("origin") == "drafted"));
    assert!(refused.iter().any(|l| l.field("origin") == "imported"));

    for line in &all {
        assert!(!line.fields.contains_key("dropped_fields"), "{line:?}");
        let text = format!("{line:?}");
        for secret in ["SECRET-", "一", "送信", "Enviar"] {
            assert!(!text.contains(secret), "message text in a log line: {text}");
        }
        assert!(
            line.event.starts_with("localisation."),
            "event names carry the module prefix: {line:?}"
        );
    }
    let _ = plain("");
}
