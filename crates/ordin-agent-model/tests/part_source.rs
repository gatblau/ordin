//! The `PartSource` contract over the test kit's `InMemorySource`, and the three `PartSource`
//! scenarios of AgentModel.md (X2). Built with `--features testkit`.
#![cfg(feature = "testkit")]

use ordin_agent_model::testkit::{assert_part_source_contract, ContractFixture, InMemorySource};
use ordin_agent_model::*;

fn scope(fixture: &ContractFixture) -> SourceScope {
    SourceScope {
        home: fixture.repositories[0].0.clone(),
        profile: vec![
            fixture.repositories[1].0.clone(),
            fixture.repositories[2].0.clone(),
        ],
        include_bundled: true,
    }
}

#[tokio::test]
async fn in_memory_source_passes_the_contract() {
    let fixture = ContractFixture::standard();
    let source = InMemorySource::new(&fixture);
    assert_part_source_contract(&source, &fixture).await;
    // The trait is object-safe and fits a per-request override.
    let shared = SourceOverride(std::sync::Arc::new(InMemorySource::new(&fixture)));
    assert_eq!(format!("{shared:?}"), "SourceOverride(..)");
    assert_part_source_contract(shared.0.as_ref(), &fixture).await;
}

// Scenario: PartSource — ambiguous borrowed identifier
#[tokio::test]
async fn partsource_ambiguous_borrowed_identifier() {
    let fixture = ContractFixture::standard();
    let source = InMemorySource::new(&fixture);
    let resolution = source
        .resolve(
            &scope(&fixture),
            PartKind::Rule,
            "urn:ordin:acme:rules:tone",
            None,
        )
        .await
        .unwrap();
    match resolution {
        Resolution::Ambiguous { repositories } => assert_eq!(
            repositories,
            vec![
                "github.com/acme/legal".to_owned(),
                "github.com/acme/policies".to_owned()
            ]
        ),
        other => panic!("{other:?}"),
    }
}

// Scenario: PartSource — a part refused at ingest is not found
#[tokio::test]
async fn partsource_a_part_refused_at_ingest_is_not_found() {
    let fixture = ContractFixture::standard();
    let source = InMemorySource::new(&fixture);
    let resolution = source
        .resolve(
            &scope(&fixture),
            PartKind::Skill,
            "urn:ordin:acme:skills:broken",
            None,
        )
        .await
        .unwrap();
    assert!(matches!(resolution, Resolution::NotFound));
}

// Scenario: PartSource — parts citing a vocabulary term
#[tokio::test]
async fn partsource_parts_citing_a_vocabulary_term() {
    let fixture = ContractFixture::standard();
    let source = InMemorySource::new(&fixture);
    let citing = source
        .citing_parts(
            &fixture.repositories[0].0,
            VocabularyAxis::Category,
            "finance",
        )
        .await
        .unwrap();
    let ids: Vec<&str> = citing.iter().map(|p| p.part.identifier.as_str()).collect();
    assert_eq!(ids, vec!["urn:ordin:acme:rules:tone"]);
    assert_eq!(citing[0].part.kind, PartKind::Rule);
    // Other lookups of the in-memory source.
    let s = scope(&fixture);
    assert!(matches!(
        source
            .resolve_any_kind(&s, "urn:ordin:acme:skills:summarise")
            .await
            .unwrap(),
        Resolution::Found(_)
    ));
    assert!(matches!(
        source
            .resolve_any_kind(&s, "urn:ordin:acme:rules:tone")
            .await
            .unwrap(),
        Resolution::Ambiguous { .. }
    ));
    assert!(source
        .citing_parts(
            &fixture.repositories[0].0,
            VocabularyAxis::Category,
            "nothing"
        )
        .await
        .unwrap()
        .is_empty());
    let found = match source
        .resolve(&s, PartKind::Rule, "urn:ordin:acme:rules:shared", None)
        .await
        .unwrap()
    {
        Resolution::Found(p) => p,
        other => panic!("{other:?}"),
    };
    let mut gone = found.clone();
    gone.part.version = "9.9.9".into();
    assert_eq!(
        source.files(&gone).await.unwrap_err().code(),
        "AM_SOURCE_CORRUPT"
    );
}
