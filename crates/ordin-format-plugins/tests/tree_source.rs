//! `TreeSource` as a `PartSource` (FormatPlugins.md *Internal Logic* 12).

mod common;

use common::*;
use ordin_agent_model::{PartKind, VocabularyAxis};
use ordin_format_plugins::*;

fn scope(id: AgentRepositoryId) -> SourceScope {
    SourceScope {
        home: RepositoryRef::Repository(id),
        profile: Vec::new(),
        include_bundled: false,
    }
}

fn source_of(outcome: &ReadOutcome) -> (TreeSource, AgentRepositoryId) {
    let id = AgentRepositoryId::new();
    (
        TreeSource::new(outcome, id, "local/pkg", &"0".repeat(40)).expect("a source"),
        id,
    )
}

fn apm_outcome() -> ReadOutcome {
    FormatPlugins::builtin()
        .get(FormatId::Apm)
        .read(&load("apm-roundtrip"), &request("acme", "x"))
        .expect("reads")
}

fn found(resolution: Resolution) -> SourcedPart {
    match resolution {
        Resolution::Found(part) => part,
        other => panic!("expected Found, got {other:?}"),
    }
}

#[tokio::test]
async fn parts_are_built_from_the_manifests() {
    let outcome = apm_outcome();
    let (source, id) = source_of(&outcome);
    let rule = found(
        source
            .resolve(
                &scope(id),
                PartKind::Rule,
                "urn:ordin:acme:rules:imported:tone",
                None,
            )
            .await
            .unwrap(),
    );
    assert_eq!(rule.part.agent_repository_id, id);
    assert_eq!(rule.part.version, "1.3.0");
    assert_eq!(rule.part.status, ordin_agent_model::PartStatus::Draft);
    assert_eq!(rule.part.owner, "imported");
    assert_eq!(rule.part.commit, "0".repeat(40));
    assert_eq!(rule.part.file_path, "models/@acme/rules/imported/tone.md");
    assert!(!rule.part.withdrawn);
    assert!(rule.part.uses.is_empty() && rule.part.vocabulary_terms_cited.is_empty());
    assert_eq!(rule.repository, RepositoryRef::Repository(id));
    assert_eq!(rule.repository_label, "local/pkg");
    assert_eq!(rule.repository_display_name, "local/pkg");
    let model = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id == rule.part.identifier)
        .unwrap();
    assert_eq!(rule.part.content_digest, model.content_digest);
    assert_eq!(source.files(&rule).await.unwrap(), model.files);
}

#[tokio::test]
async fn root_is_the_one_workspace_with_its_kind_default_loading() {
    let outcome = apm_outcome();
    let (source, _) = source_of(&outcome);
    let root = source.root().unwrap();
    assert_eq!(root.id, "urn:ordin:acme:workspaces:supplier-analyst");
    assert_eq!(root.name, "supplier-analyst");
    assert_eq!(root.version, "1.3.0");
    assert!(root.content_hash.starts_with("sha256:"));
    assert_eq!(
        root.loading,
        Some(ordin_agent_model::LoadingMode::Structural)
    );
}

#[tokio::test]
async fn resolution_answers_only_for_its_own_repository() {
    let outcome = apm_outcome();
    let (source, id) = source_of(&outcome);
    let tone = "urn:ordin:acme:rules:imported:tone";
    let elsewhere = scope(AgentRepositoryId::new());
    let bundled = SourceScope {
        home: RepositoryRef::Bundled,
        profile: Vec::new(),
        include_bundled: true,
    };
    for other in [&elsewhere, &bundled] {
        assert!(matches!(
            source
                .resolve(other, PartKind::Rule, tone, None)
                .await
                .unwrap(),
            Resolution::NotFound
        ));
        assert!(matches!(
            source.resolve_any_kind(other, tone).await.unwrap(),
            Resolution::NotFound
        ));
        assert!(source.ingredients(other).await.unwrap().is_empty());
        assert_eq!(
            source
                .vocabulary(other)
                .await
                .unwrap()
                .codes(VocabularyAxis::Category),
            Vec::<String>::new()
        );
    }
    assert!(source
        .repository_parts(&RepositoryRef::Bundled)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        source
            .repository_parts(&RepositoryRef::Repository(id))
            .await
            .unwrap()
            .len(),
        8
    );
    assert!(source.withdrawn(&scope(id)).await.unwrap().is_empty());
    // A wrong kind or a version the package does not hold is not found.
    let home = scope(id);
    assert!(matches!(
        source
            .resolve(&home, PartKind::Command, tone, None)
            .await
            .unwrap(),
        Resolution::NotFound
    ));
    assert!(matches!(
        source
            .resolve(&home, PartKind::Rule, tone, Some("9.9.9"))
            .await
            .unwrap(),
        Resolution::NotFound
    ));
    assert!(matches!(
        source
            .resolve(&home, PartKind::Rule, tone, Some("1.3.0"))
            .await
            .unwrap(),
        Resolution::Found(_)
    ));
    assert!(matches!(
        source.resolve_any_kind(&home, tone).await.unwrap(),
        Resolution::Found(_)
    ));
    assert!(matches!(
        source
            .resolve_any_kind(&home, "urn:ordin:acme:rules:none")
            .await
            .unwrap(),
        Resolution::NotFound
    ));
}

#[tokio::test]
async fn the_highest_version_wins_and_two_kinds_are_ambiguous() {
    let mut outcome = apm_outcome();
    let tone = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with("tone"))
        .unwrap()
        .clone();
    for version in ["1.10.0", "2.0.0", "1.9.0"] {
        let mut newer = tone.clone();
        newer.manifest.metadata.version = version.to_owned();
        outcome.parts.push(newer);
    }
    let (source, id) = source_of(&outcome);
    let home = scope(id);
    let best = found(
        source
            .resolve(
                &home,
                PartKind::Rule,
                "urn:ordin:acme:rules:imported:tone",
                None,
            )
            .await
            .unwrap(),
    );
    assert_eq!(best.part.version, "2.0.0");
    let exact = found(
        source
            .resolve(
                &home,
                PartKind::Rule,
                "urn:ordin:acme:rules:imported:tone",
                Some("1.10.0"),
            )
            .await
            .unwrap(),
    );
    assert_eq!(exact.part.version, "1.10.0");

    let mut clash = tone.clone();
    clash.kind = PartKind::Command;
    outcome.parts.push(clash);
    let (source, id) = source_of(&outcome);
    match source
        .resolve_any_kind(&scope(id), "urn:ordin:acme:rules:imported:tone")
        .await
        .unwrap()
    {
        Resolution::Ambiguous { repositories } => {
            assert_eq!(repositories, vec!["local/pkg".to_owned()])
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

#[tokio::test]
async fn files_of_a_part_from_another_source_are_corrupt() {
    let outcome = apm_outcome();
    let (one, _) = source_of(&outcome);
    let (two, two_id) = source_of(&outcome);
    let stranger = found(
        two.resolve(
            &scope(two_id),
            PartKind::Rule,
            "urn:ordin:acme:rules:imported:tone",
            None,
        )
        .await
        .unwrap(),
    );
    let error = one.files(&stranger).await.unwrap_err();
    assert_eq!(
        error,
        PartSourceError::Corrupt {
            part_identifier: "urn:ordin:acme:rules:imported:tone".to_owned()
        }
    );
}

#[tokio::test]
async fn ingredients_are_the_packs_and_overlays_latest_version_each() {
    let mut outcome = apm_outcome();
    let template = outcome
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with("tone"))
        .unwrap()
        .clone();
    let mut parts = Vec::new();
    for (kind, id, version) in [
        (
            PartKind::PracticePack,
            "urn:ordin:acme:practice-packs:stack:go",
            "1.0.0",
        ),
        (
            PartKind::PracticePack,
            "urn:ordin:acme:practice-packs:stack:go",
            "1.2.0",
        ),
        (
            PartKind::StyleOverlay,
            "urn:ordin:acme:overlays:style:plain",
            "1.0.0",
        ),
    ] {
        let mut part = template.clone();
        part.kind = kind;
        part.manifest.metadata.id = id.to_owned();
        part.manifest.metadata.version = version.to_owned();
        parts.push(part);
    }
    outcome.parts.extend(parts);
    let (source, id) = source_of(&outcome);
    let ingredients = source.ingredients(&scope(id)).await.unwrap();
    let summary: Vec<(PartKind, String)> = ingredients
        .iter()
        .map(|p| (p.part.kind, p.part.version.clone()))
        .collect();
    assert_eq!(
        summary,
        vec![
            (PartKind::PracticePack, "1.2.0".to_owned()),
            (PartKind::StyleOverlay, "1.0.0".to_owned())
        ]
    );
}

#[tokio::test]
async fn vocabulary_is_built_from_the_vocabulary_parts_and_a_duplicate_axis_is_corrupt() {
    let outcome = read_ordin(&load("ordin"));
    let (source, id) = source_of(&outcome);
    let set = source.vocabulary(&scope(id)).await.unwrap();
    assert!(set.has(VocabularyAxis::Category, "writing"));
    assert!(set.has(VocabularyAxis::RuleSubvariant, "rule-layer"));

    let mut duplicated = outcome.clone();
    let mut again = duplicated
        .parts
        .iter()
        .find(|p| p.manifest.metadata.id.ends_with("categories"))
        .unwrap()
        .clone();
    again.manifest.metadata.id = "urn:ordin:acme:vocabulary:categories-again".to_owned();
    duplicated.parts.push(again);
    let (source, id) = source_of(&duplicated);
    let error = source.vocabulary(&scope(id)).await.unwrap_err();
    assert!(
        matches!(error, PartSourceError::Corrupt { .. }),
        "{error:?}"
    );
    assert_eq!(error.code(), "AM_SOURCE_CORRUPT");
}

#[tokio::test]
async fn used_by_walks_the_references_of_each_workspace() {
    let outcome = apm_outcome();
    let (source, id) = source_of(&outcome);
    let home = scope(id);
    let tone = found(
        source
            .resolve(
                &home,
                PartKind::Rule,
                "urn:ordin:acme:rules:imported:tone",
                None,
            )
            .await
            .unwrap(),
    );
    let users = source.used_by(&tone).await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(
        users[0].part.identifier,
        "urn:ordin:acme:workspaces:supplier-analyst"
    );
    assert!(
        source.used_by(&users[0]).await.unwrap().is_empty(),
        "a workspace is not used by itself"
    );

    // A part of another source is not in this one.
    let (other, other_id) = source_of(&outcome);
    let foreign = found(
        other
            .resolve(
                &scope(other_id),
                PartKind::Rule,
                "urn:ordin:acme:rules:imported:tone",
                None,
            )
            .await
            .unwrap(),
    );
    assert!(source.used_by(&foreign).await.unwrap().is_empty());
}

#[tokio::test]
async fn used_by_follows_references_through_other_parts() {
    // workspace -> capability -> skill: the skill is reached through the capability.
    let tree = tree(&[
        ("models/@a/workspaces/w/workspace-profile.yaml", "apiVersion: ai.enterprise/v1\nkind: workspace-profile\nmetadata:\n  id: urn:ordin:a:workspaces:w\nskills: []\ncapabilities:\n  - urn:ordin:a:capabilities:c\n"),
        ("models/@a/capabilities/c.yaml", "apiVersion: ai.enterprise/v1\nkind: capability\nmetadata:\n  id: urn:ordin:a:capabilities:c\nskills:\n  - urn:ordin:a:skills:s\n  - urn:ordin:a:capabilities:c\n"),
        ("models/@a/skills/s/SKILL.md", "---\napiVersion: ai.enterprise/v1\nkind: skill\nmetadata:\n  id: urn:ordin:a:skills:s\n---\nS\n"),
    ]);
    let outcome = read_ordin(&tree);
    let (source, id) = source_of(&outcome);
    let skill = found(
        source
            .resolve(&scope(id), PartKind::Skill, "urn:ordin:a:skills:s", None)
            .await
            .unwrap(),
    );
    let users = source.used_by(&skill).await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].part.identifier, "urn:ordin:a:workspaces:w");
}

#[tokio::test]
async fn citing_parts_answer_category_class_and_role_only() {
    let tree = tree(&[
        ("models/@a/rules/z.md", "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:z\n  category: writing\n  class: stack\n  audience:\n    roles: [analyst]\n---\nZ\n"),
        ("models/@a/rules/y.md", "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:y\n  category: writing\n---\nY\n"),
    ]);
    let outcome = read_ordin(&tree);
    let (source, id) = source_of(&outcome);
    let repository = RepositoryRef::Repository(id);
    let names = |parts: Vec<SourcedPart>| -> Vec<String> {
        parts.into_iter().map(|p| p.part.identifier).collect()
    };
    assert_eq!(
        names(
            source
                .citing_parts(&repository, VocabularyAxis::Category, "writing")
                .await
                .unwrap()
        ),
        vec![
            "urn:ordin:a:rules:y".to_owned(),
            "urn:ordin:a:rules:z".to_owned()
        ]
    );
    assert_eq!(
        names(
            source
                .citing_parts(&repository, VocabularyAxis::PackClass, "stack")
                .await
                .unwrap()
        ),
        vec!["urn:ordin:a:rules:z".to_owned()]
    );
    assert_eq!(
        names(
            source
                .citing_parts(&repository, VocabularyAxis::AudienceRole, "analyst")
                .await
                .unwrap()
        ),
        vec!["urn:ordin:a:rules:z".to_owned()]
    );
    assert!(source
        .citing_parts(&repository, VocabularyAxis::LoopStrategy, "writing")
        .await
        .unwrap()
        .is_empty());
    assert!(source
        .citing_parts(&RepositoryRef::Bundled, VocabularyAxis::Category, "writing")
        .await
        .unwrap()
        .is_empty());
}
