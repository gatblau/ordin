//! X2: one test per Gherkin scenario of Installers.md, named after the scenario in snake_case.
//!
//! Two scenarios live elsewhere: *Parity — layout equals the committed golden tree apart from the
//! named differences* in `tests/golden.rs`, and *Error — Copilot ruled out* in the unit test
//! `types::tests::error_copilot_ruled_out`, because `COPILOT_SUPPORTED` is a constant that only the
//! crate itself can treat as false.

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use common::*;
use ordin_agent_folders::LockMode;
use ordin_installers::*;

fn read(folder: &Path, path: &str) -> String {
    text(&std::fs::read(folder.join(path)).unwrap_or_else(|e| panic!("{path}: {e}")))
}

fn entry<'a>(record: &'a InstallRecord, path: &str) -> &'a InstalledFile {
    record
        .files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} is not recorded"))
}

/// The Markdown body of a part file: what follows its front matter.
fn body(markdown: &str) -> &str {
    let rest = markdown.strip_prefix("---\n").expect("front matter");
    let end = rest.find("\n---\n").expect("the end of the front matter");
    &rest[end + "\n---\n".len()..]
}

// Scenario: Happy path — Claude Code install with the C-61 additions
#[tokio::test]
async fn happy_path_claude_code_install_with_the_c_61_additions() {
    // Given a bundle for cspa-431 and an empty agent folder; when install is called for ClaudeCode.
    let world = World::new().await;
    let (agent, bundle, outcome) = world.installed_cspa().await;
    let folder = &agent.folder_path;
    // Then the folder holds ordin.lock, CLAUDE.md, instructions.md and plugin.json named "cspa-431".
    assert_eq!(
        std::fs::read(folder.join("ordin.lock")).unwrap(),
        bundle.lock_file_bytes
    );
    assert!(read(folder, "CLAUDE.md").contains("@.claude/rules/system-role.md"));
    let manifest: serde_json::Value =
        serde_json::from_str(&read(folder, "plugin/.claude-plugin/plugin.json")).unwrap();
    assert_eq!(manifest["name"], "cspa-431");
    assert_eq!(manifest["version"], "1.0.0");
    // And instructions.md holds the text of system-role.md in place of its "@" line.
    let instructions = read(folder, "instructions.md");
    let role = read(folder, ".claude/rules/system-role.md");
    let role_body = body(&role).trim();
    assert!(!role_body.is_empty());
    assert!(instructions.contains(role_body), "{instructions}");
    assert!(
        !instructions
            .lines()
            .any(|l| l.trim() == "@.claude/rules/system-role.md"),
        "{instructions}"
    );
    // And the record's launch names instructions.md and the plugin directory.
    assert_eq!(
        outcome.record.launch,
        HarnessLaunch::ClaudeCode {
            working_dir: folder.clone(),
            append_system_prompt_file: folder.join("instructions.md"),
            plugin_dir: folder.join("plugin"),
            plugin_name: "cspa-431".to_owned(),
        }
    );
    // And the command "urn:ordin:default:commands:lifecycle:apply" is named "cspa-431:apply".
    assert_eq!(
        outcome.record.commands,
        vec![CommandName {
            command_id: APPLY.to_owned(),
            session_name: "cspa-431:apply".to_owned(),
        }]
    );
    // And no ".claude/settings.local.json" or ".mcp.json" is written.
    assert!(!folder.join(".claude/settings.local.json").exists());
    assert!(!folder.join(".mcp.json").exists());
    // The record matches what is on disk, and cspa's loop is human-pulled.
    assert!(outcome.record.needs_person_present);
    assert_eq!(outcome.record.record_schema, RECORD_SCHEMA);
    assert_eq!(outcome.record.previous_plan_hash, None);
    assert_eq!(
        outcome.record.tool_policy,
        bundle.manifest.resolved_tool_policy
    );
    let on_disk: Vec<String> = tree_of(folder).into_keys().collect();
    let recorded: Vec<String> = outcome
        .record
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect();
    assert_eq!(on_disk, recorded);
    assert!(outcome
        .record
        .files
        .iter()
        .all(|f| f.action == FileAction::Created && f.backup.is_none()));
    assert!(agent.meta_path.join("bundle/ordin.lock").is_file());
    assert!(agent
        .meta_path
        .join("bundle/bundle.manifest.json")
        .is_file());
    assert_eq!(
        world.installers.verify(agent.agent_instance_id).await,
        Ok(VerifyOutcome::Verified)
    );
}

// Scenario: Edge — update keeps what the session wrote
#[tokio::test]
async fn edge_update_keeps_what_the_session_wrote() {
    // Given an installed agent whose session wrote "results/q3.xlsx" and changed apply.md.
    let world = World::new().await;
    let (agent, _, before) = world.installed_cspa().await;
    let folder = &agent.folder_path;
    std::fs::create_dir_all(folder.join("results")).unwrap();
    std::fs::write(folder.join("results/q3.xlsx"), b"the session's workbook").unwrap();
    std::fs::write(
        folder.join(".claude/commands/apply.md"),
        b"changed by the session",
    )
    .unwrap();
    // When update is called with a newer bundle.
    let newer = world
        .bundle(CSPA, HarnessKind::ClaudeCode, &[TEST_PYRAMID])
        .await;
    assert_ne!(newer.plan.plan_hash, before.record.plan_hash);
    let outcome = world
        .installers
        .update(&world.request(&agent, HarnessKind::ClaudeCode, &newer))
        .await
        .expect("updates");
    // Then "results/q3.xlsx" is unchanged.
    assert_eq!(
        std::fs::read(folder.join("results/q3.xlsx")).unwrap(),
        b"the session's workbook"
    );
    assert!(!outcome
        .record
        .files
        .iter()
        .any(|f| f.path == "results/q3.xlsx"));
    // And the changed "apply.md" is backed up under meta/backups before it is replaced.
    let apply = entry(&outcome.record, ".claude/commands/apply.md");
    assert_eq!(apply.action, FileAction::Replaced);
    let backup = apply.backup.as_deref().expect("a backup");
    assert!(backup.starts_with("backups/"), "{backup}");
    assert!(backup.ends_with("/.claude/commands/apply.md"), "{backup}");
    assert_eq!(
        std::fs::read(agent.meta_path.join(backup)).unwrap(),
        b"changed by the session"
    );
    assert_ne!(
        std::fs::read(folder.join(".claude/commands/apply.md")).unwrap(),
        b"changed by the session"
    );
    assert_eq!(
        outcome.record.previous_plan_hash.as_deref(),
        Some(before.record.plan_hash.as_str())
    );
    assert_eq!(outcome.record.installed_at, before.record.installed_at);
    assert!(!outcome.restart_required, "update is between runs");
    assert_eq!(
        entry(&outcome.record, ".claude/rules/testing/pyramid.md").action,
        FileAction::Created
    );
    assert_eq!(
        entry(&outcome.record, ".claude/rules/tone.md").action,
        FileAction::Unchanged
    );
}

// Scenario: Edge — uninstall restores byte for byte
//
// Step 9 refuses a first install into a non-empty folder, so the only file an install can overwrite
// is one written after it, which the next update backs up (step 10). The file is written at the path
// the testing pack's rule takes, and the update that adds the pack overwrites it.
#[tokio::test]
async fn edge_uninstall_restores_byte_for_byte() {
    let world = World::new().await;
    let (agent, _, _) = world.installed_cspa().await;
    let folder = &agent.folder_path;
    let notes = ".claude/rules/testing/pyramid.md";
    std::fs::create_dir_all(folder.join(".claude/rules/testing")).unwrap();
    std::fs::write(folder.join(notes), b"my own notes\r\nbyte for byte\n").unwrap();
    let with_pack = world
        .bundle(CSPA, HarnessKind::ClaudeCode, &[TEST_PYRAMID])
        .await;
    let updated = world
        .installers
        .update(&world.request(&agent, HarnessKind::ClaudeCode, &with_pack))
        .await
        .expect("updates");
    assert_ne!(
        std::fs::read(folder.join(notes)).unwrap(),
        b"my own notes\r\nbyte for byte\n",
        "overwritten by the update"
    );
    assert!(entry(&updated.record, notes).backup.is_some());
    // A second update keeps the earliest backup for uninstall.
    std::fs::write(folder.join(notes), b"changed again").unwrap();
    let again = world
        .installers
        .update(&world.request(&agent, HarnessKind::ClaudeCode, &with_pack))
        .await
        .expect("updates again");
    assert_eq!(
        entry(&again.record, notes).backup,
        entry(&updated.record, notes).backup
    );
    // When uninstall is called.
    world
        .installers
        .uninstall(agent.agent_instance_id)
        .await
        .expect("uninstalls");
    // Then "notes" holds its original bytes and every other installed file is gone.
    let left = tree_of(folder);
    assert_eq!(left.keys().collect::<Vec<_>>(), vec![notes]);
    assert_eq!(left[notes], b"my own notes\r\nbyte for byte\n");
    assert!(!folder.join("plugin").exists(), "empty directories removed");
    assert!(!agent.meta_path.join(RECORD_FILE).exists());
    assert!(!agent.meta_path.join("bundle").exists());
    assert!(!agent.meta_path.join("backups").exists());
    assert_eq!(
        world.installers.record(agent.agent_instance_id).await,
        Err(InstallError::NotInstalled)
    );
}

// Scenario: Edge — pack chosen mid-session
#[tokio::test]
async fn edge_pack_chosen_mid_session() {
    // Given a running Claude Code session on the agent.
    let world = World::new().await;
    let (agent, _, _) = world.installed_cspa().await;
    let _session = world
        .folders
        .lock(agent.agent_instance_id, LockMode::Session)
        .await
        .expect("a session lease");
    // When update_in_session adds the testing pack's files.
    let with_pack = world
        .bundle(CSPA, HarnessKind::ClaudeCode, &[TEST_PYRAMID])
        .await;
    let outcome = world
        .installers
        .update_in_session(&world.request(&agent, HarnessKind::ClaudeCode, &with_pack))
        .await
        .expect("updates beside the session");
    // Then the outcome's restart_required is true because instructions.md changed.
    assert!(outcome.restart_required);
    assert_eq!(
        entry(&outcome.record, "instructions.md").action,
        FileAction::Replaced
    );
    assert!(read(&agent.folder_path, "instructions.md").contains("few end-to-end tests"));

    // The same bundle again changes nothing a session reads at start: no restart.
    let again = world
        .installers
        .update_in_session(&world.request(&agent, HarnessKind::ClaudeCode, &with_pack))
        .await
        .expect("updates again");
    assert!(!again.restart_required);
}

// Scenario: Edge — verify finds a changed file and repairs it
#[tokio::test]
async fn edge_verify_finds_a_changed_file_and_repairs_it() {
    // Given an installed agent whose ".claude/rules/system-role.md" was edited on disk.
    let world = World::new().await;
    let (agent, _, _) = world.installed_cspa().await;
    let id = agent.agent_instance_id;
    let role = ".claude/rules/system-role.md";
    let original = std::fs::read(agent.folder_path.join(role)).unwrap();
    std::fs::write(agent.folder_path.join(role), b"edited on disk").unwrap();
    std::fs::remove_file(agent.folder_path.join("plugin/commands/apply.md")).unwrap();
    assert_eq!(
        world.installers.verify(id).await,
        Ok(VerifyOutcome::Mismatch {
            changed: vec![role.to_owned()],
            missing: vec!["plugin/commands/apply.md".to_owned()],
        })
    );
    // When verify_and_repair is called.
    let outcome = world
        .installers
        .verify_and_repair(id, &scope())
        .await
        .expect("repairs");
    // Then the file is rewritten from the bundle and repaired is true.
    assert!(outcome.repaired);
    assert_eq!(
        std::fs::read(agent.folder_path.join(role)).unwrap(),
        original
    );
    assert!(agent.folder_path.join("plugin/commands/apply.md").is_file());
    assert_eq!(
        world.installers.verify(id).await,
        Ok(VerifyOutcome::Verified)
    );
    // An intact folder is not repaired.
    let intact = world
        .installers
        .verify_and_repair(id, &scope())
        .await
        .expect("verifies");
    assert!(!intact.repaired);
}

// Scenario: Error — loop the harness cannot run
#[tokio::test]
async fn error_loop_the_harness_cannot_run() {
    // Given an agent whose control loop has strategy "plan-then-execute".
    let world = World::new().await;
    let bundle = world.bundle(CSPA, HarnessKind::CopilotCli, &[]).await;
    let agent = world.agent().await;
    // When install is called for harness CopilotCli.
    let error = world
        .installers
        .install(&world.request(&agent, HarnessKind::CopilotCli, &bundle))
        .await
        .unwrap_err();
    // Then it fails with INST_LOOP_UNSUPPORTED naming "plan-then-execute".
    assert_eq!(error.code(), "INST_LOOP_UNSUPPORTED");
    let InstallError::LoopUnsupported { strategy, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(strategy, "plan-then-execute");
    assert!(tree_of(&agent.folder_path).is_empty());
}

// Scenario: Error — install while a session runs
#[tokio::test]
async fn error_install_while_a_session_runs() {
    // Given a session holds the agent folder.
    let world = World::new().await;
    let (agent, bundle, _) = world.installed_cspa().await;
    let _session = world
        .folders
        .lock(agent.agent_instance_id, LockMode::Session)
        .await
        .expect("a session lease");
    // When update is called (it waits the folder manager's write wait, 30 s, first).
    let error = world
        .installers
        .update(&world.request(&agent, HarnessKind::ClaudeCode, &bundle))
        .await
        .unwrap_err();
    // Then it fails with INST_FOLDER_UNAVAILABLE and retryable is true.
    assert_eq!(error.code(), "INST_FOLDER_UNAVAILABLE");
    assert!(error.retryable());
}

// Scenario: Error — repair impossible
#[tokio::test]
async fn error_repair_impossible() {
    // Given an installed agent with a changed file whose bundle is gone and whose lock-file parts
    // are no longer in the clone.
    let world = World::bound_to(Arc::new(empty_source()), Some(Arc::new(catalogue_source()))).await;
    let (agent, bundle, _) = world.installed_cspa().await;
    std::fs::write(agent.folder_path.join("CLAUDE.md"), b"changed").unwrap();
    std::fs::remove_dir_all(&bundle.dir).unwrap();
    // When verify_and_repair is called.
    let error = world
        .installers
        .verify_and_repair(agent.agent_instance_id, &scope())
        .await
        .unwrap_err();
    // Then it fails with INST_REPAIR_FAILED with code "RES_LOCK_PART_UNAVAILABLE".
    assert_eq!(
        error,
        InstallError::RepairFailed {
            code: "RES_LOCK_PART_UNAVAILABLE".to_owned()
        }
    );
    // The folder is left as it was.
    assert_eq!(
        std::fs::read(agent.folder_path.join("CLAUDE.md")).unwrap(),
        b"changed"
    );
}

// Scenario: Error — circular import
#[tokio::test]
async fn error_circular_import() {
    // Given a rule file that imports itself with "@.claude/rules/a.md".
    let world = World::bound_to(Arc::new(circular_catalogue()), None).await;
    let bundle = world.bundle(CSPA, HarnessKind::ClaudeCode, &[]).await;
    // When render is called for Claude Code.
    let error = world
        .installers
        .render(InstallerKind::ClaudeCode, &bundle)
        .unwrap_err();
    // Then it fails with INST_IMPORT_CYCLE.
    assert_eq!(error.code(), "INST_IMPORT_CYCLE");
}

// Scenario: Happy path — session form for a release bundle
#[tokio::test]
async fn happy_path_session_form_for_a_release_bundle() {
    // Given a ClaudeSessionFormInput for "support-agent" version "2026.10.1" with skill "triage"
    // and command "summarise".
    let dir = TempDir::new();
    let bundle_dir = dir.path().join("bundle");
    std::fs::create_dir_all(bundle_dir.join("rules")).unwrap();
    std::fs::write(
        bundle_dir.join("rules/tone.md"),
        "---\nname: tone\n---\nUse short sentences.\n",
    )
    .unwrap();
    let input = ClaudeSessionFormInput {
        agent_name: "support-agent".to_owned(),
        version: "2026.10.1".to_owned(),
        description: "Answers support questions.".to_owned(),
        instructions: "# Support agent\n\n@rules/tone.md\n".to_owned(),
        skills: BTreeMap::from([(
            "triage".to_owned(),
            BTreeMap::from([
                ("SKILL.md".to_owned(), b"Triage the ticket.\n".to_vec()),
                ("scripts/sort.sh".to_owned(), b"#!/bin/sh\n".to_vec()),
            ]),
        )]),
        commands: BTreeMap::from([("summarise".to_owned(), b"Summarise the thread.\n".to_vec())]),
    };
    // When render_claude_session_form is called and write_tree writes the tree.
    let tree = render_claude_session_form(&input, &bundle_dir).expect("renders");
    let target = bundle_dir.join(".ordin/claude");
    write_tree(&target, &tree).await.expect("writes");
    // Then instructions.md holds the instructions with "@" imports inlined.
    let instructions = read(&target, "instructions.md");
    assert_eq!(instructions, "# Support agent\n\nUse short sentences.\n");
    // And plugin.json names "support-agent".
    let manifest: serde_json::Value =
        serde_json::from_str(&read(&target, "plugin/.claude-plugin/plugin.json")).unwrap();
    assert_eq!(
        manifest,
        serde_json::json!({
            "description": "Answers support questions.",
            "name": "support-agent",
            "version": "2026.10.1"
        })
    );
    // And the command "summarise" is named "support-agent:summarise".
    assert_eq!(
        tree.commands,
        vec![CommandName {
            command_id: "summarise".to_owned(),
            session_name: "support-agent:summarise".to_owned(),
        }]
    );
    assert_eq!(
        tree_of(&target).into_keys().collect::<Vec<_>>(),
        vec![
            "instructions.md",
            "plugin/.claude-plugin/plugin.json",
            "plugin/commands/summarise.md",
            "plugin/skills/triage/SKILL.md",
            "plugin/skills/triage/scripts/sort.sh",
        ]
    );
    assert!(tree.reference_only_parts.is_empty());
    assert_eq!(
        tree.launch_paths,
        Some((
            "instructions.md".to_owned(),
            "plugin".to_owned(),
            "support-agent".to_owned()
        ))
    );
    // The bundle's own files are untouched.
    assert!(read(&bundle_dir, "rules/tone.md").starts_with("---\n"));
}

// Scenario: Edge — the program reads launch paths from the record
#[tokio::test]
async fn edge_the_program_reads_launch_paths_from_the_record() {
    // Given an installed Claude Code agent "cspa-431".
    let world = World::new().await;
    let (agent, _, outcome) = world.installed_cspa().await;
    // When record is called for the instance.
    let record = world
        .installers
        .record(agent.agent_instance_id)
        .await
        .expect("a record");
    // Then launch.append_system_prompt_file is "<folder>/instructions.md" and plugin_name "cspa-431".
    let HarnessLaunch::ClaudeCode {
        append_system_prompt_file,
        plugin_name,
        plugin_dir,
        working_dir,
    } = &record.launch
    else {
        panic!("{:?}", record.launch);
    };
    assert_eq!(
        append_system_prompt_file,
        &agent.folder_path.join("instructions.md")
    );
    assert_eq!(plugin_name, "cspa-431");
    assert_eq!(plugin_dir, &agent.folder_path.join("plugin"));
    assert_eq!(working_dir, &agent.folder_path);
    assert_eq!(record, outcome.record);
}

// A Copilot install carries the working folder only, and its commands by slug (step 4).
#[tokio::test]
async fn a_copilot_install_records_the_working_folder() {
    let world = World::new().await;
    let bundle = world.bundle(HELPDESK, HarnessKind::CopilotCli, &[]).await;
    let agent = world.agent().await;
    let outcome = world
        .installers
        .install(&world.request(&agent, HarnessKind::CopilotCli, &bundle))
        .await
        .expect("installs");
    assert_eq!(outcome.record.installer, InstallerKind::Vscode);
    assert_eq!(
        outcome.record.launch,
        HarnessLaunch::Copilot {
            working_dir: agent.folder_path.clone()
        }
    );
    assert!(outcome
        .record
        .commands
        .iter()
        .all(|c| !c.session_name.contains(':')));
    // Copilot must be resumed after any mid-session change until C-64 confirms otherwise.
    let again = world
        .installers
        .update_in_session(&world.request(&agent, HarnessKind::CopilotCli, &bundle))
        .await
        .expect("updates");
    assert!(again.restart_required);
}

// ---- Amendment January 2026 — a task's inputs in the harness's form (ADR-119) ----

const APPLY_WITH_INPUTS: &str = "---\napiVersion: ai.enterprise/v1\nkind: command\nmetadata:\n  id: urn:ordin:default:commands:lifecycle:apply\n  name: Apply\n  version: 1.1.0\n  owner: engineering-enablement\n  description: Applies one phase of a plan.\ntitle: Apply a plan phase\ninputs:\n  - name: plan\n    label: Plan\n    kind: document\n  - name: phase\n    label: Phase number\n    kind: number\n---\nApply phase ${input:phase} of the plan ${input:plan}.\n";

const INPUTS_GIVEN_TAIL: &str = "## Inputs given\n\nEach input written above as [name] is given below on a line \"name: value\". A value of several lines continues on the following lines, each indented by two spaces. An input that is not listed was left empty.\n\n$ARGUMENTS\n";

// Scenario: Happy path — the apply command is written with one place for its values
// spec: ordin-desktop/components/Installers#INST-TF-1
#[tokio::test]
async fn happy_path_the_apply_command_is_written_with_one_place_for_its_values() {
    let source = catalogue_source_with(&[(
        "models/@default/commands/lifecycle/apply.md",
        APPLY_WITH_INPUTS.to_owned(),
    )]);
    let world = World::bound_to(Arc::new(source), None).await;
    let (agent, _, _) = world.installed_cspa().await;
    let folder = &agent.folder_path;
    for path in [".claude/commands/apply.md", "plugin/commands/apply.md"] {
        let text = read(folder, path);
        assert!(
            text.contains("Apply phase [phase] of the plan [plan]."),
            "{path}: {text}"
        );
        assert!(!text.contains("${input:"), "{path}: {text}");
        assert!(text.ends_with(INPUTS_GIVEN_TAIL), "{path}: {text}");
        assert!(
            text.starts_with("---\napiVersion: ai.enterprise/v1\nkind: command\n"),
            "the front matter is kept as read"
        );
        assert!(text.contains("title: Apply a plan phase\n"), "{path}");
    }
}

// Scenario: Edge — a command without inputs is copied unchanged
#[tokio::test]
async fn edge_a_command_without_inputs_is_copied_unchanged() {
    let world = World::new().await;
    let (agent, _, _) = world.installed_cspa().await;
    let part_file =
        std::fs::read(fixtures().join("catalogue/models/@default/commands/lifecycle/apply.md"))
            .expect("the fixture command");
    assert_eq!(
        std::fs::read(agent.folder_path.join(".claude/commands/apply.md")).unwrap(),
        part_file
    );
}

// Scenario: Edge — the vscode layout renders the same body
#[tokio::test]
async fn edge_the_vscode_layout_renders_the_same_body() {
    let summarise = "---\napiVersion: ai.enterprise/v1\nkind: command\nmetadata:\n  id: urn:ordin:default:commands:triage:summarise\n  name: Summarise\n  version: 1.1.0\n  owner: engineering-enablement\n  description: Summarises a ticket.\ntitle: Summarise a ticket\ninputs:\n  - name: ticket\n    label: Ticket\n    kind: short-text\n  - name: length\n    label: Length\n    kind: choice\n    choices: [Short, Long]\n---\nSummarise ${input:ticket} at the length ${input:length}.\n";
    let source = catalogue_source_with(&[(
        "models/@default/commands/triage/summarise.md",
        summarise.to_owned(),
    )]);
    let world = World::bound_to(Arc::new(source), None).await;
    let bundle = world.bundle(HELPDESK, HarnessKind::CopilotCli, &[]).await;
    let agent = world.agent().await;
    world
        .installers
        .install(&world.request(&agent, HarnessKind::CopilotCli, &bundle))
        .await
        .expect("installs");
    let text = read(&agent.folder_path, ".github/prompts/summarise.prompt.md");
    assert!(
        text.starts_with("Summarise [ticket] at the length [length].\n\n## Inputs given\n"),
        "{text}"
    );
    assert!(text.ends_with("$ARGUMENTS\n"), "{text}");
}

// Error-table row: a Command's inputs do not read when its file is rendered → written unchanged.
#[tokio::test]
async fn error_a_command_whose_inputs_do_not_read_is_written_unchanged() {
    let broken = APPLY_WITH_INPUTS.replace("kind: number", "kind: file");
    let source = catalogue_source_with(&[(
        "models/@default/commands/lifecycle/apply.md",
        broken.clone(),
    )]);
    let world = World::bound_to(Arc::new(source), None).await;
    let (agent, _, _) = world.installed_cspa().await;
    assert_eq!(
        read(&agent.folder_path, ".claude/commands/apply.md"),
        broken
    );
}

// The session form for a release bundle renders a command with inputs the same way (INST-TF-1).
#[test]
fn edge_the_session_form_renders_a_command_with_inputs() {
    let dir = TempDir::new();
    let input = ClaudeSessionFormInput {
        agent_name: "release-desk".to_owned(),
        version: "1.0.0".to_owned(),
        description: String::new(),
        instructions: "Be brief.\n".to_owned(),
        skills: BTreeMap::new(),
        commands: BTreeMap::from([("apply".to_owned(), APPLY_WITH_INPUTS.as_bytes().to_vec())]),
    };
    let tree = render_claude_session_form(&input, dir.path()).expect("renders");
    let text = text(&tree.files["plugin/commands/apply.md"]);
    assert!(
        text.contains("Apply phase [phase] of the plan [plan]."),
        "{text}"
    );
    assert!(text.ends_with(INPUTS_GIVEN_TAIL), "{text}");
}
