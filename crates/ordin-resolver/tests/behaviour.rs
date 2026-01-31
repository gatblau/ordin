//! Behaviour beyond the Gherkin scenarios: the tool policy, staged parts, pack and overlay choices,
//! bundles on disk, pruning, withdrawal matching and the instruction text.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use common::*;
use ordin_agent_model::{
    content_digest, ChosenBy, DecimalString, McpServerDecl, ModelPart, PartKind, PartStatus,
    RepositoryRef, StagedPart,
};
use ordin_resolver::{compile_instructions, ResolveError, ResolverAndCompiler};
use ordin_types::domain::HarnessKind;

const ZETA: &str = "urn:ordin:t:tool-bindings:zeta";

fn tool_catalogue() -> Cat {
    let mut cat = Cat::basic(&format!(
        "toolBindings:\n  - {ZETA}\n  - {BINDING}\nselectablePracticePacks:\n  - {GO}\ntoolPermissions:\n  workspaceOverrides:\n    prohibited:\n      - rm\n      - curl\n      - rm\n"
    ));
    cat.yaml(
        "capability",
        CAP,
        "1.0.0",
        "",
        &format!("skills:\n  - {SKILL}\ndefaultToolPolicy:\n  requiresHumanApproval:\n    - email_send\n    - deploy\n"),
    );
    cat.yaml(
        "tool-binding",
        BINDING,
        "1.0.0",
        "",
        "bindings:\n  - name: email_send\n    type: mcp\n    permission: write\n    serverType: http\n    transport: http\n    scope: user\n    envRef: MAIL_TOKEN\n    forbiddenFlags:\n      - --bcc-all\n    coverageFloor: 0.75\n  - name: git\n    type: cli\n    permission: read\n    forbiddenFlags:\n      - --force\n",
    );
    cat.yaml(
        "tool-binding",
        ZETA,
        "1.0.0",
        "",
        "bindings:\n  - name: email_send\n    type: mcp\n    permission: read\n    serverType: sse\n    transport: sse\n  - name: chat\n    type: mcp\n    permission: read\n    serverType: stdio\n    transport: stdio\n    forbiddenFlags:\n      - --force\n",
    );
    cat.pack(
        GO,
        "stack",
        &["go"],
        "toolchain:\n  coverageFloor: 80\n  forbiddenFlags:\n    - -race=false\n",
    );
    cat
}

#[tokio::test]
async fn the_tool_policy_is_composed_from_the_closure() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&tool_catalogue(), vec![]));
    let mut req = request(ROOT);
    req.workspace_languages = vec!["GO".into()];
    let plan = resolver.resolve(&req).await.unwrap();
    let policy = plan.tool_policy.expect("a policy");
    assert_eq!(
        policy.forbidden_flags,
        ["--bcc-all", "--force", "-race=false"]
    );
    assert_eq!(policy.coverage_floor, Some(DecimalString("80".into())));
    assert_eq!(policy.prohibited_verbs, ["curl", "rm"]);
    assert_eq!(policy.requires_human_approval, ["deploy", "email_send"]);
    let names: Vec<&str> = policy.mcp_servers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["chat", "email_send"]);
    let mail: &McpServerDecl = &policy.mcp_servers[1];
    assert_eq!(
        mail.declared_by, BINDING,
        "the first by binding identifier wins"
    );
    assert_eq!(mail.server_type, "http");
    assert_eq!(mail.scope.as_deref(), Some("user"));
    assert_eq!(mail.env_ref.as_deref(), Some("MAIL_TOKEN"));
    assert_eq!(mail.transport.as_deref(), Some("http"));
    // Without the Go pack the floor is the binding's.
    let plan = resolver.resolve(&request(ROOT)).await.unwrap();
    let policy = plan.tool_policy.unwrap();
    assert_eq!(policy.coverage_floor, Some(DecimalString("0.75".into())));
    assert!(!policy.forbidden_flags.contains(&"-race=false".to_owned()));
}

#[tokio::test]
async fn the_harness_is_written_as_its_snake_case_name() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let mut req = request(ROOT);
    req.target_harness = HarnessKind::CopilotCli;
    let copilot = resolver.build(&req).await.unwrap();
    assert_eq!(copilot.plan.target_harness, "copilot_cli");
    assert_eq!(copilot.lock_file.target_harness, "copilot_cli");
    let claude = resolver.build(&request(ROOT)).await.unwrap();
    assert_eq!(claude.plan.target_harness, "claude_code");
    assert_ne!(claude.plan.plan_hash, copilot.plan.plan_hash);
}

fn staged(cat: &Cat, label: &str) -> Vec<StagedPart> {
    cat.parts()
        .into_iter()
        .map(|part| StagedPart {
            part,
            repository: RepositoryRef::Bundled,
            repository_label: label.to_owned(),
        })
        .collect()
}

#[tokio::test]
async fn staged_parts_shadow_the_source_and_carry_a_zero_commit() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let mut draft = Cat::new();
    draft.md("rule", RULE_A, "1.0.0", "", "", "Staged alpha text.");
    draft.md(
        "rule",
        RULE_B,
        "2.0.0",
        "  status: draft\n",
        "",
        "Staged beta, a draft.",
    );
    let mut req = request(ROOT);
    req.staged = staged(&draft, "github.com/t/draft");
    let bundle = resolver.build(&req).await.unwrap();
    for id in [RULE_A, RULE_B] {
        let part = bundle.plan.resolved.iter().find(|p| p.id == id).unwrap();
        assert_eq!(part.commit, "0".repeat(40));
        assert_eq!(part.repository, "github.com/t/draft");
        let files = bundle.read_part_files(part).unwrap();
        let text: String = files
            .0
            .values()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect();
        assert!(text.contains("Staged"), "{id}");
    }
    let beta = bundle
        .plan
        .resolved
        .iter()
        .find(|p| p.id == RULE_B)
        .unwrap();
    assert_eq!(beta.version, "2.0.0", "the staged draft is allowed");
    let plain = resolver.resolve(&request(ROOT)).await.unwrap();
    assert_ne!(plain.plan_hash, bundle.plan.plan_hash);
}

#[tokio::test]
async fn a_staged_root_wins_over_the_source() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let mut draft = Cat::new();
    draft.yaml(
        "workspace-profile",
        ROOT,
        "3.0.0",
        "  status: draft\n",
        &format!("defaultRules:\n  - {RULE_A}\n"),
    );
    let mut req = request(ROOT);
    req.staged = staged(&draft, "github.com/t/draft");
    req.allow_draft_root = true;
    let plan = resolver.resolve(&req).await.unwrap();
    assert_eq!(plan.root.version, "3.0.0");
    assert_eq!(plan.resolved.len(), 2, "the staged root and its one rule");
    req.allow_draft_root = false;
    assert!(matches!(
        error_of(resolver.resolve(&req).await),
        ResolveError::RootNotApproved {
            status: PartStatus::Draft,
            ..
        }
    ));
}

#[tokio::test]
async fn packs_are_chosen_by_maker_by_person_and_by_match() {
    let body = format!(
        "requiredPracticePacks:\n  - atLeast: 1\n    where:\n      class: stack\nselectablePracticePacks:\n  - {GO}\n  - {RUST}\n  - {BDD}\nfixedPracticePacks:\n  - {GO}\n"
    );
    let mut cat = Cat::basic(&body);
    cat.with_packs();
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cat, vec![]));
    let chosen = |plan: &ordin_agent_model::ResolutionPlan| -> Vec<(String, ChosenBy)> {
        plan.selections
            .practice_packs
            .iter()
            .map(|p| (p.id.clone(), p.chosen_by))
            .collect()
    };
    // No code: the maker's fixed Go pack settles the rule.
    let plan = resolver.resolve(&request(ROOT)).await.unwrap();
    assert_eq!(chosen(&plan), [(GO.to_owned(), ChosenBy::MakerFixed)]);
    // Rust code: the workspace match comes before the fixed pack.
    let mut rust = request(ROOT);
    rust.workspace_languages = vec!["rust".into()];
    let plan = resolver.resolve(&rust).await.unwrap();
    assert_eq!(chosen(&plan), [(RUST.to_owned(), ChosenBy::WorkspaceCode)]);
    // The person changes the stack pack and adds an optional one.
    let mut changed = request(ROOT);
    changed.pack_changes = vec![RUST.into(), BDD.into()];
    let plan = resolver.resolve(&changed).await.unwrap();
    assert_eq!(
        chosen(&plan),
        [
            (RUST.to_owned(), ChosenBy::PersonChange),
            (BDD.to_owned(), ChosenBy::PersonChange)
        ]
    );
    assert_eq!(plan.practice_packs.len(), 2);
    // An optional pack the workspace matches is included next to the required one.
    let mut both = request(ROOT);
    both.workspace_languages = vec!["go".into()];
    let plan = resolver.resolve(&both).await.unwrap();
    assert_eq!(chosen(&plan), [(GO.to_owned(), ChosenBy::WorkspaceCode)]);
}

#[tokio::test]
async fn a_rule_by_identifiers_skips_candidates_that_resolve_to_nothing() {
    let body = format!(
        "requiredPracticePacks:\n  - atLeast: 1\n    anyOf:\n      - urn:ordin:t:practice-packs:stack:missing\n      - {GO}\nfixedPracticePacks:\n  - {GO}\n"
    );
    let mut cat = Cat::basic(&body);
    cat.with_packs();
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cat, vec![]));
    let plan = resolver.resolve(&request(ROOT)).await.unwrap();
    assert_eq!(plan.selections.practice_packs[0].id, GO);
    // An ask entry covers a rule written by identifiers when it names the same set.
    let asked = format!(
        "requiredPracticePacks:\n  - atLeast: 1\n    anyOf:\n      - {BDD}\n      - {PYRAMID}\nfixedPracticePacks:\n  - ask:\n      ids:\n        - {PYRAMID}\n        - {BDD}\n"
    );
    let mut ask_cat = Cat::basic(&asked);
    ask_cat.with_packs();
    let resolver = bound(&TempDir::new(), memory(&ask_cat, vec![]));
    let plan = resolver.resolve(&request(ROOT)).await.unwrap();
    assert_eq!(plan.selections.pending_choices.len(), 1);
    assert_eq!(plan.selections.pending_choices[0].class, None);
    assert_eq!(plan.selections.pending_choices[0].choices, [BDD, PYRAMID]);
}

#[tokio::test]
async fn overlays_are_chosen_by_the_person_and_applied_in_tier_order() {
    let offer = format!(
        "governanceMetadata:\n  languageOverlays:\n    available:\n      - {EN_GB}\n      - {FR}\n    default: {EN_GB}\n  styleOverlays:\n    - {PLAIN}\nrequiredStyleOverlays:\n  anyOf:\n    - {PLAIN}\n"
    );
    let mut cat = Cat::basic(&offer);
    cat.with_languages();
    cat.overlay("style-overlay", PLAIN, "Plain.");
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cat, vec![]));
    let mut req = request(ROOT);
    req.overlay_changes = vec![FR.into(), PLAIN.into(), FR.into()];
    let plan = resolver.resolve(&req).await.unwrap();
    let applied: Vec<&str> = plan.overlay_order.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(applied, [PLAIN, FR], "style comes before language");
    assert!(plan
        .selections
        .overlays
        .iter()
        .all(|o| o.chosen_by == ChosenBy::PersonChange));
    // The style overlay's `appliesTo` pins: caret ranges allow a newer minor version of this agent.
    let mut pinned = Cat::basic(&offer);
    pinned.with_languages();
    pinned.md(
        "style-overlay",
        PLAIN,
        "1.0.0",
        "",
        &format!("appliesTo:\n  workspaces:\n    - \"{ROOT}@^1.0.0\"\n"),
        "Plain.",
    );
    let resolver = bound(&TempDir::new(), memory(&pinned, vec![]));
    let mut with_style = request(ROOT);
    with_style.overlay_changes = vec![PLAIN.into()];
    assert!(resolver.resolve(&with_style).await.is_ok());
    let mut exact = Cat::basic(&offer);
    exact.with_languages();
    exact.md(
        "style-overlay",
        PLAIN,
        "1.0.0",
        "",
        &format!("appliesTo:\n  workspaces:\n    - \"{ROOT}@2.0.0\"\n"),
        "Plain.",
    );
    let resolver = bound(&TempDir::new(), memory(&exact, vec![]));
    assert!(matches!(
        error_of(resolver.resolve(&with_style).await),
        ResolveError::OverlayNotApplicable { .. }
    ));
}

#[tokio::test]
async fn a_bundle_holds_each_part_as_committed_and_opens_again() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let bundle = resolver.build(&request(ROOT)).await.unwrap();
    for part in &bundle.plan.resolved {
        let files = bundle.read_part_files(part).unwrap();
        assert_eq!(
            format!("sha256:{}", content_digest(&files).0),
            part.content_hash
        );
    }
    let opened = resolver.open_bundle(&bundle.plan.plan_hash).unwrap();
    assert_eq!(opened.manifest, bundle.manifest);
    assert_eq!(opened.lock_file, bundle.lock_file);
    assert_eq!(opened.lock_file_bytes, bundle.lock_file_bytes);
    assert_eq!(opened.plan.resolved, bundle.plan.resolved);
    assert_eq!(opened.plan.tool_policy, bundle.plan.tool_policy);
    assert!(opened.plan.warnings.is_empty());
    let loading: Vec<(PartKind, String)> = bundle
        .manifest
        .artefacts
        .iter()
        .map(|a| (a.part.kind, format!("{:?}", a.loading)))
        .collect();
    assert!(loading.contains(&(PartKind::Rule, "AlwaysOn".into())));
    assert!(loading.contains(&(PartKind::Skill, "OnDemand".into())));
    assert_eq!(bundle.manifest.compiler_version, "0.1.0-test");
    assert_eq!(bundle.manifest.lockfile_sha256.len(), 64);
}

#[tokio::test]
async fn compile_finds_the_parts_of_a_plan_another_resolver_made() {
    let cat = Cat::basic("");
    let first_dir = TempDir::new();
    let plan = bound(&first_dir, memory(&cat, vec![]))
        .resolve(&request(ROOT))
        .await
        .unwrap();
    // A resolver that never resolved this plan reads the bundled catalogue.
    let second_dir = TempDir::new();
    let second = bound(&second_dir, memory(&cat, vec![]));
    let bundle = second.compile(&plan, &[]).await.unwrap();
    assert_eq!(bundle.plan.plan_hash, plan.plan_hash);
    // And a plan naming a part nobody holds is refused.
    let mut gone = plan.clone();
    gone.resolved[0].id = "urn:ordin:t:rules:gone".to_owned();
    gone.plan_hash = format!("sha256:{}", "1".repeat(64));
    assert!(matches!(
        error_of(second.compile(&gone, &[]).await),
        ResolveError::LockPartUnavailable { .. }
    ));
}

#[tokio::test]
async fn rebuild_from_lock_repairs_a_missing_or_corrupt_bundle() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![]));
    let bundle = resolver.build(&request(ROOT)).await.unwrap();
    let hash = bundle.plan.plan_hash.clone();
    std::fs::remove_dir_all(&bundle.dir).unwrap();
    assert!(matches!(
        error_of(resolver.open_bundle(&hash)),
        ResolveError::BundleNotFound { .. }
    ));
    let rebuilt = resolver
        .rebuild_from_lock(&bundle.lock_file, &scope())
        .await
        .unwrap();
    assert_eq!(rebuilt.plan.plan_hash, hash);
    assert_eq!(rebuilt.lock_file_bytes, bundle.lock_file_bytes);
    // Corrupt one part file: the rebuild replaces the directory.
    let part = rebuilt.plan.resolved[0].clone();
    let file = rebuilt
        .part_dir(&part)
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(file, b"broken").unwrap();
    assert!(resolver.open_bundle(&hash).is_err());
    let repaired = resolver
        .rebuild_from_lock(&bundle.lock_file, &scope())
        .await
        .unwrap();
    assert!(resolver.open_bundle(&hash).is_ok());
    assert!(repaired.read_part_files(&part).is_ok());
}

#[tokio::test]
async fn prune_removes_bundles_that_are_not_kept() {
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&cspa_like(), vec![]));
    let kept = resolver.build(&request(ROOT)).await.unwrap();
    let mut other_request = request(ROOT);
    other_request.target_harness = HarnessKind::CopilotCli;
    let doomed = resolver.build(&other_request).await.unwrap();
    let root = dir.path().join("bundles");
    // A fresh staging directory stays; an unrelated file stays.
    std::fs::create_dir_all(root.join(".tmp-fresh")).unwrap();
    std::fs::write(root.join("notes.txt"), b"x").unwrap();
    let keep: BTreeSet<String> = [kept.plan.plan_hash.clone()].into();
    let report = resolver.prune(&keep).unwrap();
    assert_eq!((report.removed, report.kept), (1, 1));
    assert!(report.bytes_freed > 0);
    assert!(resolver.open_bundle(&kept.plan.plan_hash).is_ok());
    assert!(matches!(
        error_of(resolver.open_bundle(&doomed.plan.plan_hash)),
        ResolveError::BundleNotFound { .. }
    ));
    assert!(root.join(".tmp-fresh").exists());
    // Hashes may be given with or without the prefix, and a root that does not exist prunes nothing.
    let bare: BTreeSet<String> = [kept.plan.plan_hash["sha256:".len()..].to_owned()].into();
    assert_eq!(resolver.prune(&bare).unwrap().removed, 0);
    std::fs::remove_dir_all(&root).unwrap();
    let report = resolver.prune(&BTreeSet::new()).unwrap();
    assert_eq!((report.removed, report.kept, report.bytes_freed), (0, 0, 0));
}

fn cspa_like() -> Cat {
    Cat::basic("")
}

#[tokio::test]
async fn a_withdrawal_names_a_version_and_a_digest() {
    let mut matching = withdrawal(PartKind::Rule, RULE_B, None);
    matching.part_version = Some("9.9.9".into());
    let dir = TempDir::new();
    let resolver = bound(&dir, memory(&Cat::basic(""), vec![matching.clone()]));
    assert!(
        resolver.resolve(&request(ROOT)).await.is_ok(),
        "another version is withdrawn, not this one"
    );
    matching.part_version = Some("1.0.0".into());
    matching.part_content_digest = Some(ordin_agent_model::Sha256Hex("0".repeat(64)));
    // The in-memory source flags a withdrawal by version only; clear its flag so the entry decides.
    let mut source = FaultySource::new(memory(&Cat::basic(""), vec![matching.clone()]));
    source.tweak = Some(Arc::new(|p| p.part.withdrawn = false));
    let resolver = bound(&TempDir::new(), source);
    assert!(
        resolver.resolve(&request(ROOT)).await.is_ok(),
        "another digest is not withdrawn"
    );
    matching.part_content_digest = None;
    let mut source = FaultySource::new(memory(&Cat::basic(""), vec![matching]));
    source.tweak = Some(Arc::new(|p| p.part.withdrawn = false));
    let resolver = bound(&TempDir::new(), source);
    assert!(matches!(
        error_of(resolver.resolve(&request(ROOT)).await),
        ResolveError::PartWithdrawn { .. }
    ));
}

#[tokio::test]
async fn a_replacement_that_resolves_to_nothing_is_unresolved() {
    let dir = TempDir::new();
    let resolver = bound(
        &dir,
        memory(
            &Cat::basic(""),
            vec![withdrawal(PartKind::Rule, RULE_B, None)],
        ),
    );
    let mut req = request(ROOT);
    req.replacements = vec![ordin_resolver::Replacement {
        withdrawn_id: RULE_B.into(),
        withdrawn_kind: PartKind::Rule,
        replacement_id: "urn:ordin:t:rules:nowhere".into(),
        replacement_version: None,
    }];
    let error = error_of(resolver.resolve(&req).await);
    assert!(
        matches!(&error, ResolveError::ReferenceUnresolved { from, id, .. } if from == RULE_B && id == "urn:ordin:t:rules:nowhere")
    );
}

#[tokio::test]
async fn a_withdrawn_flag_without_a_list_entry_withdraws_the_part() {
    let dir = TempDir::new();
    let mut source = FaultySource::new(memory(&Cat::basic(""), vec![]));
    source.tweak = Some(Arc::new(|p| {
        if p.part.identifier == RULE_A {
            p.part.withdrawn = true;
        }
    }));
    let error = error_of(bound(&dir, source).resolve(&request(ROOT)).await);
    assert!(
        matches!(&error, ResolveError::PartWithdrawn { part, replacement: None } if part == RULE_A)
    );
}

#[tokio::test]
async fn a_diamond_is_walked_once_and_a_shared_part_counts_once() {
    let (a, b, shared) = (
        "urn:ordin:t:context-resources:a",
        "urn:ordin:t:context-resources:b",
        "urn:ordin:t:context-resources:shared",
    );
    let mut cat = Cat::basic(&format!("contextResources:\n  - {a}\n  - {b}\n"));
    for id in [a, b] {
        cat.yaml(
            "context-resource",
            id,
            "1.0.0",
            "",
            &format!("contextResources:\n  - {shared}\n  - {shared}\n"),
        );
    }
    cat.md("context-resource", shared, "1.0.0", "", "", "Shared.");
    let dir = TempDir::new();
    let plan = bound(&dir, memory(&cat, vec![]))
        .resolve(&request(ROOT))
        .await
        .unwrap();
    assert_eq!(plan.resolved.iter().filter(|p| p.id == shared).count(), 1);
    let order: Vec<PartKind> = plan.resolved.iter().map(|p| p.kind).collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted, "resolved is sorted by kind");
}

#[tokio::test]
async fn instructions_join_always_on_markdown_and_skip_yaml_parts() {
    let mut cat = Cat::basic("");
    cat.files.insert(
        format!("models/rule/{}-1.0.0.md", RULE_A.replace(':', "_")),
        format!(
            "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: {RULE_A}\n  name: alpha\n  version: 1.0.0\n  owner: acme\n---\r\n\r\n\r\nWindows text.\r\nSecond line.\r\n\r\n"
        )
        .into_bytes(),
    );
    cat.yaml(
        "output-contract",
        "urn:ordin:t:output-contracts:report",
        "1.0.0",
        "",
        "sections:\n  - name: summary\n    meaning: A summary.\n    required: true\n",
    );
    cat.rule("urn:ordin:t:rules:empty", "");
    let root_body = format!(
        "outputContracts:\n  - urn:ordin:t:output-contracts:report\ndefaultRules:\n  - {RULE_A}\n  - urn:ordin:t:rules:empty\n"
    );
    let text = String::from_utf8(
        cat.files
            .iter()
            .find(|(p, _)| p.contains("workspace"))
            .unwrap()
            .1
            .clone(),
    )
    .unwrap();
    let key = cat
        .files
        .keys()
        .find(|p| p.contains("workspace"))
        .unwrap()
        .clone();
    let patched = text.replace(
        &format!("defaultRules:\n  - {RULE_A}\n  - {RULE_B}\n"),
        &root_body,
    );
    cat.files.insert(key, patched.into_bytes());
    let source = memory(&cat, vec![]);
    let out = compile_instructions(&source, &request(ROOT)).await.unwrap();
    assert_eq!(
        out.agents_md,
        "You build things.\n\nWindows text.\nSecond line.\n"
    );
    // Resolution errors come back unchanged.
    let missing =
        error_of(compile_instructions(&source, &request("urn:ordin:t:workspaces:none")).await);
    assert!(matches!(missing, ResolveError::RootNotFound { .. }));
    // Tampered files are refused.
    let mut tampered = FaultySource::new(memory(&Cat::basic(""), vec![]));
    tampered.tamper_files = true;
    assert!(matches!(
        error_of(compile_instructions(&tampered, &request(ROOT)).await),
        ResolveError::DigestMismatch { .. }
    ));
}

#[tokio::test]
async fn an_empty_closure_text_compiles_to_nothing() {
    let mut cat = Cat::new();
    cat.yaml("workspace-profile", ROOT, "1.0.0", "", "");
    let source = memory(&cat, vec![]);
    let out = compile_instructions(&source, &request(ROOT)).await.unwrap();
    assert_eq!(out.agents_md, "");
}

#[tokio::test]
async fn a_second_resolver_over_the_same_root_reuses_what_the_first_wrote() {
    let dir = TempDir::new();
    let first = bound(&dir, memory(&Cat::basic(""), vec![]));
    let built = first.build(&request(ROOT)).await.unwrap();
    let second = bound(&dir, memory(&Cat::basic(""), vec![]));
    let again = second.build(&request(ROOT)).await.unwrap();
    assert_eq!(again.dir, built.dir);
    let _ = ResolverAndCompiler::new(config(&dir))
        .unwrap()
        .open_bundle(&built.plan.plan_hash)
        .unwrap();
}

#[test]
fn model_types_are_reachable_through_this_crate() {
    let kind: ordin_resolver::model::PartKind = PartKind::Rule;
    assert_eq!(kind, PartKind::Rule);
    let _unused: Option<ModelPart> = None;
}
