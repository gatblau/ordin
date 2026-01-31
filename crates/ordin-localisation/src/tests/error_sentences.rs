//! ErrorHandling.md amendment January 2026: every code a desktop build with no switch can return has
//! its own en-GB sentence (EH-SEN-1), and no error sentence holds a technical word (EH-SEN-2).

use ordin_types::error_registry::entries;

use crate::catalogue::english;
use crate::service::error_message_id;

/// The owners of EH-SEN-1: the workspace crates `cargo tree -p ordin-desktop -e normal` lists with no
/// build switch on, and the screens module. A crate a switch adds joins this list when the switch is
/// turned on for a release.
const OWNERS: &[&str] = &[
    "@ordin/screens",
    "ordin-agent-folders",
    "ordin-agent-model",
    "ordin-approval",
    "ordin-brand",
    "ordin-catalogue",
    "ordin-checker",
    "ordin-claude-adapter",
    "ordin-command-bridge",
    "ordin-confluence",
    "ordin-craft-toolkit",
    "ordin-crew-orchestrator",
    "ordin-desktop",
    "ordin-files",
    "ordin-format-plugins",
    "ordin-home-search",
    "ordin-hook-relay",
    "ordin-host",
    "ordin-identity",
    "ordin-installers",
    "ordin-jira",
    "ordin-journal",
    "ordin-knowledge",
    "ordin-knowledge-query-client",
    "ordin-localisation",
    "ordin-mcp-front",
    "ordin-memory-service",
    "ordin-organisation-profile",
    "ordin-package-catalogue",
    "ordin-package-inspector",
    "ordin-projects",
    "ordin-push",
    "ordin-resolver",
    "ordin-rules",
    "ordin-run-queue",
    "ordin-runtime-adapter",
    "ordin-server-connector",
    "ordin-service-api",
    "ordin-session-supervisor",
    "ordin-setup",
    "ordin-sharepoint",
    // On by default since DSK-TW-1.
    "ordin-team-knowledge",
    "ordin-types",
    "ordin-usage-planner",
    "ordin-workspaces",
];

/// EH-SEN-2's words, matched as whole words regardless of case.
const TECHNICAL: &[&str] = &[
    "server",
    "request",
    "http",
    "token",
    "administrator",
    "database",
    "api",
    "json",
    "config",
    "null",
    "exception",
    "panic",
    "stack",
];

// spec: ordin-desktop/cross-cutting/ErrorHandling#EH-SEN-1
#[test]
fn every_code_a_desktop_build_returns_has_its_own_sentence() {
    let english = english();
    let missing: Vec<&str> = entries()
        .iter()
        .filter(|entry| OWNERS.contains(&entry.owner_crate))
        .filter(|entry| !english.entries.contains_key(&error_message_id(entry.code)))
        .map(|entry| entry.code)
        .collect();
    assert!(
        missing.is_empty(),
        "{} codes have no en-GB sentence, first: {:?}",
        missing.len(),
        &missing[..missing.len().min(20)]
    );
}

// spec: ordin-desktop/cross-cutting/ErrorHandling#EH-SEN-1
#[test]
fn every_owner_named_is_an_owner_in_the_registry() {
    for owner in OWNERS {
        assert!(
            entries().iter().any(|entry| entry.owner_crate == *owner),
            "{owner} owns no registered code"
        );
    }
}

// spec: ordin-desktop/cross-cutting/ErrorHandling#EH-SEN-2
#[test]
fn no_error_sentence_holds_a_technical_word() {
    let mut found = Vec::new();
    for (id, source) in english()
        .entries
        .iter()
        .filter(|(id, _)| id.starts_with("error-"))
    {
        let text = source.split_once('=').map_or(source.as_str(), |(_, t)| t);
        for word in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
        {
            if TECHNICAL.contains(&word.to_lowercase().as_str()) {
                found.push(format!("{id}: {word}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "technical words in error sentences: {found:?}"
    );
}

// spec: ordin-desktop/cross-cutting/ErrorHandling#EH-SEN-3
#[test]
fn the_fallback_says_what_to_do() {
    let source = english()
        .entries
        .get("error-srv-internal")
        .expect("the fallback");
    assert!(source.contains("ordin couldn't finish that. Try again in a moment"));
}
