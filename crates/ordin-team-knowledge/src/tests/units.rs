//! The file rules of Shared Context 2 and §3, and the words of the cards.

use crate::files::{self, PathKind};
use crate::text;

#[test]
fn location_forms_admit_no_credential() {
    assert!(files::location_ok(
        "sharepoint",
        "graph:drives/b!x9Fq2k/items/01ABCDEF2GHIJ"
    ));
    assert!(files::location_ok("confluence", "confluence:page/123456"));
    assert!(files::location_ok("confluence", "confluence:space/PROC"));
    assert!(files::location_ok(
        "jira_issue",
        "https://acme.atlassian.net/browse/SUP-42"
    ));
    assert!(files::location_ok(
        "git_remote",
        "git:https://github.com/acme/a#main:docs"
    ));
    for (kind, bad) in [
        ("git_remote", "git:https://x:secret@github.com/acme/a#main:"),
        ("git_remote", "git:https://github.com/acme/a?token=x#main:"),
        ("git_remote", "git:https://github.com/acme/a#main:#x"),
        ("git_remote", "git:https://github.com/acme/a#main:../up"),
        (
            "jira_issue",
            "https://user@acme.atlassian.net/browse/SUP-42",
        ),
        ("sharepoint", "graph:drives/b?x/items/1"),
        ("confluence", "confluence:page/abc"),
        ("other", "x"),
    ] {
        assert!(!files::location_ok(kind, bad), "{bad}");
    }
}

#[test]
fn result_paths_follow_the_one_rule() {
    assert!(files::result_path_ok("Kestrel quote — comparison.md"));
    assert!(files::result_path_ok("notes/board pack.md"));
    for bad in [
        "../x.md",
        ".hidden/x.md",
        "a\\b.md",
        "C:/x.md",
        "CON.md",
        "notes/aux.txt",
        "report.",
        "/abs.md",
        "",
    ] {
        assert!(!files::result_path_ok(bad), "{bad}");
    }
}

#[test]
fn both_kept_names_follow_sa_team_tw_12() {
    assert_eq!(
        files::both_kept_name("Board pack.md", "asha-patel", 1),
        "Board pack (asha-patel).md"
    );
    assert_eq!(
        files::both_kept_name("Board pack.md", "asha-patel", 2),
        "Board pack (asha-patel 2).md"
    );
    assert_eq!(
        files::both_kept_name("notes/README", "sam", 1),
        "notes/README (sam)"
    );
}

#[test]
fn the_walk_reads_its_names_and_ignores_every_other() {
    assert_eq!(
        files::classify("ordin-team-knowledge.json"),
        Some(PathKind::Format)
    );
    assert!(matches!(
        files::classify("facts/0192a2d0-1c3e-7a10-8b2f-5d6e7f809a1b.json"),
        Some(PathKind::Item { .. })
    ));
    assert!(matches!(
        files::classify("terms/accountManager.json"),
        Some(PathKind::Item { .. })
    ));
    assert_eq!(files::classify("index.md"), None);
    assert_eq!(files::classify("facts/nested/x.json"), None);
    assert_eq!(files::classify("README.md"), None);
}

#[test]
fn a_description_is_read_strictly() {
    let ok = r#"{"format":1,"workspace_key":"0192a0f1-1111-7222-8333-444455556666","name":"Q3 supplier review","name_changed_by":"asha-patel","name_changed_at":"2026-01-20T09:30:02.114Z","places":[{"key":"0192c100-0000-7000-8000-0000000000a1","kind":"sharepoint","location":"graph:drives/b!x9Fq2k/items/01ABCDEF2GHIJ","display_name":"Contracts","changed_by":"asha-patel","changed_at":"2026-01-20T09:30:02.114Z"}],"agents":[{"key":"0192c100-0000-7000-8000-0000000000b1","repository":{"profile_key":"acme-agents"},"id":"urn:ordin:acme:workspaces:supplier-analyst","version":"1.3.0","name":"Supplier analyst","access":[{"place":"0192c100-0000-7000-8000-0000000000a1","access":"read_and_change"},{"place":"results","access":"read_and_change"}],"changed_by":"asha-patel","changed_at":"2026-01-20T09:30:02.114Z"}],"results":{"kind":"team_folder","place":null,"changed_by":"asha-patel","changed_at":"2026-01-20T09:30:02.114Z"}}"#;
    let file = files::parse_description(ok.as_bytes()).expect("the spec's example reads");
    // Rendered with the rules of item files, it reads back the same.
    assert_eq!(
        files::parse_description(&files::render(&file)).unwrap(),
        file
    );
    assert_eq!(
        files::parse_description(ok.replace("\"format\":1", "\"format\":2").as_bytes()),
        Err(Some(2))
    );
    assert_eq!(
        files::parse_description(
            ok.replace("\"name\":\"Q3", "\"extra\":1,\"name\":\"Q3")
                .as_bytes()
        ),
        Err(None)
    );
    assert_eq!(
        files::parse_description(
            ok.replace("\"place\":\"results\"", "\"place\":\"elsewhere\"")
                .as_bytes()
        ),
        Err(None)
    );
    assert_eq!(
        files::parse_description(
            ok.replace(
                "asha-patel\",\"name_changed_at",
                "asha patel\",\"name_changed_at"
            )
            .as_bytes()
        ),
        Err(None)
    );
    assert_eq!(
        files::parse_description(&vec![b' '; files::DESCRIPTION_MAX_BYTES + 1]),
        Err(None)
    );
}

#[test]
fn an_activity_file_is_read_strictly() {
    let ok = r#"{"format":1,"id":"0192c2a0-7b1c-7d2e-8f30-4a5b6c7d8e9f","kind":"run","by":"asha-patel","at":"2026-01-20T10:12:40.001Z","task":"Compare documents","agent":"Supplier analyst","outcome":"completed","results":["Kestrel quote — comparison.md"],"change":null,"subject":null,"replaced_by":null}"#;
    assert!(files::parse_activity(ok.as_bytes(), "0192c2a0-7b1c-7d2e-8f30-4a5b6c7d8e9f").is_ok());
    assert!(
        files::parse_activity(ok.as_bytes(), "0192c2a0-7b1c-7d2e-8f30-000000000000").is_err(),
        "the stem is the id"
    );
    assert!(files::parse_activity(
        ok.replace("\"run\"", "\"chat\"").as_bytes(),
        "0192c2a0-7b1c-7d2e-8f30-4a5b6c7d8e9f"
    )
    .is_err());
    assert!(files::parse_activity(
        ok.replace("\"change\":null", "\"change\":\"renamed\"")
            .as_bytes(),
        "0192c2a0-7b1c-7d2e-8f30-4a5b6c7d8e9f"
    )
    .is_err());
    assert!(files::parse_activity(
        ok.replace("Kestrel quote", "../Kestrel quote").as_bytes(),
        "0192c2a0-7b1c-7d2e-8f30-4a5b6c7d8e9f"
    )
    .is_err());
}

#[test]
fn a_fact_file_is_rendered_and_read_back_byte_for_byte() {
    let e = super::support::entity("Kestrel");
    let f = super::support::fact("Kestrel's Q3 quote is valid until 30 November.", &e.id);
    let bytes = files::render_fact(&f, "Asha Patel");
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        text.ends_with("}\n")
            && text.contains("\"shared_by\": \"Asha Patel\"")
            && text.contains("\"triple\": null")
    );
    let back = files::parse_fact(&bytes, &f.id.to_string()).unwrap();
    assert_eq!(files::render_fact(&back, "Someone else"), bytes);
    assert!(
        files::parse_fact(&bytes, "0192a2d0-1c3e-7a10-8b2f-000000000000").is_err(),
        "the name is the id"
    );
    assert!(files::parse_fact(
        &text
            .replace("\"about\"", "\"colour\": 1, \"about\"")
            .into_bytes(),
        &f.id.to_string()
    )
    .is_err());
}

#[test]
fn a_page_takes_the_sharers_name_in_its_empty_line_only() {
    let id = ordin_types::KnowledgePageId::new();
    let bytes = super::support::page(
        id,
        "Supplier rates",
        "shared_by: \"\" in the body stays.",
        "",
    );
    let named = String::from_utf8(files::name_page(&bytes, "Asha \"A\" Patel")).unwrap();
    assert!(named.contains("shared_by: \"Asha \\\"A\\\" Patel\""));
    assert!(named.contains("shared_by: \"\" in the body stays."));
    assert_eq!(
        files::parse_page(named.as_bytes(), &id.to_string())
            .unwrap()
            .0,
        id
    );
}

#[test]
fn the_cards_words_hold_no_git_word() {
    let mut plan = crate::sync::Plan::default();
    plan.counts.new_facts = 12;
    plan.counts.changed_pages = 2;
    plan.counts.removed_pages = 1;
    plan.changed_titles = vec!["Supplier rates".into(), "Board pack".into()];
    plan.results = vec!["a.md".into()];
    plan.activity = 1;
    plan.description_changed = true;
    plan.workspace_subjects = vec!["Contracts".into()];
    plan.held_for_check = 2;
    let labels = text::count_labels(&plan.counts, &plan.changed_titles);
    assert_eq!(
        labels,
        [
            "12 new facts",
            "2 changed pages: Supplier rates, Board pack",
            "1 page removed"
        ]
    );
    let mut words = vec![
        text::commit_message("Asha", &plan),
        text::request_text("Asha", &plan).1,
    ];
    for line in [
        "Share what you've learned with acme/supplier-team?",
        "Your team will see them in Team activity.",
        "Everyone who can open this folder can open this workspace with the same places and agents, see its results and see what each person did, but never your conversations.",
    ] {
        words.push(line.to_owned());
    }
    let card_texts: Vec<String> = labels.clone();
    for t in card_texts.iter().chain(words.iter().skip(2)) {
        let lower = t.to_lowercase();
        for w in text::GIT_WORDS {
            assert!(
                !lower.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w),
                "{t}"
            );
        }
    }
}

#[test]
fn names_and_branches_have_their_forms() {
    assert_eq!(
        text::author_name("Asha Patel", Some("asha-patel")),
        "Asha Patel (asha-patel)"
    );
    let long = "x".repeat(300);
    let a = text::author_name(&long, Some("asha-patel"));
    assert_eq!(a.chars().count(), 200);
    assert!(a.ends_with(" (asha-patel)"));
    let q = text::question("Share what you've learned with ".to_owned() + &"a".repeat(200) + "?");
    assert!(q.chars().count() <= 120 && q.ends_with('?'));
    let id = ordin_types::TeamShareId::new();
    let b = text::review_branch(id);
    assert!(b.starts_with("ordin/team-knowledge/") && b.len() == 29);
}

#[test]
fn every_code_is_registered_under_this_crates_prefix() {
    for code in crate::ALL_CODES {
        let entry = ordin_types::error_registry::lookup(code).unwrap_or_else(|| panic!("{code}"));
        assert_eq!(entry.owner_crate, "ordin-team-knowledge");
    }
    let owned = ordin_types::error_registry::entries()
        .iter()
        .filter(|e| e.owner_crate == "ordin-team-knowledge")
        .count();
    assert_eq!(owned, crate::ALL_CODES.len());
}
