//! X1: one test per Error Table row; X4: every owned code is in the registry and the prefix is owned;
//! the en-GB sentence of each returned code is the Error Table's message.

use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::{CodedError, RepairAction};

use super::support::plain;
use super::world::{draft, Opts, World, JAPANESE};
use crate::{ArgValue, EntryProblem, LocalisationError, MessageArgs, ALL_CODES};

const READ: &str = "run-progress-documents-read";

fn code_of<T: std::fmt::Debug>(result: Result<T, LocalisationError>) -> &'static str {
    result.expect_err("an error").code()
}

#[test]
fn all_codes_are_registered_and_the_prefix_is_owned() {
    assert_eq!(ALL_CODES.len(), 21);
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    assert_eq!(
        sorted, ALL_CODES,
        "sorted, so the registry's binary search order matches"
    );
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
        assert_eq!(entry.owner_crate, "ordin-localisation", "{code}");
        assert!(owns_prefix("ordin-localisation", "LOC"));
    }
}

#[test]
fn every_error_variant_has_its_code_retryable_flag_and_repair_action() {
    let variants: Vec<(LocalisationError, &str)> = vec![
        (LocalisationError::TagInvalid, "LOC_TAG_INVALID"),
        (LocalisationError::NotOffered, "LOC_NOT_OFFERED"),
        (LocalisationError::CatalogueMissing, "LOC_CATALOGUE_MISSING"),
        (
            LocalisationError::StoreUnavailable(sqlx::Error::PoolClosed),
            "LOC_STORE_UNAVAILABLE",
        ),
        (LocalisationError::StoreCorrupt, "LOC_STORE_CORRUPT"),
        (
            LocalisationError::LanguagesUnavailable,
            "LOC_LANGUAGES_UNAVAILABLE",
        ),
        (
            LocalisationError::LanguageNotEditable,
            "LOC_LANGUAGE_NOT_EDITABLE",
        ),
        (LocalisationError::LanguageUnknown, "LOC_LANGUAGE_UNKNOWN"),
        (LocalisationError::MessageUnknown, "LOC_MESSAGE_UNKNOWN"),
        (
            LocalisationError::MessageRefused {
                message_id: String::new(),
                problem: EntryProblem::TooLong,
            },
            "LOC_MESSAGE_REFUSED",
        ),
        (LocalisationError::NotShipped, "LOC_NOT_SHIPPED"),
        (LocalisationError::FileUnreadable, "LOC_FILE_UNREADABLE"),
        (LocalisationError::FileTooLarge, "LOC_FILE_TOO_LARGE"),
        (LocalisationError::LanguageExists, "LOC_LANGUAGE_EXISTS"),
        (LocalisationError::LanguageDrafting, "LOC_LANGUAGE_DRAFTING"),
        (
            LocalisationError::ReviewIncomplete { count: 1 },
            "LOC_REVIEW_INCOMPLETE",
        ),
        (
            LocalisationError::LanguageNotConfirmed,
            "LOC_LANGUAGE_NOT_CONFIRMED",
        ),
        (LocalisationError::LanguageActive, "LOC_LANGUAGE_ACTIVE"),
        (
            LocalisationError::TooManyLanguages,
            "LOC_TOO_MANY_LANGUAGES",
        ),
    ];
    for (error, code) in variants {
        assert_eq!(error.code(), code);
        let store = code == "LOC_STORE_UNAVAILABLE";
        assert_eq!(error.retryable(), store, "{code}");
        assert_eq!(
            error.repair(),
            store.then_some(RepairAction::TryAgain),
            "{code}"
        );
        assert!(
            !error.to_string().contains("sqlx"),
            "Display holds no driver text"
        );
        assert!(ALL_CODES.contains(&code));
    }
}

#[tokio::test]
async fn row_loc_tag_invalid() {
    let world = World::start(Opts::default()).await;
    assert_eq!(
        code_of(world.loc.set_locale(Some("fr_FR!")).await),
        "LOC_TAG_INVALID"
    );
    assert_eq!(code_of(world.loc.catalogue("fr_FR!")), "LOC_TAG_INVALID");
}

#[tokio::test]
async fn row_loc_not_offered() {
    let world = World::start(Opts {
        offered: vec!["en-GB"],
        ..Opts::default()
    })
    .await;
    assert_eq!(
        code_of(world.loc.set_locale(Some("fr-FR")).await),
        "LOC_NOT_OFFERED"
    );
}

#[tokio::test]
async fn row_loc_catalogue_missing() {
    let world = World::start(Opts::default()).await;
    assert_eq!(
        code_of(world.loc.catalogue("ja-JP")),
        "LOC_CATALOGUE_MISSING"
    );
}

#[tokio::test]
async fn row_loc_store_unavailable() {
    let world = World::start(Opts::default()).await;
    world.sql("DROP TABLE locale_preferences").await;
    assert_eq!(
        code_of(world.loc.set_locale(Some("fr-FR")).await),
        "LOC_STORE_UNAVAILABLE"
    );
    // A person-layer write reports the same code and changes nothing in memory.
    world.sql("DROP TABLE person_messages").await;
    let before = world
        .loc
        .messages("fr-FR", &Default::default(), None)
        .expect("rows")
        .items;
    let error = world
        .loc
        .edit_message("fr-FR", READ, "run-progress-documents-read = { $count }")
        .await;
    assert_eq!(code_of(error), "LOC_STORE_UNAVAILABLE");
    assert_eq!(
        world
            .loc
            .messages("fr-FR", &Default::default(), None)
            .expect("rows")
            .items,
        before
    );
}

#[tokio::test]
async fn row_loc_store_corrupt() {
    // A timestamp that does not parse: start continues and replaces the row; the error is only logged.
    let world = World::start(Opts::default()).await;
    world
        .sql("UPDATE locale_preferences SET updated_at = 'yesterday-ish'")
        .await;
    let world = world
        .restart(Opts {
            os: vec!["fr-FR"],
            ..Opts::default()
        })
        .await;
    assert_eq!(world.loc.active().locale, "fr-FR");
    let (locale, source) = world.preference().await.expect("row");
    assert_eq!(
        (locale.as_str(), source.as_str()),
        ("fr-FR", "operating_system")
    );
    let at: (String,) = sqlx::query_as("SELECT updated_at FROM locale_preferences")
        .fetch_one(world.pool())
        .await
        .expect("row");
    assert!(ordin_types::time::parse_rfc3339_ms_utc(&at.0).is_ok());
    assert_eq!(LocalisationError::StoreCorrupt.code(), "LOC_STORE_CORRUPT");
}

#[tokio::test]
async fn row_loc_format_error() {
    // A missing argument: Fluent reports it, the string it produced is returned.
    let world = World::start(Opts::default()).await;
    let text = world
        .loc
        .message(crate::id::APPROVAL_CARD_TITLE, &MessageArgs::new());
    assert!(text.starts_with("Approve"), "{text}");
    assert!(lookup("LOC_FORMAT_ERROR").is_some_and(|e| e.http_status.is_none()));
}

#[test]
fn row_loc_build_catalogue_invalid() {
    // The build script's refusals are tested in `build_script.rs`; the code is registered here.
    assert!(ALL_CODES.contains(&"LOC_BUILD_CATALOGUE_INVALID"));
    assert!(lookup("LOC_BUILD_CATALOGUE_INVALID").is_some());
}

#[tokio::test]
async fn row_loc_languages_unavailable() {
    let agent = World::agent().await;
    assert_eq!(
        code_of(agent.edit_message("fr-FR", READ, "x").await),
        "LOC_LANGUAGES_UNAVAILABLE"
    );
    assert_eq!(
        code_of(agent.remove_language("fr-FR").await),
        "LOC_LANGUAGES_UNAVAILABLE"
    );
    assert_eq!(
        code_of(agent.confirm_language("fr-FR").await),
        "LOC_LANGUAGES_UNAVAILABLE"
    );
    assert_eq!(
        code_of(agent.mark_reviewed("fr-FR", &[]).await),
        "LOC_LANGUAGES_UNAVAILABLE"
    );
    assert_eq!(
        code_of(agent.restore_language("fr-FR").await),
        "LOC_LANGUAGES_UNAVAILABLE"
    );
    assert_eq!(
        code_of(agent.add_drafted("fr-FR", "x").await),
        "LOC_LANGUAGES_UNAVAILABLE"
    );
    assert_eq!(
        code_of(agent.end_draft("fr-FR", crate::DraftEnd::Finished).await),
        "LOC_LANGUAGES_UNAVAILABLE"
    );
    assert_eq!(
        code_of(agent.draft_sources("fr-FR")),
        "LOC_LANGUAGES_UNAVAILABLE"
    );
}

#[tokio::test]
async fn row_loc_language_not_editable() {
    let world = World::start(Opts::default()).await;
    for tag in ["en-GB", "en-XA", "ar-XB"] {
        assert_eq!(
            code_of(world.loc.restore_language(tag).await),
            "LOC_LANGUAGE_NOT_EDITABLE"
        );
        assert_eq!(
            code_of(world.loc.export_language(tag)),
            "LOC_LANGUAGE_NOT_EDITABLE"
        );
        assert_eq!(
            code_of(world.loc.begin_draft(tag, "x", 1).await),
            "LOC_LANGUAGE_NOT_EDITABLE"
        );
        assert_eq!(
            code_of(world.loc.draft_sources(tag)),
            "LOC_LANGUAGE_NOT_EDITABLE"
        );
        let file =
            format!("### ordin language file\n### locale: {tag}\napproval-card-send-button = A\n");
        assert_eq!(
            code_of(world.loc.import_language("f.ftl", &file, false).await),
            "LOC_LANGUAGE_NOT_EDITABLE"
        );
    }
}

#[tokio::test]
async fn row_loc_language_unknown() {
    let world = World::start(Opts::default()).await;
    assert_eq!(
        code_of(world.loc.messages("xx-YY", &Default::default(), None)),
        "LOC_LANGUAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.restore_message("xx-YY", READ).await),
        "LOC_LANGUAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.restore_language("xx-YY").await),
        "LOC_LANGUAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.mark_reviewed("xx-YY", &[]).await),
        "LOC_LANGUAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.confirm_language("xx-YY").await),
        "LOC_LANGUAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.remove_language("xx-YY").await),
        "LOC_LANGUAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.export_language("xx-YY")),
        "LOC_LANGUAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.edit_message("xx-YY", READ, "x").await),
        "LOC_LANGUAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.messages("fr_FR!", &Default::default(), None)),
        "LOC_TAG_INVALID"
    );
}

#[tokio::test]
async fn row_loc_message_unknown() {
    let world = World::start(Opts::default()).await;
    assert_eq!(
        code_of(
            world
                .loc
                .edit_message("fr-FR", "no-such", "no-such = x")
                .await
        ),
        "LOC_MESSAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.restore_message("fr-FR", "no-such").await),
        "LOC_MESSAGE_UNKNOWN"
    );
    assert_eq!(
        code_of(world.loc.mark_reviewed("fr-FR", &["no-such".into()]).await),
        "LOC_MESSAGE_UNKNOWN"
    );
}

#[tokio::test]
async fn row_loc_message_refused() {
    let world = World::start(Opts::default()).await;
    let error = world
        .loc
        .edit_message("fr-FR", READ, "run-progress-documents-read = Documents lus")
        .await;
    assert_eq!(code_of(error), "LOC_MESSAGE_REFUSED");
    // The Error Table's sentence, built from the error's own fields.
    let problem_args = MessageArgs::new().with("name", ArgValue::Text("count".into()));
    let problem = plain(&world.loc.message_in(
        "en-GB",
        crate::id::LANGUAGE_PROBLEM_VARIABLE_MISSING,
        &problem_args,
    ));
    assert_eq!(problem, "it leaves out count, which the English uses.");
    let args = MessageArgs::new()
        .with("message", ArgValue::Text(READ.into()))
        .with("problem", ArgValue::Text(problem));
    assert_eq!(
        plain(&world.loc.error_sentence("LOC_MESSAGE_REFUSED", &args)),
        "“run-progress-documents-read” wasn't saved: it leaves out count, which the English uses."
    );
}

#[tokio::test]
async fn row_loc_not_shipped() {
    let world = World::start(Opts::default()).await;
    draft(&world, "ja-JP", JAPANESE).await;
    assert_eq!(
        code_of(world.loc.restore_language("ja-JP").await),
        "LOC_NOT_SHIPPED"
    );
}

#[tokio::test]
async fn row_loc_file_unreadable() {
    let world = World::start(Opts::default()).await;
    assert_eq!(
        code_of(
            world
                .loc
                .import_language("f.ftl", "not a language file", false)
                .await
        ),
        "LOC_FILE_UNREADABLE"
    );
    assert_eq!(
        code_of(world.loc.import_language("f.ftl", "", false).await),
        "LOC_FILE_UNREADABLE"
    );
}

#[tokio::test]
async fn row_loc_file_too_large() {
    let world = World::start(Opts::default()).await;
    let file = "x".repeat(2_097_153);
    assert_eq!(
        code_of(world.loc.import_language("f.ftl", &file, false).await),
        "LOC_FILE_TOO_LARGE"
    );
}

#[tokio::test]
async fn row_loc_language_exists() {
    let world = World::start(Opts::default()).await;
    let file = "### ordin language file\n### locale: pt-BR\napproval-card-send-button = Enviar\n";
    world
        .loc
        .import_language("f.ftl", file, false)
        .await
        .expect("first");
    assert_eq!(
        code_of(world.loc.import_language("f.ftl", file, false).await),
        "LOC_LANGUAGE_EXISTS"
    );
    world
        .loc
        .import_language("f.ftl", file, true)
        .await
        .expect("replace");
}

#[tokio::test]
async fn row_loc_language_drafting() {
    let world = World::start(Opts::default()).await;
    world
        .loc
        .begin_draft("ja-JP", "Japanese", 3)
        .await
        .expect("begin");
    assert_eq!(
        code_of(world.loc.begin_draft("ja-JP", "Japanese", 3).await),
        "LOC_LANGUAGE_DRAFTING"
    );
    assert_eq!(
        code_of(world.loc.mark_reviewed("ja-JP", &[]).await),
        "LOC_LANGUAGE_DRAFTING"
    );
    assert_eq!(
        code_of(world.loc.restore_message("ja-JP", READ).await),
        "LOC_LANGUAGE_DRAFTING"
    );
    let file = "### ordin language file\n### locale: ja-JP\napproval-card-send-button = 送信\n";
    assert_eq!(
        code_of(world.loc.import_language("f.ftl", file, true).await),
        "LOC_LANGUAGE_DRAFTING"
    );
    // Reading is allowed while drafting.
    world.loc.export_language("ja-JP").expect("export");
    world
        .loc
        .messages("ja-JP", &Default::default(), None)
        .expect("list");
}

#[tokio::test]
async fn row_loc_review_incomplete() {
    let world = World::start(Opts::default()).await;
    draft(&world, "ja-JP", JAPANESE).await;
    let error = world
        .loc
        .confirm_language("ja-JP")
        .await
        .expect_err("incomplete");
    let LocalisationError::ReviewIncomplete { count } = error else {
        panic!("{error:?}")
    };
    let args = MessageArgs::new().with("count", ArgValue::Count(14));
    assert_eq!(
        plain(&world.loc.error_sentence("LOC_REVIEW_INCOMPLETE", &args)),
        "Check the 14 approval messages first."
    );
    assert!(count > 0);
}

#[tokio::test]
async fn row_loc_language_not_confirmed() {
    let world = World::start(Opts::default()).await;
    draft(&world, "ja-JP", JAPANESE).await;
    assert_eq!(
        code_of(world.loc.set_locale(Some("ja-JP")).await),
        "LOC_LANGUAGE_NOT_CONFIRMED"
    );
}

#[tokio::test]
async fn row_loc_language_active() {
    let world = World::start(Opts::default()).await;
    // The active locale cannot be removed, shipped or not.
    assert_eq!(
        code_of(world.loc.remove_language("en-GB").await),
        "LOC_LANGUAGE_NOT_EDITABLE"
    );
    world.loc.set_locale(Some("fr-FR")).await.expect("set");
    assert_eq!(
        code_of(world.loc.remove_language("fr-FR").await),
        "LOC_LANGUAGE_ACTIVE"
    );
}

#[tokio::test]
async fn row_loc_too_many_languages() {
    let world = World::start(Opts::default()).await;
    for n in 0..20 {
        let tag = format!("q{}", char::from(b'a' + n));
        let file =
            format!("### ordin language file\n### locale: {tag}\napproval-card-send-button = A\n");
        world
            .loc
            .import_language("f.ftl", &file, false)
            .await
            .expect("import");
    }
    let file = "### ordin language file\n### locale: qz\napproval-card-send-button = A\n";
    assert_eq!(
        code_of(world.loc.import_language("f.ftl", file, false).await),
        "LOC_TOO_MANY_LANGUAGES"
    );
}

#[tokio::test]
async fn error_sentences_match_the_error_table_and_unknown_codes_fall_back() {
    let world = World::start(Opts::default()).await;
    let none = MessageArgs::new();
    let table = [
        ("LOC_TAG_INVALID", "That language isn't recognised."),
        (
            "LOC_NOT_OFFERED",
            "Your company doesn't offer that language.",
        ),
        (
            "LOC_CATALOGUE_MISSING",
            "That language isn't available in this version.",
        ),
        (
            "LOC_STORE_UNAVAILABLE",
            "Your language choice couldn't be saved.",
        ),
        (
            "LOC_LANGUAGES_UNAVAILABLE",
            "Languages can't be changed here.",
        ),
        (
            "LOC_LANGUAGE_NOT_EDITABLE",
            "English (UK) is ordin's own wording and can't be changed.",
        ),
        (
            "LOC_LANGUAGE_UNKNOWN",
            "That language isn't on this computer.",
        ),
        (
            "LOC_MESSAGE_UNKNOWN",
            "That message doesn't exist in this version.",
        ),
        (
            "LOC_NOT_SHIPPED",
            "Only a language that comes with ordin can be restored.",
        ),
        (
            "LOC_FILE_UNREADABLE",
            "That file isn't an ordin language file.",
        ),
        (
            "LOC_FILE_TOO_LARGE",
            "That file is too large to be a language file.",
        ),
        (
            "LOC_LANGUAGE_EXISTS",
            "You already have that language. Choose Replace to use the new file.",
        ),
        (
            "LOC_LANGUAGE_DRAFTING",
            "Ordin is still drafting that language.",
        ),
        (
            "LOC_LANGUAGE_NOT_CONFIRMED",
            "Review that language and choose Use this language first.",
        ),
        (
            "LOC_LANGUAGE_ACTIVE",
            "Choose another language before removing this one.",
        ),
        (
            "LOC_TOO_MANY_LANGUAGES",
            "You have 20 languages of your own, the most ordin keeps.",
        ),
        // Brand.md's error table (FU-033, plan 01 Phase 12).
        ("BRAND_ASSET_UNKNOWN", "That brand file doesn't exist."),
        (
            "BRAND_STORE_UNAVAILABLE",
            "ordin couldn't load its appearance.",
        ),
    ];
    for (code, sentence) in table {
        assert_eq!(
            plain(&world.loc.error_sentence(code, &none)),
            sentence,
            "{code}"
        );
    }
    let internal = plain(&world.loc.error_sentence("NOT_A_REGISTERED_CODE", &none));
    assert_eq!(
        internal,
        "ordin couldn't finish that. Try again in a moment; if it happens again, choose Save logs for support… in Settings › Diagnostics."
    );
    // Registered, but no sentence in this build's catalogue (a Telegram code; Telegram is behind a
    // build switch, ErrorHandling.md EH-SEN-1): the same fallback.
    assert_eq!(
        plain(&world.loc.error_sentence("TG_APPROVAL_UNAVAILABLE", &none)),
        internal
    );
    let review = MessageArgs::new().with("count", ArgValue::Count(1));
    assert_eq!(
        plain(&world.loc.error_sentence("LOC_REVIEW_INCOMPLETE", &review)),
        "Check the approval message first."
    );
}
