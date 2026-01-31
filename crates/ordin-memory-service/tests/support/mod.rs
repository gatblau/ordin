//! What the integration test binaries share.
#![allow(dead_code)]

pub mod models;
pub mod pg;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use models::{WordEmbedder, WordReranker};
use ordin_memory_service::{MemoryServiceConfig, ModelManifest, ProgramKind, TestSeams};

pub fn config(program: ProgramKind) -> MemoryServiceConfig {
    MemoryServiceConfig {
        program,
        models_dir: PathBuf::from("/nonexistent/ordin-models"),
        agent_wide_scope: None,
        log_level: tracing::Level::INFO,
        recall_log_days: 90,
        knowledge_scope: None,
        model_manifest: ModelManifest::default(),
    }
}

pub fn seams() -> TestSeams {
    TestSeams {
        embedder: Arc::new(WordEmbedder),
        reranker: Arc::new(WordReranker),
        rate_per_min: 60_000_000,
        reconcile_every: Duration::from_millis(20),
        recall_limit_secs: 10,
    }
}

/// A test's statement as facts (ADR-118): links are the facts when the input states any, else
/// `{subject: about | first thing | "Ordin", predicate: "prefers", object: {text: statement}}`; a
/// summary (`made_from`) still goes through `remember`. Answers the first fact's receipt.
pub async fn remember_text(
    svc: &ordin_memory_service::MemoryService,
    who: ordin_memory_service::MemoryCaller,
    input: ordin_memory_service::RememberInput,
) -> Result<ordin_memory_service::RememberReceipt, ordin_memory_service::MemoryError> {
    use ordin_memory_service::*;
    if !input.made_from.is_empty() {
        return svc.remember(who, input).await;
    }
    let named = |name: &str| ThingInput {
        name: name.to_owned(),
        other_names: input
            .entities
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.other_names.clone())
            .unwrap_or_default(),
    };
    let mut facts = Vec::new();
    for l in &input.links {
        facts.push(TripleInput {
            subject: named(&l.from).into(),
            predicate: PredicateInput::Propose {
                propose: TermProposal {
                    label: l.kind.clone(),
                    meaning: format!("The subject {} the object.", l.kind),
                    object: ObjectKindInput::Thing,
                    single_valued: false,
                },
            },
            object: ObjectInput::Thing(named(&l.to)),
            changed: false,
            ended: l.ended,
            confidence_permille: l.confidence_permille,
            valid_from: None,
        });
    }
    if facts.is_empty() {
        let subject = input
            .about
            .clone()
            .or_else(|| input.entities.first().map(|t| t.name.clone()))
            .unwrap_or_else(|| "Ordin".to_owned());
        facts.push(TripleInput {
            subject: named(&subject).into(),
            predicate: PredicateInput::Term("prefers".to_owned()),
            object: ObjectInput::Text(input.statement.clone()),
            changed: false,
            ended: false,
            confidence_permille: None,
            valid_from: None,
        });
    }
    let source = match (&input.source, &who) {
        (Some(s), _) => Some(FactSourceInput::Document(s.clone())),
        (None, MemoryCaller::Ordin { .. }) => Some(FactSourceInput::Conversation),
        (None, _) => None,
    };
    let receipt = svc
        .remember_facts(
            who,
            RememberFactsInput {
                source,
                fact_class: input.fact_class.clone(),
                facts,
            },
        )
        .await?;
    receipt
        .items
        .into_iter()
        .next()
        .ok_or(MemoryError::Refused {
            code: "MEM_INTERNAL",
        })
}

/// What a fact says: a triple's text object, else its statement.
pub fn said(view: &ordin_memory_service::FactView) -> String {
    match view.triple.as_ref().map(|t| &t.object) {
        Some(ordin_memory_service::ObjectView::Text { value }) => value.clone(),
        _ => view.statement.clone(),
    }
}
