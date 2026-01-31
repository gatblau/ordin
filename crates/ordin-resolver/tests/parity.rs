//! Parity over the committed fixtures in `testdata/parity/workspaces/` (Playbook Step 15, as amended by
//! the user's decision on committed fixtures; testdata/README.md).
//!
//! - `catalogue/`: a parts tree in the ordin format with its `withdrawn.yaml`, holding every
//!   WorkspaceProfile the cases build and the packs, overlays and constituents they use.
//! - `cases/<name>/request.json`: the choices given to `build` (languages, interface language, answers,
//!   changes, replacements).
//! - `cases/<name>/expected/`: either `error.json` (the code and its detail), or `plan.json`,
//!   `ordin.lock`, `bundle.manifest.json` and `files.json` (the file names each part holds in the bundle).
//!
//! A missing or empty fixture folder fails the test; it never skips.

mod common;

use std::collections::BTreeSet;

use common::fixtures::*;
use common::TempDir;
use serde_json::Value;

fn expected_json(case: &str, file: &str) -> Value {
    let path = root().join("cases").join(case).join("expected").join(file);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} cannot be read: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn expected_bytes(case: &str, file: &str) -> Vec<u8> {
    let path = root().join("cases").join(case).join("expected").join(file);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{} cannot be read: {e}", path.display()))
}

// Scenario: Happy path — parity with the committed fixtures (the specification's scenario of this name
// compares with a reference implementation; the repository is standalone, so the expected output is
// committed instead).
#[tokio::test]
async fn happy_path_parity_with_the_committed_fixtures() {
    let catalogue = catalogue();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut built = 0;
    let mut refused = 0;
    for name in case_names() {
        let dir = TempDir::new();
        let request_path = root().join("cases").join(&name).join("request.json");
        let request: Value =
            serde_json::from_slice(&std::fs::read(&request_path).expect("request.json")).unwrap();
        covered.insert(request["root"].as_str().expect("a root").to_owned());
        let has_error = root()
            .join("cases")
            .join(&name)
            .join("expected")
            .join("error.json")
            .exists();
        match run_case(&catalogue, &name, &dir.path().join("bundles")).await {
            Outcome::Refused(actual) => {
                assert!(
                    has_error,
                    "{name}: refused, but the case expects a bundle: {actual}"
                );
                assert_eq!(actual, expected_json(&name, "error.json"), "{name}: error");
                refused += 1;
            }
            Outcome::Built {
                plan,
                lock,
                manifest,
                files,
            } => {
                assert!(!has_error, "{name}: built, but the case expects a refusal");
                assert_eq!(plan, expected_json(&name, "plan.json"), "{name}: plan");
                assert_eq!(
                    lock,
                    expected_bytes(&name, "ordin.lock"),
                    "{name}: lock file"
                );
                assert_eq!(
                    manifest,
                    expected_bytes(&name, "bundle.manifest.json"),
                    "{name}: bundle manifest"
                );
                assert_eq!(files, expected_json(&name, "files.json"), "{name}: files");
                // Each part holds the version, digest and file set of the catalogue.
                for part in plan["resolved"].as_array().expect("resolved") {
                    let id = part["id"].as_str().unwrap();
                    let version = part["version"].as_str().unwrap();
                    let source = catalogue
                        .parts
                        .iter()
                        .find(|p| {
                            p.manifest.metadata.id == id && p.manifest.metadata.version == version
                        })
                        .unwrap_or_else(|| {
                            panic!("{name}: {id}@{version} is not in the catalogue")
                        });
                    assert_eq!(
                        part["contentHash"],
                        format!("sha256:{}", source.content_digest.0),
                        "{name}: {id}"
                    );
                    let names: Vec<&str> = source.files.0.keys().map(String::as_str).collect();
                    let listed: Vec<&str> = files[format!("{id}@{version}")]
                        .as_array()
                        .expect("a file list")
                        .iter()
                        .map(|v| v.as_str().unwrap())
                        .collect();
                    assert_eq!(listed, names, "{name}: {id} file set");
                }
                built += 1;
            }
        }
    }
    assert!(
        built >= 8 && refused >= 5,
        "the fixtures cover builds ({built}) and refusals ({refused})"
    );
    // Every WorkspaceProfile of the catalogue is built by at least one case.
    let all: BTreeSet<String> = catalogue.workspaces().into_iter().collect();
    assert_eq!(
        covered, all,
        "every WorkspaceProfile of the catalogue has a case"
    );
}

#[test]
fn every_case_carries_its_expected_output() {
    for name in case_names() {
        let expected = root().join("cases").join(&name).join("expected");
        assert!(expected.is_dir(), "{name}: the expected folder is missing");
        let files: BTreeSet<String> = read_dir(&expected).into_keys().collect();
        let refusal: BTreeSet<String> = ["error.json".to_owned()].into();
        let bundle: BTreeSet<String> = [
            "bundle.manifest.json",
            "files.json",
            "ordin.lock",
            "plan.json",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        assert!(
            files == refusal || files == bundle,
            "{name}: unexpected files {files:?}"
        );
    }
}

#[test]
#[should_panic(expected = "cannot be read")]
fn a_missing_fixture_folder_fails() {
    read_dir(&root().join("no-such-folder"));
}
