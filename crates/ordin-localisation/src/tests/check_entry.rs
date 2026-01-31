//! `check_entry`: one failing and one passing entry for each check (Playbook Step 12 extra gate item),
//! and the scenario outline of the same name.

use crate::{check_entry, EntryProblem};

const READ: &str = "run-progress-documents-read";

fn select(keys: &[(&str, &str)], default: (&str, &str)) -> String {
    let mut text = format!("{READ} = {{ $count ->\n");
    for (key, value) in keys {
        text.push_str(&format!("    [{key}] {value}\n"));
    }
    text.push_str(&format!("   *[{}] {}\n}}", default.0, default.1));
    text
}

fn problem(locale: &str, expected: Option<&str>, entry: &str) -> Option<EntryProblem> {
    check_entry(locale, expected, entry).err()
}

#[test]
fn each_fixed_check_refuses_a_failing_entry_and_accepts_a_passing_one() {
    let ok_french = select(
        &[("one", "Un document lu")],
        ("other", "{ NUMBER($count) } documents lus"),
    );
    let few = select(&[("few", "Peu")], ("other", "{ $count }"));
    let many = select(&[("many", "Beaucoup")], ("other", "{ $count } lus"));
    let cases: Vec<(String, Option<EntryProblem>)> = vec![
        (ok_french, None),
        (
            format!("{READ} = {{ $count -> [one] Un *[other] Deux"),
            Some(EntryProblem::DoesNotParse),
        ),
        (
            format!("{READ} = Lu\nother-message = Lu"),
            Some(EntryProblem::NotOneMessage),
        ),
        (
            "no-such-message = Lu".to_string(),
            Some(EntryProblem::UnknownMessage),
        ),
        (
            format!("{READ} = Documents lus"),
            Some(EntryProblem::VariableMissing {
                name: "count".into(),
            }),
        ),
        (
            format!("{READ} = {{ $count }} {{ $user }} lus"),
            Some(EntryProblem::VariableUnknown {
                name: "user".into(),
            }),
        ),
        (
            few,
            Some(EntryProblem::PluralCategory { key: "few".into() }),
        ),
        (many, None),
        (
            format!("{READ} = {{ UPPER($count) }}"),
            Some(EntryProblem::FunctionNotAllowed {
                name: "UPPER".into(),
            }),
        ),
        (
            format!("{READ} = {{ $count }} \u{202E} lus"),
            Some(EntryProblem::ForbiddenCharacter),
        ),
    ];
    for (entry, expected) in cases {
        assert_eq!(problem("fr-FR", None, &entry), expected, "entry: {entry}");
    }
}

#[test]
fn length_check_fails_over_4096_characters_and_passes_at_the_limit() {
    let fill = |n: usize| format!("{READ} = {{ $count }} {}", "x".repeat(n));
    let at_limit = fill(4_096 - format!("{READ} = {{ $count }} ").chars().count());
    assert_eq!(at_limit.chars().count(), 4_096);
    assert_eq!(problem("fr-FR", None, &at_limit), None);
    assert_eq!(
        problem("fr-FR", None, &format!("{at_limit}x")),
        Some(EntryProblem::TooLong)
    );
    assert_eq!(problem("fr-FR", None, ""), Some(EntryProblem::TooLong));
}

#[test]
fn character_check_refuses_controls_and_bidirectional_marks_but_allows_tab() {
    for forbidden in [
        '\u{0007}', '\u{0000}', '\u{202A}', '\u{202E}', '\u{2066}', '\u{2069}', '\r',
    ] {
        let entry = format!("{READ} = {{ $count }}{forbidden}x");
        assert_eq!(
            problem("fr-FR", None, &entry),
            Some(EntryProblem::ForbiddenCharacter),
            "{forbidden:?}"
        );
    }
    assert_eq!(
        problem("fr-FR", None, &format!("{READ} = {{ $count }}\tlus")),
        None
    );
}

#[test]
fn syntax_checks_separate_junk_from_a_second_message_and_a_term() {
    assert_eq!(
        problem("fr-FR", None, &format!("{READ} = {{ $count")),
        Some(EntryProblem::DoesNotParse)
    );
    assert_eq!(
        problem("fr-FR", None, "-brand = Ordin"),
        Some(EntryProblem::NotOneMessage)
    );
    assert_eq!(
        problem("fr-FR", None, "# only a comment"),
        Some(EntryProblem::NotOneMessage)
    );
    assert_eq!(
        problem("fr-FR", None, &format!("# remark\n{READ} = {{ $count }}")),
        None
    );
}

#[test]
fn identifier_check_compares_with_the_expected_identifier_when_given() {
    let entry = format!("{READ} = {{ $count }}");
    assert_eq!(problem("fr-FR", Some(READ), &entry), None);
    assert_eq!(
        problem("fr-FR", Some("approval-card-send-button"), &entry),
        Some(EntryProblem::IdMismatch)
    );
    assert_eq!(problem("fr-FR", None, &entry), None);
}

#[test]
fn attribute_check_needs_the_same_attribute_names_as_en_gb() {
    assert_eq!(
        problem(
            "fr-FR",
            None,
            "run-progress-amount = Le total est { NUMBER($amount) }.\n    .title = Total"
        ),
        None
    );
    assert_eq!(
        problem(
            "fr-FR",
            None,
            "run-progress-amount = Le total est { NUMBER($amount) }."
        ),
        Some(EntryProblem::AttributeMismatch)
    );
    assert_eq!(
        problem(
            "fr-FR",
            None,
            "run-progress-amount = Total { NUMBER($amount) }\n    .title = A\n    .extra = B"
        ),
        Some(EntryProblem::AttributeMismatch)
    );
    assert_eq!(
        problem(
            "fr-FR",
            None,
            "approval-card-send-button = Envoyer\n    .title = Envoyer"
        ),
        Some(EntryProblem::AttributeMismatch)
    );
}

#[test]
fn reference_check_allows_en_gb_messages_and_refuses_other_references() {
    let known = format!("{READ} = {{ locale-native-name }} {{ $count }}");
    assert_eq!(problem("fr-FR", None, &known), None);
    let unknown = format!("{READ} = {{ missing-message }} {{ $count }}");
    assert_eq!(
        problem("fr-FR", None, &unknown),
        Some(EntryProblem::ReferenceUnknown {
            id: "missing-message".into()
        })
    );
    let term = format!("{READ} = {{ -brand }} {{ $count }}");
    assert_eq!(
        problem("fr-FR", None, &term),
        Some(EntryProblem::ReferenceUnknown { id: "brand".into() })
    );
    let date = "run-progress-finished-at = Terminé le { DATETIME($at, style: \"date\") }";
    assert_eq!(problem("fr-FR", None, date), None);
}

#[test]
fn plural_check_takes_the_categories_of_the_language_numeric_keys_and_en_gb_keys() {
    // Japanese has only `other`; `one` is still allowed because en-GB selects on it.
    let one_in_japanese = select(&[("one", "1")], ("other", "{ $count }"));
    assert_eq!(problem("ja-JP", None, &one_in_japanese), None);
    // Arabic has `zero`, `two`, `few`, `many`.
    let arabic = select(
        &[("zero", "0"), ("two", "2"), ("few", "3")],
        ("many", "{ $count }"),
    );
    assert_eq!(problem("ar", None, &arabic), None);
    let numeric =
        format!("{READ} = {{ $count ->\n    [0] Zero\n   *[other] {{ NUMBER($count) }}\n}}");
    assert_eq!(problem("fr-FR", None, &numeric), None);
    let zero_in_english_chain = select(&[("zero", "0")], ("other", "{ $count }"));
    assert_eq!(
        problem("en-GB", None, &zero_in_english_chain),
        Some(EntryProblem::PluralCategory { key: "zero".into() })
    );
    // A tag ICU4X does not know has the root's `other` only.
    assert_eq!(
        problem(
            "zz-ZZ",
            None,
            &select(&[("few", "3")], ("other", "{ $count }"))
        ),
        Some(EntryProblem::PluralCategory { key: "few".into() })
    );
    assert_eq!(
        problem("not a tag", None, &select(&[], ("other", "{ $count }"))),
        None
    );
}

#[test]
fn every_problem_has_a_catalogue_sentence() {
    let ids = crate::message_ids();
    for problem in [
        EntryProblem::TooLong,
        EntryProblem::ForbiddenCharacter,
        EntryProblem::DoesNotParse,
        EntryProblem::NotOneMessage,
        EntryProblem::UnknownMessage,
        EntryProblem::IdMismatch,
        EntryProblem::VariableMissing {
            name: String::new(),
        },
        EntryProblem::VariableUnknown {
            name: String::new(),
        },
        EntryProblem::AttributeMismatch,
        EntryProblem::FunctionNotAllowed {
            name: String::new(),
        },
        EntryProblem::ReferenceUnknown { id: String::new() },
        EntryProblem::PluralCategory { key: String::new() },
        EntryProblem::LanguageFull,
    ] {
        assert!(ids.contains(&problem.sentence_id()), "{problem:?}");
        assert!(!problem.kind().is_empty());
    }
}
