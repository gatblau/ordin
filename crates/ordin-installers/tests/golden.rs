//! Golden-tree tests of each installer's pure render against the committed fixtures in
//! `testdata/parity/installers/` (Playbook Step 31, as amended by the user's decision on committed
//! fixtures; testdata/README.md).
//!
//! - `catalogue/`: a parts tree in the ordin format holding every WorkspaceProfile the cases build.
//! - `cases/<name>/case.json`: the root, the harness it is built for and the installer that renders it.
//! - `cases/<name>/expected/`: either `error.json` (the refusal), or `tree/` (every file the
//!   installer renders except `ordin.lock`, which is the bundle's own lock-file bytes) and
//!   `outcome.json` (commands, launch paths and reference-only parts).
//!
//! The expected trees are written from Installers.md, not produced by running this crate. A missing
//! fixture folder fails the test; it never skips.

mod common;

use std::collections::BTreeSet;

use common::*;
use ordin_installers::*;
use serde_json::Value;

fn case_names() -> Vec<String> {
    let dir = fixtures().join("cases");
    let entries =
        std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{} cannot be read: {e}", dir.display()));
    let mut names: Vec<String> = entries
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "{} holds no case", dir.display());
    names
}

fn json(path: &std::path::Path) -> Value {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} cannot be read: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn harness_of(text: &str) -> HarnessKind {
    serde_json::from_value(Value::String(text.to_owned())).expect("a harness")
}

fn installer_of(text: &str) -> InstallerKind {
    serde_json::from_value(Value::String(text.to_owned())).expect("an installer")
}

// Scenario: Parity — layout equals the committed golden tree apart from the named differences.
// The repository is standalone, so the expected trees are committed and the named differences are
// asserted directly.
#[tokio::test]
async fn parity_layout_equals_the_committed_golden_tree_apart_from_the_named_differences() {
    let world = World::new().await;
    let mut installers_covered: BTreeSet<String> = BTreeSet::new();
    let mut refused = 0;
    for name in case_names() {
        let case_dir = fixtures().join("cases").join(&name);
        let case = json(&case_dir.join("case.json"));
        let harness = harness_of(case["target_harness"].as_str().expect("a harness"));
        let installer = installer_of(case["installer"].as_str().expect("an installer"));
        let bundle = world
            .bundle(case["root"].as_str().expect("a root"), harness, &[])
            .await;
        let rendered = world.installers.render(installer, &bundle);
        let expected = case_dir.join("expected");
        if expected.join("error.json").exists() {
            let error = rendered.expect_err(&format!("{name}: refused"));
            let want = json(&expected.join("error.json"));
            assert_eq!(error.code(), want["code"], "{name}: code");
            if let InstallError::LoopUnsupported {
                strategy,
                missing_capability,
            } = &error
            {
                assert_eq!(strategy, &want["strategy"], "{name}: strategy");
                assert_eq!(
                    missing_capability
                        .as_deref()
                        .map(Value::from)
                        .unwrap_or(Value::Null),
                    want["missing_capability"],
                    "{name}: missing capability"
                );
            }
            refused += 1;
            continue;
        }
        let mut tree = rendered.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        installers_covered.insert(installer_text(installer));
        // `ordin.lock` is the bundle's lock file, byte for byte (step 6).
        assert_eq!(
            tree.files.remove("ordin.lock"),
            Some(bundle.lock_file_bytes.clone()),
            "{name}: ordin.lock"
        );
        let want = read_dir_files(&expected.join("tree"));
        let got_paths: Vec<&String> = tree.files.keys().collect();
        let want_paths: Vec<&String> = want.keys().collect();
        assert_eq!(got_paths, want_paths, "{name}: file set");
        for (path, bytes) in &want {
            assert_eq!(
                String::from_utf8_lossy(&tree.files[path]),
                String::from_utf8_lossy(bytes),
                "{name}: {path}"
            );
        }
        // The named differences: the files never written.
        for absent in [
            ".claude/settings.local.json",
            ".mcp.json",
            ".vscode/mcp.json",
            "mcp.json",
        ] {
            assert!(!tree.files.contains_key(absent), "{name}: {absent} written");
        }
        assert!(
            !tree.files.keys().any(|p| p.ends_with(".policy.md")),
            "{name}: a policy file written"
        );
        let outcome = json(&expected.join("outcome.json"));
        assert_eq!(
            serde_json::to_value(&tree.commands).unwrap(),
            outcome["commands"],
            "{name}: commands"
        );
        assert_eq!(
            serde_json::to_value(&tree.launch_paths).unwrap(),
            outcome["launch_paths"],
            "{name}: launch paths"
        );
        assert_eq!(
            serde_json::to_value(&tree.reference_only_parts).unwrap(),
            outcome["reference_only_parts"],
            "{name}: reference-only parts"
        );
    }
    let all: BTreeSet<String> = ["agents-md", "claude-code", "vscode"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(installers_covered, all, "every installer has a golden tree");
    assert!(refused >= 1, "the fixtures hold a refusal");
}

fn installer_text(kind: InstallerKind) -> String {
    serde_json::to_value(kind)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn every_case_carries_its_expected_output() {
    for name in case_names() {
        let expected = fixtures().join("cases").join(&name).join("expected");
        let files: BTreeSet<String> = read_dir_files(&expected).into_keys().collect();
        let refusal = files.len() == 1 && files.contains("error.json");
        let tree = files.contains("outcome.json")
            && files
                .iter()
                .all(|f| f == "outcome.json" || f.starts_with("tree/"))
            && !files.contains("tree/ordin.lock");
        assert!(refusal || tree, "{name}: unexpected files {files:?}");
    }
}

#[test]
#[should_panic(expected = "cannot be read")]
fn a_missing_fixture_folder_fails() {
    read_dir_files(&fixtures().join("no-such-folder"));
}
