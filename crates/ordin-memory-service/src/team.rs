//! Team knowledge and read folders (*Internal Logic* E, F, G, H, P, Q; ADR-103, ADR-110, ADR-111).

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use memory::types::api::{ExportRequest, ImportRequest, ImportResult};
use memory::types::domain::{Entity, Fact, MemoryClass, Relationship, Source, Visibility};
use memory::types::scope::ScopeRef;
use ordin_types::time::Timestamp;
use ordin_types::{MemoryFactId, TeamFolderId, WorkspaceId};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::config::TENANT;
use crate::db::{placeholders, q};
use crate::error::{map_call_error, MemoryError};
use crate::model::{
    FactState, MemoryPlace, TeamEntity, TeamExportPage, TeamFact, TeamImportBatch,
    TeamImportOutcome, TeamImportResult, TeamRelationship, TeamSource,
};
use crate::remember::{is_unseen, statement_ok, PAGE_ORIGIN};
use crate::service::{new_correlation_id, store_error_of, Inner, MemoryService};
use crate::store::{self, RefRow};
use crate::views::text_of;
use memory::service::CallError;

const FACT_INVALID: &str = "MEM_TEAM_FACT_INVALID";
const FACT_ELSEWHERE: &str = "MEM_TEAM_FACT_ELSEWHERE";
const GATE_REFUSED: &str = "MEM_TEAM_GATE_REFUSED";

fn permille(value: f64) -> u16 {
    (value * 1000.0).round().clamp(0.0, 1000.0) as u16
}

fn class_text(class: MemoryClass) -> &'static str {
    match class {
        MemoryClass::Episodic => "episodic",
        MemoryClass::Semantic => "semantic",
        MemoryClass::Consolidated => "consolidated",
    }
}

fn parse_class(raw: &str) -> Option<MemoryClass> {
    match raw {
        "episodic" => Some(MemoryClass::Episodic),
        "semantic" => Some(MemoryClass::Semantic),
        "consolidated" => Some(MemoryClass::Consolidated),
        _ => None,
    }
}

/// `<table>:<uuid>`.
fn is_ref(id: &str, table: &str) -> bool {
    id.strip_prefix(table)
        .and_then(|rest| rest.strip_prefix(':'))
        .is_some_and(|raw| Uuid::parse_str(raw).is_ok())
}

/// Step F.2's field rules.
fn fact_is_valid(fact: &TeamFact) -> bool {
    let opt_fact = |id: &Option<String>| id.as_deref().is_none_or(|id| is_ref(id, "fact"));
    fact.format == 1
        && is_ref(&fact.memory_fact_id, "fact")
        && fact
            .source_id
            .as_deref()
            .is_none_or(|id| is_ref(id, "source"))
        && opt_fact(&fact.supersedes)
        && opt_fact(&fact.superseded_by)
        && fact.derived_from.iter().all(|id| is_ref(id, "fact"))
        && fact.entities.iter().all(|id| is_ref(id, "entity"))
        && statement_ok(&fact.statement, 4000)
        && fact
            .about
            .as_deref()
            .is_none_or(|about| (1..=200).contains(&about.chars().count()))
        && parse_class(&fact.memory_class).is_some()
        && fact
            .fact_class
            .as_deref()
            .is_none_or(|class| (1..=128).contains(&class.chars().count()))
        && matches!(
            fact.origin_kind.as_str(),
            "conversation" | "agent" | "document" | "person"
        )
        && (1..=64).contains(&fact.entities.len())
        && fact.derived_from.len() <= 64
        && fact.confidence_permille <= 1000
        && fact.valid_to.is_none_or(|to| to >= fact.valid_from)
        && (1..=200).contains(&fact.shared_by.chars().count())
        && !fact.shared_by.chars().any(is_unseen)
}

fn millis(ts: Timestamp) -> DateTime<Utc> {
    ts.0
}

/// The most a team file's stated confidence counts for: a colleague's 1000 is a claim, not a fact
/// about this workspace.
// spec: ordin-desktop/components/MemoryService#SA-TRUST-03
const TEAM_CONFIDENCE_CAP: f64 = 0.85;

fn team_confidence(permille: u16) -> f64 {
    (f64::from(permille) / 1000.0).min(TEAM_CONFIDENCE_CAP)
}

/// Step F.3.
fn fact_of_team(fact: &TeamFact, owner: &ScopeRef, collection: &str) -> Option<Fact> {
    let mut content = json!({ "statement": fact.statement });
    if let Some(about) = &fact.about {
        content["subject"] = Value::String(about.clone());
    }
    Some(Fact {
        id: fact.memory_fact_id.clone(),
        content,
        entities: fact.entities.clone(),
        source_id: fact.source_id.clone(),
        memory_class: parse_class(&fact.memory_class)?,
        fact_class: fact.fact_class.clone(),
        visibility: Visibility::TeamShared,
        owner: owner.clone(),
        valid_from: millis(fact.valid_from),
        valid_to: fact.valid_to.map(millis),
        ingested_at: Utc::now(),
        confidence: team_confidence(fact.confidence_permille),
        salience: 0.5,
        stability: 1.0,
        pii_review: false,
        supersedes: fact.supersedes.clone(),
        superseded_by: fact.superseded_by.clone(),
        derived_from: fact.derived_from.clone(),
        last_recalled_at: None,
        collection: collection.to_owned(),
    })
}

fn call_failure(error: CallError) -> MemoryError {
    map_call_error(error)
}

impl MemoryService {
    /// Rule E.
    pub async fn export_team(
        &self,
        workspace_id: WorkspaceId,
        cursor: Option<String>,
    ) -> Result<TeamExportPage, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if !inner.is_workspace_program() {
            return Err(MemoryError::not_supported());
        }
        let place = MemoryPlace::Workspace(workspace_id);
        let guard = inner.auth.mint(inner.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        let page = inner
            .service
            .export_scope(guard.cx(&correlation_id, None), ExportRequest { cursor })
            .await
            .map_err(call_failure)?
            .data;
        drop(guard);

        // Step 3.
        let ids: Vec<String> = page.facts.iter().map(|f| f.id.clone()).collect();
        let rows: HashMap<String, RefRow> = store::refs_by_facts(&inner.db, &ids)
            .await?
            .into_iter()
            .map(|row| (row.memory_fact_id.clone(), row))
            .collect();
        // A forget takes every version; this holds back those of a forget from before it did.
        let scope_key = store::scope_key(place);
        let earlier = store::earlier_than_forgotten(&inner.db, &scope_key).await?;
        let confirmed = store::confirmed_citations(&inner.db, &scope_key).await?;
        let unconfirmed = store::unconfirmed_citations(&inner.db, &scope_key).await?;
        let resting_on_unconfirmed = store::summaries_resting_on(&inner.db, &unconfirmed).await?;
        let now = Timestamp::now();
        let mut held_for_check = 0u32;
        let mut facts = Vec::new();
        let mut named_entities = BTreeSet::new();
        let mut named_sources = BTreeSet::new();
        for fact in &page.facts {
            let Some(row) = rows.get(&fact.id) else {
                continue;
            };
            let closed = matches!(row.state, FactState::Replaced | FactState::Retired)
                && fact.valid_to.is_some();
            let exportable = !row.everywhere
                && row.origin_kind != "knowledge_page"
                && row.read_folder_id.is_none()
                && !earlier.contains(&row.id)
                && (row.state == FactState::Kept || closed);
            if !exportable {
                continue;
            }
            // An agent can name any document, so what it cites waits for a person.
            // spec: ordin-desktop/components/MemoryService#SA-CITE-02
            let cited_by_agent = row.origin_kind == "document" && row.agent_instance_id.is_some();
            if fact.pii_review
                || (cited_by_agent && !confirmed.contains(&row.id))
                || resting_on_unconfirmed.contains(&row.id)
            {
                held_for_check += 1;
                continue;
            }
            // Step 4, rule E-3.
            let shared_at = match (row.shared_at, row.first_exported_at) {
                (Some(at), _) | (None, Some(at)) => at,
                (None, None) => {
                    inner
                        .db
                        .exec(
                            q("UPDATE memory_fact_refs SET first_exported_at = $1 WHERE id = $2")
                                .ts(now)
                                .uuid(row.id.0),
                        )
                        .await?;
                    now
                }
            };
            named_entities.extend(fact.entities.iter().cloned());
            named_sources.extend(fact.source_id.iter().cloned());
            facts.push(TeamFact {
                format: 1,
                id: row.id,
                memory_fact_id: fact.id.clone(),
                statement: text_of(fact, "statement").unwrap_or_default(),
                about: text_of(fact, "subject"),
                memory_class: class_text(fact.memory_class).to_owned(),
                fact_class: fact.fact_class.clone(),
                origin_kind: row.origin_kind.clone(),
                source_id: fact.source_id.clone(),
                entities: fact.entities.clone(),
                valid_from: Timestamp(fact.valid_from),
                valid_to: fact.valid_to.map(Timestamp),
                confidence_permille: permille(fact.confidence),
                supersedes: fact.supersedes.clone(),
                superseded_by: fact.superseded_by.clone(),
                derived_from: fact.derived_from.clone(),
                written_at: row.written_at,
                shared_by: row.shared_by.clone().unwrap_or_default(),
                shared_at,
            });
        }

        // spec: ordin-desktop/components/MemoryService#SA-AMEM-04
        // Step 5 and SA-AMEM-04.
        let entities = page
            .entities
            .iter()
            .filter(|e| named_entities.contains(&e.id))
            .map(|e| TeamEntity {
                format: 1,
                id: e.id.clone(),
                canonical_name: e.canonical_name.clone(),
                aliases: e.aliases.clone(),
            })
            .collect();
        let sources = page
            .sources
            .iter()
            .filter(|s| named_sources.contains(&s.id) && !s.origin_ref.starts_with(PAGE_ORIGIN))
            .map(|s| TeamSource {
                format: 1,
                id: s.id.clone(),
                origin_ref: s.origin_ref.clone(),
                modification_marker: s.modification_marker.clone(),
            })
            .collect();
        let exported: Vec<String> = facts.iter().map(|f| f.memory_fact_id.clone()).collect();
        let linked = inner.links_of_facts(&exported).await?;
        let relationships = page
            .relationships
            .iter()
            .filter(|r| linked.contains(&r.id))
            .map(|r| TeamRelationship {
                format: 1,
                id: r.id.clone(),
                kind: r.kind.clone(),
                from: r.from.clone(),
                to: r.to.clone(),
                valid_from: Timestamp(r.valid_from),
                valid_to: r.valid_to.map(Timestamp),
                confidence_permille: permille(r.confidence),
                source_id: r.source_id.clone(),
            })
            .collect();
        tracing::info!(
            target: "ordin::memory",
            scope_key = %scope_key,
            facts = facts.len(),
            held = held_for_check,
            "memory.team_exported"
        );
        Ok(TeamExportPage {
            facts,
            entities,
            sources,
            relationships,
            held_for_check,
            next_cursor: page.next_cursor,
        })
    }

    /// Rule F, with the ADR-110 and ADR-111 changes.
    pub async fn import_team(
        &self,
        workspace_id: WorkspaceId,
        batch: TeamImportBatch,
    ) -> Result<Vec<TeamImportOutcome>, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if !inner.is_workspace_program() {
            return Err(MemoryError::not_supported());
        }
        if batch.facts.len() > 200 || (batch.facts.is_empty() && batch.relationships.is_empty()) {
            tracing::error!(target: "ordin::memory", "memory.team_batch_refused");
            return Err(MemoryError::invalid("MEM_TEAM_BATCH_INVALID", "facts"));
        }
        if batch.entities.len() > 200 {
            tracing::error!(target: "ordin::memory", "memory.team_batch_refused");
            return Err(MemoryError::invalid("MEM_TEAM_BATCH_INVALID", "entities"));
        }
        let place = MemoryPlace::Workspace(workspace_id);
        let owner = Inner::owner_of(place);
        let scope_key = store::scope_key(place);
        let collection = batch
            .read_folder
            .map(|f| format!("tk:{f}"))
            .unwrap_or_default();

        // Step 2. A source a file brings is shown beside the fact as `remember`'s is, so it passes the
        // same checks; a fact citing one that fails is not taken. A `knowledge-page:` origin stands:
        // a colleague's page section carries one, and only `origin_kind` makes a row a page.
        let mut refused_sources: HashSet<&str> = HashSet::new();
        for source in &batch.sources {
            if !inner.team_source_is_clean(place, source).await? {
                refused_sources.insert(source.id.as_str());
            }
        }
        let mut outcomes: Vec<TeamImportOutcome> = Vec::with_capacity(batch.facts.len());
        let mut valid: Vec<&TeamFact> = Vec::new();
        let mut seen_in_batch: HashSet<String> = HashSet::new();
        let mut sharers: HashMap<String, bool> = HashMap::new();
        for fact in &batch.facts {
            let cites_refused = fact
                .source_id
                .as_deref()
                .is_some_and(|id| refused_sources.contains(id));
            // One row and one fact each: a second use of either would store a fact with no row.
            let repeated = !seen_in_batch.insert(fact.id.to_string())
                | !seen_in_batch.insert(fact.memory_fact_id.clone());
            let verdict = if cites_refused || repeated {
                Verdict::Refused("MEM_TEAM_FACT_INVALID")
            } else {
                inner
                    .judge(fact, place, &scope_key, batch.read_folder, &mut sharers)
                    .await?
            };
            match verdict {
                Verdict::Refused(code) => outcomes.push(TeamImportOutcome {
                    item_id: fact.id.to_string(),
                    result: TeamImportResult::NotTaken { code },
                }),
                // A person's own fact comes back through the team folder, or a fact this computer
                // forgot does. This computer's is the record: a file, which a colleague can edit,
                // does not rewrite it or bring it back.
                Verdict::Own => outcomes.push(TeamImportOutcome {
                    item_id: fact.id.to_string(),
                    result: TeamImportResult::Unchanged,
                }),
                Verdict::Goes => valid.push(fact),
            }
        }

        // Step 3.
        let mut facts = Vec::with_capacity(valid.len());
        for fact in &valid {
            facts.push(fact_of_team(fact, &owner, &collection).ok_or_else(MemoryError::internal)?);
        }
        let request = ImportRequest {
            facts,
            entities: batch
                .entities
                .iter()
                .map(|e| Entity {
                    id: e.id.clone(),
                    canonical_name: e.canonical_name.clone(),
                    aliases: e.aliases.clone(),
                    owner: owner.clone(),
                    collection: collection.clone(),
                    private_aliases: Vec::new(),
                    alias_folders: Vec::new(),
                })
                .collect(),
            sources: batch
                .sources
                .iter()
                .filter(|s| !refused_sources.contains(s.id.as_str()))
                .map(|s| Source {
                    id: s.id.clone(),
                    origin_ref: s.origin_ref.clone(),
                    modification_marker: s.modification_marker.clone(),
                    trust_signal: inner.config.source_trust_default,
                    owner: owner.clone(),
                    collection: collection.clone(),
                })
                .collect(),
            relationships: batch
                .relationships
                .iter()
                .map(|r| Relationship {
                    id: r.id.clone(),
                    kind: r.kind.clone(),
                    from: r.from.clone(),
                    to: r.to.clone(),
                    valid_from: r.valid_from.0,
                    valid_to: r.valid_to.map(|t| t.0),
                    ingested_at: Utc::now(),
                    confidence: team_confidence(r.confidence_permille),
                    source_id: r.source_id.clone(),
                    owner: owner.clone(),
                    collection: collection.clone(),
                    replaces: None,
                    owner_only: false,
                    author: None,
                })
                .collect(),
        };

        // Step 4.
        let guard = inner.auth.mint(inner.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        let report = inner
            .service
            // The file's confidence was capped when it was read (SA-TRUST-03); scaling it by its
            // source's trust as well would hold every fact citing a new source.
            .import_scored_facts(guard.cx(&correlation_id, None), request)
            .await
            .map_err(call_failure)?
            .data;
        drop(guard);

        // Step 5.
        let sources: HashMap<&str, &TeamSource> =
            batch.sources.iter().map(|s| (s.id.as_str(), s)).collect();
        for (id, result) in report.results {
            let Some(fact) = valid.iter().find(|f| f.memory_fact_id == id) else {
                if let ImportResult::Invalid { reason } = result {
                    tracing::warn!(target: "ordin::memory", reason = %reason, "memory.team_item_refused");
                }
                continue;
            };
            let result = match result {
                ImportResult::Admitted => {
                    inner
                        .upsert_imported(fact, workspace_id, batch.read_folder, &sources)
                        .await?;
                    TeamImportResult::Kept
                }
                ImportResult::Unchanged => TeamImportResult::Unchanged,
                ImportResult::Quarantined | ImportResult::Rejected => {
                    TeamImportResult::NotTaken { code: GATE_REFUSED }
                }
                ImportResult::Invalid { reason } => {
                    tracing::warn!(target: "ordin::memory", reason = %reason, "memory.team_item_refused");
                    TeamImportResult::NotTaken { code: FACT_INVALID }
                }
            };
            outcomes.push(TeamImportOutcome {
                item_id: fact.id.to_string(),
                result,
            });
        }

        // Step 7.
        let kept = outcomes
            .iter()
            .filter(|o| o.result == TeamImportResult::Kept)
            .count();
        let unchanged = outcomes
            .iter()
            .filter(|o| o.result == TeamImportResult::Unchanged)
            .count();
        let not_taken = outcomes.len() - kept - unchanged;
        inner
            .journal(
                "team_imported",
                None,
                None,
                Some(workspace_id),
                "Facts were got from a team folder.",
                json!({
                    "workspace_id": workspace_id.to_string(),
                    "kept": kept,
                    "unchanged": unchanged,
                    "not_taken": not_taken,
                }),
            )
            .await;
        tracing::info!(
            target: "ordin::memory",
            scope_key = %scope_key,
            kept,
            unchanged,
            not_taken,
            "memory.team_imported"
        );
        Ok(outcomes)
    }

    /// Rule G.
    pub async fn forget_team_fact(
        &self,
        workspace_id: WorkspaceId,
        id: MemoryFactId,
    ) -> Result<(), MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let Some(row) = store::ref_by_id(&inner.db, id).await? else {
            return Ok(());
        };
        if row.scope_key != store::scope_key(MemoryPlace::Workspace(workspace_id)) {
            return Err(MemoryError::fact_not_found());
        }
        if row.state == FactState::Forgotten {
            return Ok(());
        }
        let forgotten = inner.forget_row(&row, "team-forget").await?;
        inner
            .journal(
                "team_fact_forgotten",
                None,
                None,
                row.workspace_id,
                "A fact a colleague deleted was forgotten here.",
                json!({ "fact_ref": id.to_string(), "proof_digest": forgotten.digest }),
            )
            .await;
        tracing::info!(target: "ordin::memory", fact_ref = %id, "memory.team_fact_forgotten");
        Ok(())
    }

    /// Rule H.
    pub async fn held_for_check(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<MemoryFactId>, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let rows = inner
            .db
            .all(
                q("SELECT r.id AS id FROM memory_fact_refs r \
                   JOIN mem_fact f ON f.id = r.memory_fact_id \
                   WHERE r.scope_key = $1 AND r.state = 'kept' AND r.everywhere = $2 \
                   AND r.origin_kind <> 'knowledge_page' AND f.pii_review = $3 \
                   ORDER BY r.written_at DESC LIMIT 1000")
                .text(store::scope_key(MemoryPlace::Workspace(workspace_id)))
                .boolean(false)
                .boolean(true),
            )
            .await?;
        rows.iter()
            .map(|row| Ok(MemoryFactId(row.uuid("id")?)))
            .collect()
    }

    /// Rule P.
    pub async fn remove_read_folder(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
    ) -> Result<u32, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if !inner.is_workspace_program() {
            return Err(MemoryError::not_supported());
        }
        let team = store::scope_key(MemoryPlace::Workspace(workspace_id));
        let proof = inner
            .storage
            .erase_collection(TENANT, &team, &format!("tk:{folder_id}"))
            .await
            .map_err(store_error_of)?;
        let mine = "(SELECT id FROM memory_fact_refs WHERE workspace_id = $1 \
                    AND read_folder_id = $2)";
        let count_sql = format!("SELECT COUNT(*) AS n FROM memory_fact_refs WHERE id IN {mine}");
        let rows_removed = store::count(
            &inner.db,
            q(count_sql.clone()).uuid(workspace_id.0).uuid(folder_id.0),
        )
        .await?;
        let mut tx = inner.db.begin().await?;
        tx.exec(
            q(format!(
                "DELETE FROM memory_recall_items WHERE fact_ref IN {mine}"
            ))
            .uuid(workspace_id.0)
            .uuid(folder_id.0),
        )
        .await?;
        tx.exec(
            q(format!(
                "DELETE FROM memory_made_from WHERE fact_ref IN {mine}"
            ))
            .uuid(workspace_id.0)
            .uuid(folder_id.0),
        )
        .await?;
        tx.exec(
            q(format!(
                "DELETE FROM memory_made_from WHERE origin_ref IN {mine}"
            ))
            .uuid(workspace_id.0)
            .uuid(folder_id.0),
        )
        .await?;
        tx.exec(
            q(format!(
                "DELETE FROM memory_consolidations WHERE fact_ref IN {mine}"
            ))
            .uuid(workspace_id.0)
            .uuid(folder_id.0),
        )
        .await?;
        tx.exec(
            q(format!(
                "UPDATE memory_fact_refs SET replaces_id = NULL WHERE replaces_id IN {mine}"
            ))
            .uuid(workspace_id.0)
            .uuid(folder_id.0),
        )
        .await?;
        tx.exec(
            q("DELETE FROM memory_fact_refs WHERE workspace_id = $1 AND read_folder_id = $2")
                .uuid(workspace_id.0)
                .uuid(folder_id.0),
        )
        .await?;
        tx.commit().await?;
        inner
            .journal(
                "read_folder_removed",
                None,
                None,
                Some(workspace_id),
                "The workspace stopped reading a folder.",
                json!({
                    "workspace_id": workspace_id.to_string(),
                    "folder_id": folder_id.to_string(),
                    "rows_removed": rows_removed,
                    "digest": proof.digest,
                }),
            )
            .await;
        tracing::info!(
            target: "ordin::memory",
            workspace_id = %workspace_id,
            folder_id = %folder_id,
            rows_removed,
            "memory.read_folder_removed"
        );
        Ok(rows_removed as u32)
    }

    // spec: ordin-desktop/components/MemoryService#SA-MREAD-03
    /// Rule Q: the person's **Adopt** and, with `again`, **Adopt again**.
    pub async fn adopt_fact(
        &self,
        workspace_id: WorkspaceId,
        source: MemoryFactId,
        statement: Option<String>,
        again: Option<MemoryFactId>,
    ) -> Result<MemoryFactId, MemoryError> {
        let inner = &self.inner;
        inner.model_gate()?;
        if !inner.is_workspace_program() {
            return Err(MemoryError::not_supported());
        }
        let place = MemoryPlace::Workspace(workspace_id);
        let scope_key = store::scope_key(place);

        // Step 1.
        let not_read = || MemoryError::Refused {
            code: "MEM_ADOPT_SOURCE_NOT_READ",
        };
        let source_row = store::ref_by_id(&inner.db, source)
            .await?
            .filter(|row| {
                row.scope_key == scope_key
                    && row.state == FactState::Kept
                    && row.read_folder_id.is_some()
            })
            .ok_or_else(not_read)?;
        let again_row = match again {
            Some(id) => Some(
                store::ref_by_id(&inner.db, id)
                    .await?
                    .filter(|row| {
                        row.scope_key == scope_key
                            && row.state == FactState::Kept
                            && row.read_folder_id.is_none()
                    })
                    .ok_or_else(MemoryError::fact_not_found)?,
            ),
            None => None,
        };

        // Step 2.
        let stored = inner.stored_fact(&source_row).await?;
        let text = statement.unwrap_or_else(|| text_of(&stored, "statement").unwrap_or_default());
        if !statement_ok(&text, 4000) {
            return Err(MemoryError::invalid("MEM_STATEMENT_INVALID", "statement"));
        }
        let about = text_of(&stored, "subject");

        // spec: ordin-desktop/components/MemoryService#SA-MREAD-04
        // Step 3 (SA-MREAD-04).
        let refused = |code| {
            tracing::info!(target: "ordin::memory", code, "memory.adopt_refused");
            MemoryError::Refused { code }
        };
        for checked in std::iter::once(&text).chain(about.iter()) {
            let found = inner.check_text(place, checked.clone()).await?;
            if found.personal_data != memory::service::PersonalDataFinding::None {
                return Err(refused("MEM_ADOPT_PERSONAL_DATA"));
            }
            if found.instruction_like {
                return Err(refused("MEM_ADOPT_REFUSED"));
            }
        }

        // Step 4.
        let new_id = again.unwrap_or_default();
        let fact_id = match &again_row {
            Some(row) => row.memory_fact_id.clone(),
            None => format!(
                "fact:{}",
                Uuid::new_v5(
                    &Uuid::NAMESPACE_DNS,
                    format!("fact:adopt:{new_id}").as_bytes()
                )
            ),
        };
        let owner = Inner::owner_of(place);
        let (entities, fact_entities) = inner.adopted_entities(place, &owner, &stored).await?;
        let mut sources = Vec::new();
        let mut source_id = None;
        if let Some(id) = &stored.source_id {
            let context = inner.place_context(place);
            let found =
                memory::types::ports::MemoryStore::get_source(inner.storage.as_ref(), &context, id)
                    .await
                    .map_err(store_error_of)?;
            if let Some(found) = found {
                let copy = format!(
                    "source:{}",
                    Uuid::new_v5(
                        &Uuid::NAMESPACE_DNS,
                        format!("source:adopt:{new_id}").as_bytes()
                    )
                );
                source_id = Some(copy.clone());
                sources.push(Source {
                    id: copy,
                    origin_ref: found.origin_ref,
                    modification_marker: found.modification_marker,
                    trust_signal: inner.config.source_trust_default,
                    owner: owner.clone(),
                    collection: String::new(),
                });
            }
        }
        let mut content = json!({ "statement": text });
        if let Some(about) = &about {
            content["subject"] = Value::String(about.clone());
        }
        let fact = Fact {
            id: fact_id.clone(),
            content,
            entities: fact_entities,
            source_id,
            memory_class: stored.memory_class,
            fact_class: stored.fact_class.clone(),
            visibility: Visibility::TeamShared,
            owner: owner.clone(),
            valid_from: stored.valid_from,
            valid_to: None,
            ingested_at: Utc::now(),
            confidence: stored.confidence,
            salience: 0.5,
            stability: 1.0,
            pii_review: false,
            supersedes: None,
            superseded_by: None,
            derived_from: Vec::new(),
            last_recalled_at: None,
            collection: String::new(),
        };
        let guard = inner.auth.mint(inner.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        let report = inner
            .service
            .import_scored_facts(
                guard.cx(&correlation_id, None),
                ImportRequest {
                    facts: vec![fact],
                    entities,
                    sources,
                    relationships: Vec::new(),
                },
            )
            .await
            .map_err(call_failure)?
            .data;
        drop(guard);
        let result = report
            .results
            .into_iter()
            .find(|(id, _)| *id == fact_id)
            .map(|(_, result)| result)
            .ok_or_else(MemoryError::internal)?;
        match result {
            ImportResult::Admitted | ImportResult::Unchanged => {}
            ImportResult::Invalid { reason } => {
                tracing::warn!(target: "ordin::memory", reason = %reason, "memory.team_item_refused");
                return Err(refused("MEM_ADOPT_REFUSED"));
            }
            ImportResult::Quarantined | ImportResult::Rejected => {
                return Err(refused("MEM_ADOPT_REFUSED"));
            }
        }

        // Step 5.
        let now = Timestamp::now();
        match &again_row {
            Some(row) => {
                inner
                    .db
                    .exec(
                        q(
                            "UPDATE memory_fact_refs SET written_at = $1, settled_at = $2 \
                           WHERE id = $3",
                        )
                        .ts(now)
                        .ts(now)
                        .uuid(row.id.0),
                    )
                    .await?;
            }
            None => {
                store::insert_ref(
                    &inner.db,
                    &RefRow {
                        id: new_id,
                        memory_fact_id: fact_id,
                        scope_key,
                        workspace_id: Some(workspace_id),
                        scope_id: None,
                        everywhere: false,
                        origin_kind: "person".to_owned(),
                        agent_instance_id: None,
                        run_id: None,
                        origin_ref: None,
                        origin_version: None,
                        fact_class: stored.fact_class.clone(),
                        job_id: "adopt".to_owned(),
                        state: FactState::Kept,
                        may_be_out_of_date: false,
                        replaces_id: None,
                        retain_until: None,
                        written_at: now,
                        settled_at: Some(now),
                        shared_by: None,
                        shared_at: None,
                        first_exported_at: None,
                        team_imported: false,
                        read_folder_id: None,
                        caller_kind: Some("person".to_owned()),
                        page_status: None,
                        page_tier: None,
                    },
                )
                .await?;
            }
        }

        // Step 6.
        inner
            .journal(
                "fact_adopted",
                None,
                None,
                Some(workspace_id),
                "The person adopted a fact from a read folder.",
                json!({
                    "fact_ref": new_id.to_string(),
                    "source_ref": source.to_string(),
                    "again": again.is_some(),
                }),
            )
            .await;
        tracing::info!(
            target: "ordin::memory",
            fact_ref = %new_id,
            again = again.is_some(),
            "memory.fact_adopted"
        );
        Ok(new_id)
    }
}

/// What step F.2 decides for one fact of a file.
enum Verdict {
    Goes,
    Own,
    Refused(&'static str),
}

impl Inner {
    /// The step F.2 verdict.
    async fn judge(
        &self,
        fact: &TeamFact,
        place: MemoryPlace,
        scope_key: &str,
        folder: Option<TeamFolderId>,
        sharers: &mut HashMap<String, bool>,
    ) -> Result<Verdict, MemoryError> {
        if !fact_is_valid(fact) {
            return Ok(Verdict::Refused(FACT_INVALID));
        }
        // `shared_by` reaches an agent beside the statement, so it passes the instruction check.
        // spec: ordin-desktop/components/MemoryService#SA-TEAM-01
        let clean = match sharers.get(&fact.shared_by) {
            Some(clean) => *clean,
            None => {
                let found = self.check_text(place, fact.shared_by.clone()).await?;
                let clean = !found.instruction_like;
                sharers.insert(fact.shared_by.clone(), clean);
                clean
            }
        };
        if !clean {
            return Ok(Verdict::Refused(FACT_INVALID));
        }
        let by_id = store::ref_by_id(&self.db, fact.id).await?;
        let by_fact = store::ref_by_fact(&self.db, &fact.memory_fact_id).await?;
        for row in by_id.iter().chain(by_fact.iter()) {
            if row.scope_key != scope_key {
                return Ok(Verdict::Refused(FACT_INVALID));
            }
            // A row belongs to one fact, so an id paired with another fact's is a forgery.
            if row.id != fact.id || row.memory_fact_id != fact.memory_fact_id {
                return Ok(Verdict::Refused(FACT_INVALID));
            }
            // spec: ordin-desktop/components/MemoryService#SA-TEAM-03
            if row.state == FactState::Forgotten {
                return Ok(Verdict::Own);
            }
            if row.read_folder_id != folder {
                return Ok(Verdict::Refused(FACT_ELSEWHERE));
            }
            if !row.team_imported {
                return Ok(Verdict::Own);
            }
        }
        Ok(Verdict::Goes)
    }

    /// Step F.5: a get that admitted a fact.
    async fn upsert_imported(
        &self,
        fact: &TeamFact,
        workspace_id: WorkspaceId,
        folder: Option<TeamFolderId>,
        sources: &HashMap<&str, &TeamSource>,
    ) -> Result<(), MemoryError> {
        let now = Timestamp::now();
        let source = fact
            .source_id
            .as_deref()
            .and_then(|id| sources.get(id))
            .filter(|_| fact.origin_kind == "document");
        let state = if fact.valid_to.is_none() {
            FactState::Kept
        } else if fact.superseded_by.is_some() {
            FactState::Replaced
        } else {
            FactState::Retired
        };
        let row = RefRow {
            id: fact.id,
            memory_fact_id: fact.memory_fact_id.clone(),
            scope_key: store::scope_key(MemoryPlace::Workspace(workspace_id)),
            workspace_id: Some(workspace_id),
            scope_id: None,
            everywhere: false,
            origin_kind: fact.origin_kind.clone(),
            agent_instance_id: None,
            run_id: None,
            origin_ref: source.map(|s| s.origin_ref.clone()),
            origin_version: source.and_then(|s| s.modification_marker.clone()),
            fact_class: fact.fact_class.clone(),
            job_id: "team-import".to_owned(),
            state,
            may_be_out_of_date: false,
            replaces_id: None,
            retain_until: None,
            written_at: fact.written_at,
            settled_at: Some(now),
            shared_by: Some(fact.shared_by.clone()),
            shared_at: Some(fact.shared_at),
            first_exported_at: None,
            team_imported: true,
            read_folder_id: folder,
            caller_kind: None,
            page_status: None,
            page_tier: None,
        };
        let mut tx = self.db.begin().await?;
        tx.exec(q("DELETE FROM memory_consolidations WHERE fact_ref = $1").uuid(row.id.0))
            .await?;
        let existing = store::ref_by_id(&self.db, row.id).await?;
        match existing {
            Some(_) => {
                tx.exec(
                    q(
                        "UPDATE memory_fact_refs SET origin_ref = $1, origin_version = $2, \
                       fact_class = $3, state = $4, written_at = $5, settled_at = $6, \
                       shared_by = $7, shared_at = $8 WHERE id = $9",
                    )
                    .opt_text(row.origin_ref.as_deref())
                    .opt_text(row.origin_version.as_deref())
                    .opt_text(row.fact_class.as_deref())
                    .text(store::state_text(row.state))
                    .ts(row.written_at)
                    .ts(now)
                    .opt_text(row.shared_by.as_deref())
                    .opt_ts(row.shared_at)
                    .uuid(row.id.0),
                )
                .await?;
            }
            None => store::insert_ref_tx(&mut tx, &row).await?,
        }
        tx.commit().await?;
        // SA-AMEM-08: a team summary mirrors those origins that map to a row of this workspace.
        if fact.memory_class == "consolidated" {
            let origins = store::refs_by_facts(&self.db, &fact.derived_from).await?;
            let ids: Vec<MemoryFactId> = origins
                .iter()
                .filter(|r| r.workspace_id == Some(workspace_id))
                .map(|r| r.id)
                .collect();
            store::insert_made_from(&self.db, fact.id, &ids).await?;
        }
        Ok(())
    }

    /// The relationships asserted by `facts` (SA-AMEM-04): one `SELECT`.
    async fn links_of_facts(&self, facts: &[String]) -> Result<HashSet<String>, MemoryError> {
        if facts.is_empty() {
            return Ok(Default::default());
        }
        let mut st = q(format!(
            "SELECT DISTINCT relationship_id FROM mem_relationship_fact WHERE fact_id IN ({})",
            placeholders(1, facts.len())
        ));
        for fact in facts {
            st = st.text(fact.clone());
        }
        self.db
            .all(st)
            .await?
            .iter()
            .map(|row| row.text("relationship_id"))
            .collect()
    }

    // spec: ordin-desktop/components/MemoryService#SA-MREAD-02
    /// SA-MREAD-02: an own entity of exactly equal name, else a copy under a new id.
    async fn adopted_entities(
        &self,
        place: MemoryPlace,
        owner: &ScopeRef,
        stored: &Fact,
    ) -> Result<(Vec<Entity>, Vec<String>), MemoryError> {
        use unicode_normalization::UnicodeNormalization;
        let context = self.place_context(place);
        let mut copies = Vec::new();
        let mut ids = Vec::with_capacity(stored.entities.len());
        for id in &stored.entities {
            let Some(entity) = self
                .storage
                .get_entity(&context, id)
                .await
                .map_err(store_error_of)?
            else {
                continue;
            };
            if entity.collection.is_empty() {
                ids.push(entity.id);
                continue;
            }
            let wanted: String = entity.canonical_name.nfc().collect();
            let own = self
                .storage
                .find_entity_by_name(&context, &entity.canonical_name)
                .await
                .map_err(store_error_of)?
                .into_iter()
                .find(|e| {
                    e.collection.is_empty() && e.canonical_name.nfc().collect::<String>() == wanted
                });
            if let Some(own) = own {
                ids.push(own.id);
                continue;
            }
            let copy_id = format!(
                "entity:{}",
                Uuid::new_v5(
                    &Uuid::NAMESPACE_DNS,
                    format!("entity:adopt:{}:{}", owner.user, entity.id).as_bytes()
                )
            );
            ids.push(copy_id.clone());
            copies.push(Entity {
                id: copy_id,
                canonical_name: entity.canonical_name,
                aliases: entity.aliases,
                owner: owner.clone(),
                collection: String::new(),
                private_aliases: Vec::new(),
                alias_folders: Vec::new(),
            });
        }
        Ok((copies, ids))
    }
}
