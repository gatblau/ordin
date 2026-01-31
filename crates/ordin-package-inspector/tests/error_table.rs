//! X1 and X4: one test per row of PackageInspector.md's *Error Table*, provoking the condition and
//! asserting the code and the retryability through the `CodedError` contract a program reads, and the
//! registry check of every code the crate owns. The row "folder cannot be listed (transient)" is
//! tested beside the classification itself (`src/error.rs`).

mod support;

use ordin_package_inspector::*;
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::{AgentInstanceId, CodedError, WorkspaceId};
use support::*;

fn assert_coded(error: &InspectError, code: &str, retryable: bool) {
    assert_eq!(CodedError::code(error), code);
    assert_eq!(
        CodedError::retryable(error),
        retryable,
        "{code} retryability"
    );
    assert_eq!(error.to_string(), code, "Display is the bare code");
    assert!(ALL_CODES.contains(&code));
    assert!(lookup(code).is_some(), "{code} is registered");
}

// X4
#[test]
fn every_code_is_registered_to_this_crate_and_its_prefix_is_owned() {
    assert!(!ALL_CODES.is_empty());
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, ALL_CODES, "ALL_CODES is sorted");
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is registered"));
        assert_eq!(entry.owner_crate, "ordin-package-inspector");
        assert!(code.starts_with("PKI_"));
    }
    assert!(owns_prefix("ordin-package-inspector", "PKI"));
    let registered = [
        "PKI_BUNDLE_UNREADABLE",
        "PKI_DECLARATION_INVALID",
        "PKI_DECLARATION_REFUSED",
        "PKI_DECLARATION_TOO_LARGE",
        "PKI_FOLDER_UNREADABLE",
        "PKI_FORMAT_UNRECOGNISED",
        "PKI_LOCK_FILE_INVALID",
        "PKI_NOT_INSPECTED",
        "PKI_PART_NOT_FOUND",
        "PKI_STORE_FAILED",
    ];
    assert_eq!(ALL_CODES, registered);
}

// Row: folder cannot be read, permanent (a folder that is missing or refuses entry).
#[tokio::test]
async fn folder_unreadable_permanent() {
    let f = Fixture::new().await;
    let missing = f.dir.path("missing");
    let error = f
        .inspector
        .inspect(&apm_request(AgentInstanceId::new(), &missing))
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_FOLDER_UNREADABLE", false);
    // Permission denied. Unix only: the folder is locked with mode bits, which Windows does not have.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let locked = f.dir.path("locked");
        std::fs::create_dir_all(&locked).expect("dir");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        let result = f
            .inspector
            .installed_files(AgentInstanceId::new(), &locked)
            .await;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let error = result.unwrap_err();
        assert_coded(&error, "PKI_FOLDER_UNREADABLE", false);
    }
}

// Row: ordin.lock does not parse or has another schema.
#[tokio::test]
async fn lock_file_invalid() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    std::fs::write(
        installed.folder.join("ordin.lock"),
        "lockfileSchema: other/v9\n",
    )
    .expect("write");
    let error = f
        .inspector
        .inspect(&installed.request(AgentInstanceId::new()))
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_LOCK_FILE_INVALID", false);
}

// Row: bundle directory missing, plan hash differs, or a part's primary file cannot be read at part_page.
#[tokio::test]
async fn bundle_unreadable() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    let id = AgentInstanceId::new();
    // No bundle directory given.
    let mut request = installed.request(id);
    request.bundle_dir = None;
    let error = f.inspector.inspect(&request).await.unwrap_err();
    assert_coded(&error, "PKI_BUNDLE_UNREADABLE", false);
    // A bundle directory that does not exist.
    request.bundle_dir = Some(f.dir.path("nowhere"));
    let error = f.inspector.inspect(&request).await.unwrap_err();
    assert_coded(&error, "PKI_BUNDLE_UNREADABLE", false);
    // A plan hash that differs from the lock file's.
    let other = install(&f.dir.0, "b", &ten_commands(), no_policy());
    let mut request = installed.request(id);
    request.bundle_dir = Some(other.bundle_dir.clone());
    let error = f.inspector.inspect(&request).await.unwrap_err();
    assert_coded(&error, "PKI_BUNDLE_UNREADABLE", false);
    // A bundle manifest that is not JSON.
    std::fs::write(other.bundle_dir.join("bundle.manifest.json"), "{").expect("write");
    let error = f.inspector.inspect(&request).await.unwrap_err();
    assert_coded(&error, "PKI_BUNDLE_UNREADABLE", false);
    // A part whose files are gone at part_page.
    f.inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    let part = &installed.lock.artefacts[0];
    let hex = part.content_hash.trim_start_matches("sha256:");
    std::fs::remove_dir_all(installed.bundle_dir.join("artefacts").join(hex)).expect("remove");
    let error = f
        .inspector
        .part_page(id, &part.id, &installed.bundle_dir, Detail::LinksCode)
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_BUNDLE_UNREADABLE", false);
}

// Row: neither ordin.lock nor apm.yml, or the APM plug-in refuses the folder.
#[tokio::test]
async fn format_unrecognised() {
    let f = Fixture::new().await;
    let folder = f.dir.path("plain");
    std::fs::create_dir_all(&folder).expect("dir");
    let error = f
        .inspector
        .inspect(&apm_request(AgentInstanceId::new(), &folder))
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_FORMAT_UNRECOGNISED", false);
    // apm.yml without a version: the plug-in refuses it.
    std::fs::write(folder.join("apm.yml"), "name: only-a-name\n").expect("write");
    let error = f
        .inspector
        .inspect(&apm_request(AgentInstanceId::new(), &folder))
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_FORMAT_UNRECOGNISED", false);
}

// Row: declaration over 1 MiB (1,048,576 bytes).
#[tokio::test]
async fn declaration_too_large() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    installed.declaration(&format!("# {}\n", "x".repeat(1_048_576)));
    let error = f
        .inspector
        .inspect(&installed.request(AgentInstanceId::new()))
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_DECLARATION_TOO_LARGE", false);
    assert!(matches!(
        error,
        InspectError::DeclarationTooLarge { bytes } if bytes > 1_048_576
    ));
    // Exactly 1 MiB is accepted by the size check.
    let text = format!("format-version: 1\n# {}\n", "x".repeat(1_048_576 - 21));
    assert_eq!(text.len(), 1_048_576);
    installed.declaration(&text);
    f.inspector
        .inspect(&installed.request(AgentInstanceId::new()))
        .await
        .expect("a declaration of exactly 1 MiB");
}

// Row: wrightrun-declaration.yaml does not parse into the Rules Engine's Declaration.
#[tokio::test]
async fn declaration_invalid() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    for text in [
        b"format-version: 1\nx: &a 1\ny: *a\n".to_vec(),
        b"- just\n- a list\n".to_vec(),
        b"format-version: one\n".to_vec(),
        vec![0xff, 0xfe, 0x00],
    ] {
        std::fs::write(installed.folder.join("wrightrun-declaration.yaml"), &text).expect("write");
        let error = f
            .inspector
            .inspect(&installed.request(AgentInstanceId::new()))
            .await
            .unwrap_err();
        assert_coded(&error, "PKI_DECLARATION_INVALID", false);
    }
}

// Row: the Rules Engine refuses the rule set.
#[tokio::test]
async fn declaration_refused() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    installed.declaration(UNBOUND_DECLARATION);
    let error = f
        .inspector
        .inspect(&installed.request(AgentInstanceId::new()))
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_DECLARATION_REFUSED", false);
    assert!(matches!(
        error,
        InspectError::DeclarationRefused {
            code: "RULE_UNBOUND_TOOL",
            ..
        }
    ));
    // The retryable flag of the engine's error is carried unchanged.
    let retryable = InspectError::DeclarationRefused {
        code: "RULE_STATE_UNAVAILABLE",
        retryable: true,
    };
    assert_coded(&retryable, "PKI_DECLARATION_REFUSED", true);
}

// Row: reload_rule_sets: one stored set unparseable or refused.
#[tokio::test]
async fn reload_failure_for_an_unparseable_set() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    let id = AgentInstanceId::new();
    f.inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    std::fs::write(f.rules_file(id), "not json").expect("write");
    // A file whose name is not an instance id is ignored.
    std::fs::write(f.store_dir.join("stray.rules.json"), "{}").expect("write");
    let failures = f.inspector.reload_rule_sets().await.expect("reload");
    assert_eq!(
        failures,
        [RuleSetReloadFailure {
            agent_instance_id: id,
            code: "PKI_STORE_FAILED".to_owned()
        }]
    );
    assert!(lookup(&failures[0].code).is_some());
}

// Row: model, made_of or part_page before any inspect.
#[tokio::test]
async fn not_inspected() {
    let f = Fixture::new().await;
    let id = AgentInstanceId::new();
    let error = f.inspector.model(id).await.unwrap_err();
    assert_coded(&error, "PKI_NOT_INSPECTED", false);
    let error = f
        .inspector
        .made_of(id, Detail::BusinessUser)
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_NOT_INSPECTED", false);
    let error = f
        .inspector
        .part_page(id, "urn:ordin:default:rules:x", &f.dir.0, Detail::LinksCode)
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_NOT_INSPECTED", false);
}

// Row: part_page for a part the model does not hold.
#[tokio::test]
async fn part_not_found() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    let id = AgentInstanceId::new();
    f.inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    let error = f
        .inspector
        .part_page(
            id,
            "urn:ordin:default:rules:absent",
            &installed.bundle_dir,
            Detail::BusinessUser,
        )
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_PART_NOT_FOUND", false);
}

// Row: model file write fails.
#[tokio::test]
async fn model_file_write_fails() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    let id = AgentInstanceId::new();
    // A directory stands where the model file goes, so the rename into place fails.
    std::fs::create_dir_all(f.model_file(id)).expect("dir");
    let error = f
        .inspector
        .inspect(&installed.request(id))
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_STORE_FAILED", true);
}

// Row: stored model unparseable (or of another schema).
#[tokio::test]
async fn stored_model_unparseable() {
    let f = Fixture::new().await;
    let installed = install(&f.dir.0, "a", &ten_commands(), no_policy());
    let id = AgentInstanceId::new();
    let outcome = f
        .inspector
        .inspect(&installed.request(id))
        .await
        .expect("inspect");
    std::fs::write(f.model_file(id), "{ not json").expect("write");
    let error = f.inspector.model(id).await.unwrap_err();
    assert_coded(&error, "PKI_STORE_FAILED", false);
    let error = f
        .inspector
        .made_of(id, Detail::LinksCode)
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_STORE_FAILED", false);
    // Another schema.
    let mut value = serde_json::to_value(&outcome.model).expect("json");
    value["model_schema"] = serde_json::json!("ordin.package-model/v9");
    std::fs::write(f.model_file(id), serde_json::to_vec(&value).expect("json")).expect("write");
    let error = f.inspector.model(id).await.unwrap_err();
    assert_coded(&error, "PKI_STORE_FAILED", false);
}

// Row: memberships.json write fails.
#[tokio::test]
async fn memberships_write_fails() {
    let f = Fixture::new().await;
    std::fs::remove_dir_all(&f.store_dir).expect("remove the store");
    let error = f
        .inspector
        .set_workspaces(AgentInstanceId::new(), vec![WorkspaceId::new()])
        .await
        .unwrap_err();
    assert_coded(&error, "PKI_STORE_FAILED", true);
}

// Row: memberships.json unparseable at directory.
#[tokio::test]
async fn memberships_unparseable_at_directory() {
    let f = Fixture::new().await;
    std::fs::write(f.store_dir.join("memberships.json"), "[1, 2").expect("write");
    let error = f.inspector.directory(WorkspaceId::new()).await.unwrap_err();
    assert_coded(&error, "PKI_STORE_FAILED", false);
}
