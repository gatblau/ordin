//! The build script's rules (Localisation.md Internal Logic A), run against temporary catalogue
//! folders: the script's own source is included and called, so a refusal is tested without a failing build.

#[allow(dead_code, clippy::all)]
#[path = "../../build.rs"]
mod script;

use std::fs;
use std::path::Path;

use super::support::Dir;

const NATIVE: &str = "locale-native-name = English\n";

/// Writes `files` (relative path, text) under a new folder and generates from it.
fn generate(files: &[(&str, &str)], pseudo: bool) -> (Dir, Result<(), String>) {
    let dir = Dir::new();
    let root = dir.path().join("catalogues");
    for (path, text) in files {
        let target = root.join(path);
        fs::create_dir_all(target.parent().expect("parent")).expect("folder");
        fs::write(&target, text).expect("file");
    }
    let out = dir.path().join("out");
    fs::create_dir_all(&out).expect("out folder");
    let result = script::generate(&root, &out, pseudo);
    (dir, result)
}

fn refused(files: &[(&str, &str)]) -> String {
    let (_dir, result) = generate(files, false);
    result.expect_err("the build is refused")
}

fn english(extra: &str) -> String {
    format!("{NATIVE}approval-card-send-button = Send\n{extra}")
}

pub fn orphan_message_is_refused() {
    let en = english("");
    let message = refused(&[
        ("en-GB/a.ftl", &en),
        (
            "fr-FR/a.ftl",
            "locale-native-name = Français\nno-such-message = Rien\n",
        ),
    ]);
    assert!(
        message.contains("no-such-message") && message.contains("orphan"),
        "{message}"
    );
}

#[test]
fn a_good_catalogue_generates_ids_areas_and_the_safety_set() {
    let en = english("# safety\nwho-acts-x = Ordin acts.\nplain-one = Plain.\n");
    let (dir, result) = generate(
        &[
            ("en-GB/a.ftl", &en),
            ("fr-FR/a.ftl", "locale-native-name = Français\n"),
        ],
        false,
    );
    result.expect("generated");
    let ids = fs::read_to_string(dir.path().join("out/message_ids.rs")).expect("ids file");
    assert!(ids.contains("pub const APPROVAL_CARD_SEND_BUTTON"));
    assert!(
        ids.contains("SAFETY_IDS: &[&str] = &[\"approval-card-send-button\", \"who-acts-x\"]"),
        "{ids}"
    );
    assert!(ids.contains("(\"plain-one\", \"a\")"), "{ids}");
    assert!(Path::new(&dir.path().join("out/catalogue-fr-FR.ftl")).exists());
}

#[test]
fn a_folder_name_that_is_not_a_locale_fails_the_build() {
    let en = english("");
    let message = refused(&[("en-GB/a.ftl", &en), ("not_a_locale/a.ftl", NATIVE)]);
    assert!(message.contains("not_a_locale"), "{message}");
}

#[test]
fn a_file_that_does_not_parse_fails_the_build_with_file_and_line() {
    let en = english("");
    let message = refused(&[
        ("en-GB/a.ftl", &en),
        ("en-GB/bad.ftl", "ok = fine\n\nbad = { $x\n"),
    ]);
    assert!(message.contains("bad.ftl:"), "{message}");
}

#[test]
fn an_identifier_that_is_not_kebab_case_fails_the_build() {
    let en = english("Not_Kebab = x\n");
    let message = refused(&[("en-GB/a.ftl", &en)]);
    assert!(message.contains("kebab"), "{message}");
}

#[test]
fn a_duplicate_identifier_across_files_fails_the_build() {
    let en = english("");
    let message = refused(&[
        ("en-GB/a.ftl", &en),
        ("en-GB/b.ftl", "approval-card-send-button = Again\n"),
    ]);
    assert!(
        message.contains("approval-card-send-button") && message.contains("both"),
        "{message}"
    );
}

#[test]
fn a_locale_without_a_native_name_fails_the_build() {
    let en = english("");
    let message = refused(&[
        ("en-GB/a.ftl", &en),
        ("fr-FR/a.ftl", "approval-card-send-button = Envoyer\n"),
    ]);
    assert!(message.contains("locale-native-name"), "{message}");
    let message = refused(&[("en-GB/a.ftl", "approval-card-send-button = Send\n")]);
    assert!(message.contains("locale-native-name"), "{message}");
}

#[test]
fn a_missing_source_catalogue_fails_the_build() {
    let message = refused(&[("fr-FR/a.ftl", "locale-native-name = Français\n")]);
    assert!(message.contains("en-GB"), "{message}");
}

#[test]
fn error_an_empty_safety_set_fails_the_build() {
    let message = refused(&[(
        "en-GB/a.ftl",
        &format!("{NATIVE}run-x = Run.\n# not a marker\nrun-y = Run.\n"),
    )]);
    assert!(message.contains("safety set is empty"), "{message}");
}

#[test]
fn a_locale_is_listed_complete_only_when_it_holds_every_english_message() {
    // A4a (LOC-CMP-1): en-GB and the generated pseudo-locales are complete by construction.
    let en = english("plain-one = Plain.\n");
    let (dir, result) = generate(
        &[
            ("en-GB/a.ftl", &en),
            (
                "de-DE/a.ftl",
                "locale-native-name = Deutsch\napproval-card-send-button = Senden\nplain-one = Schlicht.\n",
            ),
            ("fr-FR/a.ftl", "locale-native-name = Français\n"),
        ],
        true,
    );
    result.expect("generated");
    let list = fs::read_to_string(dir.path().join("out/catalogues.rs")).expect("catalogues.rs");
    assert!(
        list.contains(
            "pub(crate) const COMPLETE_LOCALES: &[&str] = &[\"en-GB\", \"ar-XB\", \"de-DE\", \"en-XA\"];"
        ),
        "{list}"
    );
}

#[test]
fn existing_gates_still_hold() {
    // Orphans and a missing native name are still refused, and with the pseudo-locales the generated
    // en-XA and ar-XB are present, accented and padded, and wrapped in the override marks.
    orphan_message_is_refused();
    let en = english(
        "run-progress = { $count ->\n    [one] One item\n   *[other] { $count } items\n}\n",
    );
    let (dir, result) = generate(&[("en-GB/a.ftl", &en)], true);
    result.expect("generated with pseudo-locales");
    let accented = fs::read_to_string(dir.path().join("out/catalogue-en-XA.ftl")).expect("en-XA");
    assert!(accented.contains("Séñd"), "{accented}");
    assert!(
        accented.contains("·"),
        "padded with the middle dot: {accented}"
    );
    assert!(
        accented.contains("{ $count }"),
        "placeables stay verbatim: {accented}"
    );
    let bidi = fs::read_to_string(dir.path().join("out/catalogue-ar-XB.ftl")).expect("ar-XB");
    assert!(bidi.contains("\u{202E}Send\u{202C}"), "{bidi}");
    let list = fs::read_to_string(dir.path().join("out/catalogues.rs")).expect("catalogues.rs");
    let at = |tag: &str| list.find(&format!("\"{tag}\"")).expect(tag);
    assert!(
        at("en-GB") < at("ar-XB") && at("ar-XB") < at("en-XA"),
        "{list}"
    );
}

#[test]
fn pseudo_accent_pads_to_one_hundred_and_forty_percent() {
    let padded = script::pseudo_accent_for_test("Send it");
    assert_eq!(padded.chars().count(), 10);
    assert!(padded.starts_with("Séñd ít"));
}
