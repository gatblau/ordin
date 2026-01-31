//! §7.2 integration: the Resolver and the Checker resolve the same part through the catalogue
//! injected as their `PartSource`, over real clones, a real SQLite journal and ordin's own fixtures.

mod support;

use ordin_agent_model::{PartKind, PartSource, RepositoryRef, Resolution};
use support::*;

#[tokio::test(flavor = "multi_thread")]
async fn the_resolver_and_the_checker_resolve_the_same_part_through_the_catalogue() {
    let owner = unique_owner();
    let env = Env::new(&names(&owner, &["agents"])).await;
    commit_tree(&owner, "agents", &agents_repo(&["helper"]));
    sync_one(&env).await;
    let id = repo_id(&env, "agents").await;
    let scope = env
        .service
        .source_scope(RepositoryRef::Repository(id))
        .await
        .unwrap();

    // The catalogue's own answer.
    let Resolution::Found(sourced) = env
        .service
        .resolve(
            &scope,
            PartKind::WorkspaceProfile,
            "urn:ordin:acme:workspaces:helper",
            None,
        )
        .await
        .unwrap()
    else {
        panic!("the agent is in the catalogue");
    };
    let files = env.service.files(&sourced).await.unwrap();
    assert_eq!(
        ordin_agent_model::content_digest(&files),
        sourced.part.content_digest
    );

    // The Checker reads the same part and its borrowed parts through the catalogue.
    let request = ordin_checker::RepositoryCheck {
        repository: RepositoryRef::Repository(id),
        repository_display_name: "agents agents".to_owned(),
        scope: scope.clone(),
        changes: Vec::new(),
        deletions: Vec::new(),
        source: None,
    };
    let report = env
        .checker
        .check_repository(
            &request,
            ordin_checker::CheckSubject::RepositoryRead {
                repository: sourced.repository_label.clone(),
                commit: "head".into(),
            },
        )
        .await
        .expect("the check");
    assert!(report.passed, "{:?}", report.findings);

    // The Resolver builds the same agent from the same parts, through the same source.
    let plan = env
        .resolver
        .resolve(&ordin_resolver::ResolveRequest {
            scope: scope.clone(),
            root_id: sourced.part.identifier.clone(),
            root_version: Some(sourced.part.version.clone()),
            target_harness: ordin_types::domain::HarnessKind::ClaudeCode,
            workspace_languages: Vec::new(),
            interface_language: "en-GB".to_owned(),
            pack_answers: Vec::new(),
            pack_changes: Vec::new(),
            overlay_changes: Vec::new(),
            replacements: Vec::new(),
            staged: Vec::new(),
            allow_draft_root: false,
            source: None,
        })
        .await
        .expect("the Resolver resolves the agent through the catalogue");
    let named: Vec<String> = plan.resolved.iter().map(|p| p.id.clone()).collect();
    assert_eq!(plan.root.id, "urn:ordin:acme:workspaces:helper");
    assert!(
        named.iter().any(|id| id == TONE_ID),
        "the Resolver and the Checker read the same rule: {named:?}"
    );
}
