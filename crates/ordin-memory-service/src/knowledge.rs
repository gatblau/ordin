//! A knowledge agent's documents (*Internal Logic* AI, with SA-MPD-05).

use memory::types::domain::Source;
use memory::write_pipeline::source_id;
use ordin_types::time::Timestamp;
use ordin_types::ScopeId;
use serde_json::json;

use crate::error::MemoryError;
use crate::model::{
    KnowledgeDocumentFacts, KnowledgeIndexReceipt, MemoryCaller, MemoryPlace, SourceKind,
};
use crate::remember::{
    entity_assertions, link_assertions, statement_ok, validate_things, RowSeed, Write,
    DOCUMENT_TRUST,
};
use crate::service::{store_error_of, Inner, MemoryService};
use crate::store::{self};

/// AI step 5.
const REMOVE_BATCH: i64 = 5_000;

// spec: ordin-desktop/components/MemoryService#SA-MPD-05
/// SA-MPD-05: the prefix's scheme and the version it takes.
fn origin_is_valid(prefix: &str, version: &str) -> bool {
    if !(1..=2048).contains(&prefix.chars().count()) {
        return false;
    }
    let count = version.chars().count();
    if prefix.starts_with("git:") {
        (7..=64).contains(&count) && version.chars().all(|c| c.is_ascii_hexdigit())
    } else if prefix.starts_with("confluence:") {
        (1..=10).contains(&count) && version.chars().all(|c| c.is_ascii_digit())
    } else if prefix.starts_with("sharepoint:") {
        (1..=256).contains(&count) && !version.chars().any(char::is_control)
    } else {
        false
    }
}

impl Inner {
    fn check_knowledge_scope(&self, scope_id: ScopeId) -> Result<(), MemoryError> {
        if self.knowledge_scope == Some(scope_id) {
            Ok(())
        } else {
            Err(MemoryError::Refused {
                code: "MEM_SCOPE_CROSS_WRITE",
            })
        }
    }
}

impl MemoryService {
    // spec: ordin-desktop/components/MemoryService#SA-MKA-10
    /// AI steps 1 to 4: one version of one document into the knowledge scope; retires the earlier
    /// version's facts (SA-MKA-10). Idempotent per (origin_prefix, version).
    pub async fn index_knowledge_document(
        &self,
        scope_id: ScopeId,
        doc: KnowledgeDocumentFacts,
    ) -> Result<KnowledgeIndexReceipt, MemoryError> {
        let inner = &self.inner;
        inner.model_gate()?;
        inner.check_knowledge_scope(scope_id)?;
        if !origin_is_valid(&doc.origin_prefix, &doc.version) {
            return Err(MemoryError::invalid("MEM_SOURCE_INVALID", "origin_prefix"));
        }
        if doc.trust_permille > 1000 || doc.facts.len() > 200 {
            return Err(MemoryError::invalid("MEM_PAGE_SECTIONS_INVALID", "facts"));
        }
        let longest = match doc.standing.kind {
            SourceKind::Page | SourceKind::Glossary => 4000,
            SourceKind::Document => 500,
        };
        for fact in &doc.facts {
            if !statement_ok(&fact.statement, longest) {
                return Err(MemoryError::invalid("MEM_STATEMENT_INVALID", "statement"));
            }
            if let Some(about) = &fact.about {
                if !(1..=200).contains(&about.chars().count()) {
                    return Err(MemoryError::invalid("MEM_ABOUT_INVALID", "about"));
                }
            }
            validate_things(fact.about.as_deref(), &fact.entities, &fact.links)?;
        }

        // Step 2.
        let place = MemoryPlace::Scope(scope_id);
        let scope_key = store::scope_key(place);
        let prefix = format!("{}#", doc.origin_prefix);
        let current = store::rows_with_prefix(&inner.db, &scope_key, &prefix, 100_000).await?;
        let at_version = current
            .iter()
            .filter(|r| r.origin_version.as_deref() == Some(doc.version.as_str()))
            .count();
        if at_version == doc.facts.len() && at_version == current.len() {
            return Ok(KnowledgeIndexReceipt {
                written: 0,
                retired: 0,
            });
        }

        // Step 3.
        let owner = Inner::owner_of(place);
        // The reader passes 500 for a document, which the write gate would hold (SA-TRUST-01).
        let trust = (f64::from(doc.trust_permille) / 1000.0).max(DOCUMENT_TRUST);
        let page_like = doc.standing.kind != SourceKind::Document;
        let stale = doc
            .standing
            .stale_after
            .is_some_and(|after| after < Timestamp::now());
        let who = MemoryCaller::KnowledgeAgent { scope_id };
        let mut written = Vec::with_capacity(doc.facts.len());
        for (at, fact) in doc.facts.iter().enumerate() {
            let n = at + 1;
            let origin_ref = format!("{}#{n}", doc.origin_prefix);
            let source = Source {
                id: source_id(&owner, &origin_ref),
                origin_ref: origin_ref.clone(),
                modification_marker: Some(doc.version.clone()),
                trust_signal: trust,
                owner: owner.clone(),
                collection: String::new(),
            };
            inner
                .storage
                .put_source(&source)
                .await
                .map_err(store_error_of)?;
            let mut seed = RowSeed::of("document", "hosted_agent");
            // spec: ordin-desktop/components/MemoryService#SA-LINK-01
            seed.author = Some(format!("knowledge:{scope_id}"));
            seed.origin_ref = Some(origin_ref.clone());
            seed.origin_version = Some(doc.version.clone());
            seed.may_be_out_of_date = stale;
            if page_like {
                seed.page_status = doc.standing.status;
                seed.page_tier = doc.standing.tier;
            }
            let entities = entity_assertions(&fact.entities);
            let links = link_assertions(&fact.links);
            let receipt = inner
                .write_paced(|| Write {
                    place,
                    context: inner.caller_context(&who, None),
                    key: format!("kdoc:{}:{}:{n}", doc.origin_prefix, doc.version),
                    statement: fact.statement.clone(),
                    about: fact.about.clone(),
                    memory_class: if page_like { "semantic" } else { "episodic" },
                    fact_class: None,
                    source: Some((origin_ref.clone(), Some(doc.version.clone()))),
                    visibility: "team-shared",
                    collection: None,
                    entities: entities.clone(),
                    links: links.clone(),
                    made_from: Vec::new(),
                    row: seed.clone(),
                })
                .await?;
            written.push(receipt.id);
        }

        // Step 4.
        let mut retired = 0u32;
        for row in &current {
            if row.origin_version.as_deref() != Some(doc.version.as_str())
                && !written.contains(&row.id)
            {
                inner
                    .retire_row(row, format!("kdoc-retire:{}", row.id))
                    .await?;
                retired += 1;
            }
        }
        let receipt = KnowledgeIndexReceipt {
            written: doc.facts.len() as u16,
            retired,
        };
        inner
            .journal(
                "knowledge_document_indexed",
                None,
                None,
                None,
                "A knowledge agent read a document.",
                json!({
                    "scope_id": scope_id.to_string(),
                    "written": receipt.written,
                    "retired": receipt.retired,
                }),
            )
            .await;
        tracing::info!(
            target: "ordin::memory",
            scope_id = %scope_id,
            written = receipt.written,
            retired = receipt.retired,
            "memory.knowledge_document_indexed"
        );
        Ok(receipt)
    }

    /// AI step 5: retires every fact whose `origin_ref` starts with `origin_prefix` in the scope.
    pub async fn remove_knowledge_document(
        &self,
        scope_id: ScopeId,
        origin_prefix: &str,
    ) -> Result<u32, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.check_knowledge_scope(scope_id)?;
        if !(1..=2049).contains(&origin_prefix.chars().count()) {
            return Err(MemoryError::invalid("MEM_SOURCE_INVALID", "origin_prefix"));
        }
        let scope_key = store::scope_key(MemoryPlace::Scope(scope_id));
        let rows =
            store::rows_with_prefix(&inner.db, &scope_key, origin_prefix, REMOVE_BATCH).await?;
        for row in &rows {
            inner
                .retire_row(row, format!("kdoc-retire:{}", row.id))
                .await?;
        }
        let retired = rows.len() as u32;
        inner
            .journal(
                "knowledge_document_removed",
                None,
                None,
                None,
                "A document left a knowledge agent's repositories.",
                json!({ "scope_id": scope_id.to_string(), "retired": retired }),
            )
            .await;
        tracing::info!(
            target: "ordin::memory",
            scope_id = %scope_id,
            retired,
            "memory.knowledge_document_removed"
        );
        Ok(retired)
    }
}
