//! LOC-CMP-1: a bundled language is offered only when its catalogue holds every `en-GB` message, or
//! when the person has taken it up in the person layer. These worlds start with the real
//! completeness, so the short bundled `fr-FR` and `de-DE` catalogues count as incomplete.

use ordin_types::CodedError;

use super::world::{Opts, World};
use crate::catalogue::{bundled_text, complete_locales, entries_by_id};
use crate::{bundled_locales, message_ids, LanguageMatch};

/// The bundled catalogues these tests rely on being short.
fn assume_short(tags: &[&str]) {
    for tag in tags {
        assert!(
            bundled_locales().contains(tag) && !complete_locales().contains(tag),
            "these tests need a short bundled {tag} catalogue; pick another tag once it is complete"
        );
    }
}

fn real(opts: Opts) -> Opts {
    Opts {
        complete: vec![],
        ..opts
    }
}

fn offered_tags(world: &World) -> Vec<String> {
    world
        .loc
        .offered()
        .into_iter()
        .map(|offered| offered.locale)
        .filter(|tag| tag != "en-XA" && tag != "ar-XB")
        .collect()
}

#[test]
fn complete_locales_are_the_bundled_catalogues_that_hold_every_english_message() {
    assert_eq!(complete_locales().first(), Some(&"en-GB"));
    for tag in bundled_locales() {
        let held = entries_by_id(bundled_text(tag).expect("bundled text"));
        let whole = message_ids().iter().all(|id| held.contains_key(*id));
        assert_eq!(complete_locales().contains(tag), whole, "{tag}");
    }
    if cfg!(feature = "pseudo-locales") {
        assert!(complete_locales().contains(&"en-XA") && complete_locales().contains(&"ar-XB"));
    }
}

// Scenario: Edge — a bundled language that does not cover the screens is not offered (LOC-CMP-1)
#[tokio::test]
async fn edge_a_bundled_language_that_does_not_cover_the_screens_is_not_offered() {
    assume_short(&["fr-FR", "de-DE"]);
    let world = World::start(real(Opts {
        offered: vec!["en-GB", "fr-FR", "de-DE"],
        ..Opts::default()
    }))
    .await;
    assert_eq!(offered_tags(&world), ["en-GB"]);
    let before = world.loc.active();
    let error = world
        .loc
        .set_locale(Some("fr-FR"))
        .await
        .expect_err("refused");
    assert_eq!(error.code(), "LOC_NOT_OFFERED");
    assert_eq!(world.loc.active(), before);
    let summary = world
        .loc
        .languages()
        .into_iter()
        .find(|language| language.locale == "fr-FR")
        .expect("still listed among the languages");
    assert!(!summary.choosable && !summary.offered_by_company);
    // Its compiled-in text stays available.
    assert!(world.loc.catalogue("fr-FR").is_ok());
}

// Scenario: Edge — the default list offers every complete bundled language (LOC-CMP-1)
#[tokio::test]
async fn edge_the_default_list_offers_every_complete_bundled_language() {
    assume_short(&["fr-FR", "de-DE"]);
    let world = World::start(real(Opts {
        offered: vec![],
        ..Opts::default()
    }))
    .await;
    let expected: Vec<String> = complete_locales()
        .iter()
        .filter(|tag| **tag != "en-XA" && **tag != "ar-XB")
        .map(|tag| (*tag).to_string())
        .collect();
    assert_eq!(offered_tags(&world), expected);
}

// Scenario: Edge — the operating system's language is not used when its catalogue is short (LOC-CMP-1)
#[tokio::test]
async fn edge_the_operating_system_s_language_is_not_used_when_its_catalogue_is_short() {
    assume_short(&["de-DE"]);
    let world = World::start(real(Opts {
        offered: vec!["en-GB", "de-DE"],
        os: vec!["de-AT", "de-DE"],
        ..Opts::default()
    }))
    .await;
    assert_eq!(world.loc.active().locale, "en-GB");
}

// Scenario: Edge — a short bundled fallback falls back to English (LOC-CMP-1)
#[tokio::test]
async fn edge_a_short_bundled_fallback_falls_back_to_english() {
    assume_short(&["fr-FR"]);
    let world = World::start(real(Opts {
        offered: vec!["en-GB", "fr-FR"],
        fallback: "fr-FR",
        ..Opts::default()
    }))
    .await;
    assert_eq!(world.loc.active().fallback_chain, ["en-GB"]);
}

// Scenario: Happy path — a person who imports a bundled language can choose it (LOC-CMP-1)
#[tokio::test]
async fn happy_path_a_person_who_imports_a_bundled_language_can_choose_it() {
    assume_short(&["fr-FR"]);
    let world = World::start(real(Opts {
        offered: vec!["en-GB", "fr-FR"],
        ..Opts::default()
    }))
    .await;
    let file = "### ordin language file\n### locale: fr-FR\nrun-progress-documents-read = { $count } documents lus\n";
    world
        .loc
        .import_language("fr.ftl", file, false)
        .await
        .expect("imported");
    assert_eq!(offered_tags(&world), ["en-GB", "fr-FR"]);
    let active = world.loc.set_locale(Some("fr-FR")).await.expect("chosen");
    assert_eq!(active.locale, "fr-FR");
    // Removing it takes the person layer away again (not while it is in force).
    world
        .loc
        .set_locale(None)
        .await
        .expect("follow the computer");
    world.loc.remove_language("fr-FR").await.expect("removed");
    assert_eq!(offered_tags(&world), ["en-GB"]);
}

// Scenario: Edge — asking for a short bundled language in words offers to make it (LOC-CMP-1)
#[tokio::test]
async fn edge_asking_for_a_short_bundled_language_in_words_offers_to_make_it() {
    assume_short(&["de-DE"]);
    let world = World::start(real(Opts::default())).await;
    assert_eq!(
        world.loc.match_language("de").expect("matched"),
        LanguageMatch::Missing {
            locale: "de".to_string()
        }
    );
}
