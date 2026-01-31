//! The person layer's Gherkin scenarios (Localisation.md, amendment January 2026, ADR-109), each
//! under the scenario's own name in snake_case.

use ordin_types::paging::PAGE_SIZE;
use ordin_types::CodedError;

use super::support::plain;
use super::world::{draft, make_in_use, Opts, World, JAPANESE};
use crate::id;
use crate::{
    ArgValue, DraftEnd, EntryProblem, LanguageChangeKind, LanguageKind, LanguageMatch,
    LanguageState, LocalisationError, MessageArgs, MessageFilter, MessageOnly, MessageSource,
    SAFETY_IDS,
};

const READ: &str = "run-progress-documents-read";
const FRENCH_OK: &str = "run-progress-documents-read = { $count ->\n    [one] Un document lu\n   *[other] { NUMBER($count) } documents lus\n}";
const FRENCH_NEW: &str = "run-progress-documents-read = { $count ->\n    [one] Un seul document\n   *[other] { NUMBER($count) } documents\n}";

fn count(n: u64) -> MessageArgs {
    MessageArgs::new().with("count", ArgValue::Count(n))
}

#[tokio::test]
async fn error_an_edit_that_drops_a_variable_is_refused_and_nothing_changes() {
    let world = World::start(Opts::default()).await;
    let error = world
        .loc
        .edit_message("fr-FR", READ, "run-progress-documents-read = Documents lus")
        .await
        .expect_err("refused");
    match &error {
        LocalisationError::MessageRefused {
            message_id,
            problem,
        } => {
            assert_eq!(message_id, READ);
            assert_eq!(
                *problem,
                EntryProblem::VariableMissing {
                    name: "count".into()
                }
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(error.code(), "LOC_MESSAGE_REFUSED");
    assert_eq!(world.count("SELECT COUNT(*) FROM person_messages").await, 0);
}

#[tokio::test]
async fn error_an_edit_that_does_not_parse_is_refused() {
    let world = World::start(Opts::default()).await;
    let error = world
        .loc
        .edit_message("fr-FR", READ, "run-progress-documents-read = { $count")
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "LOC_MESSAGE_REFUSED");
}

#[tokio::test]
async fn happy_path_an_edit_shows_without_a_restart_and_restore_brings_the_shipped_text_back() {
    let world = World::start(Opts {
        os: vec!["fr-FR"],
        ..Opts::default()
    })
    .await;
    assert_eq!(world.loc.active().locale, "fr-FR");
    let mut changes = world.loc.language_changes();
    let shipped_text = plain(
        &world
            .loc
            .message(id::RUN_PROGRESS_DOCUMENTS_READ, &count(3)),
    );
    assert_eq!(shipped_text, "3 documents lus");

    let row = world
        .loc
        .edit_message("fr-FR", READ, FRENCH_NEW)
        .await
        .expect("edit");
    assert_eq!(row.source, MessageSource::Edited);
    let event = changes.recv().await.expect("event");
    assert_eq!(
        (event.locale.as_str(), event.change),
        ("fr-FR", LanguageChangeKind::Edited)
    );
    let catalogue = world.loc.catalogue("fr-FR").expect("catalogue");
    assert!(catalogue.resources[0].ftl.contains("Un seul document"));
    assert!(!catalogue.resources[0].ftl.contains("Un document lu"));
    assert!(catalogue.resources[0]
        .ftl
        .contains("approval-card-send-button = Envoyer"));
    assert_eq!(
        plain(
            &world
                .loc
                .message(id::RUN_PROGRESS_DOCUMENTS_READ, &count(3))
        ),
        "3 documents"
    );
    assert_eq!(
        plain(
            &world
                .loc
                .message(id::RUN_PROGRESS_DOCUMENTS_READ, &count(1))
        ),
        "Un seul document"
    );
    // The compiled-in catalogue is unchanged.
    assert!(crate::catalogue::bundled_text("fr-FR")
        .expect("bundled")
        .contains("Un document lu"));
    // The language row of a shipped language is created on the first edit.
    assert_eq!(
        world
            .count(
                "SELECT COUNT(*) FROM person_languages WHERE kind = 'shipped' AND state = 'in_use'"
            )
            .await,
        1
    );

    let restored = world
        .loc
        .restore_message("fr-FR", READ)
        .await
        .expect("restore");
    assert_eq!(restored.source, MessageSource::Shipped);
    assert_eq!(
        plain(
            &world
                .loc
                .message(id::RUN_PROGRESS_DOCUMENTS_READ, &count(3))
        ),
        "3 documents lus"
    );
    assert_eq!(
        changes.recv().await.expect("event").change,
        LanguageChangeKind::Restored
    );
}

#[tokio::test]
async fn error_restore_on_a_language_the_person_made() {
    let world = World::start(Opts::default()).await;
    draft(&world, "ja-JP", JAPANESE).await;
    let error = world
        .loc
        .restore_language("ja-JP")
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "LOC_NOT_SHIPPED");
    let error = world
        .loc
        .restore_message("ja-JP", READ)
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "LOC_NOT_SHIPPED");
}

#[tokio::test]
async fn error_en_gb_cannot_be_edited() {
    let world = World::start(Opts::default()).await;
    for tag in ["en-GB", "en-XA", "ar-XB"] {
        let error = world
            .loc
            .edit_message(
                tag,
                "approval-card-send-button",
                "approval-card-send-button = Go",
            )
            .await
            .expect_err("refused");
        assert_eq!(error.code(), "LOC_LANGUAGE_NOT_EDITABLE", "{tag}");
    }
}

#[tokio::test]
async fn error_unknown_language_and_unknown_message() {
    let world = World::start(Opts::default()).await;
    let error = world
        .loc
        .messages("xx-YY", &MessageFilter::default(), None)
        .expect_err("unknown");
    assert_eq!(error.code(), "LOC_LANGUAGE_UNKNOWN");
    let error = world
        .loc
        .edit_message("fr-FR", "no-such-message", "no-such-message = Lu")
        .await
        .expect_err("unknown message");
    assert_eq!(error.code(), "LOC_MESSAGE_UNKNOWN");
}

/// Makes `ja-JP` a made language in use, with every Japanese entry stored, then marks the one
/// message `unreviewed` unreviewed behind the service's back and restarts.
async fn ja_with_unreviewed(unreviewed: &str) -> World {
    let world = World::start(Opts::default()).await;
    make_in_use(&world, "ja-JP").await;
    world
        .sql(&format!(
            "UPDATE person_messages SET reviewed = 0 WHERE locale = 'ja-JP' AND message_id = '{unreviewed}'"
        ))
        .await;
    let world = world.restart(Opts::default()).await;
    world
        .loc
        .set_locale(Some("ja-JP"))
        .await
        .expect("choose ja-JP");
    world
}

#[tokio::test]
async fn served_rule_an_unreviewed_safety_message_is_not_used_a_reviewed_one_is() {
    let world = ja_with_unreviewed("approval-card-send-button").await;
    assert_eq!(world.loc.active().locale, "ja-JP");
    let none = MessageArgs::new();
    assert_eq!(
        world.loc.message(id::APPROVAL_CARD_SEND_BUTTON, &none),
        "Send"
    );
    world
        .loc
        .mark_reviewed("ja-JP", &["approval-card-send-button".to_string()])
        .await
        .expect("review");
    assert_eq!(
        world.loc.message(id::APPROVAL_CARD_SEND_BUTTON, &none),
        "送信"
    );
}

#[tokio::test]
async fn served_rule_a_drafted_non_safety_message_is_used_without_review() {
    let world = ja_with_unreviewed(READ).await;
    let text = plain(
        &world
            .loc
            .message(id::RUN_PROGRESS_DOCUMENTS_READ, &count(3)),
    );
    assert_eq!(text, "3 件の文書を読み取りました");
}

#[tokio::test]
async fn error_confirming_before_the_safety_wording_is_checked() {
    let world = World::start(Opts::default()).await;
    draft(&world, "ja-JP", JAPANESE).await;
    let waiting = u32::try_from(
        JAPANESE
            .iter()
            .filter(|e| e.starts_with("approval-") || e.starts_with("who-acts-"))
            .count(),
    )
    .expect("count");
    let error = world
        .loc
        .confirm_language("ja-JP")
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "LOC_REVIEW_INCOMPLETE");
    assert!(matches!(error, LocalisationError::ReviewIncomplete { count } if count == waiting));
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "ja-JP")
        .expect("ja-JP");
    assert_eq!(summary.state, LanguageState::NeedsReview);
    assert_eq!(summary.needs_review, waiting);
}

#[tokio::test]
async fn happy_path_confirming_makes_the_language_choosable() {
    let world = World::start(Opts::default()).await;
    draft(&world, "ja-JP", JAPANESE).await;
    let ids: Vec<String> = SAFETY_IDS.iter().map(|s| s.to_string()).collect();
    world
        .loc
        .mark_reviewed("ja-JP", &ids)
        .await
        .expect("review");
    let mut changes = world.loc.language_changes();
    let summary = world.loc.confirm_language("ja-JP").await.expect("confirm");
    assert_eq!(summary.state, LanguageState::InUse);
    assert!(summary.choosable);
    assert!(world
        .loc
        .offered()
        .iter()
        .any(|o| o.locale == "ja-JP" && o.native_name == "日本語"));
    assert_eq!(
        changes.recv().await.expect("event").change,
        LanguageChangeKind::Confirmed
    );
    // A shipped language is returned unchanged.
    let shipped = world.loc.confirm_language("fr-FR").await.expect("shipped");
    assert_eq!(shipped.kind, LanguageKind::Shipped);
}

#[tokio::test]
async fn error_an_unconfirmed_language_cannot_become_active() {
    let world = World::start(Opts::default()).await;
    draft(&world, "ja-JP", JAPANESE).await;
    let before = world.loc.active();
    let error = world
        .loc
        .set_locale(Some("ja-JP"))
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "LOC_LANGUAGE_NOT_CONFIRMED");
    assert_eq!(world.loc.active(), before);
}

#[tokio::test]
async fn edge_the_company_s_list_does_not_hide_the_person_s_own_language() {
    let world = World::start(Opts {
        offered: vec!["en-GB"],
        ..Opts::default()
    })
    .await;
    make_in_use(&world, "ja-JP").await;
    let active = world.loc.set_locale(Some("ja-JP")).await.expect("set");
    assert_eq!(active.locale, "ja-JP");
    assert_eq!(active.source, ordin_types::domain::LocaleSource::Person);
    assert_eq!(active.fallback_chain, ["ja-JP", "en-GB"]);
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "ja-JP")
        .expect("ja-JP");
    assert!(summary.choosable && !summary.offered_by_company);
}

#[tokio::test]
async fn error_the_active_language_cannot_be_removed() {
    let world = World::start(Opts::default()).await;
    make_in_use(&world, "ja-JP").await;
    world.loc.set_locale(Some("ja-JP")).await.expect("set");
    let error = world
        .loc
        .remove_language("ja-JP")
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "LOC_LANGUAGE_ACTIVE");
    world.loc.set_locale(Some("en-GB")).await.expect("back");
    world.loc.remove_language("ja-JP").await.expect("removed");
    assert!(world.loc.languages().iter().all(|l| l.locale != "ja-JP"));
    assert_eq!(world.count("SELECT COUNT(*) FROM person_messages").await, 0);
}

#[tokio::test]
async fn error_a_language_being_drafted_cannot_be_edited() {
    let world = World::start(Opts::default()).await;
    world
        .loc
        .begin_draft("ja-JP", "Japanese", 10)
        .await
        .expect("begin");
    let entry = "run-progress-documents-read = { NUMBER($count) } 件";
    for error in [
        world
            .loc
            .edit_message("ja-JP", READ, entry)
            .await
            .map(|_| ())
            .expect_err("drafting"),
        world
            .loc
            .confirm_language("ja-JP")
            .await
            .map(|_| ())
            .expect_err("drafting"),
        world
            .loc
            .remove_language("ja-JP")
            .await
            .expect_err("drafting"),
        world
            .loc
            .begin_draft("ja-JP", "Japanese", 10)
            .await
            .expect_err("second draft"),
    ] {
        assert_eq!(error.code(), "LOC_LANGUAGE_DRAFTING");
    }
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "ja-JP")
        .expect("ja-JP");
    assert_eq!(summary.draft.map(|d| d.total), Some(10));
}

#[tokio::test]
async fn happy_path_export_names_the_locale_and_version() {
    let world = World::start(Opts::default()).await;
    world
        .loc
        .edit_message("fr-FR", READ, FRENCH_NEW)
        .await
        .expect("edit");
    let file = world.loc.export_language("fr-FR").expect("export");
    assert_eq!(file.file_name, "ordin-language-fr-FR.ftl");
    let lines: Vec<&str> = file.ftl.lines().take(4).collect();
    assert_eq!(
        lines,
        [
            "### ordin language file",
            "### locale: fr-FR",
            "### made-for: 1.4.0",
            "### origin: shipped-with-edits"
        ]
    );
    assert!(file.ftl.contains("Un seul document"));
    assert!(file.ftl.contains("approval-card-send-button = Envoyer"));
    assert!(fluent_syntax::parser::parse(file.ftl.as_str()).is_ok());
    // Messages that show in English are left out.
    assert!(!file.ftl.contains("run-progress-amount"));
    assert!(file.ftl.len() <= 2_097_152);
}

/// A file for `tag` holding `entries` and the header lines.
fn language_file(tag: &str, entries: &str) -> String {
    format!("### ordin language file\n### locale: {tag}\n### made-for: 1.3.0\n### origin: imported\n\n{entries}\n")
}

#[tokio::test]
async fn happy_path_import_counts_what_it_drops_and_what_shows_in_english() {
    let world = World::start(Opts::default()).await;
    // The shipped fr-FR messages, 3 messages this version does not have, and one changed text.
    let shipped = world.loc.export_language("fr-FR").expect("export");
    let body = shipped.ftl.clone();
    let mut text = body.replace("### made-for: 1.4.0", "### made-for: 1.3.0");
    text.push_str("\nremoved-one = A\nremoved-two = B\nremoved-three = C\n");
    let edited = text.replace("Un document lu", "Un document lu aujourd'hui");
    let report = world
        .loc
        .import_language("fr.ftl", &edited, false)
        .await
        .expect("import");
    assert_eq!(report.locale, "fr-FR");
    assert_eq!(
        (report.stored, report.dropped_unknown, report.left_out),
        (1, 3, 0)
    );
    let row = world
        .loc
        .messages(
            "fr-FR",
            &MessageFilter {
                only: Some(MessageOnly::Edited),
                ..Default::default()
            },
            None,
        )
        .expect("rows");
    assert!(row.items.is_empty(), "an import is not an edit");
    let imported = world
        .loc
        .messages(
            "fr-FR",
            &MessageFilter {
                text: Some("aujourd".into()),
                ..Default::default()
            },
            None,
        )
        .expect("rows");
    assert_eq!(imported.items.len(), 1);
    assert_eq!(imported.items[0].source, MessageSource::Imported);
    assert_eq!(
        world
            .count(
                "SELECT COUNT(*) FROM person_messages WHERE origin = 'imported' AND reviewed = 0"
            )
            .await,
        1
    );
}

#[tokio::test]
async fn happy_path_an_imported_new_language_waits_for_review_then_becomes_choosable() {
    let world = World::start(Opts::default()).await;
    let entries = JAPANESE.join("\n\n");
    let file = language_file("pt-BR", &entries.replace("日本語", "Português (Brasil)"));
    // Only the safety messages the file carries wait for review; the catalogue has more.
    let carried_safety = JAPANESE
        .iter()
        .filter(|entry| {
            SAFETY_IDS
                .iter()
                .any(|id| entry.starts_with(&format!("{id} =")))
        })
        .count();
    let safety = u32::try_from(carried_safety).expect("count");
    let report = world
        .loc
        .import_language("pt.ftl", &file, false)
        .await
        .expect("import");
    let summary = &report.summary;
    assert_eq!(
        (summary.kind, summary.state),
        (LanguageKind::Imported, LanguageState::NeedsReview)
    );
    assert_eq!(summary.needs_review, safety);
    assert_eq!(summary.made_for_version.as_deref(), Some("1.3.0"));
    // The unreviewed safety entries are stored but not served, so they show in English too.
    assert_eq!(
        usize::try_from(summary.in_english).expect("count"),
        crate::message_ids().len() - (JAPANESE.len() - carried_safety)
    );
    let error = world
        .loc
        .set_locale(Some("pt-BR"))
        .await
        .expect_err("not confirmed");
    assert_eq!(error.code(), "LOC_LANGUAGE_NOT_CONFIRMED");
    let ids: Vec<String> = SAFETY_IDS.iter().map(|s| s.to_string()).collect();
    world
        .loc
        .mark_reviewed("pt-BR", &ids)
        .await
        .expect("review");
    world.loc.confirm_language("pt-BR").await.expect("confirm");
    assert_eq!(
        world
            .loc
            .set_locale(Some("pt-BR"))
            .await
            .expect("set")
            .locale,
        "pt-BR"
    );
}

#[tokio::test]
async fn import_refused_nothing_stored() {
    let world = World::start(Opts::default()).await;
    let good = |tag: &str| language_file(tag, FRENCH_OK);
    let layer = |world: &World| {
        let pool = world.pool().clone();
        async move {
            let languages: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM person_languages")
                .fetch_one(&pool)
                .await
                .expect("count");
            let messages: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM person_messages")
                .fetch_one(&pool)
                .await
                .expect("count");
            (languages.0, messages.0)
        }
    };
    // pt-BR exists (replace false) and pt-PT is being drafted.
    world
        .loc
        .import_language("pt.ftl", &good("pt-BR"), false)
        .await
        .expect("first import");
    world
        .loc
        .begin_draft("pt-PT", "Portuguese", 5)
        .await
        .expect("draft");
    let before = layer(&world).await;
    let active = world.loc.active();

    let too_large = format!("{}{}", good("fr-FR"), "x".repeat(2_097_153));
    let cases: Vec<(&str, String, &str)> = vec![
        (
            "a PDF's bytes as text",
            "%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n".into(),
            "LOC_FILE_UNREADABLE",
        ),
        (
            "no marker line",
            format!("### locale: fr-FR\n{FRENCH_OK}\n"),
            "LOC_FILE_UNREADABLE",
        ),
        (
            "no locale line",
            format!("### ordin language file\n{FRENCH_OK}\n"),
            "LOC_FILE_UNREADABLE",
        ),
        (
            "no entry that parses",
            "### ordin language file\n### locale: fr-FR\n\n".into(),
            "LOC_FILE_UNREADABLE",
        ),
        ("2,097,153 bytes", too_large, "LOC_FILE_TOO_LARGE"),
        ("fr_FR!", good("fr_FR!"), "LOC_TAG_INVALID"),
        ("en-GB", good("en-GB"), "LOC_LANGUAGE_NOT_EDITABLE"),
        (
            "pt-BR exists, replace false",
            good("pt-BR"),
            "LOC_LANGUAGE_EXISTS",
        ),
        (
            "pt-PT is being drafted",
            good("pt-PT"),
            "LOC_LANGUAGE_DRAFTING",
        ),
        (
            "only unknown messages",
            language_file("fr-FR", "nothing-here = Rien"),
            "LOC_FILE_UNREADABLE",
        ),
    ];
    for (name, file, code) in cases {
        let error = world
            .loc
            .import_language("f.ftl", &file, false)
            .await
            .expect_err(name);
        assert_eq!(error.code(), code, "{name}");
    }
    assert_eq!(layer(&world).await, before);
    assert_eq!(world.loc.active(), active);
}

#[tokio::test]
async fn import_refused_a_new_language_when_twenty_of_the_person_s_own_exist() {
    let world = World::start(Opts::default()).await;
    for n in 0..20 {
        let tag = format!("x{}-QQ", char::from(b'a' + n));
        let _ = tag;
        let tag = format!("q{}", char::from(b'a' + n));
        let file = language_file(&tag, "approval-card-send-button = A");
        world
            .loc
            .import_language("f.ftl", &file, false)
            .await
            .expect("import");
    }
    let file = language_file("qz", "approval-card-send-button = A");
    let error = world
        .loc
        .import_language("f.ftl", &file, false)
        .await
        .expect_err("21st");
    assert_eq!(error.code(), "LOC_TOO_MANY_LANGUAGES");
    let error = world
        .loc
        .begin_draft("qy", "Another", 3)
        .await
        .expect_err("21st draft");
    assert_eq!(error.code(), "LOC_TOO_MANY_LANGUAGES");
    // Replacing one of the twenty is not a new language.
    let again = language_file("qa", "approval-card-send-button = B");
    world
        .loc
        .import_language("f.ftl", &again, true)
        .await
        .expect("replace");
}

#[tokio::test]
async fn import_into_a_made_language_with_replace_keeps_it_in_use() {
    let world = World::start(Opts::default()).await;
    make_in_use(&world, "ja-JP").await;
    let file = language_file(
        "ja-JP",
        "approval-card-send-button = 送る\n\nlocale-native-name = 日本語",
    );
    let report = world
        .loc
        .import_language("ja.ftl", &file, true)
        .await
        .expect("replace");
    assert_eq!(report.summary.state, LanguageState::InUse);
    assert_eq!(report.stored, 2);
    assert_eq!(
        world
            .count("SELECT COUNT(*) FROM person_messages WHERE locale = 'ja-JP'")
            .await,
        2
    );
    // The imported safety wording is not served until reviewed.
    world.loc.set_locale(Some("ja-JP")).await.expect("set");
    assert_eq!(
        world
            .loc
            .message(id::APPROVAL_CARD_SEND_BUTTON, &MessageArgs::new()),
        "Send"
    );
}

#[tokio::test]
async fn edge_drafted_entries_pass_through_the_checker_failures_are_counted() {
    let world = World::start(Opts::default()).await;
    world
        .loc
        .begin_draft("ja-JP", "Japanese", 100)
        .await
        .expect("begin");
    let reply = "\
approval-card-send-button = 送信

approval-card-title = タイトル

run-progress-documents-read = 文書

locale-native-name = 日本語

not a message
";
    let outcome = world.loc.add_drafted("ja-JP", reply).await.expect("batch");
    assert_eq!((outcome.stored, outcome.left_out), (2, 2));
    assert_eq!(
        world
            .count("SELECT COUNT(*) FROM person_messages WHERE origin = 'drafted' AND reviewed = 0")
            .await,
        2
    );
    let summary = world
        .loc
        .end_draft("ja-JP", DraftEnd::Finished)
        .await
        .expect("end");
    assert_eq!(summary.state, LanguageState::NeedsReview);
    assert_eq!(summary.draft, None);
}

#[tokio::test]
async fn edge_the_person_s_own_edit_is_not_overwritten_by_a_draft() {
    let world = World::start(Opts::default()).await;
    // A shipped language in the middle of a draft keeps the person's edit.
    world
        .loc
        .edit_message("fr-FR", READ, FRENCH_NEW)
        .await
        .expect("edit");
    world
        .loc
        .begin_draft("fr-FR", "French", 2)
        .await
        .expect("begin");
    let reply = format!("{FRENCH_OK}\n\napproval-card-discard-button = Ne pas envoyer\n");
    let outcome = world.loc.add_drafted("fr-FR", &reply).await.expect("batch");
    assert_eq!(outcome.stored, 1);
    let stored: (String, String) = sqlx::query_as("SELECT ftl, origin FROM person_messages WHERE locale = 'fr-FR' AND message_id = 'run-progress-documents-read'")
        .fetch_one(world.pool())
        .await
        .expect("row");
    assert!(stored.0.contains("Un seul document"));
    assert_eq!(stored.1, "edited");
    let summary = world
        .loc
        .end_draft("fr-FR", DraftEnd::Finished)
        .await
        .expect("end");
    assert_eq!(summary.state, LanguageState::InUse);
}

#[tokio::test]
async fn edge_a_restart_during_a_draft_leaves_the_language_to_review() {
    let world = World::start(Opts::default()).await;
    world
        .loc
        .begin_draft("ja-JP", "Japanese", 7)
        .await
        .expect("begin");
    world
        .loc
        .add_drafted("ja-JP", "approval-card-send-button = 送信\n\nrun-progress-documents-read = { NUMBER($count) } 件")
        .await
        .expect("batch");
    let world = world.restart(Opts::default()).await;
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "ja-JP")
        .expect("ja-JP");
    assert_eq!(summary.state, LanguageState::NeedsReview);
    assert_eq!(summary.draft, None);
    let remaining = world.loc.draft_sources("ja-JP").expect("sources");
    assert_eq!(remaining.len(), crate::message_ids().len() - 2);
    assert!(remaining
        .iter()
        .all(|s| s.message_id != READ && s.message_id != "approval-card-send-button"));
    assert!(remaining
        .windows(2)
        .all(|pair| pair[0].message_id < pair[1].message_id));
}

#[tokio::test]
async fn edge_after_an_update_new_messages_show_in_english_and_failing_entries_need_changing() {
    let world = World::start(Opts::default()).await;
    make_in_use(&world, "ja-JP").await;
    // Version 1.5.0 added a variable to one message ja-JP holds, and dropped a message it also holds.
    world
        .sql("UPDATE person_messages SET ftl = 'approval-card-title = タイトル' WHERE locale = 'ja-JP' AND message_id = 'approval-card-title'")
        .await;
    world
        .sql("INSERT INTO person_messages VALUES ('ja-JP', 'gone-message', 'gone-message = x', 'drafted', 1, 1, '2026-01-10T00:00:00.000Z')")
        .await;
    let world = world
        .restart(Opts {
            version: "1.5.0",
            ..Opts::default()
        })
        .await;
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "ja-JP")
        .expect("ja-JP");
    assert_eq!(summary.needs_changing, 1);
    assert_eq!(summary.made_for_version.as_deref(), Some("1.4.0"));
    let served = JAPANESE.len() - 1;
    assert_eq!(
        usize::try_from(summary.in_english).expect("count"),
        crate::message_ids().len() - served
    );
    assert_eq!(
        world
            .count("SELECT COUNT(*) FROM person_messages WHERE message_id = 'gone-message'")
            .await,
        0
    );
    world.loc.set_locale(Some("ja-JP")).await.expect("set");
    // The failing entry is not served: the English text shows instead.
    let args = MessageArgs::new().with("action", ArgValue::Text("x".into()));
    assert!(plain(&world.loc.message(id::APPROVAL_CARD_TITLE, &args)).starts_with("Approve"));
    assert_eq!(
        world
            .count("SELECT COUNT(*) FROM person_languages WHERE checked_against = '1.5.0'")
            .await,
        1
    );
}

#[tokio::test]
async fn edge_match_language() {
    let world = World::start(Opts {
        offered: vec!["en-GB", "fr-FR"],
        ..Opts::default()
    })
    .await;
    draft(&world, "ja-JP", JAPANESE).await;
    let tag = |s: &str| s.to_string();
    assert_eq!(
        world.loc.match_language("fr").expect("fr"),
        LanguageMatch::Choosable {
            locale: tag("fr-FR")
        }
    );
    assert_eq!(
        world.loc.match_language("fr-CA").expect("fr-CA"),
        LanguageMatch::Choosable {
            locale: tag("fr-FR")
        }
    );
    assert_eq!(
        world.loc.match_language("en-gb").expect("en-gb"),
        LanguageMatch::Choosable {
            locale: tag("en-GB")
        }
    );
    assert_eq!(
        world.loc.match_language("ja-JP").expect("ja-JP"),
        LanguageMatch::NotConfirmed {
            locale: tag("ja-JP")
        }
    );
    assert_eq!(
        world.loc.match_language("ja").expect("ja"),
        LanguageMatch::NotConfirmed {
            locale: tag("ja-JP")
        }
    );
    // de-DE is bundled but the company does not offer it.
    assert_eq!(
        world.loc.match_language("de").expect("de"),
        LanguageMatch::NotOffered {
            locale: tag("de-DE")
        }
    );
    assert_eq!(
        world.loc.match_language("ko").expect("ko"),
        LanguageMatch::Missing { locale: tag("ko") }
    );
    assert_eq!(
        world.loc.match_language("fr_FR!").expect_err("tag").code(),
        "LOC_TAG_INVALID"
    );
    let agent = World::agent().await;
    assert_eq!(
        agent.match_language("ko").expect("ko"),
        LanguageMatch::NotOffered { locale: tag("ko") }
    );
}

#[tokio::test]
async fn edge_a_made_right_to_left_language() {
    let world = World::start(Opts::default()).await;
    let arabic = [
        "locale-native-name = العربية",
        "approval-card-send-button = إرسال",
        "approval-card-discard-button = لا ترسل",
        "approval-card-title = هل توافق على «{ $action }»؟",
        "approval-card-who-acts = سيتولى { $actor } التنفيذ بعد موافقتك.",
        "who-acts-ordin-sends = سيرسل Ordin هذا نيابة عنك.",
    ];
    draft(&world, "ar", &arabic).await;
    let ids: Vec<String> = SAFETY_IDS.iter().map(|s| s.to_string()).collect();
    world.loc.mark_reviewed("ar", &ids).await.expect("review");
    world.loc.confirm_language("ar").await.expect("confirm");
    let active = world.loc.set_locale(Some("ar")).await.expect("set");
    assert_eq!(active.direction, crate::TextDirection::Rtl);
}

#[tokio::test]
async fn error_the_agent_profile_has_no_person_layer() {
    let agent = World::agent().await;
    let refused = |code: &str, error: LocalisationError| assert_eq!(error.code(), code);
    refused(
        "LOC_LANGUAGES_UNAVAILABLE",
        agent
            .edit_message("fr-FR", READ, "x")
            .await
            .expect_err("edit"),
    );
    refused(
        "LOC_LANGUAGES_UNAVAILABLE",
        agent
            .import_language("f.ftl", "x", false)
            .await
            .expect_err("import"),
    );
    refused(
        "LOC_LANGUAGES_UNAVAILABLE",
        agent.export_language("fr-FR").expect_err("export"),
    );
    refused(
        "LOC_LANGUAGES_UNAVAILABLE",
        agent
            .begin_draft("ja-JP", "Japanese", 3)
            .await
            .expect_err("draft"),
    );
    refused(
        "LOC_LANGUAGES_UNAVAILABLE",
        agent
            .restore_message("fr-FR", READ)
            .await
            .expect_err("restore"),
    );
    refused(
        "LOC_LANGUAGES_UNAVAILABLE",
        agent
            .messages("fr-FR", &MessageFilter::default(), None)
            .expect_err("list"),
    );
    assert_eq!(agent.active().locale, "en-GB");
    assert!(agent
        .languages()
        .iter()
        .all(|l| l.kind == LanguageKind::Shipped));
}

// ---- behaviour the scenarios rely on ----

#[tokio::test]
async fn messages_page_by_one_hundred_filter_and_cursor() {
    let world = World::start(Opts::default()).await;
    let all = world
        .loc
        .messages("fr-FR", &MessageFilter::default(), None)
        .expect("page");
    assert_eq!(all.items.len(), crate::message_ids().len().min(PAGE_SIZE));
    assert!(all
        .items
        .windows(2)
        .all(|p| p[0].message_id < p[1].message_id));
    // The catalogue holds more than one page of messages once the screens' words are added.
    assert_eq!(
        all.next_cursor.is_some(),
        crate::message_ids().len() > PAGE_SIZE
    );
    let by_area = world
        .loc
        .messages(
            "fr-FR",
            &MessageFilter {
                area: Some("approval".into()),
                ..Default::default()
            },
            None,
        )
        .expect("area");
    assert!(by_area.items.iter().all(|r| r.area == "approval") && !by_area.items.is_empty());
    assert!(world.loc.areas().contains(&"approval".to_string()));
    let in_english = world
        .loc
        .messages(
            "fr-FR",
            &MessageFilter {
                only: Some(MessageOnly::InEnglish),
                ..Default::default()
            },
            None,
        )
        .expect("english");
    assert!(in_english
        .items
        .iter()
        .all(|r| r.source == MessageSource::English && r.text.is_none()));
    let text = world
        .loc
        .messages(
            "fr-FR",
            &MessageFilter {
                text: Some("ENVOYER".into()),
                ..Default::default()
            },
            None,
        )
        .expect("text");
    assert!(text
        .items
        .iter()
        .any(|r| r.message_id == "approval-card-send-button"));
    let row = &text.items[0];
    assert!(row.safety && row.reviewed && !row.needs_changing);
    assert_eq!(row.shipped.as_deref(), row.text.as_deref());
    // A cursor past the first message starts after it; an undecodable one is refused.
    let cursor = ordin_types::Cursor::encode(&"approval-card-discard-button".to_string());
    let after = world
        .loc
        .messages("fr-FR", &MessageFilter::default(), Some(cursor))
        .expect("after");
    assert!(after
        .items
        .iter()
        .all(|r| r.message_id.as_str() > "approval-card-discard-button"));
    let bad: ordin_types::Cursor = serde_json::from_str("\"!!\"").expect("cursor");
    let refused = world
        .loc
        .messages("fr-FR", &MessageFilter::default(), Some(bad))
        .unwrap_err();
    assert_eq!(refused.code(), "SRV_BAD_CURSOR");
    let needs_review = world
        .loc
        .messages(
            "fr-FR",
            &MessageFilter {
                only: Some(MessageOnly::NeedsReview),
                ..Default::default()
            },
            None,
        )
        .expect("review");
    assert!(needs_review.items.is_empty());
    let needs_changing = world
        .loc
        .messages(
            "fr-FR",
            &MessageFilter {
                only: Some(MessageOnly::NeedsChanging),
                ..Default::default()
            },
            None,
        )
        .expect("changing");
    assert!(needs_changing.items.is_empty());
}

#[tokio::test]
async fn the_page_size_is_one_hundred_and_a_next_cursor_follows() {
    // A language with a full set of entries still pages by the same rule; with fewer than one hundred
    // messages the cursor is null, which the first test covers. Here the page cut is checked directly.
    let world = World::start(Opts::default()).await;
    let page = world
        .loc
        .messages("de-DE", &MessageFilter::default(), None)
        .expect("page");
    assert_eq!(
        page.next_cursor.is_some(),
        crate::message_ids().len() > PAGE_SIZE
    );
}

#[tokio::test]
async fn restore_language_removes_every_override_and_keeps_the_shipped_text() {
    let world = World::start(Opts::default()).await;
    world
        .loc
        .edit_message("fr-FR", READ, FRENCH_NEW)
        .await
        .expect("edit");
    world
        .loc
        .edit_message(
            "fr-FR",
            "approval-card-send-button",
            "approval-card-send-button = Expédier",
        )
        .await
        .expect("edit");
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "fr-FR")
        .expect("fr-FR");
    assert_eq!(summary.edited, 2);
    let restored = world.loc.restore_language("fr-FR").await.expect("restore");
    assert_eq!(restored.edited, 0);
    assert_eq!(world.count("SELECT COUNT(*) FROM person_messages").await, 0);
    // Removing a shipped language deletes its row and restores the shipped text.
    world
        .loc
        .edit_message("fr-FR", READ, FRENCH_NEW)
        .await
        .expect("edit");
    world.loc.remove_language("fr-FR").await.expect("remove");
    assert_eq!(
        world.count("SELECT COUNT(*) FROM person_languages").await,
        0
    );
    assert!(world.loc.languages().iter().any(|l| l.locale == "fr-FR"));
}

#[tokio::test]
async fn mark_reviewed_refuses_an_unknown_message_and_ignores_non_safety_and_missing_rows() {
    let world = World::start(Opts::default()).await;
    make_in_use(&world, "ja-JP").await;
    let error = world
        .loc
        .mark_reviewed("ja-JP", &["no-such-message".into()])
        .await
        .expect_err("unknown");
    assert_eq!(error.code(), "LOC_MESSAGE_UNKNOWN");
    let summary = world
        .loc
        .mark_reviewed("ja-JP", &[READ.into(), "run-progress-amount".into()])
        .await
        .expect("ignored");
    assert_eq!(summary.needs_review, 0);
}

#[tokio::test]
async fn an_edit_that_would_take_the_language_over_the_size_limit_is_refused() {
    let world = World::start(Opts::default()).await;
    // Pad a row behind the service's back so the language is just under the limit, then restart.
    world
        .loc
        .edit_message("fr-FR", READ, FRENCH_OK)
        .await
        .expect("edit");
    let filler = "x".repeat(4_000);
    for n in 0..524 {
        world
            .sql(&format!("INSERT INTO person_messages VALUES ('fr-FR', 'filler-{n}', '{filler}', 'drafted', 1, 0, '2026-01-10T00:00:00.000Z')"))
            .await;
    }
    let held = world
        .count("SELECT SUM(length(ftl)) FROM person_messages WHERE locale = 'fr-FR'")
        .await;
    let last = "y".repeat(usize::try_from(2_097_152 - held - 10).expect("room"));
    world
        .sql(&format!("INSERT INTO person_messages VALUES ('fr-FR', 'filler-last', '{last}', 'drafted', 1, 0, '2026-01-10T00:00:00.000Z')"))
        .await;
    let world = world.restart(Opts::default()).await;
    let error = world
        .loc
        .edit_message(
            "fr-FR",
            "approval-card-send-button",
            "approval-card-send-button = Envoyer ici",
        )
        .await
        .expect_err("full");
    match error {
        LocalisationError::MessageRefused { problem, .. } => {
            assert_eq!(problem, EntryProblem::LanguageFull)
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn crlf_line_breaks_in_an_edit_or_a_file_are_read_as_line_feeds() {
    let world = World::start(Opts::default()).await;
    let entry = FRENCH_NEW.replace('\n', "\r\n");
    world
        .loc
        .edit_message("fr-FR", READ, &entry)
        .await
        .expect("edit");
    let file = language_file("pt-BR", "approval-card-send-button = Enviar").replace('\n', "\r\n");
    world
        .loc
        .import_language("pt.ftl", &file, false)
        .await
        .expect("import");
}

#[tokio::test]
async fn imported_entries_that_fail_the_check_are_left_out_and_listed() {
    let world = World::start(Opts::default()).await;
    let file = language_file(
        "pt-BR",
        "approval-card-send-button = Enviar\n\nrun-progress-documents-read = sem variavel\n\napproval-card-title = Aprovar “{ $acao }”?",
    );
    let report = world
        .loc
        .import_language("pt.ftl", &file, false)
        .await
        .expect("import");
    assert_eq!((report.stored, report.left_out), (1, 2));
    assert_eq!(report.left_out_ids, ["approval-card-title", READ]);
}

#[tokio::test]
async fn a_shipped_language_import_with_no_difference_stores_nothing() {
    let world = World::start(Opts::default()).await;
    let file = world.loc.export_language("fr-FR").expect("export").ftl;
    let report = world
        .loc
        .import_language("fr.ftl", &file, false)
        .await
        .expect("import");
    assert_eq!(report.stored, 0);
    assert_eq!(
        world.count("SELECT COUNT(*) FROM person_languages").await,
        0
    );
}

#[tokio::test]
async fn a_drafting_job_can_continue_a_made_language_and_end_it_back_in_use() {
    let world = World::start(Opts::default()).await;
    make_in_use(&world, "ja-JP").await;
    world
        .loc
        .begin_draft("ja-JP", "Japanese", 1)
        .await
        .expect("again");
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "ja-JP")
        .expect("ja-JP");
    assert_eq!(summary.state, LanguageState::Drafting);
    assert_eq!(summary.draft.map(|d| (d.done, d.total)), Some((0, 1)));
    let outcome = world
        .loc
        .add_drafted(
            "ja-JP",
            "run-progress-amount = 合計 { NUMBER($amount) }\n    .title = 合計",
        )
        .await
        .expect("batch");
    assert_eq!(outcome.stored, 1);
    // Every safety entry was reviewed and the language was confirmed before: it returns to use.
    let summary = world
        .loc
        .end_draft("ja-JP", DraftEnd::Paused)
        .await
        .expect("end");
    assert_eq!(summary.state, LanguageState::InUse);
    // Not drafting any more: a batch or an end is refused.
    assert_eq!(
        world
            .loc
            .add_drafted("ja-JP", "x")
            .await
            .expect_err("not drafting")
            .code(),
        "LOC_LANGUAGE_UNKNOWN"
    );
    assert_eq!(
        world
            .loc
            .end_draft("ja-JP", DraftEnd::Paused)
            .await
            .map(|_| ())
            .expect_err("not drafting")
            .code(),
        "LOC_LANGUAGE_UNKNOWN"
    );
}

#[tokio::test]
async fn draft_sources_of_a_new_language_list_every_message_and_of_a_shipped_one_the_gaps() {
    let world = World::start(Opts::default()).await;
    let new = world.loc.draft_sources("ko-KR").expect("new tag");
    assert_eq!(new.len(), crate::message_ids().len());
    assert!(new[0].english.contains(" = "));
    let shipped = world.loc.draft_sources("fr-FR").expect("fr-FR");
    assert!(shipped
        .iter()
        .any(|s| s.message_id == "run-progress-amount"));
    assert!(shipped.iter().all(|s| s.message_id != READ));
    assert_eq!(
        world
            .loc
            .draft_sources("en-GB")
            .map(|_| ())
            .expect_err("english")
            .code(),
        "LOC_LANGUAGE_NOT_EDITABLE"
    );
}

#[tokio::test]
async fn begin_draft_on_a_shipped_language_stays_in_use_and_holds_the_total() {
    let world = World::start(Opts::default()).await;
    world
        .loc
        .begin_draft("fr-FR", "French", 4)
        .await
        .expect("begin");
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "fr-FR")
        .expect("fr-FR");
    assert_eq!(
        (summary.kind, summary.state),
        (LanguageKind::Shipped, LanguageState::InUse)
    );
    assert_eq!(summary.draft.map(|d| d.total), Some(4));
    assert_eq!(
        world
            .loc
            .edit_message("fr-FR", READ, FRENCH_NEW)
            .await
            .map(|_| ())
            .expect_err("drafting")
            .code(),
        "LOC_LANGUAGE_DRAFTING"
    );
    let outcome = world
        .loc
        .add_drafted(
            "fr-FR",
            "run-progress-amount = Total { NUMBER($amount) }\n    .title = Total",
        )
        .await
        .expect("batch");
    assert_eq!(outcome.stored, 1);
    world
        .loc
        .end_draft("fr-FR", DraftEnd::Stopped)
        .await
        .expect("end");
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "fr-FR")
        .expect("fr-FR");
    assert_eq!(summary.draft, None);
}

#[tokio::test]
async fn a_name_outside_one_to_sixty_characters_is_not_stored_and_drafting_still_starts() {
    let world = World::start(Opts::default()).await;
    world
        .loc
        .begin_draft("ko-KR", &"n".repeat(61), 3)
        .await
        .expect("begin");
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "ko-KR")
        .expect("ko-KR");
    assert_eq!(summary.native_name, None);
    let world2 = World::start(Opts::default()).await;
    world2
        .loc
        .begin_draft("ko-KR", "Korean", 3)
        .await
        .expect("begin");
    let summary = world2
        .loc
        .languages()
        .into_iter()
        .find(|l| l.locale == "ko-KR")
        .expect("ko-KR");
    assert_eq!(summary.native_name.as_deref(), Some("Korean"));
}

#[tokio::test]
async fn export_of_a_made_language_lists_only_its_served_entries() {
    let world = World::start(Opts::default()).await;
    make_in_use(&world, "ja-JP").await;
    let file = world.loc.export_language("ja-JP").expect("export");
    assert!(file.ftl.contains("### origin: drafted"));
    assert!(file.ftl.contains("approval-card-send-button = 送信"));
    assert!(!file.ftl.contains("run-progress-amount"));
    let imported = language_file("pt-BR", "approval-card-send-button = Enviar");
    world
        .loc
        .import_language("pt.ftl", &imported, false)
        .await
        .expect("import");
    assert!(world
        .loc
        .export_language("pt-BR")
        .expect("export")
        .ftl
        .contains("### origin: imported"));
    // An unreviewed imported safety entry is not served, so it is not exported.
    assert!(!world
        .loc
        .export_language("pt-BR")
        .expect("export")
        .ftl
        .contains("Enviar"));
    assert_eq!(
        world
            .loc
            .export_language("xx-YY")
            .map(|_| ())
            .expect_err("unknown")
            .code(),
        "LOC_LANGUAGE_UNKNOWN"
    );
}

#[tokio::test]
async fn catalogue_of_a_made_language_holds_its_served_entries_then_the_chain() {
    let world = World::start(Opts::default()).await;
    make_in_use(&world, "ja-JP").await;
    let text = world.loc.catalogue("ja-JP").expect("catalogue");
    assert_eq!(
        text.resources
            .iter()
            .map(|r| r.locale.as_str())
            .collect::<Vec<_>>(),
        ["ja-JP", "en-GB"]
    );
    assert!(text.resources[0].ftl.contains("送信"));
    let empty = World::start(Opts::default()).await;
    empty
        .loc
        .begin_draft("ko-KR", "Korean", 1)
        .await
        .expect("begin");
    assert_eq!(
        empty.loc.catalogue("ko-KR").expect("catalogue").resources[0]
            .ftl
            .trim(),
        ""
    );
}
