//! The Gherkin scenarios of the original three commands (Localisation.md *Acceptance Criteria*),
//! each under the scenario's own name in snake_case. Scenarios that read log lines are in
//! `tests/logging.rs`.

use ordin_organisation_profile::ProfileSnapshot;
use ordin_types::domain::LocaleSource;
use ordin_types::num::DecimalString;
use ordin_types::time::Timestamp;
use ordin_types::CodedError;
use ordin_types::RepairAction;

use super::support::{plain, snapshot};
use super::world::{Opts, World};
use crate::id;
use crate::{
    bundled_locales, ArgValue, DateTimeStyle, LocalisationError, MessageArgs, TextDirection,
};

#[tokio::test]
async fn happy_path_first_start_follows_the_operating_system() {
    let world = World::start(Opts {
        os: vec!["fr-FR"],
        ..Opts::default()
    })
    .await;
    let active = world.loc.active();
    assert_eq!(active.locale, "fr-FR");
    assert_eq!(active.source, LocaleSource::OperatingSystem);
    assert_eq!(active.direction, TextDirection::Ltr);
    assert_eq!(active.fallback_chain, ["fr-FR", "en-GB"]);
    assert_eq!(
        world.preference().await,
        Some(("fr-FR".to_string(), "operating_system".to_string()))
    );
    assert_eq!(
        world.count("SELECT COUNT(*) FROM locale_preferences").await,
        1
    );
}

#[tokio::test]
async fn edge_operating_system_locale_matched_by_language_subtag() {
    let world = World::start(Opts {
        offered: vec!["en-GB", "de-DE"],
        os: vec!["de-AT", "en-US"],
        ..Opts::default()
    })
    .await;
    assert_eq!(world.loc.active().locale, "de-DE");
}

#[tokio::test]
async fn happy_path_the_person_chooses_a_locale() {
    let world = World::start(Opts::default()).await;
    let mut subscriber = world.loc.subscribe();
    let active = world.loc.set_locale(Some("fr-FR")).await.expect("set");
    assert_eq!(active.locale, "fr-FR");
    assert_eq!(active.source, LocaleSource::Person);
    subscriber.changed().await.expect("subscriber told");
    assert_eq!(*subscriber.borrow(), active);
    assert_eq!(
        world.preference().await,
        Some(("fr-FR".to_string(), "person".to_string()))
    );
}

#[tokio::test]
async fn error_locale_not_offered() {
    let world = World::start(Opts {
        offered: vec!["en-GB"],
        ..Opts::default()
    })
    .await;
    let before = world.loc.active();
    let error = world
        .loc
        .set_locale(Some("fr-FR"))
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "LOC_NOT_OFFERED");
    assert_eq!(world.loc.active(), before);
}

#[tokio::test]
async fn error_invalid_tag() {
    let world = World::start(Opts::default()).await;
    let error = world
        .loc
        .set_locale(Some("fr_FR!"))
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "LOC_TAG_INVALID");
}

#[tokio::test]
async fn edge_a_newer_profile_withdraws_the_person_s_locale() {
    let world = World::start(Opts::default()).await;
    world.loc.set_locale(Some("fr-FR")).await.expect("set");
    let mut subscriber = world.loc.subscribe();
    subscriber.mark_unchanged();
    world
        .profile
        .send(snapshot(&["en-GB"], "en-GB"))
        .expect("profile changes");
    subscriber.changed().await.expect("subscribers told");
    assert_eq!(subscriber.borrow().locale, "en-GB");
    assert_eq!(
        world.preference().await,
        Some(("en-GB".to_string(), "operating_system".to_string()))
    );
}

#[tokio::test]
async fn edge_right_to_left_locale() {
    let world = World::start(Opts {
        offered: vec![],
        ..Opts::default()
    })
    .await;
    if cfg!(feature = "pseudo-locales") {
        let active = world.loc.set_locale(Some("ar-XB")).await.expect("set");
        assert_eq!(active.direction, TextDirection::Rtl);
        let text = world.loc.catalogue("ar-XB").expect("catalogue");
        assert_eq!(text.direction, TextDirection::Rtl);
    } else {
        // Without the feature the pseudo-locales are not bundled, so they cannot be chosen.
        assert!(!bundled_locales().contains(&"ar-XB"));
        let error = world
            .loc
            .set_locale(Some("ar-XB"))
            .await
            .expect_err("not bundled");
        assert_eq!(error.code(), "LOC_NOT_OFFERED");
    }
}

#[tokio::test]
async fn error_catalogue_for_an_unbundled_locale() {
    let world = World::start(Opts::default()).await;
    let error = world.loc.catalogue("ja-JP").expect_err("not bundled");
    assert_eq!(error.code(), "LOC_CATALOGUE_MISSING");
}

#[tokio::test]
async fn error_preference_store_unavailable() {
    let world = World::start(Opts::default()).await;
    world.sql("DROP TABLE locale_preferences").await;
    let before = world.loc.active();
    let error = world
        .loc
        .set_locale(Some("fr-FR"))
        .await
        .expect_err("store refuses");
    assert_eq!(error.code(), "LOC_STORE_UNAVAILABLE");
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
    assert_eq!(world.loc.active(), before);
}

#[tokio::test]
async fn error_build_refuses_an_unknown_message_identifier() {
    // The compile-time half is the `compile_fail` example on `crate::id`; here the identifier list
    // and the build script's own refusal are checked.
    assert!(!crate::message_ids().contains(&"no-such-message"));
    assert!(crate::message_ids().contains(&"approval-card-send-button"));
    super::build_script::orphan_message_is_refused();
}

#[tokio::test]
async fn edge_plural_and_number_rules_come_from_cldr() {
    let world = World::start(Opts::default()).await;
    let one = MessageArgs::new().with("count", ArgValue::Count(1));
    let many = MessageArgs::new().with("count", ArgValue::Count(1200));
    assert_eq!(
        plain(&world.loc.message(id::RUN_PROGRESS_DOCUMENTS_READ, &one)),
        "Read one document"
    );
    assert_eq!(
        plain(&world.loc.message(id::RUN_PROGRESS_DOCUMENTS_READ, &many)),
        "Read 1,200 documents"
    );
    assert_eq!(
        world.loc.format_number(&DecimalString("1234567.5".into())),
        "1,234,567.5"
    );
}

#[tokio::test]
async fn edge_agent_profile() {
    let loc = World::agent().await;
    let active = loc.active();
    assert_eq!(active.locale, "en-GB");
    assert_eq!(active.source, LocaleSource::OperatingSystem);
    // No table is read or written: the person layer refuses, and the only choice is the fallback.
    assert!(matches!(
        loc.edit_message("fr-FR", "approval-card-send-button", "x")
            .await,
        Err(LocalisationError::LanguagesUnavailable)
    ));
    assert_eq!(
        loc.set_locale(Some("en-GB"))
            .await
            .expect("fallback")
            .locale,
        "en-GB"
    );
    assert!(matches!(
        loc.set_locale(Some("fr-FR")).await,
        Err(LocalisationError::NotOffered)
    ));
    assert_eq!(loc.set_locale(None).await.expect("none").locale, "en-GB");
}

#[tokio::test]
async fn error_a_store_failure_carries_its_repair_action() {
    let world = World::start(Opts::default()).await;
    world.sql("DROP TABLE locale_preferences").await;
    let error = world
        .loc
        .set_locale(Some("fr-FR"))
        .await
        .expect_err("store refuses");
    assert_eq!(error.code(), "LOC_STORE_UNAVAILABLE");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
}

#[tokio::test]
async fn happy_path_a_locale_change_reaches_the_screens_with_the_whole_active_locale() {
    let world = World::start(Opts::default()).await;
    let mut subscriber = world.loc.subscribe();
    world.loc.set_locale(Some("fr-FR")).await.expect("set");
    subscriber.changed().await.expect("changed");
    let seen = subscriber.borrow().clone();
    assert_eq!(seen.locale, "fr-FR");
    assert_eq!(seen.source, LocaleSource::Person);
    assert_eq!(seen.direction, TextDirection::Ltr);
    assert_eq!(seen.fallback_chain, ["fr-FR", "en-GB"]);
    // The program forwards it as {"active": <that ActiveLocale>}.
    let data = serde_json::json!({ "active": seen });
    assert_eq!(data["active"]["fallback_chain"][0], "fr-FR");
}

// ---- behaviour the scenarios rely on, not scenarios themselves ----

#[tokio::test]
async fn following_the_operating_system_again_clears_the_person_choice() {
    let world = World::start(Opts {
        os: vec!["fr-FR"],
        ..Opts::default()
    })
    .await;
    world.loc.set_locale(Some("en-GB")).await.expect("set");
    assert_eq!(world.loc.active().source, LocaleSource::Person);
    let back = world.loc.set_locale(None).await.expect("none");
    assert_eq!(back.locale, "fr-FR");
    assert_eq!(back.source, LocaleSource::OperatingSystem);
}

#[tokio::test]
async fn formatting_helpers_follow_the_active_locale() {
    let world = World::start(Opts::default()).await;
    let at = Timestamp::from_unix_millis(1_768_046_400_000).expect("2026-01-10T12:00:00Z");
    assert_eq!(
        world.loc.format_datetime(at, DateTimeStyle::Date),
        "10 Jan 2026"
    );
    assert!(!world
        .loc
        .format_datetime(at, DateTimeStyle::DateTime)
        .is_empty());
    assert!(!world
        .loc
        .format_datetime(at, DateTimeStyle::Time)
        .is_empty());
    assert!(!world
        .loc
        .format_datetime(at, DateTimeStyle::ShortDate)
        .is_empty());
    let items = ["A", "B", "C"].map(String::from);
    assert_eq!(world.loc.format_list(&items), "A, B and C");
    assert_eq!(world.loc.compare("a", "b"), std::cmp::Ordering::Less);
    world.loc.set_locale(Some("fr-FR")).await.expect("set");
    assert_eq!(
        world.loc.format_datetime(at, DateTimeStyle::Date),
        "10 janv. 2026"
    );
    assert_eq!(
        world.loc.format_number(&DecimalString("-12.50".into())),
        "-12,50"
    );
    let french = MessageArgs::new().with("count", ArgValue::Count(12));
    assert_eq!(
        plain(&world.loc.message(id::RUN_PROGRESS_DOCUMENTS_READ, &french)),
        "12 documents lus"
    );
    assert_eq!(
        plain(
            &world
                .loc
                .message_in("en-GB", id::RUN_PROGRESS_DOCUMENTS_READ, &french)
        ),
        "Read 12 documents"
    );
}

#[tokio::test]
async fn date_and_amount_arguments_are_formatted_by_icu() {
    let world = World::start(Opts::default()).await;
    let at = Timestamp::from_unix_millis(1_768_046_400_000).expect("timestamp");
    let date = MessageArgs::new().with("at", ArgValue::DateTime(at));
    assert_eq!(
        plain(&world.loc.message(id::RUN_PROGRESS_FINISHED_AT, &date)),
        "Finished on 10 Jan 2026."
    );
    let amount =
        MessageArgs::new().with("amount", ArgValue::Number(DecimalString("1234.50".into())));
    assert_eq!(
        plain(&world.loc.message(id::RUN_PROGRESS_AMOUNT, &amount)),
        "The total is 1,234.50."
    );
    let text = MessageArgs::new().with("action", ArgValue::Text("send".into()));
    assert!(plain(&world.loc.message(id::APPROVAL_CARD_TITLE, &text)).contains("send"));
}

#[tokio::test]
async fn offered_lists_the_profile_order_with_native_names() {
    let world = World::start(Opts {
        offered: vec!["fr-FR", "en-GB", "xx-ZZ"],
        ..Opts::default()
    })
    .await;
    let offered = world.loc.offered();
    let tags: Vec<&str> = offered.iter().map(|o| o.locale.as_str()).collect();
    let mut expected = vec!["fr-FR", "en-GB"];
    if cfg!(feature = "pseudo-locales") {
        expected.extend(["ar-XB", "en-XA"]);
    }
    assert_eq!(tags, expected);
    assert_eq!(offered[0].native_name, "Français");
    assert_eq!(offered[1].native_name, "English (UK)");
}

#[tokio::test]
async fn an_empty_or_unbundled_offer_falls_back_to_en_gb_and_a_missing_fallback_is_replaced() {
    let world = World::start(Opts {
        offered: vec!["xx-ZZ"],
        fallback: "yy-ZZ",
        ..Opts::default()
    })
    .await;
    assert_eq!(world.loc.active().locale, "en-GB");
    assert_eq!(world.loc.active().fallback_chain, ["en-GB"]);
    world
        .profile
        .send(snapshot(&[], "fr-FR"))
        .expect("profile changes");
    // The follow task runs on the runtime; wait for the publication.
    let mut subscriber = world.loc.subscribe();
    subscriber.mark_unchanged();
    let _ = tokio::time::timeout(std::time::Duration::from_millis(50), subscriber.changed()).await;
    assert_eq!(world.loc.offered().len(), bundled_locales().len());
}

#[tokio::test]
async fn message_in_names_an_explicit_locale_and_skips_unknown_tags() {
    let world = World::start(Opts::default()).await;
    let none = MessageArgs::new();
    assert_eq!(
        world
            .loc
            .message_in("de-DE", id::APPROVAL_CARD_SEND_BUTTON, &none),
        "Senden"
    );
    assert_eq!(
        world
            .loc
            .message_in("ja-JP", id::APPROVAL_CARD_SEND_BUTTON, &none),
        "Send"
    );
    assert_eq!(
        world
            .loc
            .message_in("not a tag", id::APPROVAL_CARD_SEND_BUTTON, &none),
        "Send"
    );
}

#[tokio::test]
async fn a_preference_the_profile_no_longer_offers_is_rewritten_at_start() {
    let world = World::start(Opts::default()).await;
    world.loc.set_locale(Some("fr-FR")).await.expect("set");
    let world = world
        .restart(Opts {
            offered: vec!["en-GB"],
            ..Opts::default()
        })
        .await;
    assert_eq!(world.loc.active().locale, "en-GB");
    assert_eq!(
        world.preference().await,
        Some(("en-GB".to_string(), "operating_system".to_string()))
    );
}

#[tokio::test]
async fn a_stored_person_choice_survives_a_restart() {
    let world = World::start(Opts::default()).await;
    world.loc.set_locale(Some("fr-FR")).await.expect("set");
    let world = world.restart(Opts::default()).await;
    assert_eq!(world.loc.active().locale, "fr-FR");
    assert_eq!(world.loc.active().source, LocaleSource::Person);
}

#[tokio::test]
async fn a_new_snapshot_with_the_same_effect_publishes_nothing() {
    let world = World::start(Opts::default()).await;
    let mut subscriber = world.loc.subscribe();
    subscriber.mark_unchanged();
    let mut again = ProfileSnapshot::defaults();
    again.document.locales.offered = vec!["en-GB".into(), "fr-FR".into()];
    world
        .profile
        .send(std::sync::Arc::new(again))
        .expect("send");
    let waited =
        tokio::time::timeout(std::time::Duration::from_millis(100), subscriber.changed()).await;
    assert!(
        waited.is_err(),
        "ActiveLocale did not change, so nothing is published"
    );
}

#[test]
fn bundled_locales_list_en_gb_first_then_alphabetical() {
    let list = bundled_locales();
    assert_eq!(list[0], "en-GB");
    let mut rest = list[1..].to_vec();
    rest.sort_unstable();
    assert_eq!(&list[1..], rest.as_slice());
    assert!(list.contains(&"fr-FR") && list.contains(&"de-DE"));
}

#[tokio::test]
async fn profile_driven_events_are_logged_with_their_fields() {
    // The recorder is global and shared with the other tests of this binary, so this test looks for
    // lines by values only it uses.
    let world = World::start(Opts {
        offered: vec!["en-GB", "qq-QQ"],
        fallback: "zz-QQ",
        ..Opts::default()
    })
    .await;
    let not_bundled = super::support::capture::event("localisation.offered_not_bundled");
    assert!(not_bundled
        .iter()
        .any(|l| l.field("locale") == "qq-QQ" && !l.field("profile_version").is_empty()));
    let fallback = super::support::capture::event("localisation.fallback_not_bundled");
    assert!(fallback.iter().any(|l| l.field("locale") == "zz-QQ"));
    world
        .profile
        .send(snapshot(&["qq-QQ"], "en-GB"))
        .expect("profile changes");
    let mut subscriber = world.loc.subscribe();
    let _ = tokio::time::timeout(std::time::Duration::from_millis(100), subscriber.changed()).await;
    assert!(!super::support::capture::event("localisation.offered_empty").is_empty());

    // A person's choice the profile withdraws is logged with the locale and the version.
    let world = World::start(Opts::default()).await;
    world.loc.set_locale(Some("fr-FR")).await.expect("set");
    let _ = World::start(Opts::default()).await;
    let world = world
        .restart(Opts {
            offered: vec!["en-GB"],
            ..Opts::default()
        })
        .await;
    let withdrawn = super::support::capture::event("localisation.preference_not_offered");
    assert!(withdrawn
        .iter()
        .any(|l| l.field("locale") == "fr-FR" && l.field("profile_version") == "defaults"));
    drop(world);
}
