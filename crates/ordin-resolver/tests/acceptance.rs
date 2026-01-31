//! X2: one test per Gherkin scenario of ResolverAndCompiler.md, named after the scenario in snake_case.
//! The parity scenario lives in `tests/parity.rs`.

mod common;

use std::sync::Arc;

use common::*;
use ordin_agent_model::{ChosenBy, PartKind, PartSourceError, SourceOverride};
use ordin_resolver::{compile_instructions, PackAnswer, Replacement, ResolveError};

const CSPA_BODY: &str = "requiredPracticePacks:
  - atLeast: 1
    where:
      class: stack
  - atLeast: 1
    where:
      class: testing
fixedPracticePacks:
  - ask:
      class: testing
";

fn cspa() -> Cat {
    let mut cat = Cat::basic(CSPA_BODY);
    cat.with_packs();
    cat
}

fn go_request() -> ordin_resolver::ResolveRequest {
    let mut req = request(ROOT);
    req.workspace_languages = vec!["go".to_owned()];
    req
}

// Scenario: Happy path — packs from the workspace's code
#[tokio::test]
async fn happy_path_packs_from_the_workspaces_code() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cspa(), vec![]));
    let bundle = resolver.build(&go_request()).await.unwrap();
    let selections = &bundle.plan.selections;
    assert_eq!(selections.practice_packs.len(), 1);
    assert_eq!(selections.practice_packs[0].id, GO);
    assert_eq!(
        selections.practice_packs[0].chosen_by,
        ChosenBy::WorkspaceCode
    );
    assert_eq!(selections.pending_choices.len(), 1);
    let pending = &selections.pending_choices[0];
    assert_eq!((pending.rule_index, pending.at_least), (1, 1));
    assert_eq!(pending.class.as_deref(), Some("testing"));
    assert_eq!(pending.choices, [BDD, PYRAMID, TROPHY, HONEYCOMB]);
    assert!(bundle.plan.warnings.is_empty());
    assert_eq!(
        bundle.dir.file_name().unwrap().to_str().unwrap(),
        &bundle.plan.plan_hash["sha256:".len()..]
    );
}

// Scenario: Edge — the person answers the agent's question
#[tokio::test]
async fn edge_the_person_answers_the_agents_question() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cspa(), vec![]));
    let first = resolver.build(&go_request()).await.unwrap();
    let mut answered = go_request();
    answered.pack_answers = vec![PackAnswer {
        rule_index: 1,
        pack_ids: vec![PYRAMID.to_owned()],
    }];
    let second = resolver.build(&answered).await.unwrap();
    assert!(second.plan.selections.pending_choices.is_empty());
    assert_ne!(second.plan.plan_hash, first.plan.plan_hash);
    let picked: Vec<(&str, ChosenBy)> = second
        .plan
        .selections
        .practice_packs
        .iter()
        .map(|p| (p.id.as_str(), p.chosen_by))
        .collect();
    assert_eq!(
        picked,
        [
            (GO, ChosenBy::WorkspaceCode),
            (PYRAMID, ChosenBy::PersonAnswer)
        ]
    );
}

// Scenario: Edge — language overlay follows the interface language
#[tokio::test]
async fn edge_language_overlay_follows_the_interface_language() {
    let offer = "governanceMetadata:
  languageOverlays:
    available:
      - urn:ordin:t:overlays:language:en-GB
      - urn:ordin:t:overlays:language:en-US
      - urn:ordin:t:overlays:language:fr
      - urn:ordin:t:overlays:language:es
    default: urn:ordin:t:overlays:language:en-GB
";
    let mut cat = Cat::basic(offer);
    cat.with_languages();
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cat, vec![]));
    let cases = [
        ("fr", FR, ChosenBy::InterfaceLanguage),
        ("FR", FR, ChosenBy::InterfaceLanguage),
        ("en-US", EN_US, ChosenBy::InterfaceLanguage),
        ("es-MX", ES, ChosenBy::InterfaceLanguage),
        ("de", EN_GB, ChosenBy::AgentDefault),
    ];
    for (language, expected, by) in cases {
        let mut req = request(ROOT);
        req.interface_language = language.to_owned();
        let plan = resolver.resolve(&req).await.unwrap();
        assert_eq!(plan.selections.overlays.len(), 1, "{language}");
        assert_eq!(plan.selections.overlays[0].id, expected, "{language}");
        assert_eq!(plan.selections.overlays[0].chosen_by, by, "{language}");
        assert_eq!(plan.overlay_order.len(), 1);
    }
}

// Scenario: Edge — determinism
#[tokio::test]
async fn edge_determinism() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cspa(), vec![]));
    let first = resolver.build(&go_request()).await.unwrap();
    let manifest_path = first.dir.join("bundle.manifest.json");
    let written = std::fs::metadata(&manifest_path)
        .unwrap()
        .modified()
        .unwrap();
    let before = listing(&dir.path().join("bundles"));
    let second = resolver.build(&go_request()).await.unwrap();
    assert_eq!(first.plan.plan_hash, second.plan.plan_hash);
    assert_eq!(first.dir, second.dir);
    assert_eq!(first.lock_file_bytes, second.lock_file_bytes);
    assert_eq!(
        std::fs::metadata(&manifest_path)
            .unwrap()
            .modified()
            .unwrap(),
        written,
        "reused, not rewritten"
    );
    assert_eq!(listing(&dir.path().join("bundles")), before);
    // A second resolver over a fresh root writes byte-identical files.
    let other = TempDir::new();
    let third = bound(&other, memory(&cspa(), vec![]))
        .build(&go_request())
        .await
        .unwrap();
    assert_eq!(third.plan.plan_hash, first.plan.plan_hash);
    assert_eq!(
        std::fs::read(third.dir.join("bundle.manifest.json")).unwrap(),
        std::fs::read(&manifest_path).unwrap()
    );
    assert_eq!(third.lock_file_bytes, first.lock_file_bytes);
}

// Scenario: Edge — a deprecated constituent warns
#[tokio::test]
async fn edge_a_deprecated_constituent_warns() {
    let mut cat = Cat::basic("");
    cat.md(
        "rule",
        RULE_A,
        "1.0.0",
        "  status: deprecated\n",
        "",
        "Old text.",
    );
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cat, vec![]));
    let plan = resolver.resolve(&request(ROOT)).await.unwrap();
    assert_eq!(
        plan.warnings,
        [format!(
            "constituent {RULE_A} is deprecated; do not add new uses"
        )]
    );
}

// Scenario: Edge — withdrawn part with a replacement
#[tokio::test]
async fn edge_withdrawn_part_with_a_replacement() {
    let replacement = "urn:ordin:t:rules:beta-v2";
    let mut cat = Cat::basic("");
    cat.rule(replacement, "Beta, second edition.");
    let dir = TempDir::new();
    let resolver = bound(
        &dir,
        memory(
            &cat,
            vec![withdrawal(PartKind::Rule, RULE_B, Some(replacement))],
        ),
    );
    let mut req = request(ROOT);
    req.replacements = vec![Replacement {
        withdrawn_id: RULE_B.to_owned(),
        withdrawn_kind: PartKind::Rule,
        replacement_id: replacement.to_owned(),
        replacement_version: None,
    }];
    let plan = resolver.resolve(&req).await.unwrap();
    let ids: Vec<&str> = plan.resolved.iter().map(|p| p.id.as_str()).collect();
    assert!(ids.contains(&replacement));
    assert!(!ids.contains(&RULE_B));
}

// Scenario: Error — no stack pack can be chosen
#[tokio::test]
async fn error_no_stack_pack_can_be_chosen() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cspa(), vec![]));
    let error = error_of(resolver.resolve(&request(ROOT)).await);
    assert!(
        matches!(&error, ResolveError::RequiredPackUnsettled { rule_index: 0, class: Some(c) } if c == "stack"),
        "{error:?}"
    );
}

// Scenario: Error — conflicting parts
#[tokio::test]
async fn error_conflicting_parts() {
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
    cat.skill(skill_b, "2.0.0", "");
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cat, vec![]));
    let mut req = request(ROOT);
    req.workspace_languages = vec!["go".into(), "rust".into()];
    let error = error_of(resolver.resolve(&req).await);
    let ResolveError::Conflict { conflicts } = &error else {
        panic!("{error:?}")
    };
    assert_eq!(conflicts[0].slot, "Skill/quality-codegen");
    let named: Vec<&str> = conflicts[0]
        .candidates
        .iter()
        .flat_map(|c| c.requested_by.iter().map(|r| r.id.as_str()))
        .collect();
    assert_eq!(named, [GO, RUST]);
}

// Scenario: Error — withdrawn part without replacement
#[tokio::test]
async fn error_withdrawn_part_without_replacement() {
    let dir = TempDir::new();
    let resolver = bound(
        &dir,
        memory(
            &Cat::basic(""),
            vec![withdrawal(PartKind::Skill, SKILL, None)],
        ),
    );
    let error = error_of(resolver.resolve(&request(ROOT)).await);
    assert!(
        matches!(&error, ResolveError::PartWithdrawn { part, .. } if part == SKILL),
        "{error:?}"
    );
}

// Scenario: Error — tampered source files
#[tokio::test]
async fn error_tampered_source_files() {
    let dir = TempDir::new();
    let mut source = FaultySource::new(memory(&Cat::basic(""), vec![]));
    source.tamper_files = true;
    let resolver = bound(&dir, source);
    let plan = resolver.resolve(&request(ROOT)).await.unwrap();
    let error = error_of(resolver.compile(&plan, &[]).await);
    assert!(matches!(error, ResolveError::DigestMismatch { .. }));
    assert_eq!(
        listing(&dir.path().join("bundles")),
        Vec::<String>::new(),
        "no bundle and no staging directory"
    );
}

// Scenario: Error — lock file names a part the source no longer has
#[tokio::test]
async fn error_lock_file_names_a_part_the_source_no_longer_has() {
    let dir = TempDir::new();
    let built = bound(&dir, memory(&Cat::basic(""), vec![]))
        .build(&request(ROOT))
        .await
        .unwrap();
    let mut newer = Cat::basic("");
    newer.files.retain(|path, _| !path.contains("craft"));
    newer.skill(SKILL, "1.1.0", "");
    let other = TempDir::new();
    let resolver = bound(&other, memory(&newer, vec![]));
    let error = error_of(resolver.rebuild_from_lock(&built.lock_file, &scope()).await);
    assert!(
        matches!(&error, ResolveError::LockPartUnavailable { part } if part == SKILL),
        "{error:?}"
    );
}

// Scenario: Error — the clone is locked
#[tokio::test]
async fn error_the_clone_is_locked() {
    let dir = TempDir::new();
    let mut source = FaultySource::new(memory(&Cat::basic(""), vec![]));
    source.resolve_error = Some(PartSourceError::Unavailable { retryable: true });
    let error = error_of(bound(&dir, source).resolve(&request(ROOT)).await);
    assert!(matches!(
        error,
        ResolveError::SourceUnavailable { retryable: true }
    ));
    assert!(error.retryable());
}

// Scenario: Error — a call before the source is bound
#[tokio::test]
async fn error_a_call_before_the_source_is_bound() {
    let dir = TempDir::new();
    let resolver = ordin_resolver::ResolverAndCompiler::new(config(&dir)).unwrap();
    let error = error_of(resolver.resolve(&request(ROOT)).await);
    assert!(matches!(error, ResolveError::SourceNotBound));
    assert!(!error.retryable());
}

// Scenario: Edge — the source is bound only once
#[tokio::test]
async fn edge_the_source_is_bound_only_once() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let mut other = Cat::basic("");
    other.rule(RULE_A, "Another text.");
    let error = error_of(resolver.install_part_source(Arc::new(memory(&other, vec![]))));
    assert!(matches!(error, ResolveError::SourceAlreadyBound));
    let first = bound(&TempDir::new(), memory(&Cat::basic(""), vec![]))
        .resolve(&request(ROOT))
        .await
        .unwrap();
    let still = resolver.resolve(&request(ROOT)).await.unwrap();
    assert_eq!(
        still.plan_hash, first.plan_hash,
        "the first source is still read"
    );
}

// Scenario: Happy path — a trial reads the person's draft through a per-request source
#[tokio::test]
async fn happy_path_a_trial_reads_the_persons_draft_through_a_per_request_source() {
    let draft_id = "urn:ordin:t:workspaces:contract-clause-checker";
    let mut draft = Cat::new();
    draft.yaml(
        "workspace-profile",
        draft_id,
        "0.1.0",
        "  status: draft\n",
        &format!("defaultRules:\n  - {RULE_A}\n"),
    );
    draft.rule(RULE_A, "Rule alpha text.");
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let mut req = request(draft_id);
    req.root_version = Some("0.1.0".to_owned());
    // The bound source does not hold the draft.
    assert!(matches!(
        error_of(resolver.build(&req).await),
        ResolveError::RootNotFound { .. }
    ));
    req.allow_draft_root = true;
    req.source = Some(SourceOverride(Arc::new(memory(&draft, vec![]))));
    let bundle = resolver.build(&req).await.unwrap();
    assert_eq!(bundle.plan.root.id, draft_id);
    assert_eq!(bundle.manifest.composition.id, draft_id);
}

// Scenario: Happy path — Check it reads the lock without writing
#[tokio::test]
async fn happy_path_check_it_reads_the_lock_without_writing() {
    let dir = TempDir::new();
    let built = bound(&dir, memory(&cspa(), vec![]))
        .build(&go_request())
        .await
        .unwrap();
    let other = TempDir::new();
    let resolver = bound(&other, memory(&cspa(), vec![]));
    let plan = resolver
        .resolve_lock(&built.lock_file, &scope())
        .await
        .unwrap();
    assert_eq!(plan.plan_hash, built.lock_file.plan_hash);
    assert_eq!(plan.plan_hash, built.plan.plan_hash);
    assert_eq!(plan.tool_policy, built.plan.tool_policy);
    assert_eq!(
        listing(&other.path().join("bundles")),
        Vec::<String>::new(),
        "nothing is written"
    );
}

// Scenario: Happy path — always-on instructions for the Release Builder
#[tokio::test]
async fn happy_path_always_on_instructions_for_the_release_builder() {
    let dir = TempDir::new();
    let source = memory(&Cat::basic(""), vec![]);
    let instructions = compile_instructions(&source, &request(ROOT)).await.unwrap();
    assert_eq!(
        instructions.agents_md,
        "You build things.\n\nRule alpha text.\n\nRule beta text.\n"
    );
    assert!(!instructions.agents_md.contains("How to craft."));
    assert_eq!(instructions.lock.composition.id, ROOT);
    assert!(instructions.lock.plan_hash.starts_with("sha256:"));
    assert_eq!(
        listing(dir.path()),
        Vec::<String>::new(),
        "no bundle directory is written"
    );
}

fn testing_packs_with_skills(at_least: u32) -> Cat {
    let body = format!(
        "requiredPracticePacks:\n  - atLeast: {at_least}\n    where:\n      class: testing\nfixedPracticePacks:\n  - ask:\n      class: testing\n"
    );
    let mut cat = Cat::basic(&body);
    let (first, second) = ("urn:ordin:a:skills:shared", "urn:ordin:b:skills:shared");
    cat.pack(BDD, "testing", &[], &format!("skills:\n  - {first}\n"));
    cat.pack(PYRAMID, "testing", &[], &format!("skills:\n  - {second}\n"));
    cat.skill(first, "1.0.0", "");
    cat.skill(second, "2.0.0", "");
    cat
}

// Scenario: Checker support — conflicts across the pack space ignore exclusive choices
#[tokio::test]
async fn checker_support_conflicts_across_the_pack_space_ignore_exclusive_choices() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&testing_packs_with_skills(1), vec![]));
    let req = request(ROOT);
    let space = resolver.pack_space(&req).await.unwrap();
    assert_eq!(space.packs.len(), 2);
    assert!(resolver.conflicts(&req, &space).await.unwrap().is_empty());

    // With a rule that needs both packs, the same skills do conflict.
    let both = bound(
        &TempDir::new(),
        memory(&testing_packs_with_skills(2), vec![]),
    );
    let space = both.pack_space(&req).await.unwrap();
    let conflicts = both.conflicts(&req, &space).await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].slot, "Skill/shared");
}
