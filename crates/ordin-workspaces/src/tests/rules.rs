//! The pure rules the Error Table names: 2C validation codes, the git remote forms, the adapter
//! error mapping, the working-copy transition table and the record-comment stripping.

use ordin_types::domain::email::EmailLinkKind;
use ordin_types::domain::places::{ConfluenceLinkState, SourceLinkKind, WorkingCopyState};
use ordin_types::time::Timestamp;
use ordin_types::{ApprovalId, PlaceId};

use crate::confluence::{check_approved, check_audience, check_space_and_parent};
use crate::items::transition_allowed;
use crate::jira::strip_record_block;
use crate::links::{
    check_display_name, check_origin, check_place_rule, map_adapter_code, parse_git_remote,
};
use crate::mail::check_email_link;
use crate::saved::check_file_name;
use crate::service::WorkspaceService;
use crate::store::check_item;
use crate::{NewEmailLink, WorkspaceError};

fn code<T: std::fmt::Debug>(result: Result<T, WorkspaceError>) -> &'static str {
    result.expect_err("an error").code()
}

fn people(addresses: &[&str], domains: &[&str]) -> NewEmailLink {
    NewEmailLink {
        kind: EmailLinkKind::PeopleAndDomains,
        folder_or_label: None,
        addresses: addresses.iter().map(|a| (*a).to_owned()).collect(),
        domains: domains.iter().map(|d| (*d).to_owned()).collect(),
        display_name: "Suppliers".to_owned(),
    }
}

#[test]
fn workspace_names_are_trimmed_and_bounded() {
    assert_eq!(
        WorkspaceService::check_name("  Q3 review ").unwrap(),
        "Q3 review"
    );
    assert_eq!(code(WorkspaceService::check_name("   ")), "WS_NAME_EMPTY");
    assert_eq!(
        WorkspaceService::check_name(&"x".repeat(80)).unwrap().len(),
        80
    );
    assert_eq!(
        code(WorkspaceService::check_name(&"x".repeat(81))),
        "WS_NAME_TOO_LONG"
    );
}

#[test]
fn a_link_origin_is_one_to_2048_bytes() {
    assert!(check_origin("a").is_ok());
    assert!(check_origin(&"a".repeat(2048)).is_ok());
    assert_eq!(code(check_origin("")), "SRC_LOCATION_INVALID");
    assert_eq!(
        code(check_origin(&"a".repeat(2049))),
        "SRC_LOCATION_INVALID"
    );
}

#[test]
fn a_link_name_is_one_to_120_characters() {
    assert_eq!(check_display_name("  Contracts ").unwrap(), "Contracts");
    assert_eq!(code(check_display_name("  ")), "SRC_NAME_INVALID");
    assert_eq!(
        code(check_display_name(&"é".repeat(121))),
        "SRC_NAME_INVALID"
    );
    assert!(check_display_name(&"é".repeat(120)).is_ok());
}

#[test]
fn a_links_place_follows_its_kind() {
    let place = Some(PlaceId::new());
    for kind in [
        SourceLinkKind::LocalFolder,
        SourceLinkKind::GitRepositoryFolder,
    ] {
        assert!(check_place_rule(kind, place).is_ok());
        assert_eq!(code(check_place_rule(kind, None)), "SRC_PLACE_MISMATCH");
    }
    for kind in [
        SourceLinkKind::SharepointLocation,
        SourceLinkKind::ConfluenceLocation,
        SourceLinkKind::JiraIssue,
    ] {
        assert!(check_place_rule(kind, place).is_ok());
        assert!(check_place_rule(kind, None).is_ok());
    }
    assert!(check_place_rule(SourceLinkKind::GitRemote, None).is_ok());
    assert_eq!(
        code(check_place_rule(SourceLinkKind::GitRemote, place)),
        "SRC_PLACE_MISMATCH"
    );
}

#[test]
fn a_working_copy_item_has_a_path_and_a_version_in_range() {
    assert!(check_item("a.md", "c4").is_ok());
    assert_eq!(code(check_item("", "c4")), "WCI_PATH_INVALID");
    assert_eq!(
        code(check_item(&"p".repeat(2049), "c4")),
        "WCI_PATH_INVALID"
    );
    assert_eq!(code(check_item("a.md", "")), "WCI_VERSION_INVALID");
    assert_eq!(
        code(check_item("a.md", &"v".repeat(257))),
        "WCI_VERSION_INVALID"
    );
    assert!(check_item(&"p".repeat(2048), &"v".repeat(256)).is_ok());
}

#[test]
fn confluence_fields_are_checked() {
    assert!(check_space_and_parent("PROC", "4000").is_ok());
    assert_eq!(
        code(check_space_and_parent("", "4000")),
        "CFL_SPACE_INVALID"
    );
    assert_eq!(
        code(check_space_and_parent(&"s".repeat(256), "4000")),
        "CFL_SPACE_INVALID"
    );
    assert_eq!(
        code(check_space_and_parent("PROC", "")),
        "CFL_PARENT_INVALID"
    );
    assert_eq!(
        code(check_space_and_parent("PROC", "40a0")),
        "CFL_PARENT_INVALID"
    );
    assert_eq!(
        code(check_space_and_parent("PROC", &"1".repeat(65))),
        "CFL_PARENT_INVALID"
    );
    assert!(check_space_and_parent("PROC", &"1".repeat(64)).is_ok());
    assert!(check_audience("Procurement team").is_ok());
    assert_eq!(code(check_audience("")), "CFL_AUDIENCE_INVALID");
    assert_eq!(
        code(check_audience(&"a".repeat(501))),
        "CFL_AUDIENCE_INVALID"
    );
}

#[test]
fn an_active_or_paused_confluence_link_must_be_approved() {
    let approval = Some(ApprovalId::new());
    let at = Some(Timestamp::now());
    assert!(check_approved(ConfluenceLinkState::Proposed, None, None).is_ok());
    assert!(check_approved(ConfluenceLinkState::Active, approval, at).is_ok());
    assert!(check_approved(ConfluenceLinkState::Paused, approval, at).is_ok());
    for state in [ConfluenceLinkState::Active, ConfluenceLinkState::Paused] {
        assert_eq!(code(check_approved(state, None, at)), "CFL_NOT_APPROVED");
        assert_eq!(
            code(check_approved(state, approval, None)),
            "CFL_NOT_APPROVED"
        );
    }
}

#[test]
fn an_email_link_has_the_fields_its_kind_needs() {
    let folder = |label: Option<&str>, addresses: &[&str]| NewEmailLink {
        kind: EmailLinkKind::FolderOrLabel,
        folder_or_label: label.map(str::to_owned),
        addresses: addresses.iter().map(|a| (*a).to_owned()).collect(),
        domains: vec![],
        display_name: "Inbox".to_owned(),
    };
    assert!(check_email_link(&folder(Some("Suppliers"), &[])).is_ok());
    assert_eq!(
        code(check_email_link(&folder(None, &[]))),
        "MAIL_LINK_FIELDS_MISMATCH"
    );
    assert_eq!(
        code(check_email_link(&folder(Some(&"f".repeat(256)), &[]))),
        "MAIL_LINK_FIELDS_MISMATCH"
    );
    assert_eq!(
        code(check_email_link(&folder(
            Some("Suppliers"),
            &["a@b.example"]
        ))),
        "MAIL_LINK_FIELDS_MISMATCH"
    );
    assert_eq!(
        code(check_email_link(&people(&[], &[]))),
        "MAIL_LINK_FIELDS_MISMATCH"
    );
    let mut with_folder = people(&["a@b.example"], &[]);
    with_folder.folder_or_label = Some("x".to_owned());
    assert_eq!(
        code(check_email_link(&with_folder)),
        "MAIL_LINK_FIELDS_MISMATCH"
    );
}

#[test]
fn email_addresses_and_domains_are_validated_lowered_and_deduplicated() {
    let (addresses, domains) = check_email_link(&people(
        &["Sam.Lee@Acme.Example", "sam.lee@acme.example"],
        &["Acme.Example", "acme.example"],
    ))
    .unwrap();
    assert_eq!(addresses, vec!["sam.lee@acme.example"]);
    assert_eq!(domains, vec!["acme.example"]);
    for bad in [
        "no-at-sign",
        "@acme.example",
        "a@",
        "a b@acme.example",
        "a..b@acme.example",
        ".a@acme.example",
        "a@-acme.example",
        "a@acme..example",
    ] {
        assert_eq!(
            code(check_email_link(&people(&[bad], &[]))),
            "MAIL_ADDRESS_INVALID",
            "{bad}"
        );
    }
    let long_local = format!("{}@acme.example", "l".repeat(65));
    assert_eq!(
        code(check_email_link(&people(&[&long_local], &[]))),
        "MAIL_ADDRESS_INVALID"
    );
    let too_long = format!("a@{}.example", "d".repeat(250));
    assert_eq!(
        code(check_email_link(&people(&[&too_long], &[]))),
        "MAIL_ADDRESS_INVALID"
    );
    let many: Vec<String> = (0..201).map(|i| format!("p{i}@acme.example")).collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(
        code(check_email_link(&people(&refs, &[]))),
        "MAIL_ADDRESS_INVALID"
    );
    for bad in [
        "",
        "acme..example",
        "-acme.example",
        "acme_example.com",
        &"d".repeat(254),
    ] {
        assert_eq!(
            code(check_email_link(&people(&[], &[bad]))),
            "MAIL_DOMAIN_INVALID",
            "{bad}"
        );
    }
    let many_domains: Vec<String> = (0..101).map(|i| format!("d{i}.example")).collect();
    let refs: Vec<&str> = many_domains.iter().map(String::as_str).collect();
    assert_eq!(
        code(check_email_link(&people(&[], &refs))),
        "MAIL_DOMAIN_INVALID"
    );
}

#[test]
fn imap_check_state_holds_32_bit_unsigned_values() {
    let ok = crate::check_imap_state(&[("INBOX".to_owned(), 1, 4_294_967_295)]).unwrap();
    assert_eq!((ok[0].uid_validity, ok[0].highest_uid), (1, u32::MAX));
    assert_eq!(
        code(crate::check_imap_state(&[("INBOX".to_owned(), -1, 0)])),
        "MAIL_IMAP_STATE_INVALID"
    );
    assert_eq!(
        code(crate::check_imap_state(&[(
            "INBOX".to_owned(),
            0,
            4_294_967_296
        )])),
        "MAIL_IMAP_STATE_INVALID"
    );
}

#[test]
fn a_git_remote_has_one_of_the_two_address_forms() {
    let (origin, name) =
        parse_git_remote("https://github.com/acme/contracts", "main", "handover").unwrap();
    assert_eq!(
        origin,
        "git:https://github.com/acme/contracts#main:handover"
    );
    assert_eq!(name, "contracts/handover");
    let (origin, name) =
        parse_git_remote("git@gitlab.example.com:acme/contracts.git", "dev", "").unwrap();
    assert_eq!(origin, "git:git@gitlab.example.com:acme/contracts#dev:");
    assert_eq!(name, "contracts");
    let (origin, _) =
        parse_git_remote("https://github.com/acme/contracts.git", "main", "").unwrap();
    assert_eq!(origin, "git:https://github.com/acme/contracts#main:");
    for url in [
        "http://github.com/acme/contracts",
        "https://github.com/acme",
        "https://github.com/acme/a/b",
        "ftp://x/y/z",
        "git@github.com",
        "git@github.com:acme",
        "https://github.com/acme/con tracts",
        "https://github.com/acme/contracts?x=1",
        "not a url",
        "",
    ] {
        assert_eq!(
            code(parse_git_remote(url, "main", "")),
            "WS_GIT_REMOTE_INVALID",
            "{url}"
        );
    }
    let long_path = "p".repeat(1025);
    for (branch, path) in [
        ("", ""),
        ("a\nb", ""),
        ("main", "../x"),
        ("main", "a/../b"),
        ("main", "/abs"),
        ("main", long_path.as_str()),
    ] {
        assert_eq!(
            code(parse_git_remote(
                "https://github.com/acme/contracts",
                branch,
                path
            )),
            "WS_GIT_REMOTE_INVALID",
            "{branch} {path}"
        );
    }
    assert_eq!(
        code(parse_git_remote(
            "https://github.com/acme/contracts",
            &"b".repeat(256),
            ""
        )),
        "WS_GIT_REMOTE_INVALID"
    );
}

#[test]
fn adapter_errors_map_to_the_services_codes() {
    let cases = [
        ("SP_SIGN_IN_NEEDED", "WS_SOURCE_SIGN_IN_NEEDED"),
        ("CONF_SIGN_IN_NEEDED", "WS_SOURCE_SIGN_IN_NEEDED"),
        ("JIRA_SIGN_IN_NEEDED", "WS_SOURCE_SIGN_IN_NEEDED"),
        ("JIRA_NOT_CONNECTED", "WS_SOURCE_SIGN_IN_NEEDED"),
        ("JIRA_TOKEN_REFUSED", "WS_SOURCE_SIGN_IN_NEEDED"),
        ("SP_NOT_CONFIGURED", "WS_SOURCE_NOT_CONFIGURED"),
        ("CONF_NOT_CONFIGURED", "WS_SOURCE_NOT_CONFIGURED"),
        ("SP_ACCESS_DENIED", "WS_SOURCE_ACCESS_DENIED"),
        ("CONF_ACCESS_DENIED", "WS_SOURCE_ACCESS_DENIED"),
        ("JIRA_ACCESS_DENIED", "WS_SOURCE_ACCESS_DENIED"),
        ("SP_NOT_FOUND", "WS_SOURCE_NOT_FOUND"),
        ("CONF_NOT_FOUND", "WS_SOURCE_NOT_FOUND"),
        ("JIRA_NOT_FOUND", "WS_SOURCE_NOT_FOUND"),
        ("SP_UNAVAILABLE", "WS_SOURCE_UNAVAILABLE"),
        ("SP_THROTTLED", "WS_SOURCE_UNAVAILABLE"),
        ("CONF_UNAVAILABLE", "WS_SOURCE_UNAVAILABLE"),
        ("CONF_THROTTLED", "WS_SOURCE_UNAVAILABLE"),
        ("JIRA_UNAVAILABLE", "WS_SOURCE_UNAVAILABLE"),
        ("JIRA_THROTTLED", "WS_SOURCE_UNAVAILABLE"),
        ("JIRA_RESPONSE_INVALID", "WS_SOURCE_UNAVAILABLE"),
        ("SP_TOO_LARGE", "WS_ITEM_TOO_LARGE"),
        ("CONF_TOO_LARGE", "WS_ITEM_TOO_LARGE"),
        ("JIRA_TOO_LARGE", "WS_ITEM_TOO_LARGE"),
        ("JIRA_SITE_MISMATCH", "WS_JIRA_SITE_MISMATCH"),
        ("SP_LOCATION_INVALID", "SRC_LOCATION_INVALID"),
    ];
    for (adapter, ours) in cases {
        assert_eq!(map_adapter_code(adapter).code(), ours, "{adapter}");
    }
    assert!(map_adapter_code("SP_THROTTLED").retryable());
    assert!(!map_adapter_code("SP_ACCESS_DENIED").retryable());
}

#[test]
fn the_working_copy_transitions_are_2cs_and_the_two_of_04_85() {
    use WorkingCopyState::*;
    let allowed = [
        (Read, Changed),
        (Changed, AwaitingPush),
        (AwaitingPush, Pushed),
        (AwaitingPush, OriginChanged),
        (OriginChanged, Read),
        (Pushed, Read),
        (AwaitingPush, Changed),
    ];
    let all = [Read, Changed, AwaitingPush, Pushed, OriginChanged];
    for from in all {
        for to in all {
            assert_eq!(
                transition_allowed(from, to),
                allowed.contains(&(from, to)),
                "{from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn a_saved_file_name_is_one_path_free_name() {
    assert_eq!(check_file_name("  quote.pdf ").unwrap(), "quote.pdf");
    for bad in [
        "",
        "   ",
        ".",
        "..",
        "a/b",
        "a\\b",
        "a:b",
        "a\nb",
        "../x.json",
    ] {
        assert_eq!(
            code(check_file_name(bad)),
            "WS_FILE_NAME_INVALID",
            "{bad:?}"
        );
    }
    assert!(check_file_name(&"n".repeat(255)).is_ok());
    assert_eq!(
        code(check_file_name(&"n".repeat(256))),
        "WS_FILE_NAME_INVALID"
    );
}

#[test]
fn the_handover_record_block_is_removed_and_the_note_stays() {
    let body =
        "Please review the rates.\n\n```yaml\n# ordin.handover/v1 record\nid: 1\n```\nThanks.";
    assert_eq!(
        strip_record_block(body),
        "Please review the rates.\n\nThanks."
    );
    let other = "Note\n\n```yaml\nplain: block\n```";
    assert_eq!(strip_record_block(other), other);
}
