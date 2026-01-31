//! Per-format behaviour beyond the Gherkin scenarios: layouts, reports, placement and the single-file
//! proposal table (FormatPlugins.md *Internal Logic* 2 to 10).

mod common;

use common::*;
use ordin_agent_model::{PartKind, VocabularyAxis};
use ordin_format_plugins::*;

fn plugins() -> FormatPlugins {
    FormatPlugins::builtin()
}

fn item<'a>(report: &'a MappingReport, construct: &str) -> &'a MappingItem {
    report
        .items
        .iter()
        .find(|i| i.construct == construct)
        .unwrap_or_else(|| panic!("no mapping item {construct}: {:?}", report.items))
}

fn kinds(parts: &[ordin_agent_model::ModelPart]) -> Vec<PartKind> {
    parts.iter().map(|p| p.kind).collect()
}

// ---- identifiers and small types ----

#[test]
fn format_ids_convert_and_report_what_they_can_do() {
    assert_eq!(FormatId::from(AgentFormat::Ordin), FormatId::Ordin);
    assert_eq!(FormatId::from(AgentFormat::Apm), FormatId::Apm);
    assert_eq!(
        FormatId::Ordin.as_repository_format(),
        Some(AgentFormat::Ordin)
    );
    assert_eq!(FormatId::Apm.as_repository_format(), Some(AgentFormat::Apm));
    assert_eq!(FormatId::ClaudeCode.as_repository_format(), None);
    assert_eq!(FormatId::Copilot.as_repository_format(), None);
    assert!(FormatId::Ordin.writable() && FormatId::Apm.writable());
    assert!(!FormatId::ClaudeCode.writable() && !FormatId::Copilot.writable());
    assert_eq!(
        serde_json::to_string(&FormatId::ClaudeCode).unwrap(),
        "\"claude_code\""
    );
}

#[test]
fn reason_codes_are_the_snake_case_spellings() {
    let unmapped = [
        (UnmappedReason::NoEquivalentKind, "no_equivalent_kind"),
        (UnmappedReason::UnknownField, "unknown_field"),
        (UnmappedReason::Executable, "executable"),
        (UnmappedReason::UnrecognisedPath, "unrecognised_path"),
        (UnmappedReason::Unreadable, "unreadable"),
    ];
    for (reason, code) in unmapped {
        assert_eq!(reason.code(), code);
        assert_eq!(
            serde_json::to_string(&reason).unwrap(),
            format!("\"{code}\"")
        );
    }
    let loss = [
        (LossReason::NoPrimitive, "no_primitive"),
        (LossReason::FieldNotHeld, "field_not_held"),
        (LossReason::BorrowedPart, "borrowed_part"),
        (LossReason::VersionUntested, "version_untested"),
    ];
    for (reason, code) in loss {
        assert_eq!(reason.code(), code);
        assert_eq!(
            serde_json::to_string(&reason).unwrap(),
            format!("\"{code}\"")
        );
    }
}

#[test]
fn each_plug_in_names_its_id_and_versions() {
    let plugins = plugins();
    for id in [
        FormatId::Ordin,
        FormatId::Apm,
        FormatId::ClaudeCode,
        FormatId::Copilot,
    ] {
        assert_eq!(plugins.get(id).id(), id);
    }
    let ordin = plugins.get(FormatId::Ordin).versions();
    assert_eq!(ordin.reads, vec!["ai.enterprise/v1".to_owned()]);
    assert_eq!(ordin.writes, vec!["ai.enterprise/v1".to_owned()]);
    let apm = plugins.get(FormatId::Apm).versions();
    assert!(
        apm.reads.is_empty() && apm.writes.is_empty(),
        "the tested list is empty until C-62"
    );
    for id in [FormatId::ClaudeCode, FormatId::Copilot] {
        let versions = plugins.get(id).versions();
        assert_eq!(versions.reads, vec!["unversioned".to_owned()]);
        assert!(versions.writes.is_empty());
    }
}

// ---- detection ----

#[test]
fn detection_lists_every_match_in_priority_order() {
    let mut files = load("ordin").0;
    files.insert("apm.yml".to_owned(), b"name: x\nversion: 1.0.0\n".to_vec());
    files.insert("CLAUDE.md".to_owned(), b"Be brief.\n".to_vec());
    files.insert(
        ".github/copilot-instructions.md".to_owned(),
        b"Be brief.\n".to_vec(),
    );
    let found = plugins().detect(&SourceTree(files)).expect("detected");
    let formats: Vec<FormatId> = found.iter().map(|d| d.format).collect();
    assert_eq!(
        formats,
        vec![
            FormatId::Ordin,
            FormatId::Apm,
            FormatId::ClaudeCode,
            FormatId::Copilot
        ]
    );
    assert_eq!(found[0].declared_version, "ai.enterprise/v1");
    assert!(found[0].version_tested);
    assert_eq!(found[0].agent_count, 1);
    assert_eq!(found[2].agent_count, 1);
    assert_eq!(found[3].agent_count, 1);
}

#[test]
fn detection_counts_agent_files_for_the_import_formats() {
    let claude = plugins().detect(&load("claude")).expect("detected");
    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].format, FormatId::ClaudeCode);
    assert_eq!(claude[0].agent_count, 2, "one agent file and CLAUDE.md");
    assert!(claude[0].version_tested);
    let copilot = plugins().detect(&load("copilot")).expect("detected");
    assert_eq!(copilot[0].format, FormatId::Copilot);
    assert_eq!(
        copilot[0].agent_count, 2,
        "one agent file and the general instructions"
    );
}

#[test]
fn an_apm_yml_that_is_not_a_mapping_is_not_recognised() {
    for text in [
        "- a\n- b\n",
        "just text",
        "a: &x 1\nb: *x\n",
        "a: [unclosed\n",
    ] {
        let error = plugins()
            .detect(&tree(&[("apm.yml", text)]))
            .expect_err("not an APM package");
        assert_eq!(error, FormatError::NotRecognised, "{text}");
    }
    let invalid_utf8 = SourceTree(
        [("apm.yml".to_owned(), vec![0xff, 0xfe])]
            .into_iter()
            .collect(),
    );
    assert_eq!(
        plugins().detect(&invalid_utf8),
        Err(FormatError::NotRecognised)
    );
}

#[test]
fn an_ordin_tree_needs_a_manifest_of_the_known_api_version_under_a_publisher() {
    let other_version =
        "apiVersion: ai.enterprise/v2\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:x\n";
    for entries in [
        vec![("models/@acme/rules/x.yaml", other_version)],
        vec![(
            "rules/x.yaml",
            "apiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:x\n",
        )],
        vec![(
            "models/@Acme/rules/x.yaml",
            "apiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:x\n",
        )],
        vec![("models/@acme", "apiVersion: ai.enterprise/v1\n")],
    ] {
        assert_eq!(
            plugins().detect(&tree(&entries)),
            Err(FormatError::NotRecognised),
            "{entries:?}"
        );
    }
}

// ---- ordin ----

#[test]
fn the_ordin_read_reports_a_refused_manifest_and_an_unowned_file() {
    let tree = tree(&[
        (
            "models/@acme/rules/a.md",
            "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:acme:rules:a\n---\nA\n",
        ),
        (
            "models/@acme/rules/b.md",
            "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:acme:rules:b\n---\nB\n",
        ),
        ("models/@acme/rules/notes.txt", "notes"),
        ("models/@acme/rules/broken.yaml", "apiVersion: ai.enterprise/v1\nkind: [rule\n"),
    ]);
    let outcome = plugins()
        .get(FormatId::Ordin)
        .read(&tree, &request("acme", "x"))
        .unwrap();
    assert_eq!(outcome.parts.len(), 2);
    let broken = outcome
        .mapping_report
        .items
        .iter()
        .find(|i| i.source_path.ends_with("broken.yaml"))
        .expect("the refused manifest");
    assert_eq!(broken.reason, UnmappedReason::Unreadable);
    assert_eq!(broken.construct, "AM_MANIFEST_MALFORMED");
    let unowned = outcome
        .mapping_report
        .items
        .iter()
        .find(|i| i.source_path.ends_with("notes.txt"))
        .expect("the unowned file");
    assert_eq!(unowned.reason, UnmappedReason::UnrecognisedPath);
}

#[test]
fn a_changed_ordin_part_is_rendered_with_its_extra_files_beside_it() {
    let mut outcome = read_ordin(&load("ordin"));
    let skill = outcome
        .parts
        .iter_mut()
        .find(|p| p.kind == PartKind::Skill)
        .expect("a skill");
    skill.manifest.metadata.name = "Careful review".to_owned();
    let skill = skill.clone();
    let mut request = write_request(vec![skill]);
    request.existing = load("ordin");
    let written = plugins()
        .get(FormatId::Ordin)
        .write(&request)
        .expect("writes");
    let paths: Vec<&str> = written.files.keys().map(String::as_str).collect();
    assert_eq!(
        paths,
        vec![
            "models/@acme/skills/review/SKILL.md",
            "models/@acme/skills/review/scripts/check.sh"
        ]
    );
    let text =
        String::from_utf8(written.files["models/@acme/skills/review/SKILL.md"].clone()).unwrap();
    assert!(text.contains("name: Careful review"), "{text}");
    assert!(
        text.ends_with("---\nReview the change against the checklist.\n"),
        "the original body is kept: {text}"
    );
    assert_eq!(
        written.files["models/@acme/skills/review/scripts/check.sh"],
        b"#!/bin/sh\necho checked\n"
    );
}

#[test]
fn a_body_override_replaces_the_text_of_a_markdown_part() {
    let outcome = read_ordin(&load("ordin"));
    let rule = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id == "urn:ordin:acme:rules:tone")
        .expect("a rule")
        .clone();
    let mut request = write_request(vec![rule]);
    request.existing = load("ordin");
    request.bodies.insert(
        "urn:ordin:acme:rules:tone".to_owned(),
        "Write in plain words.".to_owned(),
    );
    let written = plugins()
        .get(FormatId::Ordin)
        .write(&request)
        .expect("writes");
    let text = String::from_utf8(written.files["models/@acme/rules/tone.md"].clone()).unwrap();
    assert!(text.ends_with("---\nWrite in plain words.\n"), "{text}");
}

#[test]
fn starting_vocabulary_files_travel_with_a_part_that_cites_an_axis() {
    let outcome = read_ordin(&load("ordin"));
    let tone = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id == "urn:ordin:acme:rules:tone")
        .expect("a rule citing a category")
        .clone();
    let categories = outcome
        .parts
        .iter()
        .find(|p| p.kind == PartKind::Vocabulary && p.manifest.metadata.id.ends_with("categories"))
        .expect("the category vocabulary")
        .clone();

    // The target lacks the vocabulary: the bundled one is generated as a starting file.
    let mut request = write_request(vec![tone.clone()]);
    request.starting_vocabulary = vec![categories.clone()];
    let written = plugins()
        .get(FormatId::Ordin)
        .write(&request)
        .expect("writes");
    assert_eq!(
        written
            .starting_files
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["models/@acme/vocabulary/categories.yaml"]
    );
    assert_eq!(written.files.keys().count(), 1);

    // The target already holds it: nothing is added.
    let mut request = write_request(vec![tone.clone()]);
    request.existing = load("ordin");
    request.starting_vocabulary = vec![categories.clone()];
    let written = plugins()
        .get(FormatId::Ordin)
        .write(&request)
        .expect("writes");
    assert!(written.starting_files.is_empty());

    // The vocabulary is among the parts written: nothing is added either.
    let request = write_request(vec![tone.clone(), categories]);
    let written = plugins()
        .get(FormatId::Ordin)
        .write(&request)
        .expect("writes");
    assert!(written.starting_files.is_empty());

    // The bundled catalogue lacks it too: the part is refused.
    let error = plugins()
        .get(FormatId::Ordin)
        .write(&write_request(vec![tone]))
        .expect_err("no vocabulary to start from");
    assert_eq!(
        error,
        FormatError::PartInvalid {
            identifier: "urn:ordin:acme:rules:tone".to_owned(),
            code: "starting_vocabulary_missing".to_owned(),
        }
    );
    let _ = VocabularyAxis::Category;
}

// ---- APM ----

#[test]
fn apm_read_maps_every_path_of_the_layout() {
    let outcome = plugins()
        .get(FormatId::Apm)
        .read(&load("apm-roundtrip"), &request("acme", "an APM package"))
        .expect("reads");
    let by_id: Vec<(PartKind, String)> = ids(&outcome.parts);
    let expected = [
        (
            PartKind::ContextResource,
            "urn:ordin:acme:context-resources:imported:glossary",
        ),
        (
            PartKind::Command,
            "urn:ordin:acme:commands:imported:compare-quotes",
        ),
        (PartKind::Rule, "urn:ordin:acme:rules:imported:go-style"),
        (PartKind::Rule, "urn:ordin:acme:rules:imported:tone"),
        (
            PartKind::OperatingMode,
            "urn:ordin:acme:rules:modes:imported:review",
        ),
        (PartKind::Skill, "urn:ordin:acme:skills:imported:review"),
        (
            PartKind::SystemRole,
            "urn:ordin:acme:workspaces:supplier-analyst:system-role",
        ),
        (
            PartKind::WorkspaceProfile,
            "urn:ordin:acme:workspaces:supplier-analyst",
        ),
    ];
    for (kind, id) in expected {
        assert!(
            by_id.contains(&(kind, id.to_owned())),
            "missing {id}: {by_id:?}"
        );
    }
    let workspace = outcome
        .parts
        .iter()
        .find(|p| p.kind == PartKind::WorkspaceProfile)
        .unwrap();
    assert_eq!(
        workspace.manifest.metadata.description,
        "Compares supplier quotes"
    );
    assert_eq!(workspace.manifest.metadata.version, "1.3.0");
    assert_eq!(workspace.manifest.metadata.owner, "imported");
    assert_eq!(workspace.manifest.metadata.status.as_deref(), Some("draft"));
    let relations: Vec<(String, String)> = workspace
        .manifest
        .references()
        .into_iter()
        .map(|r| (r.relation, r.id))
        .collect();
    for needed in [
        (
            "systemRole",
            "urn:ordin:acme:workspaces:supplier-analyst:system-role",
        ),
        ("defaultMode", "urn:ordin:acme:rules:modes:imported:review"),
        (
            "allowedOperatingModes",
            "urn:ordin:acme:rules:modes:imported:review",
        ),
        ("skills", "urn:ordin:acme:skills:imported:review"),
        (
            "commands",
            "urn:ordin:acme:commands:imported:compare-quotes",
        ),
        (
            "contextResources",
            "urn:ordin:acme:context-resources:imported:glossary",
        ),
        ("defaultRules", "urn:ordin:acme:rules:imported:tone"),
    ] {
        assert!(
            relations.contains(&(needed.0.to_owned(), needed.1.to_owned())),
            "{needed:?} not in {relations:?}"
        );
    }
    let go = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with("go-style"))
        .unwrap();
    assert_eq!(go.manifest.metadata.name, "Go formatting");
    assert_eq!(
        go.manifest.body.get("appliesToFiles"),
        Some(&YamlValue::Str("**/*.go".to_owned()))
    );
    let skill = outcome
        .parts
        .iter()
        .find(|p| p.kind == PartKind::Skill)
        .unwrap();
    assert_eq!(skill.manifest.metadata.name, "Review a change");
    assert_eq!(
        skill.files.0.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["SKILL.md", "scripts/check.sh"]
    );
    assert_eq!(
        skill.manifest_path,
        "models/@acme/skills/imported/review/SKILL.md"
    );
    assert_parts_consistent(&outcome.parts);
    assert_eq!(kinds(&outcome.parts).len(), 8);
}

#[test]
fn apm_read_reports_what_it_cannot_map() {
    let tree = tree(&[
        (
            "apm.yml",
            "name: demo\nversion: 0.1.0\nscripts:\n  build: make\n",
        ),
        (".apm/agents/a-first.agent.md", "First role.\n"),
        (".apm/agents/b-second.agent.md", "Second role.\n"),
        (".apm/skills/orphan/notes.md", "no SKILL.md here\n"),
        (
            ".apm/instructions/bad.instructions.md",
            "---\nname: &a x\n---\nBody\n",
        ),
        (
            ".apm/instructions/list.instructions.md",
            "---\n- a\n---\nBody\n",
        ),
        (
            ".apm/instructions/extra.instructions.md",
            "---\nname: Extra\ntags: x\nhooks: y\n---\nBody\n",
        ),
        (".apm/prompts/nested/deep.prompt.md", "deep"),
        (".apm/unknown/file.txt", "x"),
        ("README.md", "readme"),
    ]);
    let outcome = plugins()
        .get(FormatId::Apm)
        .read(&tree, &request("acme", "x"))
        .expect("reads");
    let report = &outcome.mapping_report;
    assert_eq!(
        item(report, "apm.yml key scripts").reason,
        UnmappedReason::UnknownField
    );
    assert_eq!(
        item(report, "agent").source_path,
        ".apm/agents/b-second.agent.md"
    );
    assert_eq!(
        item(report, "agent").reason,
        UnmappedReason::NoEquivalentKind
    );
    let orphan = report
        .items
        .iter()
        .find(|i| i.source_path.contains("orphan"))
        .unwrap();
    assert_eq!(orphan.reason, UnmappedReason::UnrecognisedPath);
    let refused = item(report, "YAML_FEATURE_REFUSED");
    assert_eq!(refused.reason, UnmappedReason::Unreadable);
    assert_eq!(
        refused.part_identifier.as_deref(),
        Some("urn:ordin:acme:rules:imported:bad")
    );
    assert_eq!(
        item(report, "front matter not a mapping").reason,
        UnmappedReason::Unreadable
    );
    assert_eq!(
        item(report, "front matter key tags").reason,
        UnmappedReason::UnknownField
    );
    assert_eq!(item(report, "hook").reason, UnmappedReason::Executable);
    let others = report
        .items
        .iter()
        .filter(|i| i.reason == UnmappedReason::UnrecognisedPath)
        .count();
    assert_eq!(others, 4, "{:?}", report.items);
    // The part from a file whose front matter was refused is still created, with the whole text as body.
    let bad = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with(":bad"))
        .expect("the part is created");
    let text = String::from_utf8_lossy(&bad.files.0["bad.md"]).into_owned();
    assert!(text.contains("name: &a x"), "{text}");
    assert_parts_consistent(&outcome.parts);
}

#[test]
fn apm_read_numbers_colliding_identifiers_in_path_order() {
    let tree = tree(&[
        ("apm.yml", "name: demo\nversion: 0.1.0\n"),
        (".apm/instructions/Tone.instructions.md", "Upper.\n"),
        (".apm/instructions/tone.instructions.md", "Lower.\n"),
        (".apm/instructions/tone-2.instructions.md", "Third.\n"),
    ]);
    let outcome = plugins()
        .get(FormatId::Apm)
        .read(&tree, &request("acme", "x"))
        .unwrap();
    let mut rules: Vec<String> = outcome
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::Rule)
        .map(|p| p.manifest.metadata.id.clone())
        .collect();
    rules.sort();
    assert_eq!(
        rules,
        vec![
            "urn:ordin:acme:rules:imported:tone".to_owned(),
            "urn:ordin:acme:rules:imported:tone-2".to_owned(),
            "urn:ordin:acme:rules:imported:tone-3".to_owned(),
        ]
    );
}

#[test]
fn apm_read_leaves_unusable_files_out_with_a_report_item() {
    let mut files = tree(&[("apm.yml", "name: demo\nversion: 0.1.0\n")]).0;
    files.insert(
        ".apm/instructions/binary.instructions.md".to_owned(),
        vec![0xff, 0xfe, 0x00],
    );
    files.insert(
        ".apm/instructions/huge.instructions.md".to_owned(),
        vec![b'a'; MAX_FILE_BYTES + 1],
    );
    files.insert(
        ".apm/instructions/!!!.instructions.md".to_owned(),
        b"text".to_vec(),
    );
    let outcome = plugins()
        .get(FormatId::Apm)
        .read(&SourceTree(files), &request("acme", "x"))
        .expect("reads");
    assert_eq!(outcome.parts.len(), 1, "only the workspace");
    let unreadable = outcome
        .mapping_report
        .items
        .iter()
        .filter(|i| i.reason == UnmappedReason::Unreadable)
        .count();
    assert_eq!(unreadable, 2);
    assert!(outcome
        .mapping_report
        .items
        .iter()
        .any(|i| i.reason == UnmappedReason::UnrecognisedPath && i.source_path.contains("!!!")));
}

#[test]
fn apm_manifest_refusals_carry_the_path_and_line() {
    let read = |text: &str| {
        plugins()
            .get(FormatId::Apm)
            .read(&tree(&[("apm.yml", text)]), &request("acme", "x"))
            .expect_err("refused")
    };
    let invalid = |line| FormatError::PackageManifestInvalid {
        path: "apm.yml".to_owned(),
        line,
    };
    assert_eq!(read("name: a\nversion: [1\n"), invalid(Some(3)));
    assert_eq!(read("- a\n"), invalid(None));
    assert_eq!(read("version: 1.0.0\n"), invalid(None));
    assert_eq!(read("name: a\n"), invalid(None));
    assert_eq!(read("name: [a]\nversion: 1.0.0\n"), invalid(None));
    assert_eq!(read("name: \"\"\nversion: 1.0.0\n"), invalid(None));
    assert_eq!(read("name: \"!!!\"\nversion: 1.0.0\n"), invalid(None));
    let mut files = tree(&[("apm.yml", "name: a\nversion: 1.0.0\n")]).0;
    files.insert("apm.yml".to_owned(), vec![b'a'; MAX_FILE_BYTES + 1]);
    assert_eq!(
        plugins()
            .get(FormatId::Apm)
            .read(&SourceTree(files), &request("acme", "x"))
            .err(),
        Some(invalid(None))
    );
    let missing = plugins()
        .get(FormatId::Apm)
        .read(&tree(&[("other.txt", "x")]), &request("acme", "x"));
    assert_eq!(missing.err(), Some(invalid(None)));
    let bytes = SourceTree([("apm.yml".to_owned(), vec![0xff])].into_iter().collect());
    assert_eq!(
        plugins()
            .get(FormatId::Apm)
            .read(&bytes, &request("acme", "x"))
            .err(),
        Some(invalid(None))
    );
}

#[test]
fn a_publisher_that_cannot_mint_is_refused_by_every_read_plug_in() {
    for (format, name) in [
        (FormatId::Apm, "apm-basic"),
        (FormatId::ClaudeCode, "claude"),
        (FormatId::Copilot, "copilot"),
    ] {
        let error = plugins()
            .get(format)
            .read(&load(name), &request("Acme Corp", "x"))
            .expect_err("a bad publisher");
        assert_eq!(
            error,
            FormatError::PartInvalid {
                identifier: "Acme Corp".to_owned(),
                code: "AM_ID_INVALID".to_owned(),
            },
            "{format:?}"
        );
    }
}

#[test]
fn apm_write_places_each_kind_by_its_loading_mode() {
    let tree = tree(&[
        ("models/@acme/rules/r.md", "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:acme:rules:r\n  name: R\n---\nRule text.\n"),
        ("models/@acme/rules/event.md", "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:acme:rules:event\n  loading: event\n---\nOn event.\n"),
        ("models/@acme/rules/control-loops/control-loop-cspa.md", "---\napiVersion: ai.enterprise/v1\nkind: control-loop\nmetadata:\n  id: urn:ordin:acme:rules:control-loops:cspa\n---\nLoop.\n"),
        ("models/@acme/overlays/style/plain.md", "---\napiVersion: ai.enterprise/v1\nkind: style-overlay\nmetadata:\n  id: urn:ordin:acme:overlays:style:plain\n---\nPlain.\n"),
        ("models/@acme/commands/c.md", "---\napiVersion: ai.enterprise/v1\nkind: command\nmetadata:\n  id: urn:ordin:acme:commands:c\n  name: Custom name\n  description: Does c\n---\nDo c.\n"),
        ("models/@acme/workspaces/w/system-role.md", "---\napiVersion: ai.enterprise/v1\nkind: system-role\nmetadata:\n  id: urn:ordin:acme:workspaces:w:system-role\n---\nRole.\n"),
        ("models/@acme/rules/standalone-role.md", "---\napiVersion: ai.enterprise/v1\nkind: system-role\nmetadata:\n  id: urn:ordin:acme:roles:standalone\n---\nStandalone.\n"),
        ("models/@acme/capabilities/cap.yaml", "apiVersion: ai.enterprise/v1\nkind: capability\nmetadata:\n  id: urn:ordin:acme:capabilities:cap\n"),
        ("models/@acme/rules/a/x.md", "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:acme:rules:a:x\n---\nA x.\n"),
        ("models/@acme/rules/b/x.md", "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:acme:rules:b:x\n---\nB x.\n"),
    ]);
    let mut parts = read_ordin(&tree).parts;
    assert_eq!(parts.len(), 10, "{:?}", ids(&parts));
    // The Agent Model reads no brand overlay from a file; a part of that kind can still be written.
    let mut brand = parts
        .iter()
        .find(|p| p.kind == PartKind::StyleOverlay)
        .expect("an overlay")
        .clone();
    brand.kind = PartKind::BrandOverlay;
    brand.manifest.metadata.id = "urn:ordin:acme:overlays:brand:house".to_owned();
    parts.push(brand);
    let mut request = write_request(parts);
    request.package = Some(PackageIdentity {
        name: "demo".to_owned(),
        version: "0.2.0".to_owned(),
    });
    request.bodies.insert(
        "urn:ordin:acme:rules:r".to_owned(),
        "Edited rule text.".to_owned(),
    );
    let written = plugins()
        .get(FormatId::Apm)
        .write(&request)
        .expect("writes");
    let paths: Vec<&str> = written.files.keys().map(String::as_str).collect();
    for expected in [
        "apm.yml",
        ".apm/instructions/r.instructions.md",
        ".apm/instructions/cspa.instructions.md",
        ".apm/instructions/plain.instructions.md",
        ".apm/prompts/c.prompt.md",
        ".apm/agents/w.agent.md",
        ".apm/instructions/standalone.instructions.md",
        ".apm/instructions/x.instructions.md",
        ".apm/instructions/b-x.instructions.md",
    ] {
        assert!(paths.contains(&expected), "{expected} not in {paths:?}");
    }
    assert_eq!(written.files["apm.yml"], b"name: demo\nversion: 0.2.0\n");
    assert_eq!(
        String::from_utf8_lossy(&written.files[".apm/instructions/r.instructions.md"]),
        "Edited rule text."
    );
    assert_eq!(
        String::from_utf8_lossy(&written.files[".apm/prompts/c.prompt.md"]),
        "---\nname: Custom name\ndescription: Does c\n---\nDo c.\n"
    );
    let losses: Vec<(&str, &str, LossReason)> = written
        .loss_report
        .items
        .iter()
        .map(|i| (i.part_identifier.as_str(), i.what.as_str(), i.reason))
        .collect();
    for expected in [
        (
            "urn:ordin:acme:rules:control-loops:cspa",
            "kind semantics",
            LossReason::FieldNotHeld,
        ),
        (
            "urn:ordin:acme:overlays:style:plain",
            "kind semantics",
            LossReason::FieldNotHeld,
        ),
        (
            "urn:ordin:acme:overlays:brand:house",
            "whole part",
            LossReason::NoPrimitive,
        ),
        (
            "urn:ordin:acme:capabilities:cap",
            "whole part",
            LossReason::NoPrimitive,
        ),
        (
            "urn:ordin:acme:rules:event",
            "whole part",
            LossReason::NoPrimitive,
        ),
    ] {
        assert!(losses.contains(&expected), "{expected:?} not in {losses:?}");
    }
    assert_eq!(written.loss_report.format, Some(FormatId::Apm));
}

#[test]
fn apm_write_keeps_one_workspace_and_the_package_identity() {
    let tree = tree(&[
        ("models/@acme/workspaces/a/workspace-profile.yaml", "apiVersion: ai.enterprise/v1\nkind: workspace-profile\nmetadata:\n  id: urn:ordin:acme:workspaces:a\n  name: 1.10\n  version: 1.10\n"),
        ("models/@acme/workspaces/b/workspace-profile.yaml", "apiVersion: ai.enterprise/v1\nkind: workspace-profile\nmetadata:\n  id: urn:ordin:acme:workspaces:b\n  name: B\n  version: 1.0.0\n"),
    ]);
    let parts = read_ordin(&tree).parts;
    let written = plugins()
        .get(FormatId::Apm)
        .write(&write_request(parts))
        .expect("writes");
    assert_eq!(
        written.files["apm.yml"],
        b"name: \"1.10\"\nversion: \"1.10\"\n"
    );
    assert_eq!(written.loss_report.items.len(), 1);
    assert_eq!(
        written.loss_report.items[0].part_identifier,
        "urn:ordin:acme:workspaces:b"
    );
    assert_eq!(written.loss_report.items[0].reason, LossReason::NoPrimitive);

    // No workspace and no package: no apm.yml. A package alone is enough to write one.
    let none = plugins()
        .get(FormatId::Apm)
        .write(&write_request(Vec::new()))
        .unwrap();
    assert!(none.files.is_empty() && none.starting_files.is_empty());
    let mut only_package = write_request(Vec::new());
    only_package.package = Some(PackageIdentity {
        name: "p".to_owned(),
        version: "2.0.0".to_owned(),
    });
    let written = plugins().get(FormatId::Apm).write(&only_package).unwrap();
    assert_eq!(written.files["apm.yml"], b"name: p\nversion: 2.0.0\n");
    // A repository that has no apm.yml is not an untested version: the starting file is the apm.yml.
    let mut existing = only_package.clone();
    existing.existing = self::tree(&[(".apm/instructions/x.instructions.md", "x")]);
    let written = plugins().get(FormatId::Apm).write(&existing).unwrap();
    assert!(written.starting_files.contains_key("apm.yml"));
}

#[test]
fn apm_write_names_borrowed_parts_of_every_spelling() {
    let mut request = write_request(Vec::new());
    request.borrowed = vec![
        "urn:ordin:default:rules:layer:sql".to_owned(),
        "urn:ordin:default:overlays:style:plain".to_owned(),
        "@default/skills/codegen".to_owned(),
        "@default".to_owned(),
        "not an identifier".to_owned(),
    ];
    let written = plugins().get(FormatId::Apm).write(&request).unwrap();
    let kinds: Vec<PartKind> = written.loss_report.items.iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        vec![
            PartKind::Rule,
            PartKind::StyleOverlay,
            PartKind::Skill,
            PartKind::Rule,
            PartKind::Rule
        ]
    );
}

// ---- Claude Code ----

#[test]
fn claude_code_read_maps_the_setup() {
    let outcome = plugins()
        .get(FormatId::ClaudeCode)
        .read(&load("claude"), &request("acme", "a Claude Code set-up"))
        .expect("reads");
    let have = ids(&outcome.parts);
    for (kind, id) in [
        (PartKind::Rule, "urn:ordin:acme:rules:imported:claude-md"),
        (PartKind::Rule, "urn:ordin:acme:rules:imported:style"),
        (PartKind::Command, "urn:ordin:acme:commands:imported:ship"),
        (PartKind::Skill, "urn:ordin:acme:skills:imported:lint"),
        (
            PartKind::SystemRole,
            "urn:ordin:acme:workspaces:code-reviewer:system-role",
        ),
        (
            PartKind::WorkspaceProfile,
            "urn:ordin:acme:workspaces:code-reviewer",
        ),
        (
            PartKind::ToolBinding,
            "urn:ordin:acme:tool-bindings:imported:read",
        ),
        (
            PartKind::ToolBinding,
            "urn:ordin:acme:tool-bindings:imported:grep",
        ),
        (
            PartKind::ToolBinding,
            "urn:ordin:acme:tool-bindings:imported:jira",
        ),
        (
            PartKind::ToolBinding,
            "urn:ordin:acme:tool-bindings:imported:wiki",
        ),
    ] {
        assert!(
            have.contains(&(kind, id.to_owned())),
            "{id} not in {have:?}"
        );
    }
    let claude_md = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with("claude-md"))
        .unwrap();
    let text = String::from_utf8_lossy(&claude_md.files.0["claude-md.md"]).into_owned();
    assert!(
        text.contains("@docs/style.md") && text.contains("@docs/missing.md"),
        "{text}"
    );
    let skill = outcome
        .parts
        .iter()
        .find(|p| p.kind == PartKind::Skill)
        .unwrap();
    assert!(skill.files.0.contains_key("references/rules.md"));
    let jira = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with("jira"))
        .unwrap();
    let bindings = jira.manifest.tool_bindings().expect("bindings read back");
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].binding_type, "mcp");
    assert_eq!(bindings[0].server_type.as_deref(), Some("stdio"));
    assert_eq!(bindings[0].transport.as_deref(), Some("stdio"));
    let wiki = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with("wiki"))
        .unwrap();
    let bindings = wiki.manifest.tool_bindings().unwrap();
    assert_eq!(bindings[0].server_type.as_deref(), Some("http"));
    assert_eq!(bindings[0].transport.as_deref(), Some("sse"));
    let read = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with("imported:read"))
        .unwrap();
    assert_eq!(
        read.manifest.tool_bindings().unwrap()[0].binding_type,
        "builtin"
    );
    let workspace = outcome
        .parts
        .iter()
        .find(|p| p.kind == PartKind::WorkspaceProfile)
        .unwrap();
    let referenced: Vec<String> = workspace
        .manifest
        .references()
        .into_iter()
        .map(|r| r.id)
        .collect();
    for needed in [
        "urn:ordin:acme:rules:imported:style",
        "urn:ordin:acme:skills:imported:lint",
        "urn:ordin:acme:commands:imported:ship",
        "urn:ordin:acme:tool-bindings:imported:read",
        "urn:ordin:acme:tool-bindings:imported:jira",
        "urn:ordin:acme:workspaces:code-reviewer:system-role",
    ] {
        assert!(
            referenced.contains(&needed.to_owned()),
            "{needed} not in {referenced:?}"
        );
    }
    let report = &outcome.mapping_report;
    assert_eq!(
        item(report, "@docs/missing.md").reason,
        UnmappedReason::UnrecognisedPath
    );
    assert_eq!(item(report, "hook").source_path, ".claude/settings.json");
    assert_eq!(item(report, "hook").reason, UnmappedReason::Executable);
    assert_eq!(
        item(report, "front matter key model").reason,
        UnmappedReason::UnknownField
    );
    assert_eq!(
        item(report, "headers Authorization")
            .part_identifier
            .as_deref(),
        Some("urn:ordin:acme:tool-bindings:imported:wiki")
    );
    assert_eq!(
        item(report, "mcp server key args").reason,
        UnmappedReason::UnknownField
    );
    assert_eq!(item(report, "file").source_path, ".claude/notes.txt");
    assert_parts_consistent(&outcome.parts);
}

#[test]
fn claude_code_without_agent_files_gets_one_workspace_named_after_the_source() {
    let outcome = plugins()
        .get(FormatId::ClaudeCode)
        .read(
            &load("claude-plain"),
            &request("acme", "a Claude Code set-up"),
        )
        .expect("reads");
    assert_eq!(
        ids(&outcome.parts),
        vec![
            (
                PartKind::Rule,
                "urn:ordin:acme:rules:imported:claude-md".to_owned()
            ),
            (
                PartKind::WorkspaceProfile,
                "urn:ordin:acme:workspaces:a-claude-code-set-up".to_owned()
            ),
        ]
    );
    // A label that cannot mint falls back to a fixed name.
    let fallback = plugins()
        .get(FormatId::ClaudeCode)
        .read(&load("claude-plain"), &request("acme", "!!!"))
        .unwrap();
    assert!(ids(&fallback.parts).contains(&(
        PartKind::WorkspaceProfile,
        "urn:ordin:acme:workspaces:imported".to_owned()
    )));
}

#[test]
fn claude_code_reports_unreadable_and_odd_mcp_and_settings_files() {
    let tree = tree(&[
        (
            "CLAUDE.md",
            "@./docs/a.md\n@../outside.md\n@docs/a.md\n@two words\nplain\n",
        ),
        ("docs/a.md", "A.\n"),
        (".mcp.json", "{ not json"),
        (".claude/settings.json", "{\"permissions\": {}}"),
        (".claude/settings.local.json", "{\"hooks\": {}}"),
        (".claude/skills/orphan/extra.md", "no skill"),
        (".claude/commands/deep/inner.md", "deep"),
    ]);
    let outcome = plugins()
        .get(FormatId::ClaudeCode)
        .read(&tree, &request("acme", "x"))
        .unwrap();
    let report = &outcome.mapping_report;
    assert_eq!(
        item(report, "JSON object").reason,
        UnmappedReason::Unreadable
    );
    assert_eq!(
        item(report, "settings").source_path,
        ".claude/settings.json"
    );
    assert_eq!(
        item(report, "hook").source_path,
        ".claude/settings.local.json"
    );
    assert_eq!(
        item(report, "@../outside.md").reason,
        UnmappedReason::UnrecognisedPath
    );
    assert_eq!(
        report
            .items
            .iter()
            .filter(|i| i.reason == UnmappedReason::UnrecognisedPath)
            .count(),
        3,
        "{:?}",
        report.items
    );
    let rules: Vec<&str> = outcome
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::Rule)
        .map(|p| p.manifest.metadata.id.as_str())
        .collect();
    assert_eq!(
        rules,
        vec![
            "urn:ordin:acme:rules:imported:a",
            "urn:ordin:acme:rules:imported:claude-md"
        ]
    );
}

#[test]
fn claude_code_mcp_files_with_unusual_shapes_are_reported() {
    let read = |json: &str| {
        plugins()
            .get(FormatId::ClaudeCode)
            .read(
                &tree(&[(".mcp.json", json), ("CLAUDE.md", "x\n")]),
                &request("acme", "x"),
            )
            .unwrap()
    };
    let outcome = read("{\"mcpServers\": [], \"other\": 1, \"hooks\": {}}");
    assert_eq!(
        item(&outcome.mapping_report, "mcpServers").reason,
        UnmappedReason::Unreadable
    );
    assert_eq!(
        item(&outcome.mapping_report, "key other").reason,
        UnmappedReason::UnknownField
    );
    assert_eq!(
        item(&outcome.mapping_report, "hook").reason,
        UnmappedReason::Executable
    );
    let outcome = read(
        "{\"mcpServers\": {\"a\": 1, \"b\": {\"env\": \"inline\", \"cwd\": \"/x\"}, \"!!!\": {}}}",
    );
    let report = &outcome.mapping_report;
    assert_eq!(
        item(report, "mcp server a").reason,
        UnmappedReason::Unreadable
    );
    assert_eq!(item(report, "env").reason, UnmappedReason::UnknownField);
    assert_eq!(
        item(report, "mcp server key cwd").reason,
        UnmappedReason::UnknownField
    );
    assert_eq!(
        item(report, "mcp server !!!").reason,
        UnmappedReason::UnrecognisedPath
    );
    let outcome = read("[1]");
    assert_eq!(
        item(&outcome.mapping_report, "JSON object").reason,
        UnmappedReason::Unreadable
    );
    let outcome = read("{}");
    assert!(!outcome
        .mapping_report
        .items
        .iter()
        .any(|i| i.source_path == ".mcp.json"));
    let mut files = tree(&[("CLAUDE.md", "x\n")]).0;
    files.insert(".mcp.json".to_owned(), vec![b' '; MAX_FILE_BYTES + 1]);
    let outcome = plugins()
        .get(FormatId::ClaudeCode)
        .read(&SourceTree(files), &request("acme", "x"))
        .unwrap();
    assert_eq!(
        item(&outcome.mapping_report, "file larger than 1 MiB").reason,
        UnmappedReason::Unreadable
    );
}

// ---- Copilot ----

#[test]
fn copilot_read_maps_the_setup() {
    let outcome = plugins()
        .get(FormatId::Copilot)
        .read(&load("copilot"), &request("acme", "a Copilot set-up"))
        .expect("reads");
    let have = ids(&outcome.parts);
    for (kind, id) in [
        (
            PartKind::Rule,
            "urn:ordin:acme:rules:imported:copilot-instructions",
        ),
        (PartKind::Rule, "urn:ordin:acme:rules:imported:rust"),
        (
            PartKind::Command,
            "urn:ordin:acme:commands:imported:release",
        ),
        (
            PartKind::OperatingMode,
            "urn:ordin:acme:rules:modes:imported:plan",
        ),
        (
            PartKind::SystemRole,
            "urn:ordin:acme:workspaces:tester:system-role",
        ),
        (
            PartKind::WorkspaceProfile,
            "urn:ordin:acme:workspaces:tester",
        ),
        (
            PartKind::ToolBinding,
            "urn:ordin:acme:tool-bindings:imported:codebase",
        ),
        (
            PartKind::ToolBinding,
            "urn:ordin:acme:tool-bindings:imported:search",
        ),
    ] {
        assert!(
            have.contains(&(kind, id.to_owned())),
            "{id} not in {have:?}"
        );
    }
    let rust = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with("rust"))
        .unwrap();
    assert_eq!(
        rust.manifest.body.get("appliesToFiles"),
        Some(&YamlValue::Str("**/*.rs".to_owned()))
    );
    assert!(
        !rust.manifest.body.contains_key("appliesTo"),
        "the key is appliesToFiles, not appliesTo"
    );
    assert_eq!(
        outcome.mapping_report.items,
        vec![MappingItem {
            source_path: ".github/workflows/ci.yml".to_owned(),
            construct: "file".to_owned(),
            reason: UnmappedReason::UnrecognisedPath,
            part_identifier: None,
        }]
    );
    assert_parts_consistent(&outcome.parts);
}

#[test]
fn copilot_without_agent_files_gets_one_workspace_named_after_the_source() {
    let tree = tree(&[(".github/copilot-instructions.md", "Be brief.\n")]);
    let outcome = plugins()
        .get(FormatId::Copilot)
        .read(&tree, &request("acme", "a Copilot set-up"))
        .unwrap();
    assert_eq!(
        ids(&outcome.parts)[1],
        (
            PartKind::WorkspaceProfile,
            "urn:ordin:acme:workspaces:a-copilot-set-up".to_owned()
        )
    );
}

#[test]
fn an_unreadable_agent_file_creates_no_workspace() {
    let mut files = tree(&[(".claude/agents/ok.md", "Role.\n")]).0;
    files.insert(".claude/agents/binary.md".to_owned(), vec![0xff, 0xfe]);
    let outcome = plugins()
        .get(FormatId::ClaudeCode)
        .read(&SourceTree(files), &request("acme", "x"))
        .unwrap();
    let workspaces = outcome
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::WorkspaceProfile)
        .count();
    assert_eq!(workspaces, 1);
    assert_eq!(
        item(&outcome.mapping_report, "file not valid UTF-8").reason,
        UnmappedReason::Unreadable
    );
}

// ---- proposals ----

#[test]
fn the_proposal_table_matches_the_first_row_that_fits() {
    let plugins = plugins();
    let cases: [(&str, &str, PartKind, FormatId, &str); 12] = [
        (".claude/skills/x/SKILL.md", "t", PartKind::Skill, FormatId::ClaudeCode, "skill-file"),
        ("notes/go.instructions.md", "t", PartKind::Rule, FormatId::Copilot, "instructions-file"),
        ("CLAUDE.md", "t", PartKind::Rule, FormatId::ClaudeCode, "instructions-file"),
        (".github/copilot-instructions.md", "t", PartKind::Rule, FormatId::Copilot, "instructions-file"),
        ("x/release.prompt.md", "t", PartKind::Command, FormatId::Copilot, "prompt-file"),
        (".claude/commands/ship.md", "t", PartKind::Command, FormatId::ClaudeCode, "prompt-file"),
        ("x/plan.chatmode.md", "t", PartKind::OperatingMode, FormatId::Copilot, "chat-mode-file"),
        ("x/tester.agent.md", "t", PartKind::SystemRole, FormatId::Copilot, "agent-file"),
        (".claude/agents/rev.md", "t", PartKind::SystemRole, FormatId::ClaudeCode, "agent-file"),
        (".apm/prompts/p.prompt.md", "t", PartKind::Command, FormatId::Apm, "prompt-file"),
        ("some/.mcp.json", "{\"mcpServers\": {}}", PartKind::ToolBinding, FormatId::ClaudeCode, "mcp-servers-file"),
        (
            "x/tone.md",
            "---\napiVersion: ai.enterprise/v1\nkind: operating-mode\nmetadata:\n  id: urn:ordin:a:rules:modes:m\n---\nText\n",
            PartKind::OperatingMode,
            FormatId::Ordin,
            "ordin-manifest",
        ),
    ];
    for (path, text, kind, format, rule) in cases {
        let proposal = plugins
            .propose_kind(path, text.as_bytes())
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(
            proposal,
            KindProposal {
                kind,
                from_format: format,
                rule
            },
            "{path}"
        );
    }
}

#[test]
fn a_file_the_table_does_not_fit_is_not_proposable() {
    let plugins = plugins();
    for (path, text) in [
        ("README.txt", "hello"),
        ("settings.json", "{\"a\": 1}"),
        ("settings.json", "{\"mcpServers\": 1}"),
        ("broken.json", "{"),
        ("plain.md", "no front matter"),
        ("old.md", "---\napiVersion: ai.enterprise/v0\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:x\n---\n"),
        ("odd.md", "---\napiVersion: ai.enterprise/v1\nkind: unheard-of\nmetadata:\n  id: urn:ordin:a:rules:x\n---\n"),
    ] {
        assert_eq!(
            plugins.propose_kind(path, text.as_bytes()),
            Err(FormatError::KindNotProposable { path: path.to_owned() }),
            "{path}"
        );
    }
    assert_eq!(
        plugins.propose_kind("../SKILL.md", b"x"),
        Err(FormatError::PathInvalid {
            path: "../SKILL.md".to_owned()
        })
    );
}

// ---- limits ----

#[test]
fn a_tree_over_the_limits_is_refused_by_every_operation() {
    let many: SourceTree = SourceTree(
        (0..=MAX_TREE_FILES)
            .map(|n| (format!("f/{n}.txt"), Vec::new()))
            .collect(),
    );
    let expected = FormatError::TreeTooLarge {
        files: 20_001,
        bytes: 0,
    };
    assert_eq!(plugins().detect(&many), Err(expected.clone()));
    for id in [
        FormatId::Ordin,
        FormatId::Apm,
        FormatId::ClaudeCode,
        FormatId::Copilot,
    ] {
        assert_eq!(
            plugins().get(id).read(&many, &request("acme", "x")).err(),
            Some(expected.clone()),
            "{id:?}"
        );
    }
    let big = SourceTree(
        [(
            "big.bin".to_owned(),
            vec![0u8; (MAX_TREE_BYTES + 1) as usize],
        )]
        .into_iter()
        .collect(),
    );
    assert_eq!(
        plugins().detect(&big),
        Err(FormatError::TreeTooLarge {
            files: 1,
            bytes: MAX_TREE_BYTES + 1
        })
    );
    let mut write = write_request(Vec::new());
    write.existing = many;
    assert_eq!(
        plugins().get(FormatId::Apm).write(&write).err(),
        Some(expected.clone())
    );
    assert_eq!(
        plugins().get(FormatId::Ordin).write(&write).err(),
        Some(expected)
    );
}

#[test]
fn every_kind_of_bad_path_is_refused() {
    for path in ["/abs", "a/../b", "a\\b", "a\0b", ""] {
        let tree = SourceTree([(path.to_owned(), b"x".to_vec())].into_iter().collect());
        assert_eq!(
            plugins().detect(&tree),
            Err(FormatError::PathInvalid {
                path: path.to_owned()
            }),
            "{path:?}"
        );
    }
}
