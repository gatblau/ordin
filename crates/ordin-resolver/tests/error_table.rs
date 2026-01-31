//! X1: one test per row of ResolverAndCompiler.md's *Error Table*, provoking the condition and asserting
//! the code, the payload and the retryability through the `CodedError` contract a program reads.

mod common;

use std::sync::Arc;

use common::*;
use ordin_agent_model::testkit::{ContractFixture, InMemorySource};
use ordin_agent_model::{
    ModelPart, PartKind, PartSourceError, PartStatus, RepositoryRef, StagedPart,
};
use ordin_resolver::{ResolveError, ResolverAndCompiler};
use ordin_types::error_registry::lookup;
use ordin_types::{AgentRepositoryId, CodedError};

fn assert_coded(error: &ResolveError, code: &str, retryable: bool) {
    let coded: &dyn CodedError = error;
    assert_eq!(coded.code(), code);
    assert_eq!(coded.retryable(), retryable, "{code} retryability");
    assert_eq!(error.to_string(), code, "Display is the bare code");
    let entry = lookup(code).unwrap_or_else(|| panic!("{code} is registered"));
    if !entry.retryable {
        assert!(!retryable, "{code} is permanent in the registry");
    }
}

async fn resolve_error(
    cat: &Cat,
    withdrawn: Vec<ordin_agent_model::WithdrawnListEntry>,
    request: ordin_resolver::ResolveRequest,
) -> ResolveError {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(cat, withdrawn));
    error_of(resolver.resolve(&request).await)
}

const CONTROL_LOOP_1: &str = "urn:ordin:t:rules:control-loops:one";
const CONTROL_LOOP_2: &str = "urn:ordin:t:rules:control-loops:two";
const MODE_REVIEW: &str = "urn:ordin:t:rules:modes:review";
const MODE_OTHER: &str = "urn:ordin:t:rules:modes:other";

fn loop_part(cat: &mut Cat, id: &str, modes: &[&str]) {
    let list = if modes.is_empty() {
        "modes: []\n".to_owned()
    } else {
        format!(
            "modes:\n{}",
            modes
                .iter()
                .map(|m| format!("  - {m}\n"))
                .collect::<String>()
        )
    };
    cat.md(
        "control-loop",
        id,
        "1.0.0",
        "",
        &format!("strategy: observe\ntrigger: every-task\n{list}"),
        "Observe first.",
    );
}

fn mode_part(cat: &mut Cat, id: &str) {
    cat.yaml(
        "operating-mode",
        id,
        "1.0.0",
        "",
        "permissions:\n  write: false\n",
    );
}

// Row: root identifier not in scope.
#[tokio::test]
async fn root_not_found() {
    let error = resolve_error(
        &Cat::basic(""),
        vec![],
        request("urn:ordin:t:workspaces:missing"),
    )
    .await;
    assert!(
        matches!(&error, ResolveError::RootNotFound { id } if id == "urn:ordin:t:workspaces:missing")
    );
    assert_coded(&error, "RES_ROOT_NOT_FOUND", false);
}

// Row: root is not a WorkspaceProfile.
#[tokio::test]
async fn not_installable() {
    let error = resolve_error(&Cat::basic(""), vec![], request(RULE_A)).await;
    assert!(matches!(
        error,
        ResolveError::NotInstallable {
            kind: PartKind::Rule
        }
    ));
    assert_coded(&error, "RES_NOT_INSTALLABLE", false);
}

// Row: root is draft (without allow_draft_root) or deprecated.
#[tokio::test]
async fn root_not_approved() {
    for status in [PartStatus::Draft, PartStatus::Deprecated] {
        let dir = TempDir::new();
        let mut source = FaultySource::new(memory(&Cat::basic(""), vec![]));
        source.tweak = Some(Arc::new(move |p| {
            if p.part.identifier == ROOT {
                p.part.status = status;
            }
        }));
        let resolver = bound(&dir, source);
        let error = error_of(resolver.resolve(&request(ROOT)).await);
        assert!(
            matches!(&error, ResolveError::RootNotApproved { id, status: s } if id == ROOT && *s == status),
            "{error:?}"
        );
        assert_coded(&error, "RES_ROOT_NOT_APPROVED", false);
    }
    // A draft root passes with allow_draft_root.
    let dir = TempDir::new();
    let mut source = FaultySource::new(memory(&Cat::basic(""), vec![]));
    source.tweak = Some(Arc::new(|p| {
        if p.part.identifier == ROOT {
            p.part.status = PartStatus::Draft;
        }
    }));
    let resolver = bound(&dir, source);
    let mut draft = request(ROOT);
    draft.allow_draft_root = true;
    assert!(resolver.resolve(&draft).await.is_ok());
}

// Row: a non-staged constituent is draft.
#[tokio::test]
async fn draft_constituent() {
    let dir = TempDir::new();
    let mut source = FaultySource::new(memory(&Cat::basic(""), vec![]));
    source.tweak = Some(Arc::new(|p| {
        if p.part.identifier == RULE_B {
            p.part.status = PartStatus::Draft;
        }
    }));
    let resolver = bound(&dir, source);
    let error = error_of(resolver.resolve(&request(ROOT)).await);
    assert!(matches!(&error, ResolveError::DraftConstituent { part } if part == RULE_B));
    assert_coded(&error, "RES_DRAFT_CONSTITUENT", false);
}

// Row: a reference resolves to nothing.
#[tokio::test]
async fn reference_unresolved() {
    let cat = Cat::basic("contextResources:\n  - urn:ordin:t:context-resources:missing\n");
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    match &error {
        ResolveError::ReferenceUnresolved { from, id, relation } => {
            assert_eq!(from, ROOT);
            assert_eq!(id, "urn:ordin:t:context-resources:missing");
            assert_eq!(relation, "contextResources");
        }
        other => panic!("{other:?}"),
    }
    assert_coded(&error, "RES_REFERENCE_UNRESOLVED", false);
}

// Row: the same identifier in two repositories at one resolution step.
#[tokio::test]
async fn source_ambiguous() {
    let repository = |n: u128| {
        AgentRepositoryId(uuid::Uuid::from_u128(
            0x0190_0000_0000_7000_8000_0000_0000_0000 | n,
        ))
    };
    let (home, first, second) = (
        RepositoryRef::Repository(repository(0xa1)),
        RepositoryRef::Repository(repository(0xa2)),
        RepositoryRef::Repository(repository(0xa3)),
    );
    let mut home_cat = Cat::basic("");
    home_cat.files.retain(|path, _| !path.contains("alpha"));
    let mut one = Cat::new();
    one.rule(RULE_A, "From one.");
    let mut two = Cat::new();
    two.rule(RULE_A, "From two.");
    let source = InMemorySource::new(&ContractFixture {
        repositories: vec![
            (
                home.clone(),
                "github.com/t/home".to_owned(),
                home_cat.parts(),
            ),
            (first.clone(), "github.com/t/one".to_owned(), one.parts()),
            (second.clone(), "github.com/t/two".to_owned(), two.parts()),
        ],
        withdrawn: vec![],
        refused: vec![],
    });
    let dir = TempDir::new();
    let resolver = bound(&dir, source);
    let mut req = request(ROOT);
    req.scope = ordin_agent_model::SourceScope {
        home,
        profile: vec![first, second],
        include_bundled: false,
    };
    let error = error_of(resolver.resolve(&req).await);
    match &error {
        ResolveError::SourceAmbiguous { id, repositories } => {
            assert_eq!(id, RULE_A);
            assert_eq!(
                repositories,
                &["github.com/t/one".to_owned(), "github.com/t/two".to_owned()]
            );
        }
        other => panic!("{other:?}"),
    }
    assert_coded(&error, "RES_SOURCE_AMBIGUOUS", false);
}

// Row: reference cycle, or depth over 64.
#[tokio::test]
async fn dependency_cycle() {
    let (a, b) = (
        "urn:ordin:t:context-resources:a",
        "urn:ordin:t:context-resources:b",
    );
    let mut cat = Cat::basic(&format!("contextResources:\n  - {a}\n"));
    cat.yaml(
        "context-resource",
        a,
        "1.0.0",
        "",
        &format!("contextResources:\n  - {b}\n"),
    );
    cat.yaml(
        "context-resource",
        b,
        "1.0.0",
        "",
        &format!("contextResources:\n  - {a}\n"),
    );
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    match &error {
        ResolveError::DependencyCycle { path } => assert_eq!(path, &[ROOT, a, b, a]),
        other => panic!("{other:?}"),
    }
    assert_coded(&error, "RES_DEPENDENCY_CYCLE", false);

    // A chain of 66 parts is deeper than 64.
    let link = |n: usize| format!("urn:ordin:t:context-resources:link{n:02}");
    let mut deep = Cat::basic(&format!("contextResources:\n  - {}\n", link(0)));
    for n in 0..66 {
        let body = if n < 65 {
            format!("contextResources:\n  - {}\n", link(n + 1))
        } else {
            String::new()
        };
        deep.yaml("context-resource", &link(n), "1.0.0", "", &body);
    }
    let error = resolve_error(&deep, vec![], request(ROOT)).await;
    assert!(
        matches!(&error, ResolveError::DependencyCycle { path } if path.len() > 64),
        "{error:?}"
    );
}

// Row: closure over 1,000 parts.
#[tokio::test]
async fn closure_too_large() {
    let id = |n: usize| format!("urn:ordin:t:context-resources:r{n:04}");
    let refs: String = (0..1_001).map(|n| format!("  - {}\n", id(n))).collect();
    let mut cat = Cat::basic(&format!("contextResources:\n{refs}"));
    for n in 0..1_001 {
        cat.md("context-resource", &id(n), "1.0.0", "", "", "t");
    }
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    assert!(
        matches!(error, ResolveError::ClosureTooLarge { parts } if parts > 1_000),
        "{error:?}"
    );
    assert_coded(&error, "RES_CLOSURE_TOO_LARGE", false);
}

fn conflicting_catalogue() -> Cat {
    let (skill_a, skill_b) = (
        "urn:ordin:a:skills:quality-codegen",
        "urn:ordin:b:skills:quality-codegen",
    );
    let mut cat = Cat::basic(&format!("selectablePracticePacks:\n  - {GO}\n  - {RUST}\n"));
    cat.pack(GO, "stack", &["go"], &format!("skills:\n  - {skill_a}\n"));
    cat.pack(
        RUST,
        "stack",
        &["rust"],
        &format!("skills:\n  - {skill_b}\n"),
    );
    cat.skill(skill_a, "1.0.0", "");
    cat.skill(skill_b, "1.0.0", "");
    cat
}

// Row: two distinct parts fill one slot.
#[tokio::test]
async fn conflict() {
    let mut req = request(ROOT);
    req.workspace_languages = vec!["go".into(), "rust".into()];
    let error = resolve_error(&conflicting_catalogue(), vec![], req).await;
    match &error {
        ResolveError::Conflict { conflicts } => {
            assert_eq!(conflicts.len(), 1);
            assert_eq!(conflicts[0].slot, "Skill/quality-codegen");
            assert_eq!(conflicts[0].candidates.len(), 2);
            let requesters: Vec<&str> = conflicts[0]
                .candidates
                .iter()
                .flat_map(|c| c.requested_by.iter().map(|r| r.id.as_str()))
                .collect();
            assert_eq!(requesters, [GO, RUST]);
        }
        other => panic!("{other:?}"),
    }
    assert_coded(&error, "RES_CONFLICT", false);
}

// Row: a resolved part is withdrawn and no replacement was given.
#[tokio::test]
async fn part_withdrawn() {
    let error = resolve_error(
        &Cat::basic(""),
        vec![withdrawal(PartKind::Rule, RULE_B, None)],
        request(ROOT),
    )
    .await;
    assert!(
        matches!(&error, ResolveError::PartWithdrawn { part, replacement: None } if part == RULE_B)
    );
    assert_coded(&error, "RES_PART_WITHDRAWN", false);
    // The list names a replacement the request did not take.
    let named = withdrawal(PartKind::Rule, RULE_B, Some("urn:ordin:t:rules:beta-v2"));
    let error = resolve_error(&Cat::basic(""), vec![named], request(ROOT)).await;
    assert!(
        matches!(&error, ResolveError::PartWithdrawn { replacement: Some(r), .. } if r == "urn:ordin:t:rules:beta-v2")
    );
}

// Row: a required pack rule cannot be satisfied and is not left to ask.
#[tokio::test]
async fn required_pack_unsettled() {
    let mut cat =
        Cat::basic("requiredPracticePacks:\n  - atLeast: 1\n    where:\n      class: stack\n");
    cat.with_packs();
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    assert!(
        matches!(&error, ResolveError::RequiredPackUnsettled { rule_index: 0, class: Some(c) } if c == "stack")
    );
    assert_coded(&error, "RES_REQUIRED_PACK_UNSETTLED", false);
}

// Row: an answered or changed pack is not allowed by the agent.
#[tokio::test]
async fn pack_not_allowed() {
    let body = "requiredPracticePacks:\n  - atLeast: 1\n    where:\n      class: testing\nfixedPracticePacks:\n  - ask:\n      class: testing\n";
    let mut cat = Cat::basic(body);
    cat.with_packs();
    let mut answered = request(ROOT);
    answered.pack_answers = vec![ordin_resolver::PackAnswer {
        rule_index: 0,
        pack_ids: vec![GO.into()],
    }];
    let error = resolve_error(&cat, vec![], answered).await;
    assert!(matches!(&error, ResolveError::PackNotAllowed { id } if id == GO));
    assert_coded(&error, "RES_PACK_NOT_ALLOWED", false);
    // A change naming a pack that is neither a candidate nor selectable is refused as well.
    let mut changed = request(ROOT);
    changed.pack_changes = vec![RUST.into()];
    let error = resolve_error(&cat, vec![], changed).await;
    assert!(matches!(&error, ResolveError::PackNotAllowed { id } if id == RUST));
}

// Row: a required overlay axis is unsatisfied.
#[tokio::test]
async fn required_overlay_missing() {
    let mut cat = Cat::basic(&format!(
        "requiredStyleOverlays:\n  anyOf:\n    - {PLAIN}\n"
    ));
    cat.overlay("style-overlay", PLAIN, "Plain.");
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    assert!(matches!(
        error,
        ResolveError::RequiredOverlayMissing {
            axis: PartKind::StyleOverlay
        }
    ));
    assert_coded(&error, "RES_REQUIRED_OVERLAY_MISSING", false);
}

fn language_offer(available: &[&str], default: &str) -> String {
    let list: String = available
        .iter()
        .map(|id| format!("      - {id}\n"))
        .collect();
    format!(
        "governanceMetadata:\n  languageOverlays:\n    available:\n{list}    default: {default}\n"
    )
}

// Row: a changed overlay is not in the axis's `available` list.
#[tokio::test]
async fn overlay_not_allowed() {
    let mut cat = Cat::basic(&language_offer(&[EN_GB], EN_GB));
    cat.with_languages();
    let mut req = request(ROOT);
    req.overlay_changes = vec![FR.into()];
    let error = resolve_error(&cat, vec![], req).await;
    assert!(matches!(&error, ResolveError::OverlayNotAllowed { id } if id == FR));
    assert_coded(&error, "RES_OVERLAY_NOT_ALLOWED", false);
    // An identifier that is no overlay at all is refused the same way.
    let mut unknown = request(ROOT);
    unknown.overlay_changes = vec!["urn:ordin:t:overlays:language:xx".into()];
    let error = resolve_error(&cat, vec![], unknown).await;
    assert!(matches!(error, ResolveError::OverlayNotAllowed { .. }));
}

// Row: two overlays on an exactly-one axis.
#[tokio::test]
async fn overlay_cardinality() {
    let mut cat = Cat::basic(&language_offer(&[EN_GB, FR], EN_GB));
    cat.with_languages();
    let mut req = request(ROOT);
    req.overlay_changes = vec![EN_GB.into(), FR.into()];
    let error = resolve_error(&cat, vec![], req).await;
    assert!(matches!(
        error,
        ResolveError::OverlayCardinality {
            axis: PartKind::LanguageOverlay
        }
    ));
    assert_coded(&error, "RES_OVERLAY_CARDINALITY", false);
}

// Row: an overlay's `appliesTo` excludes this agent.
#[tokio::test]
async fn overlay_not_applicable() {
    let mut cat = Cat::basic(&language_offer(&[EN_GB], EN_GB));
    cat.md(
        "language-overlay",
        EN_GB,
        "1.0.0",
        "",
        "appliesTo:\n  workspaces:\n    - urn:ordin:t:workspaces:other\n",
        "Language text.",
    );
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    assert!(matches!(&error, ResolveError::OverlayNotApplicable { overlay } if overlay == EN_GB));
    assert_coded(&error, "RES_OVERLAY_NOT_APPLICABLE", false);
}

// Row: more than one memory contract.
#[tokio::test]
async fn memory_cardinality() {
    let (m1, m2) = ("urn:ordin:t:memories:one", "urn:ordin:t:memories:two");
    let holder = "urn:ordin:t:context-resources:holder";
    let mut cat = Cat::basic(&format!("memory: {m1}\ncontextResources:\n  - {holder}\n"));
    cat.yaml(
        "context-resource",
        holder,
        "1.0.0",
        "",
        &format!("memory: {m2}\n"),
    );
    for id in [m1, m2] {
        cat.yaml("memory", id, "1.0.0", "", "retains:\n  - decisions\n");
    }
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    assert!(matches!(&error, ResolveError::MemoryCardinality { parts } if parts == &[m1, m2]));
    assert_coded(&error, "RES_MEMORY_CARDINALITY", false);
}

// Row: required control loop absent.
#[tokio::test]
async fn loop_missing() {
    let error = resolve_error(
        &Cat::basic("requiredControlLoop:\n  required: true\n"),
        vec![],
        request(ROOT),
    )
    .await;
    assert!(matches!(error, ResolveError::LoopMissing));
    assert_coded(&error, "RES_LOOP_MISSING", false);
}

// Row: more than one control loop.
#[tokio::test]
async fn loop_multiple() {
    let holder = "urn:ordin:t:context-resources:holder";
    let mut cat = Cat::basic(&format!(
        "controlLoop: {CONTROL_LOOP_1}\ncontextResources:\n  - {holder}\n"
    ));
    cat.yaml(
        "context-resource",
        holder,
        "1.0.0",
        "",
        &format!("controlLoop: {CONTROL_LOOP_2}\n"),
    );
    loop_part(&mut cat, CONTROL_LOOP_1, &[]);
    loop_part(&mut cat, CONTROL_LOOP_2, &[]);
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    assert!(
        matches!(&error, ResolveError::LoopMultiple { parts } if parts == &[CONTROL_LOOP_1, CONTROL_LOOP_2])
    );
    assert_coded(&error, "RES_LOOP_MULTIPLE", false);
}

// Row: loop outside the root's allow-list.
#[tokio::test]
async fn loop_not_permitted() {
    let mut cat = Cat::basic(&format!(
        "controlLoop: {CONTROL_LOOP_1}\nrequiredControlLoop:\n  ids:\n    - {CONTROL_LOOP_2}\n"
    ));
    loop_part(&mut cat, CONTROL_LOOP_1, &[]);
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    assert!(
        matches!(&error, ResolveError::LoopNotPermitted { loop_id } if loop_id == CONTROL_LOOP_1)
    );
    assert_coded(&error, "RES_LOOP_NOT_PERMITTED", false);
}

// Row: loop names a mode the root does not allow.
#[tokio::test]
async fn loop_mode_escape() {
    let mut cat = Cat::basic(&format!(
        "controlLoop: {CONTROL_LOOP_1}\nallowedOperatingModes:\n  - {MODE_REVIEW}\n"
    ));
    loop_part(&mut cat, CONTROL_LOOP_1, &[MODE_OTHER]);
    mode_part(&mut cat, MODE_REVIEW);
    mode_part(&mut cat, MODE_OTHER);
    let error = resolve_error(&cat, vec![], request(ROOT)).await;
    assert!(
        matches!(&error, ResolveError::LoopModeEscape { loop_id, mode } if loop_id == CONTROL_LOOP_1 && mode == MODE_OTHER),
        "{error:?}"
    );
    assert_coded(&error, "RES_LOOP_MODE_ESCAPE", false);
    // The same loop passes when the root allows its mode.
    let mut ok = Cat::basic(&format!(
        "controlLoop: {CONTROL_LOOP_1}\nallowedOperatingModes:\n  - {MODE_OTHER}\n"
    ));
    loop_part(&mut ok, CONTROL_LOOP_1, &[MODE_OTHER]);
    mode_part(&mut ok, MODE_OTHER);
    let dir = TempDir::new();
    assert!(bound(&dir, memory(&ok, vec![]))
        .resolve(&request(ROOT))
        .await
        .is_ok());
}

// Row: an MCP binding lacks name, server type or a valid transport.
#[tokio::test]
async fn tool_policy_invalid() {
    let cases = [
        (
            "- name: ''\n    type: mcp\n    serverType: http\n    transport: http\n",
            "name_missing",
        ),
        (
            "- name: mail\n    type: mcp\n    transport: http\n",
            "server_type_missing",
        ),
        (
            "- name: mail\n    type: mcp\n    serverType: http\n    transport: carrier-pigeon\n",
            "transport_invalid",
        ),
        (
            "- name: mail\n    type: mcp\n    serverType: http\n",
            "transport_invalid",
        ),
    ];
    for (binding, reason) in cases {
        let mut cat = Cat::basic(&format!("toolBindings:\n  - {BINDING}\n"));
        cat.yaml(
            "tool-binding",
            BINDING,
            "1.0.0",
            "",
            &format!("bindings:\n  {binding}"),
        );
        let error = resolve_error(&cat, vec![], request(ROOT)).await;
        assert!(
            matches!(&error, ResolveError::ToolPolicyInvalid { binding, reason: r } if binding == BINDING && *r == reason),
            "{reason}: {error:?}"
        );
        assert_coded(&error, "RES_TOOL_POLICY_INVALID", false);
    }
}

// Row: a manifest field the resolver reads has the wrong shape.
#[tokio::test]
async fn manifest_invalid() {
    let error = resolve_error(
        &Cat::basic("requiredPracticePacks: 3\n"),
        vec![],
        request(ROOT),
    )
    .await;
    assert!(
        matches!(&error, ResolveError::ManifestInvalid { part, code } if part == ROOT && code == "AM_FIELD_SHAPE_INVALID")
    );
    assert_coded(&error, "RES_MANIFEST_INVALID", false);
}

// Row: manifest invalid, from a staged part whose file name leaves the bundle (compile).
#[tokio::test]
async fn manifest_invalid_for_an_unsafe_file_name() {
    let mut cat = Cat::new();
    cat.rule("urn:ordin:t:rules:staged", "Staged.");
    let mut part: ModelPart = cat.parts().remove(0);
    part.files
        .0
        .insert(["..", "escape.txt"].join("/"), b"x".to_vec());
    part.content_digest = ordin_agent_model::content_digest(&part.files);
    let mut req = request(ROOT);
    req.staged = vec![StagedPart {
        part,
        repository: RepositoryRef::Bundled,
        repository_label: "bundled".into(),
    }];
    // The root refers to the staged rule instead of beta.
    let mut base = Cat::basic("");
    base.files.retain(|path, _| !path.contains("beta"));
    let path = base
        .files
        .keys()
        .find(|p| p.contains("workspace"))
        .cloned()
        .unwrap();
    let text = String::from_utf8(base.files[&path].clone()).unwrap();
    base.files.insert(
        path,
        text.replace(RULE_B, "urn:ordin:t:rules:staged")
            .into_bytes(),
    );
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&base, vec![]));
    let error = error_of(resolver.build(&req).await);
    assert!(
        matches!(&error, ResolveError::ManifestInvalid { code, .. } if code == "AM_PATH_INVALID"),
        "{error:?}"
    );
    assert_coded(&error, "RES_MANIFEST_INVALID", false);
    assert_eq!(
        listing(&dir.path().join("bundles")),
        Vec::<String>::new(),
        "no staging directory is left"
    );
}

// Row: a part's files do not hash to its recorded digest.
#[tokio::test]
async fn digest_mismatch() {
    let dir = TempDir::new();
    let mut source = FaultySource::new(memory(&Cat::basic(""), vec![]));
    source.tamper_files = true;
    let resolver = bound(&dir, source);
    let error = error_of(resolver.build(&request(ROOT)).await);
    assert!(matches!(error, ResolveError::DigestMismatch { .. }));
    assert_coded(&error, "RES_DIGEST_MISMATCH", false);
    // A row whose files are gone reads the same way.
    let dir = TempDir::new();
    let mut gone = FaultySource::new(memory(&Cat::basic(""), vec![]));
    gone.files_error = Some(PartSourceError::Corrupt {
        part_identifier: RULE_A.into(),
    });
    let resolver = bound(&dir, gone);
    let error = error_of(resolver.build(&request(ROOT)).await);
    assert!(matches!(&error, ResolveError::DigestMismatch { part } if part == RULE_A));
}

// Row: a lock-file entry is gone, withdrawn or changed in the source.
#[tokio::test]
async fn lock_part_unavailable() {
    let dir = TempDir::new();
    let built = bound(&dir, memory(&Cat::basic(""), vec![]))
        .build(&request(ROOT))
        .await
        .unwrap();
    // Gone: the source holds another version of the skill only.
    let mut newer = Cat::basic("");
    newer.files.retain(|path, _| !path.contains("craft"));
    newer.skill(SKILL, "1.1.0", "");
    let other = TempDir::new();
    let resolver = bound(&other, memory(&newer, vec![]));
    let error = error_of(resolver.resolve_lock(&built.lock_file, &scope()).await);
    assert!(matches!(&error, ResolveError::LockPartUnavailable { part } if part == SKILL));
    assert_coded(&error, "RES_LOCK_PART_UNAVAILABLE", false);
    // Withdrawn.
    let resolver = bound(
        &other,
        memory(
            &Cat::basic(""),
            vec![withdrawal(PartKind::Rule, RULE_A, None)],
        ),
    );
    let error = error_of(resolver.resolve_lock(&built.lock_file, &scope()).await);
    assert!(matches!(&error, ResolveError::LockPartUnavailable { part } if part == RULE_A));
    // Changed.
    let mut changed = Cat::basic("");
    changed.rule(RULE_A, "Different text.");
    let resolver = bound(&other, memory(&changed, vec![]));
    let error = error_of(resolver.resolve_lock(&built.lock_file, &scope()).await);
    assert!(matches!(&error, ResolveError::LockPartUnavailable { part } if part == RULE_A));
}

// Row: recomputed plan hash differs from the lock file's.
#[tokio::test]
async fn lock_mismatch() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let built = resolver.build(&request(ROOT)).await.unwrap();
    let mut lock = built.lock_file.clone();
    let forged = format!("sha256:{}", "0".repeat(64));
    lock.plan_hash = forged.clone();
    let error = error_of(resolver.resolve_lock(&lock, &scope()).await);
    match &error {
        ResolveError::LockMismatch { expected, found } => {
            assert_eq!(expected, &forged);
            assert_eq!(found, &built.plan.plan_hash);
        }
        other => panic!("{other:?}"),
    }
    assert_coded(&error, "RES_LOCK_MISMATCH", false);
}

// Row: no bundle directory for a plan hash.
#[tokio::test]
async fn bundle_not_found() {
    let dir = TempDir::new();
    let resolver = ResolverAndCompiler::new(config(&dir)).unwrap();
    for hash in [
        format!("sha256:{}", "0".repeat(64)),
        "not-a-hash".to_owned(),
        format!("sha256:{}", [".."; 2].join("/")),
    ] {
        let error = error_of(resolver.open_bundle(&hash));
        assert!(matches!(&error, ResolveError::BundleNotFound { plan_hash } if *plan_hash == hash));
        assert_coded(&error, "RES_BUNDLE_NOT_FOUND", false);
    }
}

// Row: bundle manifest, lock file or part files fail their digests.
#[tokio::test]
async fn bundle_corrupt() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let built = resolver.build(&request(ROOT)).await.unwrap();
    let rule = built
        .plan
        .resolved
        .iter()
        .find(|p| p.id == RULE_A)
        .unwrap()
        .clone();
    let file = built
        .part_dir(&rule)
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&file, b"tampered").unwrap();
    let error = error_of(resolver.open_bundle(&built.plan.plan_hash));
    assert!(
        matches!(&error, ResolveError::BundleCorrupt { plan_hash } if *plan_hash == built.plan.plan_hash)
    );
    assert_coded(&error, "RES_BUNDLE_CORRUPT", false);
    assert!(matches!(
        error_of(built.read_part_files(&rule)),
        ResolveError::BundleCorrupt { .. }
    ));
    // A lock file that no longer matches the manifest's digest is corrupt too.
    let again = bound(&TempDir::new(), memory(&Cat::basic(""), vec![]));
    let fresh = again.build(&request(ROOT)).await.unwrap();
    std::fs::write(
        fresh.dir.join("ordin.lock"),
        b"lockfileSchema: ordin.lock/v1\n",
    )
    .unwrap();
    assert!(matches!(
        error_of(again.open_bundle(&fresh.plan.plan_hash)),
        ResolveError::BundleCorrupt { .. }
    ));
}

// Row: PartSource unavailable, or the withdrawn list unreadable.
#[tokio::test]
async fn source_unavailable() {
    for retryable in [true, false] {
        let dir = TempDir::new();
        let mut source = FaultySource::new(memory(&Cat::basic(""), vec![]));
        source.resolve_error = Some(PartSourceError::Unavailable { retryable });
        let error = error_of(bound(&dir, source).resolve(&request(ROOT)).await);
        assert!(
            matches!(error, ResolveError::SourceUnavailable { retryable: r } if r == retryable)
        );
        assert_coded(&error, "RES_SOURCE_UNAVAILABLE", retryable);
    }
    let dir = TempDir::new();
    let mut source = FaultySource::new(memory(&Cat::basic(""), vec![]));
    source.withdrawn_error = Some(PartSourceError::WithdrawnListUnreadable {
        repository: "bundled".into(),
    });
    let error = error_of(bound(&dir, source).resolve(&request(ROOT)).await);
    assert!(matches!(
        error,
        ResolveError::SourceUnavailable { retryable: false }
    ));
    assert_coded(&error, "RES_SOURCE_UNAVAILABLE", false);
}

// Row: disk write or rename fails.
#[tokio::test]
async fn store_write_failed() {
    let dir = TempDir::new();
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"a file, not a directory").unwrap();
    let mut cfg = config(&dir);
    cfg.bundle_root = blocker.join("bundles");
    let error = error_of(ResolverAndCompiler::new(cfg).map(|_| ()));
    assert!(matches!(
        error,
        ResolveError::StoreWriteFailed { retryable: true }
    ));
    assert_coded(&error, "RES_STORE_WRITE_FAILED", true);

    // The bundle root disappears after start-up and becomes a file.
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    std::fs::remove_dir_all(dir.path().join("bundles")).unwrap();
    std::fs::write(dir.path().join("bundles"), b"in the way").unwrap();
    let error = error_of(resolver.build(&request(ROOT)).await);
    assert!(matches!(
        error,
        ResolveError::StoreWriteFailed { retryable: true }
    ));
    // Pruning an unreadable root fails the same way.
    let error = error_of(resolver.prune(&Default::default()));
    assert!(matches!(
        error,
        ResolveError::StoreWriteFailed { retryable: true }
    ));
}

// Row: a part-reading call before `install_part_source` and without `request.source`.
#[tokio::test]
async fn source_not_bound() {
    let dir = TempDir::new();
    let resolver = ResolverAndCompiler::new(config(&dir)).unwrap();
    let error = error_of(resolver.resolve(&request(ROOT)).await);
    assert!(matches!(error, ResolveError::SourceNotBound));
    assert_coded(&error, "RES_SOURCE_NOT_BOUND", false);
    for result in [
        resolver.build(&request(ROOT)).await.map(|_| ()),
        resolver.pack_space(&request(ROOT)).await.map(|_| ()),
        resolver
            .conflicts(&request(ROOT), &Default::default())
            .await
            .map(|_| ()),
    ] {
        assert!(matches!(error_of(result), ResolveError::SourceNotBound));
    }
}

// Row: `install_part_source` called a second time.
#[tokio::test]
async fn source_already_bound() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let error = error_of(resolver.install_part_source(Arc::new(memory(&Cat::new(), vec![]))));
    assert!(matches!(error, ResolveError::SourceAlreadyBound));
    assert_coded(&error, "RES_SOURCE_ALREADY_BOUND", false);
}
