//! X1: one test per row of Installers.md's *Error Table*, provoking the condition and asserting the
//! code, the payload and the retryability through the `CodedError` contract a caller reads.
//!
//! The first row (`COPILOT_SUPPORTED = false`) cannot be provoked from outside the crate while the
//! constant is true; it is provoked by the unit test `types::tests::error_copilot_ruled_out`, which
//! calls the same mapping with Copilot ruled out.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use ordin_agent_folders::LockMode;
use ordin_installers::*;
use ordin_types::error_registry::lookup;
use ordin_types::{AgentInstanceId, CodedError};

fn assert_coded(error: &InstallError, code: &str, retryable: bool) {
    let coded: &dyn CodedError = error;
    assert_eq!(coded.code(), code);
    assert_eq!(coded.retryable(), retryable, "{code} retryability");
    assert_eq!(error.to_string(), code, "Display is the bare code");
    assert!(lookup(code).is_some(), "{code} is registered");
}

fn session_form(skills: BTreeMap<String, BTreeMap<String, Vec<u8>>>) -> ClaudeSessionFormInput {
    ClaudeSessionFormInput {
        agent_name: "support-agent".to_owned(),
        version: "2026.10.1".to_owned(),
        description: String::new(),
        instructions: "Be helpful.\n".to_owned(),
        skills,
        commands: BTreeMap::new(),
    }
}

// Row: C-60/C-01 rule out Copilot — while they have not, Copilot maps to the vscode installer.
#[test]
fn harness_unsupported_is_permanent_and_copilot_is_supported_until_ruled_out() {
    const { assert!(COPILOT_SUPPORTED) };
    assert_eq!(
        installer_for(HarnessKind::CopilotCli),
        Ok(InstallerKind::Vscode)
    );
    let error = InstallError::HarnessUnsupported {
        harness: HarnessKind::CopilotCli,
    };
    assert_coded(&error, "INST_HARNESS_UNSUPPORTED", false);
}

// Row: `write_tree` target already holds a path of the tree.
#[tokio::test]
async fn write_tree_into_a_target_holding_a_tree_path_is_refused() {
    let dir = TempDir::new();
    let tree =
        render_claude_session_form(&session_form(BTreeMap::new()), dir.path()).expect("renders");
    let target = dir.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("instructions.md"), b"already here").unwrap();
    let error = write_tree(&target, &tree).await.unwrap_err();
    assert_eq!(error, InstallError::FolderNotEmpty);
    assert_coded(&error, "INST_FOLDER_NOT_EMPTY", false);
    // Nothing was written, and what was there is untouched.
    assert_eq!(
        std::fs::read(target.join("instructions.md")).unwrap(),
        b"already here"
    );
    assert!(!target.join("plugin").exists());
}

// Row: agent folder missing.
#[tokio::test]
async fn a_missing_agent_folder_is_permanent() {
    let world = World::new().await;
    let bundle = world.bundle(CSPA, HarnessKind::ClaudeCode, &[]).await;
    let request = InstallRequest {
        agent_instance_id: AgentInstanceId::new(),
        harness: HarnessKind::ClaudeCode,
        plan_hash: bundle.plan.plan_hash.clone(),
    };
    let error = world.installers.install(&request).await.unwrap_err();
    assert_eq!(
        error,
        InstallError::FolderUnavailable {
            code: "AFM_FOLDER_NOT_FOUND",
            retryable: false
        }
    );
    assert_coded(&error, "INST_FOLDER_UNAVAILABLE", false);
}

// Row: agent folder leased by a session or another write. The Write lease waits the Agent Folder
// Manager's `WRITE_LOCK_WAIT` (30 s) before it is refused.
#[tokio::test]
async fn a_busy_agent_folder_is_retryable() {
    let world = World::new().await;
    let (agent, bundle, _) = world.installed_cspa().await;
    let _session = world
        .folders
        .lock(agent.agent_instance_id, LockMode::Session)
        .await
        .expect("a session lease");
    let error = world
        .installers
        .update(&world.request(&agent, HarnessKind::ClaudeCode, &bundle))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        InstallError::FolderUnavailable {
            code: "AFM_FOLDER_BUSY",
            retryable: true
        }
    );
    assert_coded(&error, "INST_FOLDER_UNAVAILABLE", true);
}

// Row: first install into a non-empty folder.
#[tokio::test]
async fn a_first_install_into_a_non_empty_folder_is_refused() {
    let world = World::new().await;
    let bundle = world.bundle(CSPA, HarnessKind::ClaudeCode, &[]).await;
    let agent = world.agent().await;
    std::fs::write(agent.folder_path.join("notes.md"), b"mine").unwrap();
    let error = world
        .installers
        .install(&world.request(&agent, HarnessKind::ClaudeCode, &bundle))
        .await
        .unwrap_err();
    assert_eq!(error, InstallError::FolderNotEmpty);
    assert_coded(&error, "INST_FOLDER_NOT_EMPTY", false);
    assert_eq!(
        tree_of(&agent.folder_path).into_keys().collect::<Vec<_>>(),
        vec!["notes.md".to_owned()]
    );
}

// Row: update, verify, repair or uninstall with no install record.
#[tokio::test]
async fn every_call_on_an_agent_with_no_record_is_not_installed() {
    let world = World::new().await;
    let bundle = world.bundle(CSPA, HarnessKind::ClaudeCode, &[]).await;
    let agent = world.agent().await;
    let id = agent.agent_instance_id;
    let request = world.request(&agent, HarnessKind::ClaudeCode, &bundle);
    let errors = [
        world.installers.update(&request).await.unwrap_err(),
        world
            .installers
            .update_in_session(&request)
            .await
            .unwrap_err(),
        world.installers.verify(id).await.unwrap_err(),
        world
            .installers
            .verify_and_repair(id, &scope())
            .await
            .unwrap_err(),
        world.installers.uninstall(id).await.unwrap_err(),
        world.installers.record(id).await.unwrap_err(),
    ];
    for error in &errors {
        assert_eq!(error, &InstallError::NotInstalled);
        assert_coded(error, "INST_NOT_INSTALLED", false);
    }
}

// Row: bundle missing or corrupt.
#[tokio::test]
async fn a_missing_or_corrupt_bundle_is_permanent() {
    let world = World::new().await;
    let agent = world.agent().await;
    let missing = InstallRequest {
        agent_instance_id: agent.agent_instance_id,
        harness: HarnessKind::ClaudeCode,
        plan_hash: format!("sha256:{}", "0".repeat(64)),
    };
    let error = world.installers.install(&missing).await.unwrap_err();
    assert_eq!(
        error,
        InstallError::BundleUnavailable {
            code: "RES_BUNDLE_NOT_FOUND".to_owned(),
            retryable: false
        }
    );
    assert_coded(&error, "INST_BUNDLE_UNAVAILABLE", false);

    // Corrupt: the bundle's lock file no longer matches its manifest.
    let bundle = world.bundle(CSPA, HarnessKind::ClaudeCode, &[]).await;
    std::fs::write(bundle.dir.join("ordin.lock"), b"not a lock file").unwrap();
    let error = world
        .installers
        .install(&world.request(&agent, HarnessKind::ClaudeCode, &bundle))
        .await
        .unwrap_err();
    let InstallError::BundleUnavailable { code, retryable } = &error else {
        panic!("{error:?}");
    };
    assert!(code.starts_with("RES_"), "{code}");
    assert!(!retryable);
    assert_coded(&error, "INST_BUNDLE_UNAVAILABLE", false);
    assert!(tree_of(&agent.folder_path).is_empty(), "nothing written");
}

// Row: loop strategy or capability not supported by the installer.
#[tokio::test]
async fn a_loop_the_installer_cannot_run_is_refused() {
    let world = World::new().await;
    let bundle = world.bundle(CSPA, HarnessKind::CopilotCli, &[]).await;
    for kind in [InstallerKind::Vscode, InstallerKind::AgentsMd] {
        let error = world.installers.render(kind, &bundle).unwrap_err();
        assert_eq!(
            error,
            InstallError::LoopUnsupported {
                strategy: "plan-then-execute".to_owned(),
                missing_capability: None
            },
            "{kind:?}"
        );
        assert_coded(&error, "INST_LOOP_UNSUPPORTED", false);
    }
}

// Row: `@` import deeper than 5 or circular.
#[tokio::test]
async fn a_circular_import_is_refused() {
    let world = World::bound_to(Arc::new(circular_catalogue()), None).await;
    let bundle = world.bundle(CSPA, HarnessKind::ClaudeCode, &[]).await;
    let error = world
        .installers
        .render(InstallerKind::ClaudeCode, &bundle)
        .unwrap_err();
    assert_eq!(
        error,
        InstallError::ImportCycle {
            file: ".claude/rules/a.md".to_owned()
        }
    );
    assert_coded(&error, "INST_IMPORT_CYCLE", false);
}

#[test]
fn an_import_deeper_than_five_is_refused() {
    let dir = TempDir::new();
    // instructions.md → f1 → f2 → … → f6: six levels of import.
    for n in 1..=6 {
        let next = if n < 6 {
            format!("@f{}.md\n", n + 1)
        } else {
            "end\n".to_owned()
        };
        std::fs::write(dir.path().join(format!("f{n}.md")), next).unwrap();
    }
    let mut input = session_form(BTreeMap::new());
    input.instructions = "@f1.md\n".to_owned();
    let error = render_claude_session_form(&input, dir.path()).unwrap_err();
    assert!(
        matches!(&error, InstallError::ImportCycle { .. }),
        "{error:?}"
    );
    assert_coded(&error, "INST_IMPORT_CYCLE", false);
}

// Row: a part file name is absolute, has `..`, `\` or NUL.
#[test]
fn an_unsafe_file_name_is_refused() {
    let dir = TempDir::new();
    for bad in [
        "/etc/passwd",
        "../escape.md",
        "a\\b.md",
        "nul\0.md",
        "hooks/hooks.json",
        "sub/Hooks/HOOKS.json",
    ] {
        let skills = BTreeMap::from([(
            "triage".to_owned(),
            BTreeMap::from([(bad.to_owned(), b"x".to_vec())]),
        )]);
        let error = render_claude_session_form(&session_form(skills), dir.path()).unwrap_err();
        let InstallError::PathInvalid { path } = &error else {
            panic!("{bad:?}: {error:?}");
        };
        assert!(path.contains(bad), "{bad:?}: {path}");
        assert_coded(&error, "INST_PATH_INVALID", false);
    }
}

// Row: a part's Markdown declares hooks in its front matter.
#[test]
fn front_matter_that_declares_hooks_is_refused_in_a_skill_or_a_command() {
    let dir = TempDir::new();
    for front in [
        "---\nname: x\nhooks:\n  Stop: []\n---\nBody\n",
        "---\r\nname: x\r\nhooks:\r\n  Stop: []\r\n---\r\nBody\r\n",
        "\u{feff}---\nhooks: {}\n---\n",
        "---  \n\"hooks\": {}\n---\n",
        "---\n'hooks' : {}\n---\n",
        "---\n{name: x, hooks: {}}\n---\n",
        "---\n? hooks\n: {}\n---\n",
        "---\n\"h\\x6Foks\": {}\n---\n",
        "---\nHooks: {}\n",
        "---\nmeta:\n  hooks: {}\n---\n",
        // A reviewer's forms: YAML reads each as a `hooks` key, and no text search sees it.
        "---\n&a hooks: {}\n---\n",
        "---\n!!str hooks: {}\n---\n",
        "---\nk: &n hooks\n*n : {}\n---\n",
        "---\n?\n  hooks\n: {}\n---\n",
        "---\n? |-\n  hooks\n: {}\n---\n",
        "---\n\"ho\\x6fks\"  : {}\n---\n",
    ] {
        let skills = BTreeMap::from([(
            "triage".to_owned(),
            BTreeMap::from([("SKILL.md".to_owned(), front.as_bytes().to_vec())]),
        )]);
        let error = render_claude_session_form(&session_form(skills), dir.path()).unwrap_err();
        assert_coded(&error, "INST_PATH_INVALID", false);

        let mut input = session_form(BTreeMap::new());
        input.commands = BTreeMap::from([("apply".to_owned(), front.as_bytes().to_vec())]);
        let error = render_claude_session_form(&input, dir.path()).unwrap_err();
        assert_coded(&error, "INST_PATH_INVALID", false);
    }
}

#[test]
fn front_matter_that_only_mentions_hooks_is_kept() {
    let dir = TempDir::new();
    for front in [
        "---\nname: x\ndescription: Reviews webhooks and git hooks\n---\nBody\n",
        "---\ndescription: \"Some hooks: a list\"\n---\n",
        "No front matter.\nhooks: {}\n",
        "---\nname: x\n---\nhooks: {}\n",
    ] {
        let skills = BTreeMap::from([(
            "triage".to_owned(),
            BTreeMap::from([("SKILL.md".to_owned(), front.as_bytes().to_vec())]),
        )]);
        render_claude_session_form(&session_form(skills), dir.path())
            .unwrap_or_else(|error| panic!("{front:?}: {error:?}"));
    }
}

// Row: file write, rename or delete fails.
#[cfg(unix)]
#[tokio::test]
async fn a_failed_file_write_is_retryable_and_leaves_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    let tree =
        render_claude_session_form(&session_form(BTreeMap::new()), dir.path()).expect("renders");
    let target = dir.path().join("target");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o500)).unwrap();
    let error = write_tree(&target, &tree).await.unwrap_err();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(error, InstallError::WriteFailed { retryable: true });
    assert_coded(&error, "INST_WRITE_FAILED", true);
    assert!(tree_of(&target).is_empty(), "files written so far deleted");
}

// Row: rendered tree over 20,000 files.
#[test]
fn a_tree_over_the_file_limit_is_permanent() {
    let dir = TempDir::new();
    let files: BTreeMap<String, Vec<u8>> = (0..MAX_INSTALLED_FILES)
        .map(|n| (format!("f{n}.md"), Vec::new()))
        .collect();
    let skills = BTreeMap::from([("big".to_owned(), files)]);
    let error = render_claude_session_form(&session_form(skills), dir.path()).unwrap_err();
    assert_eq!(error, InstallError::WriteFailed { retryable: false });
    assert_coded(&error, "INST_WRITE_FAILED", false);
}

// Row: `install-record.json` does not parse or has another schema.
#[tokio::test]
async fn an_unreadable_or_foreign_record_is_invalid() {
    let world = World::new().await;
    let (agent, _, outcome) = world.installed_cspa().await;
    let id = agent.agent_instance_id;
    let record_path = agent.meta_path.join(RECORD_FILE);

    std::fs::write(&record_path, b"{ not json").unwrap();
    let error = world.installers.record(id).await.unwrap_err();
    assert_eq!(error, InstallError::RecordInvalid);
    assert_coded(&error, "INST_RECORD_INVALID", false);

    let mut foreign = serde_json::to_value(&outcome.record).unwrap();
    foreign["record_schema"] = serde_json::Value::from("ordin.install/v2");
    std::fs::write(&record_path, serde_json::to_vec(&foreign).unwrap()).unwrap();
    for error in [
        world.installers.record(id).await.unwrap_err(),
        world.installers.verify(id).await.unwrap_err(),
        world.installers.uninstall(id).await.unwrap_err(),
    ] {
        assert_eq!(error, InstallError::RecordInvalid);
    }
}

// Row: repair cannot rebuild from the lock file.
#[tokio::test]
async fn a_repair_that_cannot_rebuild_fails_with_the_resolver_code() {
    let world = World::bound_to(Arc::new(empty_source()), Some(Arc::new(catalogue_source()))).await;
    let (agent, bundle, _) = world.installed_cspa().await;
    std::fs::write(agent.folder_path.join(".claude/rules/tone.md"), b"edited").unwrap();
    std::fs::remove_dir_all(&bundle.dir).unwrap();
    let error = world
        .installers
        .verify_and_repair(agent.agent_instance_id, &scope())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        InstallError::RepairFailed {
            code: "RES_LOCK_PART_UNAVAILABLE".to_owned()
        }
    );
    assert_coded(&error, "INST_REPAIR_FAILED", false);

    // The trusted lock-file copy unreadable: AM_LOCK_FILE_INVALID.
    std::fs::write(agent.meta_path.join("bundle/ordin.lock"), b": not yaml [").unwrap();
    let error = world
        .installers
        .verify_and_repair(agent.agent_instance_id, &scope())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        InstallError::RepairFailed {
            code: "AM_LOCK_FILE_INVALID".to_owned()
        }
    );
}
