//! X2: one test per Gherkin scenario of AgentModel.md (and its January 2026 amendment), each named after
//! its scenario; the exact title is the comment above each test. The three `PartSource` scenarios
//! are in `part_source.rs`, built with the `testkit` feature.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ordin_agent_model::*;
use ordin_types::domain::agents::{AgentDraft, DraftJob, DraftOrigin, ReviewDraftState};
use ordin_types::AgentDraftId;

fn parse(path: &str, text: &str) -> Manifest {
    parse_manifest(path, text.as_bytes())
        .expect("the manifest parses")
        .expect("the file is a manifest")
}

fn workspace(body: &str) -> Manifest {
    parse(
        "models/@default/workspaces/w/workspace-profile.yaml",
        &format!(
            "apiVersion: ai.enterprise/v1\nkind: workspace-profile\nmetadata:\n  id: urn:ordin:default:workspaces:w\n{body}"
        ),
    )
}

fn files(entries: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
    entries
        .iter()
        .map(|(p, b)| ((*p).to_owned(), b.to_vec()))
        .collect()
}

fn fixture_dir(name: &str) -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root")
        .join("testdata")
        .join("parity")
        .join("agent-model")
        .join(name)
}

fn fixture_file(relative: &str) -> Vec<u8> {
    let path = fixture_dir("tree").join(relative);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{} is missing: {e}", path.display()))
}

fn draft(name: &str, jobs: &[&str], needs: &[&str], will: &[&str]) -> AgentDraft {
    AgentDraft {
        id: AgentDraftId::new(),
        agent_repository_id: BUNDLED_REPOSITORY_ID,
        name: name.to_owned(),
        purpose: "Reviews supplier contracts before signature.".to_owned(),
        jobs: jobs
            .iter()
            .map(|text| DraftJob {
                text: (*text).to_owned(),
                came_from: "interview".to_owned(),
            })
            .collect(),
        tasks: Vec::new(),
        will: will.iter().map(|s| (*s).to_owned()).collect(),
        wont: Vec::new(),
        needs: needs.iter().map(|s| (*s).to_owned()).collect(),
        reused_parts: Vec::new(),
        origin: DraftOrigin::Made,
        branch: "draft/contract-reviewer".to_owned(),
        check_results_ref: None,
        wording_changes: Vec::new(),
        field_authors: Vec::new(),
        rehearsal_id: None,
        trial: false,
        pull_request: None,
        state: ReviewDraftState::Draft,
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    }
}

const CODEGEN_EXAMPLE: &str = "---\napiVersion: ai.enterprise/v1\nkind: skill\nmetadata:\n  id: \"urn:ordin:default:skills:quality:codegen\"\n  name: Code generation\n  version: 1.4.0\n  owner: engineering-enablement\n  description: \"Generates a component from its specification.\"\n---\n# Code generation\n1. Read the component spec.\n";

// Scenario: Happy path — a skill manifest is read and placed
#[test]
fn happy_path_a_skill_manifest_is_read_and_placed() {
    let path = "models/@default/skills/quality/codegen/SKILL.md";
    let manifest = parse(path, CODEGEN_EXAMPLE);
    assert_eq!(manifest.api_version, API_VERSION);
    assert_eq!(manifest.authored_kind, "skill");
    assert_eq!(
        manifest.metadata.id,
        "urn:ordin:default:skills:quality:codegen"
    );
    assert_eq!(manifest.metadata.name, "Code generation");
    assert_eq!(manifest.metadata.version, "1.4.0");
    assert_eq!(manifest.metadata.owner, "engineering-enablement");
    assert_eq!(manifest.metadata.status, None);
    assert!(manifest.body.is_empty());
    assert_eq!(manifest.slug(), "quality-codegen");
    assert_eq!(
        part_file_path(PartKind::Skill, &manifest.metadata.id).unwrap(),
        path
    );
    let digest = content_digest(&PartFiles(BTreeMap::from([(
        "SKILL.md".to_owned(),
        CODEGEN_EXAMPLE.as_bytes().to_vec(),
    )])));
    assert_eq!(digest.0.len(), 64);
    assert_eq!(prefixed_digest(&digest), format!("sha256:{}", digest.0));
}

// Scenario: Parity — every frozen … fixture reads
// The scenario's title names the reference project's frozen catalogue; by the user's decision the
// fixtures are ordin's own, committed in testdata/parity/agent-model/tree, so the test is named for
// those (the table-driven comparison is tests/parity.rs).
#[test]
fn parity_every_committed_fixture_reads() {
    let mut tree = BTreeMap::new();
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).expect("the fixture tree is readable") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let relative = path.strip_prefix(base).expect("under the base");
                let key = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(key, std::fs::read(&path).expect("a readable file"));
            }
        }
    }
    let base = fixture_dir("tree");
    walk(&base, &base, &mut tree);
    assert!(!tree.is_empty(), "the fixture tree is empty");
    let first = read_tree(&tree, &VocabularySet::default());
    let vocabularies: Vec<&ModelPart> = first
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::Vocabulary)
        .collect();
    let set = VocabularySet::from_parts(&vocabularies).unwrap();
    let second = read_tree(&tree, &set);
    assert!(second.refused.is_empty(), "{:?}", second.refused);
    let expected: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture_dir("expected").join("tree.json")).expect("expected/tree.json"),
    )
    .expect("JSON");
    let kinds: Vec<(String, serde_json::Value)> = second
        .parts
        .iter()
        .map(|p| {
            (
                p.manifest_path.clone(),
                serde_json::to_value(p.kind).unwrap(),
            )
        })
        .collect();
    let expected_kinds: Vec<(String, serde_json::Value)> = expected["parts"]
        .as_array()
        .expect("parts")
        .iter()
        .map(|p| {
            (
                p["manifestPath"].as_str().unwrap().to_owned(),
                p["kind"].clone(),
            )
        })
        .collect();
    assert_eq!(kinds, expected_kinds);
}

// Scenario: Edge — a file without a manifest marker is not a part
#[test]
fn edge_a_file_without_a_manifest_marker_is_not_a_part() {
    assert_eq!(
        parse_manifest(
            "models/@default/workspaces/README.md",
            b"# Workspaces\n\nEach folder holds one agent.\n"
        ),
        Ok(None)
    );
}

// Scenario: Edge — the digest ignores file order and timestamps
#[test]
fn edge_the_digest_ignores_file_order_and_timestamps() {
    let mut first = BTreeMap::new();
    first.insert("SKILL.md".to_owned(), b"skill".to_vec());
    first.insert("scripts/run.sh".to_owned(), b"echo".to_vec());
    let mut second = BTreeMap::new();
    second.insert("scripts/run.sh".to_owned(), b"echo".to_vec());
    second.insert("SKILL.md".to_owned(), b"skill".to_vec());
    let (a, b) = (PartFiles(first), PartFiles(second));
    assert_eq!(content_digest(&a), content_digest(&b));
    // The archive carries no time: every mtime field is zero.
    let archive = canonical_archive(&a);
    assert_eq!(&archive[136..148], b"00000000000\0");
}

// Scenario: Edge — a file shared by two manifests in one directory belongs to no part
#[test]
fn edge_a_file_shared_by_two_manifests_in_one_directory_belongs_to_no_part() {
    let rule = |id: &str| {
        format!(
            "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: {id}\n---\nText.\n"
        )
    };
    let a = rule("urn:ordin:default:rules:d:a");
    let b = rule("urn:ordin:default:rules:d:b");
    let reading = read_tree(
        &files(&[
            ("models/@default/rules/d/a.md", a.as_bytes()),
            ("models/@default/rules/d/b.md", b.as_bytes()),
            ("models/@default/rules/d/notes.txt", b"shared"),
        ]),
        &VocabularySet::default(),
    );
    assert_eq!(
        reading.unowned_files,
        vec!["models/@default/rules/d/notes.txt".to_owned()]
    );
    assert_eq!(reading.parts.len(), 2);
    assert!(reading.parts.iter().all(|p| p.files.0.len() == 1));
}

// Scenario: Edge — absent fixedPracticePacks leaves the file untouched
#[test]
fn edge_absent_fixedpracticepacks_leaves_the_file_untouched() {
    let relative = "models/@default/workspaces/cspa-431/workspace-profile.yaml";
    let bytes = fixture_file(relative);
    let reading = read_tree(&files(&[(relative, &bytes)]), &VocabularySet::default());
    let part = &reading.parts[0];
    assert_eq!(part.manifest.fixed_practice_packs().unwrap(), vec![]);
    assert!(!part.manifest.body.contains_key("fixedPracticePacks"));
    let own = PartFiles(BTreeMap::from([(
        "workspace-profile.yaml".to_owned(),
        bytes.clone(),
    )]));
    assert_eq!(part.content_digest, content_digest(&own));
    // Rendering writes no `fixedPracticePacks` key, so the file's bytes are unchanged.
    let rendered = render_part(PartKind::WorkspaceProfile, &part.manifest, None).unwrap();
    assert_eq!(rendered.content, bytes);
}

// Scenario: Edge — an ask entry in fixedPracticePacks
#[test]
fn edge_an_ask_entry_in_fixedpracticepacks() {
    let manifest = workspace(
        "fixedPracticePacks:\n  - urn:ordin:default:practice-packs:stack:go\n  - ask:\n      class: testing\n",
    );
    assert_eq!(
        manifest.fixed_practice_packs().unwrap(),
        vec![
            FixedPackEntry::Pack("urn:ordin:default:practice-packs:stack:go".into()),
            FixedPackEntry::Ask {
                class: Some("testing".into()),
                ids: vec![]
            }
        ]
    );
}

// Scenario: Error — unclosed front matter
#[test]
fn error_unclosed_front_matter() {
    let error =
        parse_manifest("models/@default/rules/x.md", b"---\nkind: rule\nText\n").unwrap_err();
    assert_eq!(error.code(), "AM_MANIFEST_MALFORMED");
}

// Scenario: Error — YAML alias refused
#[test]
fn error_yaml_alias_refused() {
    let error = parse_manifest(
        "models/@default/rules/x.yaml",
        b"apiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:default:rules:x\nbase: &base\n  a: 1\ncopy: *base\n",
    )
    .unwrap_err();
    assert_eq!(
        error,
        AmError::YamlFeatureRefused {
            path: "models/@default/rules/x.yaml".into(),
            feature: "anchor, alias or custom tag"
        }
    );
}

// Scenario: Error — required pack rule with two selectors
#[test]
fn error_required_pack_rule_with_two_selectors() {
    let manifest = workspace(
        "requiredPracticePacks:\n  - atLeast: 1\n    class: stack\n    ids: [urn:ordin:default:practice-packs:stack:go]\n",
    );
    match manifest.required_practice_packs().unwrap_err() {
        AmError::FieldShapeInvalid { key, .. } => assert_eq!(key, "requiredPracticePacks"),
        other => panic!("{other:?}"),
    }
}

// Scenario: Error — no file convention
#[test]
fn error_no_file_convention() {
    assert_eq!(
        part_file_path(PartKind::CrewProfile, "urn:ordin:default:crews:review").unwrap_err(),
        AmError::NoFileConvention {
            kind: PartKind::CrewProfile
        }
    );
}

// Scenario: Error — path traversal in a tree
#[test]
fn error_path_traversal_in_a_tree() {
    // Built in two pieces so no source line spells a parent-folder step.
    let traversal = concat!("models/..", "/secrets.yaml");
    let reading = read_tree(
        &files(&[(traversal, b"apiVersion: ai.enterprise/v1\nkind: rule\n")]),
        &VocabularySet::default(),
    );
    assert!(reading.parts.is_empty());
    assert_eq!(
        reading.refused,
        vec![(
            traversal.to_owned(),
            AmError::PathInvalid {
                path: traversal.to_owned()
            }
        )]
    );
}

// Scenario: Edge — a command's argument hint is read
#[test]
fn edge_a_commands_argument_hint_is_read() {
    let relative = "models/@default/commands/compare-quotes.md";
    let manifest = parse(
        relative,
        std::str::from_utf8(&fixture_file(relative)).unwrap(),
    );
    assert_eq!(
        manifest.argument_hint().unwrap().as_deref(),
        Some("<suppliers> <quarter>")
    );
}

// Scenario: Error — argument hint of the wrong shape
#[test]
fn error_argument_hint_of_the_wrong_shape() {
    let manifest = parse(
        "models/@default/commands/x.md",
        "---\napiVersion: ai.enterprise/v1\nkind: command\nmetadata:\n  id: urn:ordin:default:commands:x\nargument-hint:\n  - suppliers\n---\n",
    );
    assert_eq!(
        manifest.argument_hint().unwrap_err(),
        AmError::FieldShapeInvalid {
            key: "argument-hint".into(),
            expected: "string up to 200 characters"
        }
    );
}

fn binding(paths: &str) -> Manifest {
    parse(
        "models/@default/tool-bindings/mail.yaml",
        &format!(
            "apiVersion: ai.enterprise/v1\nkind: tool-binding\nmetadata:\n  id: urn:ordin:default:tool-bindings:mail\nbindings:\n  - name: email_send\n    type: mcp\n    permission: write\n    argumentPaths: {paths}\n"
        ),
    )
}

// Scenario: Edge — a tool binding's argument paths
#[test]
fn edge_a_tool_bindings_argument_paths() {
    let bindings = binding(r#"["/to/[]/address", "/attachments/*"]"#)
        .tool_bindings()
        .unwrap();
    assert_eq!(bindings[0].name, "email_send");
    assert_eq!(
        bindings[0].argument_paths,
        vec!["/to/[]/address".to_owned(), "/attachments/*".to_owned()]
    );
}

// Scenario: Error — an argument path that is not a JSON Pointer
#[test]
fn error_an_argument_path_that_is_not_a_json_pointer() {
    assert_eq!(
        binding(r#"["to.address"]"#).tool_bindings().unwrap_err(),
        AmError::FieldShapeInvalid {
            key: "argumentPaths".into(),
            expected: "JSON Pointer list"
        }
    );
}

// Scenario: Happy path — OutputContract sections
#[test]
fn happy_path_outputcontract_sections() {
    let relative = "models/@default/output-contracts/role-dossier.yaml";
    let manifest = parse(
        relative,
        std::str::from_utf8(&fixture_file(relative)).unwrap(),
    );
    let contract = OutputContract::read(&manifest).unwrap();
    let first = &contract.sections()[0];
    assert_eq!(first.name, "role-identity");
    assert!(first.required);
    assert_eq!(first.evidence_threshold, 1);
    assert!(!first.derived);
    let last = contract.sections().last().unwrap();
    assert!(last.derived && !last.required && last.evidence_threshold == 0);
}

// Scenario: Error — an OutputContract section without a name
#[test]
fn error_an_outputcontract_section_without_a_name() {
    let manifest = parse(
        "models/@default/output-contracts/x.yaml",
        "apiVersion: ai.enterprise/v1\nkind: output-contract\nmetadata:\n  id: urn:ordin:default:output-contracts:x\nsections:\n  - name: role-identity\n  - meaning: no name\n",
    );
    assert_eq!(
        OutputContract::read(&manifest).unwrap_err(),
        AmError::FieldShapeInvalid {
            key: "sections".into(),
            expected: "list of sections with a name"
        }
    );
}

fn rate_compare_list() -> WithdrawnListFile {
    WithdrawnListFile {
        list_schema: WITHDRAWN_LIST_SCHEMA.to_owned(),
        entries: vec![WithdrawnFileEntry {
            part: WithdrawnFilePart {
                kind: PartKind::Skill,
                id: "urn:ordin:default:skills:rate-compare".into(),
                version: Some("1.2.0".into()),
                content_hash: None,
            },
            reason: "Gave wrong totals.".into(),
            withdrawn_at: "2026-01-10T09:00:00.000Z".into(),
            replacement: Some(WithdrawnFileReplacement {
                id: "urn:ordin:default:skills:rate-compare".into(),
                version: Some("1.2.1".into()),
            }),
        }],
    }
}

// Scenario: Withdrawn list — round trip
#[test]
fn withdrawn_list_round_trip() {
    let text = render_withdrawn_list(&rate_compare_list());
    let parsed = parse_withdrawn_list(text.as_bytes()).unwrap();
    assert_eq!(parsed, rate_compare_list());
    let again = parse_withdrawn_list(render_withdrawn_list(&parsed).as_bytes()).unwrap();
    assert_eq!(again, parsed);
    // The property over many lists: strings that need quoting, absent options, several entries.
    let awkward = [
        "plain",
        "1.0",
        "yes",
        "a: b",
        "a #b",
        "@legacy/x",
        "tab\there",
        "quote\"s",
        "naïve",
        "",
        "~",
        "line\nbreak",
        "end:",
    ];
    for (i, text) in awkward.iter().enumerate() {
        let mut list = rate_compare_list();
        list.entries[0].reason = (*text).to_owned();
        list.entries[0].part.content_hash = (i % 2 == 0).then(|| "ab".repeat(32));
        if i % 3 == 0 {
            list.entries[0].replacement = None;
        }
        list.entries.push(WithdrawnFileEntry {
            part: WithdrawnFilePart {
                kind: PartKind::Rule,
                id: (*text).to_owned(),
                version: None,
                content_hash: None,
            },
            reason: "r".into(),
            withdrawn_at: (*text).to_owned(),
            replacement: None,
        });
        assert_eq!(
            parse_withdrawn_list(render_withdrawn_list(&list).as_bytes()),
            Ok(list),
            "{text:?}"
        );
    }
    let empty = WithdrawnListFile {
        list_schema: WITHDRAWN_LIST_SCHEMA.to_owned(),
        entries: vec![],
    };
    assert_eq!(
        parse_withdrawn_list(render_withdrawn_list(&empty).as_bytes()),
        Ok(empty)
    );
}

// Scenario: Withdrawn list — wrong schema
#[test]
fn withdrawn_list_wrong_schema() {
    assert_eq!(
        parse_withdrawn_list(b"listSchema: other/v9\nentries: []\n").unwrap_err(),
        AmError::WithdrawnListInvalid {
            reason: "schema",
            entry: None
        }
    );
}

// Scenario: Draft model — fields round trip
#[test]
fn draft_model_fields_round_trip() {
    // The bundled skill as the ordin format writes it.
    let original = "---\napiVersion: ai.enterprise/v1\nkind: skill\nmetadata:\n  id: urn:ordin:default:skills:quality:codegen\n  name: Code generation\n  version: 1.4.0\n  owner: engineering-enablement\n  description: Generates a component from its specification.\n---\n# Code generation\n1. Read the component spec.\n";
    let reading = read_tree(
        &files(&[(
            "models/@default/skills/quality/codegen/SKILL.md",
            original.as_bytes(),
        )]),
        &VocabularySet::default(),
    );
    let part = &reading.parts[0];
    let model = FieldModel;
    let again = model
        .part_from_fields(PartKind::Skill, &model.fields_of(part))
        .unwrap();
    assert_eq!(again.files.0["SKILL.md"], original.as_bytes());
    assert_eq!(again.content_digest, part.content_digest);
    assert_eq!(again.manifest_path, part.manifest_path);
}

// Scenario: Draft model — an agent draft becomes parts
#[test]
fn draft_model_an_agent_draft_becomes_parts() {
    let draft = draft(
        "Contract reviewer",
        &["Read the contract", "Flag risky clauses"],
        &["Contract folder"],
        &[],
    );
    let reused = [ReusedPart {
        kind: PartKind::Rule,
        identifier: "urn:ordin:default:rules:layer:output-language".into(),
    }];
    let generated = [GeneratedPart {
        kind: PartKind::Skill,
        name: "Flag clauses".into(),
        fields: serde_json::json!({ "text": "Flag the clauses.\n" }),
    }];
    let parts = FieldModel
        .agent_parts_from_fields(&draft, "acme", &reused, &generated)
        .unwrap();
    let ids: Vec<(&str, PartKind)> = parts
        .iter()
        .map(|p| (p.manifest.metadata.id.as_str(), p.kind))
        .collect();
    assert_eq!(
        ids,
        vec![
            (
                "urn:ordin:acme:workspaces:contract-reviewer",
                PartKind::WorkspaceProfile
            ),
            (
                "urn:ordin:acme:workspaces:contract-reviewer:system-role",
                PartKind::SystemRole
            ),
            ("urn:ordin:acme:skills:flag-clauses", PartKind::Skill),
        ]
    );
    let workspace = &parts[0].manifest;
    assert_eq!(
        workspace.body["defaultRules"],
        YamlValue::Seq(vec![YamlValue::Str(
            "urn:ordin:default:rules:layer:output-language".into()
        )])
    );
    assert_eq!(
        workspace.body["skills"],
        YamlValue::Seq(vec![YamlValue::Str(
            "urn:ordin:acme:skills:flag-clauses".into()
        )])
    );
    assert_eq!(
        workspace.body["systemRole"],
        YamlValue::Str("urn:ordin:acme:workspaces:contract-reviewer:system-role".into())
    );
    assert_eq!(workspace.metadata.status.as_deref(), Some("draft"));
    assert_eq!(parts[1].manifest.metadata.name, "Contract reviewer role");
    assert_eq!(
        parts[0].manifest_path,
        "models/@acme/workspaces/contract-reviewer/workspace-profile.yaml"
    );
    assert_eq!(
        parts[1].manifest_path,
        "models/@acme/workspaces/contract-reviewer/system-role.md"
    );
}

// Scenario: Draft model — a generated overlay is refused
#[test]
fn draft_model_a_generated_overlay_is_refused() {
    let generated = [GeneratedPart {
        kind: PartKind::BrandOverlay,
        name: "House brand".into(),
        fields: serde_json::Value::Null,
    }];
    assert_eq!(
        FieldModel
            .agent_parts_from_fields(&draft("Writer", &[], &[], &[]), "acme", &[], &generated)
            .unwrap_err(),
        AmError::DraftKindNotComposable {
            kind: PartKind::BrandOverlay
        }
    );
}

// Scenario: Draft model — write-up headings
#[test]
fn draft_model_write_up_headings() {
    let text = FieldModel.render_write_up(&draft(
        "Contract reviewer",
        &["Read the contract"],
        &["Contract folder"],
        &[],
    ));
    let headings: Vec<&str> = text.lines().filter(|l| l.starts_with("## ")).collect();
    assert_eq!(
        headings,
        vec![
            "## role-identity",
            "## responsibilities",
            "## tools-systems"
        ]
    );
    assert!(!text.contains("decisions-owned"));
    assert!(text.ends_with("- Contract folder\n") && !text.ends_with("\n\n"));
}

const LIFECYCLE: &str = "apiVersion: ai.enterprise/v1\nkind: journey\nmetadata:\n  id: urn:ordin:default:journeys:review\n  name: Review\n  version: 1.0.0\n  owner: engineering-enablement\nstages:\n  - handoverPlace:\n      target: designs\n      type: folder\n    id: design\n    name: Design\n    next: []\n    role: designer\n    sendsBackTo: []\n    together: false\n";

// Scenario: A stage names a Slack channel
#[test]
fn a_stage_names_a_slack_channel() {
    let relative = "models/@default/journeys/delivery.yaml";
    let manifest = parse(
        relative,
        std::str::from_utf8(&fixture_file(relative)).unwrap(),
    );
    let journey = manifest.journey().unwrap().unwrap();
    let design = journey.stages.iter().find(|s| s.name == "Design").unwrap();
    assert_eq!(
        design.slack_channel,
        Some(JourneySlackChannel {
            id: "C04ABCDEF12".into(),
            name: "design-reviews".into()
        })
    );
}

// Scenario: A stage without the key
#[test]
fn a_stage_without_the_key() {
    let relative = "models/@default/journeys/handover.yaml";
    let reference_style = parse(
        relative,
        std::str::from_utf8(&fixture_file(relative)).unwrap(),
    );
    let body = reference_style.journey().unwrap().unwrap();
    assert!(!body.seams.is_empty());
    assert!(body.stages.iter().all(|s| s.slack_channel.is_none()));
    let ordin_style = parse("models/@default/journeys/review.yaml", LIFECYCLE);
    let stages = ordin_style.journey().unwrap().unwrap().stages;
    assert!(!stages.is_empty());
    assert!(stages.iter().all(|s| s.slack_channel.is_none()));
}

// Scenario: A malformed slackChannel
#[test]
fn a_malformed_slackchannel() {
    let manifest = parse(
        "models/@default/journeys/x.yaml",
        "apiVersion: ai.enterprise/v1\nkind: journey\nmetadata:\n  id: urn:ordin:default:journeys:x\nstages:\n  - id: design\n    slackChannel: design-reviews\n",
    );
    assert_eq!(
        manifest.journey().unwrap_err(),
        AmError::FieldShapeInvalid {
            key: "slackChannel".into(),
            expected: "a map with id and name"
        }
    );
}

// Scenario: Rendering keeps lifecycles without Slack unchanged
#[test]
fn rendering_keeps_lifecycles_without_slack_unchanged() {
    let manifest = parse("models/@default/journeys/review.yaml", LIFECYCLE);
    let rendered = render_part(PartKind::Journey, &manifest, None).unwrap();
    assert_eq!(rendered.content, LIFECYCLE.as_bytes());
    assert!(!String::from_utf8(rendered.content.clone())
        .unwrap()
        .contains("slackChannel"));
    let back = parse(
        &rendered.path,
        std::str::from_utf8(&rendered.content).unwrap(),
    );
    assert_eq!(back, manifest);
}
