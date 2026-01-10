//! X1: one test per row of AgentModel.md's *Error Table*, provoking the condition and asserting the
//! code and the retryability through the `CodedError` contract a program reads. The table states no
//! HTTP or exit status (every error is a returned value of an in-process API).

use std::collections::BTreeMap;

use ordin_agent_model::*;
use ordin_types::error_registry::lookup;
use ordin_types::CodedError;

fn assert_coded(error: &dyn CodedError, code: &str, retryable: bool) {
    assert_eq!(error.code(), code);
    assert_eq!(error.retryable(), retryable, "{code} retryability");
    assert_eq!(error.to_string(), code, "Display is the bare code");
    let entry = lookup(code).unwrap_or_else(|| panic!("{code} is registered"));
    assert_eq!(entry.retryable, retryable, "{code} registry retryability");
}

fn tree(entries: &[(&str, &[u8])]) -> BTreeMap<String, Vec<u8>> {
    entries
        .iter()
        .map(|(p, b)| ((*p).to_owned(), b.to_vec()))
        .collect()
}

fn manifest(body: &str) -> Manifest {
    let text = format!(
        "apiVersion: ai.enterprise/v1\nkind: workspace-profile\nmetadata:\n  id: urn:ordin:a:workspaces:w\n{body}"
    );
    parse_manifest("w.yaml", text.as_bytes()).unwrap().unwrap()
}

// Row: a file is larger than 1 MiB.
#[test]
fn file_too_large() {
    let mut bytes = b"kind: rule\n".to_vec();
    bytes.resize(1_048_577, b' ');
    let error = parse_manifest("x.yaml", &bytes).unwrap_err();
    assert_eq!(
        error,
        AmError::FileTooLarge {
            path: "x.yaml".into(),
            bytes: 1_048_577
        }
    );
    assert_coded(&error, "AM_FILE_TOO_LARGE", false);
}

// Row: front matter never closed, YAML syntax error, top level not a mapping, duplicate key.
#[test]
fn manifest_malformed() {
    for (path, text) in [
        ("x.md", "---\nkind: rule\nno close\n"),
        ("x.yaml", "kind: [rule\n"),
        ("x.yaml", "[a,\nkind: rule]\n"),
        ("x.yaml", "kind: rule\nkind: skill\n"),
        ("x.yaml", "kind: rule\n? [a]\n: b\n"),
    ] {
        let error = parse_manifest(path, text.as_bytes()).unwrap_err();
        assert!(
            matches!(error, AmError::ManifestMalformed { .. }),
            "{text:?}"
        );
        assert_coded(&error, "AM_MANIFEST_MALFORMED", false);
    }
}

// Row: apiVersion, kind or metadata.id missing.
#[test]
fn manifest_field_missing() {
    let error = parse_manifest("x.yaml", b"kind: rule\nmetadata: {name: x}\n").unwrap_err();
    assert_eq!(
        error,
        AmError::ManifestFieldMissing {
            path: "x.yaml".into(),
            fields: vec!["apiVersion", "metadata.id"]
        }
    );
    assert_coded(&error, "AM_MANIFEST_FIELD_MISSING", false);
}

// Row: anchor, alias or custom tag in a manifest.
#[test]
fn yaml_feature_refused() {
    for text in [
        "kind: rule\na: &x 1\n",
        "kind: rule\nb: &x 1\nc: *x\n",
        "kind: rule\na: !include x.yaml\n",
    ] {
        let error = parse_manifest("x.yaml", text.as_bytes()).unwrap_err();
        assert_coded(&error, "AM_YAML_FEATURE_REFUSED", false);
    }
}

// Row: the authored kind maps to no kind.
#[test]
fn kind_unknown() {
    let reading = read_tree(
        &tree(&[(
            "m/x.yaml",
            b"apiVersion: ai.enterprise/v1\nkind: brand-overlay\nmetadata: {id: urn:ordin:a:overlays:brand:b}\n",
        )]),
        &VocabularySet::default(),
    );
    let (_, error) = &reading.refused[0];
    assert_eq!(
        error,
        &AmError::KindUnknown {
            path: "m/x.yaml".into(),
            authored: "brand-overlay".into()
        }
    );
    assert_coded(error, "AM_KIND_UNKNOWN", false);
}

// Row: apiVersion is not ai.enterprise/v1 when require_api_version is called.
#[test]
fn api_version_unknown() {
    let m = parse_manifest(
        "x.yaml",
        b"apiVersion: ai.enterprise/v2\nkind: rule\nmetadata: {id: urn:ordin:a:rules:r}\n",
    )
    .unwrap()
    .unwrap();
    let error = m.require_api_version().unwrap_err();
    assert!(
        matches!(&error, AmError::ApiVersionUnknown { found, .. } if found == "ai.enterprise/v2")
    );
    assert_coded(&error, "AM_API_VERSION_UNKNOWN", false);
    assert!(manifest("").require_api_version().is_ok());
}

// Row: path absolute, with a parent step, a backslash, a NUL, or not representable in ustar.
#[test]
fn path_invalid() {
    let long = format!("m/{}.md", "n".repeat(120));
    let traversal = concat!("m/..", "/x.md");
    let reading = read_tree(
        &tree(&[
            ("/abs.md", b"x"),
            (traversal, b"x"),
            ("m\\x.md", b"x"),
            ("m/a\0.md", b"x"),
        ]),
        &VocabularySet::default(),
    );
    assert_eq!(reading.refused.len(), 4);
    for (_, error) in &reading.refused {
        assert_coded(error, "AM_PATH_INVALID", false);
    }
    let rule = b"---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:r\n---\n";
    let reading = read_tree(
        &tree(&[("m/r.md", rule), (long.as_str(), b"x")]),
        &VocabularySet::default(),
    );
    assert_coded(&reading.refused[0].1, "AM_PATH_INVALID", false);
}

// Row: an identifier does not parse as a URN where one is required.
#[test]
fn id_invalid() {
    let error = part_file_path(PartKind::Rule, "@a/rules/r").unwrap_err();
    assert_eq!(
        error,
        AmError::IdInvalid {
            id: "@a/rules/r".into()
        }
    );
    assert_coded(&error, "AM_ID_INVALID", false);
}

// Row: a version does not match the version pattern.
#[test]
fn version_invalid() {
    let error = compare_versions("1.0.0", "v2").unwrap_err();
    assert_eq!(
        error,
        AmError::VersionInvalid {
            version: "v2".into()
        }
    );
    assert_coded(&error, "AM_VERSION_INVALID", false);
}

// Row: metadata.status is not a known status term.
#[test]
fn status_unknown() {
    let m = parse_manifest(
        "x.yaml",
        b"apiVersion: ai.enterprise/v1\nkind: rule\nmetadata: {id: urn:ordin:a:rules:r, status: retired}\n",
    )
    .unwrap()
    .unwrap();
    let error = m.status_or_default().unwrap_err();
    assert_coded(&error, "AM_STATUS_UNKNOWN", false);
}

// Row: a typed key is present with the wrong shape.
#[test]
fn field_shape_invalid() {
    let error = manifest("requiredControlLoop: [x]\n")
        .required_control_loop()
        .unwrap_err();
    assert_coded(&error, "AM_FIELD_SHAPE_INVALID", false);
}

// Row: render or placement asked for a kind with no file convention.
#[test]
fn no_file_convention() {
    let error = render_part(PartKind::ToolBinding, &manifest(""), None).unwrap_err();
    assert_eq!(
        error,
        AmError::NoFileConvention {
            kind: PartKind::ToolBinding
        }
    );
    assert_coded(&error, "AM_NO_FILE_CONVENTION", false);
}

// Row: two vocabulary parts for one axis.
#[test]
fn vocabulary_axis_duplicate() {
    let vocabulary = |id: &str| {
        format!("apiVersion: ai.enterprise/v1\nkind: vocabulary\nmetadata: {{id: {id}}}\naxis: category\nterms: []\n")
    };
    let (a, b) = (
        vocabulary("urn:ordin:a:vocabulary:one"),
        vocabulary("urn:ordin:a:vocabulary:two"),
    );
    let reading = read_tree(
        &tree(&[
            ("m/v/one.yaml", a.as_bytes()),
            ("m/w/two.yaml", b.as_bytes()),
        ]),
        &VocabularySet::default(),
    );
    let parts: Vec<&ModelPart> = reading.parts.iter().collect();
    let error = VocabularySet::from_parts(&parts).unwrap_err();
    assert_eq!(
        error,
        AmError::VocabularyAxisDuplicate {
            axis: VocabularyAxis::Category
        }
    );
    assert_coded(&error, "AM_VOCABULARY_AXIS_DUPLICATE", false);
}

// Row: sha256: digest text malformed.
#[test]
fn digest_invalid() {
    let error = parse_prefixed_digest("sha256:ABC").unwrap_err();
    assert_eq!(
        error,
        AmError::DigestInvalid {
            value: "sha256:ABC".into()
        }
    );
    assert_coded(&error, "AM_DIGEST_INVALID", false);
}

// Row: lock file YAML does not parse, schema is not ordin.lock/v1, or a required key is missing.
#[test]
fn lock_file_invalid() {
    for (text, reason) in [
        (&b"planHash: [\n"[..], "yaml"),
        (b"lockfileSchema: ordin.lock/v2\n", "schema"),
        (b"lockfileSchema: ordin.lock/v1\nplanHash: x\n", "key"),
    ] {
        let error = LockFile::from_yaml(text).unwrap_err();
        assert_eq!(error, AmError::LockFileInvalid { reason });
        assert_coded(&error, "AM_LOCK_FILE_INVALID", false);
    }
}

// Row: a PartSource cannot read its clone or index (retryable).
#[test]
fn source_unavailable() {
    assert_coded(
        &PartSourceError::Unavailable { retryable: true },
        "AM_SOURCE_UNAVAILABLE",
        true,
    );
}

// Row: a PartSource has no readable withdrawn list and no earlier copy.
#[test]
fn source_withdrawn_list_unreadable() {
    assert_coded(
        &PartSourceError::WithdrawnListUnreadable {
            repository: "github.com/acme/x".into(),
        },
        "AM_SOURCE_WITHDRAWN_LIST_UNREADABLE",
        false,
    );
}

// Row: PartSource files do not hash to the part's digest.
#[test]
fn source_corrupt() {
    assert_coded(
        &PartSourceError::Corrupt {
            part_identifier: "urn:ordin:a:rules:r".into(),
        },
        "AM_SOURCE_CORRUPT",
        false,
    );
}

// Row: withdrawn.yaml not a mapping, wrong listSchema, entries not a list, entry shape or unknown key.
#[test]
fn withdrawn_list_invalid() {
    let entry = "listSchema: ordin.withdrawn/v1\nentries:\n  - part: {kind: Skill, id: x}\n    reason: r\n    withdrawnAt: t\n";
    for (text, reason, n) in [
        ("- x\n".to_owned(), "not_a_mapping", None),
        ("listSchema: x\n".to_owned(), "schema", None),
        (
            "listSchema: ordin.withdrawn/v1\nentries: {}\n".to_owned(),
            "entries_not_a_list",
            None,
        ),
        (
            "listSchema: ordin.withdrawn/v1\nentries:\n  - reason: r\n".to_owned(),
            "entry_shape",
            Some(1),
        ),
        (
            format!("{entry}    note: x\n"),
            "entry_unknown_key",
            Some(1),
        ),
    ] {
        let error = parse_withdrawn_list(text.as_bytes()).unwrap_err();
        assert_eq!(
            error,
            AmError::WithdrawnListInvalid { reason, entry: n },
            "{text}"
        );
        assert_coded(&error, "AM_WITHDRAWN_LIST_INVALID", false);
    }
}

// Row: draft fields not an object, metadata.id missing, wrong type, a non-integer number, an empty name.
#[test]
fn draft_fields_invalid() {
    let model = FieldModel;
    for fields in [
        serde_json::json!("x"),
        serde_json::json!({ "metadata": {} }),
        serde_json::json!({ "metadata": { "id": "urn:ordin:a:rules:r", "owner": [] } }),
        serde_json::json!({ "metadata": { "id": "urn:ordin:a:rules:r" }, "spec": { "x": 0.5 } }),
    ] {
        let error = model.part_from_fields(PartKind::Rule, &fields).unwrap_err();
        assert_coded(&error, "AM_DRAFT_FIELDS_INVALID", false);
    }
    let draft: ordin_types::domain::agents::AgentDraft = serde_json::from_value(serde_json::json!({
        "id": ordin_types::AgentDraftId::new(), "agent_repository_id": BUNDLED_REPOSITORY_ID,
        "name": "  ", "purpose": "", "jobs": [], "tasks": [], "will": [], "wont": [], "needs": [],
        "reused_parts": [], "origin": "made", "branch": "b", "wording_changes": [],
        "field_authors": [], "trial": false, "state": "draft",
        "created_at": "2026-01-10T00:00:00.000Z", "updated_at": "2026-01-10T00:00:00.000Z"
    }))
    .expect("a draft");
    let error = model
        .agent_parts_from_fields(&draft, "acme", &[], &[])
        .unwrap_err();
    assert_eq!(
        error,
        AmError::DraftFieldsInvalid {
            field: "name".into(),
            expected: "non-empty text"
        }
    );
    assert_coded(&error, "AM_DRAFT_FIELDS_INVALID", false);
}

// Row: a generated or reused part a WorkspaceProfile cannot reference, or a second Memory or ControlLoop.
#[test]
fn draft_kind_not_composable() {
    let draft: ordin_types::domain::agents::AgentDraft =
        serde_json::from_value(serde_json::json!({
            "id": ordin_types::AgentDraftId::new(), "agent_repository_id": BUNDLED_REPOSITORY_ID,
            "name": "Writer", "purpose": "Writes.", "jobs": [], "tasks": [], "will": [], "wont": [],
            "needs": [], "reused_parts": [], "origin": "made", "branch": "b", "wording_changes": [],
            "field_authors": [], "trial": false, "state": "draft",
            "created_at": "2026-01-10T00:00:00.000Z", "updated_at": "2026-01-10T00:00:00.000Z"
        }))
        .expect("a draft");
    let memory = |id: &str| ReusedPart {
        kind: PartKind::Memory,
        identifier: id.to_owned(),
    };
    let cases: [(Vec<ReusedPart>, PartKind); 2] = [
        (
            vec![ReusedPart {
                kind: PartKind::Journey,
                identifier: "urn:ordin:a:journeys:j".into(),
            }],
            PartKind::Journey,
        ),
        (
            vec![
                memory("urn:ordin:a:memories:a"),
                memory("urn:ordin:a:memories:b"),
            ],
            PartKind::Memory,
        ),
    ];
    for (reused, kind) in cases {
        let error = FieldModel
            .agent_parts_from_fields(&draft, "acme", &reused, &[])
            .unwrap_err();
        assert_eq!(error, AmError::DraftKindNotComposable { kind });
        assert_coded(&error, "AM_DRAFT_KIND_NOT_COMPOSABLE", false);
    }
}
