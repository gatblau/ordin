//! X2: one test per Gherkin scenario of FormatPlugins.md, named after the scenario in snake_case.

mod common;

use common::*;
use ordin_agent_model::{PartKind, VocabularySet};
use ordin_format_plugins::*;

fn plugins() -> FormatPlugins {
    FormatPlugins::builtin()
}

/// The three vocabularies a written part can cite, as the bundled catalogue would supply them.
fn starting_vocabulary() -> Vec<ordin_agent_model::ModelPart> {
    let one = |axis: &str, term: &str| {
        (
            format!("models/@default/vocabulary/{axis}.yaml"),
            format!(
                "apiVersion: ai.enterprise/v1\nkind: vocabulary\nmetadata:\n  id: urn:ordin:default:vocabulary:{axis}\n  name: {axis}\n  version: 1.0.0\n  owner: ops\naxis: {axis}\nterms:\n  - code: {term}\n    label: {term}\n"
            ),
        )
    };
    let files = [
        one("category", "writing"),
        one("pack-class", "stack"),
        one("audience-role", "analyst"),
    ];
    let entries: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    parts_of(&entries)
}

// Scenario: Happy path — ordin format round trip is lossless
#[test]
fn happy_path_ordin_format_round_trip_is_lossless() {
    // The committed fixture trees of this repository: this crate's own, and the Agent Model's.
    for name in ["parity/format-plugins/ordin", "parity/agent-model/tree"] {
        let tree = load_at(name);
        let mut request = request("acme", "an ordin tree");
        request.vocabulary = vocabulary_of(&tree);
        let plugin = plugins();
        let ordin = plugin.get(FormatId::Ordin);
        let outcome = ordin.read(&tree, &request).expect("the tree reads");
        assert!(!outcome.parts.is_empty(), "{name}");
        let mut writing = write_request(outcome.parts.clone());
        writing.existing = tree.clone();
        writing.starting_vocabulary = starting_vocabulary();
        let written = ordin.write(&writing).expect("the parts write");
        assert!(!written.files.is_empty(), "{name}");
        for (path, bytes) in &written.files {
            let original = tree
                .0
                .get(path)
                .unwrap_or_else(|| panic!("{name}: {path} was not in the tree"));
            assert_eq!(bytes, original, "{name}: {path} is not byte-identical");
        }
        let file_count: usize = outcome.parts.iter().map(|p| p.files.0.len()).sum();
        assert_eq!(written.files.len(), file_count, "{name}");
        assert!(written.loss_report.items.is_empty(), "{name}");
        if name.ends_with("format-plugins/ordin") {
            assert!(outcome.mapping_report.items.is_empty());
            assert_eq!(outcome.mapping_report.format, Some(FormatId::Ordin));
        }
    }
}

// Scenario: Happy path — APM package read with a mapping report
#[test]
fn happy_path_apm_package_read_with_a_mapping_report() {
    let tree = load("apm-basic");
    let (detection, outcome) = plugins()
        .read_any(&tree, &request("acme", "an APM package"))
        .expect("the package reads");
    assert_eq!(
        detection,
        Detection {
            format: FormatId::Apm,
            declared_version: "unversioned".to_owned(),
            version_tested: false,
            agent_count: 1,
        }
    );
    assert_eq!(
        ids(&outcome.parts),
        vec![
            (
                PartKind::Command,
                "urn:ordin:acme:commands:imported:compare-quotes".to_owned()
            ),
            (
                PartKind::Rule,
                "urn:ordin:acme:rules:imported:tone".to_owned()
            ),
            (
                PartKind::WorkspaceProfile,
                "urn:ordin:acme:workspaces:supplier-analyst".to_owned()
            ),
        ]
    );
    let workspace = outcome
        .parts
        .iter()
        .find(|p| p.kind == PartKind::WorkspaceProfile)
        .expect("a workspace");
    let references: Vec<String> = workspace
        .manifest
        .references()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert!(references.contains(&"urn:ordin:acme:rules:imported:tone".to_owned()));
    assert!(references.contains(&"urn:ordin:acme:commands:imported:compare-quotes".to_owned()));
    assert_eq!(
        outcome.package,
        Some(PackageIdentity {
            name: "supplier-analyst".to_owned(),
            version: "1.3.0".to_owned(),
        })
    );
    assert_eq!(
        outcome.mapping_report,
        MappingReport {
            format: Some(FormatId::Apm),
            items: vec![MappingItem {
                source_path: ".apm/hooks/pre-commit.sh".to_owned(),
                construct: "hook".to_owned(),
                reason: UnmappedReason::Executable,
                part_identifier: None,
            }],
        }
    );
    assert_eq!(
        serde_json::to_string(&outcome.mapping_report).expect("serialises"),
        r#"{"format":"apm","items":[{"source_path":".apm/hooks/pre-commit.sh","construct":"hook","reason":"executable","part_identifier":null}]}"#
    );
    assert_parts_consistent(&outcome.parts);
}

// Scenario: Edge — APM round trip through the ordin format
#[test]
fn edge_apm_round_trip_through_the_ordin_format() {
    let tree = load("apm-roundtrip");
    let plugin = plugins();
    let apm = plugin.get(FormatId::Apm);
    let ordin = plugin.get(FormatId::Ordin);
    let request = request("acme", "an APM package");
    let first = apm.read(&tree, &request).expect("APM reads");
    assert!(
        first.mapping_report.items.is_empty(),
        "{:?}",
        first.mapping_report
    );
    assert_eq!(first.parts.len(), 8, "{:?}", ids(&first.parts));
    assert_parts_consistent(&first.parts);

    let to_ordin = ordin
        .write(&write_request(first.parts.clone()))
        .expect("ordin writes");
    let second = ordin
        .read(&SourceTree(to_ordin.files), &request)
        .expect("ordin reads");
    assert!(
        second.mapping_report.items.is_empty(),
        "{:?}",
        second.mapping_report
    );
    assert_eq!(ids(&second.parts), ids(&first.parts));

    let mut back = write_request(second.parts);
    back.package = first.package.clone();
    let written = apm.write(&back).expect("APM writes");
    assert!(
        written.loss_report.items.is_empty(),
        "{:?}",
        written.loss_report
    );
    assert_eq!(written.files, tree.0);
    assert!(written.starting_files.contains_key("apm.yml"));
}

fn workspace_with_fixed_packs() -> Vec<ordin_agent_model::ModelPart> {
    parts_of(&[(
        "models/@acme/workspaces/w/workspace-profile.yaml",
        "apiVersion: ai.enterprise/v1\nkind: workspace-profile\nmetadata:\n  id: urn:ordin:acme:workspaces:w\n  name: W\n  version: 1.0.0\n  owner: ops\nfixedPracticePacks:\n  - urn:ordin:default:practice-packs:stack:go\n",
    )])
}

// Scenario: Edge — write to APM loses the fixed packs
#[test]
fn edge_write_to_apm_loses_the_fixed_packs() {
    let written = plugins()
        .get(FormatId::Apm)
        .write(&write_request(workspace_with_fixed_packs()))
        .expect("APM writes");
    assert_eq!(written.loss_report.format, Some(FormatId::Apm));
    assert_eq!(
        written.loss_report.items,
        vec![LossItem {
            part_identifier: "urn:ordin:acme:workspaces:w".to_owned(),
            kind: PartKind::WorkspaceProfile,
            what: "fixedPracticePacks".to_owned(),
            reason: LossReason::FieldNotHeld,
        }]
    );
    assert_eq!(
        written
            .files
            .get("apm.yml")
            .map(|b| String::from_utf8_lossy(b).into_owned()),
        Some("name: W\nversion: 1.0.0\n".to_owned())
    );
}

// Scenario: Edge — borrowed part to an APM repository
#[test]
fn edge_borrowed_part_to_an_apm_repository() {
    let mut request = write_request(workspace_with_fixed_packs());
    request.borrowed = vec!["urn:ordin:default:skills:quality:codegen".to_owned()];
    let written = plugins()
        .get(FormatId::Apm)
        .write(&request)
        .expect("APM writes");
    let borrowed: Vec<&LossItem> = written
        .loss_report
        .items
        .iter()
        .filter(|i| i.what == "borrowed part")
        .collect();
    assert_eq!(borrowed.len(), 1);
    assert_eq!(borrowed[0].reason, LossReason::BorrowedPart);
    assert_eq!(
        borrowed[0].part_identifier,
        "urn:ordin:default:skills:quality:codegen"
    );
    assert_eq!(borrowed[0].kind, PartKind::Skill);
}

// Scenario: Edge — Claude Code set-up never copies MCP credentials
#[test]
fn edge_claude_code_set_up_never_copies_mcp_credentials() {
    let tree = load("claude");
    let outcome = plugins()
        .get(FormatId::ClaudeCode)
        .read(&tree, &request("acme", "a Claude Code set-up"))
        .expect("the set-up reads");
    let jira = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id == "urn:ordin:acme:tool-bindings:imported:jira")
        .expect("a ToolBinding for jira");
    assert_eq!(jira.kind, PartKind::ToolBinding);
    for part in &outcome.parts {
        for bytes in part.files.0.values() {
            let text = String::from_utf8_lossy(bytes);
            assert!(
                !text.contains("placeholder-not-a-secret"),
                "{}",
                part.manifest_path
            );
            assert!(!text.contains("JIRA_TOKEN"), "{}", part.manifest_path);
        }
    }
    let env_item = outcome
        .mapping_report
        .items
        .iter()
        .find(|i| i.construct == "env JIRA_TOKEN")
        .expect("an item for the env entry");
    assert_eq!(env_item.reason, UnmappedReason::UnknownField);
    assert_eq!(env_item.source_path, ".mcp.json");
    assert_eq!(
        env_item.part_identifier.as_deref(),
        Some("urn:ordin:acme:tool-bindings:imported:jira")
    );
}

// Scenario: Edge — a single SKILL.md is proposed as an ability
#[test]
fn edge_a_single_skill_md_is_proposed_as_an_ability() {
    let proposal = plugins()
        .propose_kind("notes/SKILL.md", b"Do the thing.\n")
        .expect("a proposal");
    assert_eq!(proposal.kind, PartKind::Skill);
    assert_eq!(proposal.rule, "skill-file");
}

// Scenario: Error — unrecognised tree
#[test]
fn error_unrecognised_tree() {
    let error = plugins()
        .detect(&tree(&[("README.txt", "hello")]))
        .expect_err("nothing recognises it");
    assert_eq!(error, FormatError::NotRecognised);
    assert_eq!(error.code(), "FMT_NOT_RECOGNISED");
}

// Scenario: Error — write on an import-only format
#[test]
fn error_write_on_an_import_only_format() {
    let error = plugins()
        .get(FormatId::Copilot)
        .write(&write_request(Vec::new()))
        .expect_err("Copilot is read only");
    assert_eq!(
        error,
        FormatError::WriteUnsupported {
            format: FormatId::Copilot
        }
    );
    assert_eq!(error.code(), "FMT_WRITE_UNSUPPORTED");
}

// Scenario: Error — APM write to an untested version
#[test]
fn error_apm_write_to_an_untested_version() {
    let mut request = write_request(workspace_with_fixed_packs());
    request.existing = tree(&[("apm.yml", "name: existing\nversion: 2.0.0\n")]);
    let error = plugins()
        .get(FormatId::Apm)
        .write(&request)
        .expect_err("the declared version is not tested");
    assert_eq!(
        error,
        FormatError::VersionUntested {
            declared: "unversioned".to_owned()
        }
    );
}

// Scenario: Error — apm.yml uses a YAML alias
#[test]
fn error_apm_yml_uses_a_yaml_alias() {
    let tree = tree(&[(
        "apm.yml",
        "name: supplier\nbase: &base 1.0\ncopy: *base\nversion: 1.0.0\n",
    )]);
    let error = plugins()
        .get(FormatId::Apm)
        .read(&tree, &request("acme", "an APM package"))
        .expect_err("an anchor is refused");
    assert_eq!(
        error,
        FormatError::PackageManifestInvalid {
            path: "apm.yml".to_owned(),
            line: Some(2),
        }
    );
}

// Scenario: Tree source — a local APM package resolves through PartSource
#[tokio::test]
async fn tree_source_a_local_apm_package_resolves_through_part_source() {
    let tree = load("apm-basic");
    let (_, outcome) = plugins()
        .read_any(&tree, &request("acme", "an APM package"))
        .expect("the package reads");
    let repository = AgentRepositoryId::new();
    let source = TreeSource::new(
        &outcome,
        repository,
        "local/supplier-analyst",
        &"0".repeat(40),
    )
    .expect("a source");
    let root = source.root().expect("one root");
    assert_eq!(root.id, "urn:ordin:acme:workspaces:supplier-analyst");
    assert_eq!(root.kind, PartKind::WorkspaceProfile);
    assert_eq!(root.repository, "local/supplier-analyst");
    assert_eq!(root.commit, "0".repeat(40));
    let scope = SourceScope {
        home: RepositoryRef::Repository(repository),
        profile: Vec::new(),
        include_bundled: false,
    };
    let found = source
        .resolve(
            &scope,
            PartKind::Rule,
            "urn:ordin:acme:rules:imported:tone",
            None,
        )
        .await
        .expect("resolves");
    assert!(matches!(found, Resolution::Found(_)));
}

// Scenario: Tree source — a tree with two agents has no single root
#[test]
fn tree_source_a_tree_with_two_agents_has_no_single_root() {
    let outcome = read_ordin(&load("ordin-two-agents"));
    let source = TreeSource::new(
        &outcome,
        AgentRepositoryId::new(),
        "local/two",
        &"0".repeat(40),
    )
    .expect("a source");
    assert_eq!(
        source.root().expect_err("two roots"),
        FormatError::RootNotSingle { found: 2 }
    );
}

// Scenario: Report codes — a mapping item converts for the Checker
#[test]
fn report_codes_a_mapping_item_converts_for_the_checker() {
    let item = MappingItem {
        source_path: ".apm/hooks/pre-commit.sh".to_owned(),
        construct: "hook".to_owned(),
        reason: UnmappedReason::Executable,
        part_identifier: None,
    };
    assert_eq!(item.reason.code(), "executable");
    let loss = LossItem {
        part_identifier: "urn:ordin:acme:rules:x".to_owned(),
        kind: PartKind::Rule,
        what: "whole part".to_owned(),
        reason: LossReason::NoPrimitive,
    };
    assert_eq!(loss.reason.code(), "no_primitive");
}

// Scenario: Error — path traversal
#[test]
fn error_path_traversal() {
    let tree = tree(&[("../etc/passwd", "root")]);
    let error = plugins()
        .read_any(&tree, &request("acme", "somewhere"))
        .expect_err("a traversal is refused");
    assert_eq!(
        error,
        FormatError::PathInvalid {
            path: "../etc/passwd".to_owned()
        }
    );
    let _ = VocabularySet::default();
}

// FU-097: an imported workspace profile carries the built-in `imported` category and audience role,
// so the Checker's descriptive-metadata rule is met without the organisation's vocabulary naming them.
#[test]
fn an_imported_workspace_profile_carries_the_built_in_category_and_role() {
    let tree = load("apm-roundtrip");
    let plugin = plugins();
    let outcome = plugin
        .get(FormatId::Apm)
        .read(&tree, &request("acme", "an APM package"))
        .expect("APM reads");
    let profiles: Vec<_> = outcome
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::WorkspaceProfile)
        .collect();
    assert!(!profiles.is_empty());
    for profile in profiles {
        let meta = &profile.manifest.metadata;
        assert_eq!(meta.category, "imported");
        assert_eq!(meta.audience.roles, vec!["imported".to_owned()]);
    }
}
