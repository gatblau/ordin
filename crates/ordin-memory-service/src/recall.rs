//! `recall` (*Internal Logic* §5 with the ADR-110, ADR-111, ADR-112/113 and ADR-115 changes).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use memory::retrieval::RecallOutcome;
use memory::types::api::RecallLink;
use memory::types::domain::Fact;
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, MemoryFactId, MemoryRecallId, RunId, WorkspaceId};
use serde_json::json;
use uuid::Uuid;

use crate::error::{map_call_error, MemoryError};
use crate::model::{
    FactState, FactView, LinkView, MemoryCaller, MemoryPlace, PageStatus, RecallInput,
    RecallResult, RecallSearchKind, WiderMode, WiderReason, WiderStatus,
};
use crate::providers::as_recall_query;
use crate::service::{new_correlation_id, place_of, Inner, MemoryService};
use crate::store::{self, NewRecall, RefRow};
use crate::views::{Names, ViewNote};
use crate::wider::WiderOutcome;

// spec: ordin-desktop/components/MemoryService#SA-MPD-01
/// §5 step 1 (ADR-110 and SA-MPD-01).
fn validate(who: &MemoryCaller, input: &RecallInput) -> Result<(), MemoryError> {
    let searches_invalid = || MemoryError::invalid("MEM_SEARCHES_INVALID", "searches");
    match (&input.query, &input.searches) {
        (Some(query), None) => {
            if !(1..=4096).contains(&query.chars().count()) {
                return Err(MemoryError::invalid("MEM_QUERY_INVALID", "query"));
            }
        }
        (None, Some(searches)) => {
            let good = (1..=4).contains(&searches.len())
                && searches.iter().all(|s| {
                    (1..=500).contains(&s.query.chars().count())
                        && !s.query.chars().any(char::is_control)
                });
            if !good {
                return Err(searches_invalid());
            }
        }
        _ => return Err(searches_invalid()),
    }
    if let Some(folders) = &input.folders {
        let distinct: HashSet<&String> = folders.iter().collect();
        if !(1..=21).contains(&folders.len())
            || distinct.len() != folders.len()
            || !folders
                .iter()
                .all(|f| (1..=300).contains(&f.chars().count()))
        {
            return Err(MemoryError::invalid("MEM_FOLDER_UNKNOWN", "folders"));
        }
    }
    let cap = if matches!(who, MemoryCaller::KnowledgeAgent { .. }) {
        50
    } else {
        20
    };
    if !(1..=cap).contains(&input.result_cap) {
        return Err(MemoryError::invalid("MEM_RESULT_CAP_INVALID", "result_cap"));
    }
    Ok(())
}

fn caller_kind(who: &MemoryCaller) -> &'static str {
    match who {
        MemoryCaller::Ordin { .. } | MemoryCaller::Person { .. } => "ordin",
        MemoryCaller::CrewMember { .. } => "crew_member",
        MemoryCaller::Agent { .. } => "agent",
        MemoryCaller::HostedAgent { .. } | MemoryCaller::KnowledgeAgent { .. } => "hosted_agent",
        MemoryCaller::KnowledgeService { .. } => "knowledge_service",
    }
}

fn run_of(who: &MemoryCaller) -> Option<RunId> {
    match who {
        MemoryCaller::Ordin { run_id, .. }
        | MemoryCaller::CrewMember { run_id, .. }
        | MemoryCaller::Agent { run_id, .. }
        | MemoryCaller::HostedAgent { run_id, .. } => Some(*run_id),
        MemoryCaller::Person { .. }
        | MemoryCaller::KnowledgeService { .. }
        | MemoryCaller::KnowledgeAgent { .. } => None,
    }
}

fn agent_of(who: &MemoryCaller) -> Option<AgentInstanceId> {
    match who {
        MemoryCaller::CrewMember {
            agent_instance_id, ..
        }
        | MemoryCaller::Agent {
            agent_instance_id, ..
        } => Some(*agent_instance_id),
        _ => None,
    }
}

fn workspace_of(who: &MemoryCaller) -> Option<WorkspaceId> {
    match place_of(who) {
        MemoryPlace::Workspace(w) => Some(w),
        MemoryPlace::Scope(_) => None,
    }
}

// spec: ordin-desktop/components/MemoryService#SA-MKA-02
/// SA-MKA-02: a fact of a deprecated page stays only when nothing else matches.
pub(crate) fn suppress_deprecated(facts: &mut Vec<FactView>, outside_alive: bool) {
    let anything_else = outside_alive
        || facts
            .iter()
            .any(|f| f.page_status != Some(PageStatus::Deprecated));
    if anything_else {
        facts.retain(|f| f.page_status != Some(PageStatus::Deprecated));
    }
}

impl MemoryService {
    /// Ordin Host's `recall` tool, the Knowledge Query Server, and the MCP Front through the tool
    /// provider.
    pub async fn recall(
        &self,
        who: MemoryCaller,
        input: RecallInput,
    ) -> Result<RecallResult, MemoryError> {
        self.inner
            .recall_as(who, input, &new_correlation_id())
            .await
    }
}

impl Inner {
    pub(crate) async fn recall_as(
        self: &Arc<Self>,
        who: MemoryCaller,
        input: RecallInput,
        correlation_id: &str,
    ) -> Result<RecallResult, MemoryError> {
        let started = Instant::now();
        self.gate()?;
        validate(&who, &input)?;
        // §3 (ADR-112): a knowledge agent reads its own scope only.
        if let MemoryCaller::KnowledgeAgent { scope_id } = who {
            if self.knowledge_scope != Some(scope_id) {
                return Err(MemoryError::Refused {
                    code: "MEM_SCOPE_CROSS_WRITE",
                });
            }
        }
        // §5 step 1 (ADR-112): `Auto` from those callers behaves as `Never`.
        let hosted = matches!(
            who,
            MemoryCaller::HostedAgent { .. } | MemoryCaller::KnowledgeAgent { .. }
        );
        if input.wider == WiderMode::Always && (hosted || self.program.is_agent_profile()) {
            return Err(MemoryError::Refused {
                code: "MEM_WIDER_NOT_SUPPORTED",
            });
        }
        self.model_gate()?;
        let place = place_of(&who);
        let collections = self.collections_of(&who, &input).await?;

        let mut body = json!({
            "result_cap": input.result_cap,
            "include_provenance": true,
        });
        if let Some(query) = &input.query {
            body["query"] = json!(query);
        }
        if let Some(searches) = &input.searches {
            let items: Vec<_> = searches
                .iter()
                .map(|s| {
                    let kind = match s.kind {
                        RecallSearchKind::Lex => "lex",
                        RecallSearchKind::Vec => "vec",
                    };
                    json!({ "type": kind, "query": s.query })
                })
                .collect();
            body["searches"] = json!(items);
        }
        if let Some(collections) = &collections {
            body["filters"] = json!({ "collections": collections });
        }
        let body = serde_json::to_vec(&body).map_err(|_| MemoryError::internal())?;

        let context = self.caller_context(&who, None);
        let guard = self.auth.mint(context.clone()).await?;
        let outcome = as_recall_query(self.service.recall(guard.cx(correlation_id, None), &body))
            .await
            .map_err(map_call_error)?
            .data;
        drop(guard);

        let abstained = outcome.abstained;
        let (mut facts, links) = self.map_outcome(outcome, &context).await?;
        suppress_deprecated(&mut facts, false);

        let recall_id = if matches!(who, MemoryCaller::KnowledgeAgent { .. }) {
            MemoryRecallId(Uuid::nil())
        } else {
            MemoryRecallId::new()
        };
        let mode = if hosted && input.wider == WiderMode::Auto {
            WiderMode::Never
        } else {
            input.wider
        };
        let outcome = self
            .wider_step(&who, &input, mode, recall_id, &mut facts)
            .await;
        let WiderOutcome {
            wider,
            order,
            status,
            outside_rows,
            log_json,
            follow,
        } = outcome;

        // The recall log (§5 step 3 of the ADR-111 change). A knowledge agent's is not kept
        // (SA-MKA-08).
        if !matches!(who, MemoryCaller::KnowledgeAgent { .. }) {
            let new = NewRecall {
                id: recall_id,
                run_id: run_of(&who),
                place,
                caller_kind: caller_kind(&who),
                agent_instance_id: agent_of(&who),
                recalled_at: Timestamp::now(),
                wider: log_json,
                facts: facts.iter().map(|f| f.id).collect(),
                links: links_for_log(&links, &facts),
                outside: outside_rows,
            };
            if let Err(error) = store::insert_recall(&self.db, new).await {
                tracing::warn!(
                    target: "ordin::memory",
                    code = error.code(),
                    "memory.recall_log_failed"
                );
            }
        }
        if let Some(follow) = follow {
            self.follow_more_found(follow);
        }
        let links = links.into_iter().map(|(view, _)| view).collect::<Vec<_>>();
        // The local `memory.recall` span with `scope_kind` in the agent profile is not built: this
        // service holds scope ids only (2C: it "only reads ids"), so a channel and a direct chat
        // cannot be told apart here.
        tracing::info!(
            target: "ordin::memory",
            scope_key = %store::scope_key(place),
            returned = facts.len(),
            abstained,
            elapsed_ms = started.elapsed().as_millis() as u64,
            searches = input.searches.as_ref().map_or(0, Vec::len),
            folders_named = input.folders.as_ref().map_or(0, Vec::len),
            links = links.len(),
            correlation_id,
            "memory.recalled"
        );
        Ok(RecallResult {
            facts,
            abstained,
            recall_id,
            links,
            wider,
            order,
            wider_status: status,
        })
    }

    /// §5 step 2 (ADR-110).
    async fn collections_of(
        &self,
        who: &MemoryCaller,
        input: &RecallInput,
    ) -> Result<Option<Vec<String>>, MemoryError> {
        let Some(names) = &input.folders else {
            return Ok(None);
        };
        let unknown = || MemoryError::invalid("MEM_FOLDER_UNKNOWN", "folders");
        let (Some(folders), Some(workspace_id)) = (self.folders(), workspace_of(who)) else {
            return Err(unknown());
        };
        folders
            .collections_for(workspace_id, names)
            .await
            .map(Some)
            .map_err(|_| unknown())
    }

    /// §5 steps 3 and 4, and the links of the page (rule LV), each with the shown facts it reached.
    async fn map_outcome(
        &self,
        outcome: RecallOutcome,
        context: &memory::types::scope::ScopeContext,
    ) -> Result<(Vec<FactView>, Vec<(LinkView, Vec<String>)>), MemoryError> {
        let ranked = outcome.response.facts;
        let ids: Vec<String> = ranked.iter().map(|r| r.fact.id.clone()).collect();
        let rows: HashMap<String, RefRow> = store::refs_by_facts(&self.db, &ids)
            .await?
            .into_iter()
            .map(|row| (row.memory_fact_id.clone(), row))
            .collect();
        let held: Vec<&Fact> = ranked.iter().map(|r| &r.fact).collect();
        let origins = self.origin_rows(&held).await?;
        let mut names = Names::new();
        let mut facts = Vec::with_capacity(ranked.len());
        let mut shown = HashSet::new();
        for ranked in &ranked {
            let Some(row) = rows.get(&ranked.fact.id) else {
                tracing::warn!(
                    target: "ordin::memory",
                    memory_fact_id = %ranked.fact.id,
                    "memory.unmapped_fact"
                );
                continue;
            };
            // The fact is stored, so the reconciler's next pass would mark it; a recall need not
            // wait for that. A row in any other state was ended by this service.
            if !matches!(row.state, FactState::Pending | FactState::Kept) {
                continue;
            }
            let note = ViewNote {
                fading: Some(ranked.fading),
                relevance_permille: Some((ranked.score * 1000.0).round().clamp(0.0, 1000.0) as u16),
            };
            let view = self
                .fact_view(&ranked.fact, Some(row), note, context, &mut names, &origins)
                .await?;
            shown.insert(ranked.fact.id.clone());
            facts.push(view);
        }
        let mut links = Vec::new();
        for link in outcome.response.links {
            let RecallLink {
                relationship,
                from_name,
                to_name,
                reached,
            } = link;
            let reached: Vec<String> = reached.into_iter().filter(|f| shown.contains(f)).collect();
            if reached.is_empty() {
                continue;
            }
            let view = self
                .link_view(
                    &relationship,
                    Some(from_name),
                    Some(to_name),
                    context,
                    &mut names,
                )
                .await?;
            links.push((view, reached));
        }
        Ok((facts, map_reached(links, rows)))
    }
}

/// Each link with the row ids of the shown facts it reached.
fn map_reached(
    links: Vec<(LinkView, Vec<String>)>,
    rows: HashMap<String, RefRow>,
) -> Vec<(LinkView, Vec<String>)> {
    links
        .into_iter()
        .map(|(view, reached)| {
            let refs = reached
                .iter()
                .filter_map(|fact| rows.get(fact))
                .map(|row| row.id.to_string())
                .collect();
            (view, refs)
        })
        .collect()
}

/// `(link id, the first shown fact it reached)` of each link whose fact is still shown after the
/// deprecated filter and the merge.
fn links_for_log(
    links: &[(LinkView, Vec<String>)],
    facts: &[FactView],
) -> Vec<(String, MemoryFactId)> {
    let shown: Vec<MemoryFactId> = facts.iter().map(|f| f.id).collect();
    links
        .iter()
        .filter_map(|(view, reached)| {
            shown
                .iter()
                .find(|id| reached.contains(&id.to_string()))
                .map(|id| (view.id.0.clone(), *id))
        })
        .collect()
}

/// The status of a recall that never considered a front door.
pub(crate) fn not_asked(reason: WiderReason) -> WiderStatus {
    WiderStatus {
        reason,
        ..WiderStatus::default()
    }
}
