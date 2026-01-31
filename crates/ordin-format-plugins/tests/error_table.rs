//! X1: one test per row of FormatPlugins.md's *Error Table*, provoking the condition and asserting
//! the code and the retryability through the `CodedError` contract a program reads. Every row is
//! `Err(permanent)`.

mod common;

use common::*;
use ordin_agent_model::PartKind;
use ordin_format_plugins::*;
use ordin_types::error_registry::lookup;
use ordin_types::CodedError;

fn assert_coded(error: &FormatError, code: &str) {
    assert_eq!(CodedError::code(error), code);
    assert!(!error.retryable(), "{code} is permanent");
    assert_eq!(error.to_string(), code, "Display is the bare code");
    let entry = lookup(code).unwrap_or_else(|| panic!("{code} is registered"));
    assert!(!entry.retryable, "{code} registry retryability");
    assert!(ALL_CODES.contains(&code));
}

fn plugins() -> FormatPlugins {
    FormatPlugins::builtin()
}

// Row: tree over 20,000 files or 256 MiB.
#[test]
fn tree_too_large() {
    let many = SourceTree(
        (0..=MAX_TREE_FILES)
            .map(|n| (format!("{n}.txt"), Vec::new()))
            .collect(),
    );
    let error = plugins().detect(&many).unwrap_err();
    assert_eq!(
        error,
        FormatError::TreeTooLarge {
            files: 20_001,
            bytes: 0
        }
    );
    assert_coded(&error, "FMT_TREE_TOO_LARGE");
    let big = SourceTree(
        [("b".to_owned(), vec![0u8; (MAX_TREE_BYTES + 1) as usize])]
            .into_iter()
            .collect(),
    );
    let error = plugins().detect(&big).unwrap_err();
    assert_eq!(
        error,
        FormatError::TreeTooLarge {
            files: 1,
            bytes: MAX_TREE_BYTES + 1
        }
    );
}

// Row: a path is absolute, has `..`, `\` or NUL.
#[test]
fn path_invalid() {
    for path in ["/etc/passwd", "a/../b", "a\\b", "a\0b"] {
        let tree = SourceTree([(path.to_owned(), Vec::new())].into_iter().collect());
        let error = plugins().detect(&tree).unwrap_err();
        assert_eq!(
            error,
            FormatError::PathInvalid {
                path: path.to_owned()
            }
        );
        assert_coded(&error, "FMT_PATH_INVALID");
    }
}

// Row: no plug-in recognises the tree.
#[test]
fn not_recognised() {
    let error = plugins().detect(&tree(&[("README.txt", "x")])).unwrap_err();
    assert_eq!(error, FormatError::NotRecognised);
    assert_coded(&error, "FMT_NOT_RECOGNISED");
    let read = plugins()
        .read_any(&tree(&[("README.txt", "x")]), &request("acme", "x"))
        .unwrap_err();
    assert_eq!(read, FormatError::NotRecognised);
}

// Row: apm.yml refused by the restricted reader, not a mapping, or lacks name or version.
#[test]
fn package_manifest_invalid() {
    let cases: [(&str, Option<u32>); 5] = [
        ("name: a\nbase: &b 1\nversion: *b\n", Some(2)),
        ("name: a\nversion: [1\n", Some(3)),
        ("- list\n", None),
        ("version: 1.0.0\n", None),
        ("name: a\n", None),
    ];
    for (text, line) in cases {
        let error = plugins()
            .get(FormatId::Apm)
            .read(&tree(&[("apm.yml", text)]), &request("acme", "x"))
            .err()
            .unwrap_or_else(|| panic!("{text:?} was accepted"));
        assert_eq!(
            error,
            FormatError::PackageManifestInvalid {
                path: "apm.yml".to_owned(),
                line
            },
            "{text:?}"
        );
        assert_coded(&error, "FMT_PACKAGE_MANIFEST_INVALID");
    }
    let too_large = SourceTree(
        [("apm.yml".to_owned(), vec![b'a'; MAX_FILE_BYTES + 1])]
            .into_iter()
            .collect(),
    );
    let error = plugins()
        .get(FormatId::Apm)
        .read(&too_large, &request("acme", "x"))
        .err()
        .unwrap();
    assert_eq!(
        error,
        FormatError::PackageManifestInvalid {
            path: "apm.yml".to_owned(),
            line: None
        }
    );
}

// Row: write called on Claude Code or Copilot.
#[test]
fn write_unsupported() {
    for format in [FormatId::ClaudeCode, FormatId::Copilot] {
        let error = plugins()
            .get(format)
            .write(&write_request(Vec::new()))
            .unwrap_err();
        assert_eq!(error, FormatError::WriteUnsupported { format });
        assert_coded(&error, "FMT_WRITE_UNSUPPORTED");
    }
}

// Row: APM write to a repository whose declared version is untested.
#[test]
fn version_untested() {
    let mut request = write_request(Vec::new());
    request.existing = tree(&[("apm.yml", "name: a\nversion: 1.0.0\n")]);
    let error = plugins().get(FormatId::Apm).write(&request).unwrap_err();
    assert_eq!(
        error,
        FormatError::VersionUntested {
            declared: "unversioned".to_owned()
        }
    );
    assert_coded(&error, "FMT_VERSION_UNTESTED");
}

// Row: an Agent Model render or placement fails for a part.
#[test]
fn part_invalid() {
    // A bad publisher cannot mint an identifier.
    let error = plugins()
        .get(FormatId::Apm)
        .read(&load("apm-basic"), &request("Not A Publisher", "x"))
        .err()
        .unwrap();
    assert_eq!(
        error,
        FormatError::PartInvalid {
            identifier: "Not A Publisher".to_owned(),
            code: "AM_ID_INVALID".to_owned(),
        }
    );
    assert_coded(&error, "FMT_PART_INVALID");

    // A changed part of a kind the Agent Model places no file for cannot be rendered.
    let outcome = plugins()
        .get(FormatId::Apm)
        .read(&load("apm-roundtrip"), &request("acme", "x"))
        .unwrap();
    let mut mode = outcome
        .parts
        .into_iter()
        .find(|p| p.kind == PartKind::OperatingMode)
        .expect("a mode");
    mode.manifest.metadata.name = "Renamed".to_owned();
    let error = plugins()
        .get(FormatId::Ordin)
        .write(&write_request(vec![mode]))
        .unwrap_err();
    assert_eq!(
        error,
        FormatError::PartInvalid {
            identifier: "urn:ordin:acme:rules:modes:imported:review".to_owned(),
            code: "AM_NO_FILE_CONVENTION".to_owned(),
        }
    );

    // A part with a status the vocabulary does not know cannot enter a tree source.
    let mut outcome = read_ordin(&load("ordin"));
    outcome.parts[0].manifest.metadata.status = Some("bogus".to_owned());
    let error = TreeSource::new(
        &outcome,
        AgentRepositoryId::new(),
        "local/x",
        &"0".repeat(40),
    )
    .err()
    .unwrap();
    assert!(matches!(&error, FormatError::PartInvalid { code, .. } if code == "AM_STATUS_UNKNOWN"));
}

// Row: propose_kind finds no rule for the file.
#[test]
fn kind_not_proposable() {
    let error = plugins().propose_kind("README.txt", b"hello").unwrap_err();
    assert_eq!(
        error,
        FormatError::KindNotProposable {
            path: "README.txt".to_owned()
        }
    );
    assert_coded(&error, "FMT_KIND_NOT_PROPOSABLE");
}

// Row: TreeSource::root finds no WorkspaceProfile or more than one.
#[test]
fn root_not_single() {
    let two = read_ordin(&load("ordin-two-agents"));
    let source =
        TreeSource::new(&two, AgentRepositoryId::new(), "local/two", &"0".repeat(40)).unwrap();
    let error = source.root().unwrap_err();
    assert_eq!(error, FormatError::RootNotSingle { found: 2 });
    assert_coded(&error, "FMT_ROOT_NOT_SINGLE");

    let none = ReadOutcome {
        parts: Vec::new(),
        mapping_report: MappingReport::default(),
        package: None,
    };
    let source = TreeSource::new(
        &none,
        AgentRepositoryId::new(),
        "local/none",
        &"0".repeat(40),
    )
    .unwrap();
    assert_eq!(
        source.root().unwrap_err(),
        FormatError::RootNotSingle { found: 0 }
    );
}

// Row: TreeSource::new given a commit that is not 40 lowercase hex characters.
#[test]
fn commit_invalid() {
    let outcome = read_ordin(&load("ordin"));
    for commit in ["", "abc", &"A".repeat(40), &"g".repeat(40), &"0".repeat(41)] {
        let error = TreeSource::new(&outcome, AgentRepositoryId::new(), "local/x", commit)
            .err()
            .unwrap_or_else(|| panic!("{commit:?} was accepted"));
        assert_eq!(
            error,
            FormatError::CommitInvalid {
                commit: commit.to_owned()
            }
        );
        assert_coded(&error, "FMT_COMMIT_INVALID");
    }
    assert!(TreeSource::new(
        &outcome,
        AgentRepositoryId::new(),
        "local/x",
        &"abcdef0123".repeat(4)
    )
    .is_ok());
}
