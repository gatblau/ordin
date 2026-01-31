//! Asking the workspace's front doors (*Internal Logic* AG, ADR-113 and ADR-115), and the seam over
//! the Knowledge Query Client.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use memory::service::ListEntitiesRequest;
use ordin_knowledge_query_client::{
    AskTarget, KnowledgeAnswerStream, KnowledgeAsk, KnowledgeBatch, KnowledgeQueryClient, KqError,
};
use ordin_types::wire::knowledge_query::{
    KnowledgeHit, KnowledgeSearch, KnowledgeSearchKind, UnansweredReason, UnansweredSource,
};
use ordin_types::{MemoryRecallId, RunId, WorkspaceId};
use serde_json::{json, Value};

use crate::error::MemoryError;
use crate::model::{
    FactOrigin, FactView, KnowledgeHitKind, MemoryCaller, MemoryPlace, MoreFoundView, PageStatus,
    PageTier, RecallInput, RecallSearchKind, RecallSlot, WiderFact, WiderMode, WiderReason,
    WiderStatus,
};
use crate::recall::{not_asked, suppress_deprecated};
use crate::service::{new_correlation_id, Inner, MemoryService};
use crate::store::{self, OutsideRow};

pub(crate) const WIDER_CONFIDENT_PERMILLE: u16 = 500;
pub(crate) const WIDER_CONFIDENT_COUNT: usize = 3;
pub(crate) const WIDER_NAMES_CHECKED: usize = 4;
pub(crate) const MORE_FOUND_HOLD: Duration = Duration::from_secs(30 * 60);
pub(crate) const WIDER_FUSION_K: f64 = 60.0;
const FIRST_DEADLINE: Duration = Duration::from_millis(1_000);
const MORE_UNTIL: Duration = Duration::from_millis(5_000);

/// The Knowledge Query Client as this service uses it, so that a test can stand in for it. The
/// program passes the real client through `set_knowledge_client`.
#[async_trait]
pub(crate) trait WiderClient: Send + Sync {
    async fn connection_names(&self, workspace_id: WorkspaceId) -> Vec<String>;
    async fn contents_names(&self, workspace_id: WorkspaceId) -> Vec<String>;
    async fn ask(&self, ask: KnowledgeAsk) -> Result<Box<dyn WiderStream>, KqError>;
}

#[async_trait]
pub(crate) trait WiderStream: Send {
    async fn first(&mut self) -> KnowledgeBatch;
    async fn more(&mut self) -> Option<KnowledgeBatch>;
}

pub(crate) struct RealClient(pub(crate) Arc<KnowledgeQueryClient>);

#[async_trait]
impl WiderClient for RealClient {
    async fn connection_names(&self, workspace_id: WorkspaceId) -> Vec<String> {
        self.0
            .connections_of(workspace_id)
            .await
            .into_iter()
            .map(|c| c.name)
            .collect()
    }

    async fn contents_names(&self, workspace_id: WorkspaceId) -> Vec<String> {
        self.0.contents_names(workspace_id).await
    }

    async fn ask(&self, ask: KnowledgeAsk) -> Result<Box<dyn WiderStream>, KqError> {
        let stream: KnowledgeAnswerStream = self.0.ask(ask).await?;
        Ok(Box::new(stream))
    }
}

#[async_trait]
impl WiderStream for KnowledgeAnswerStream {
    async fn first(&mut self) -> KnowledgeBatch {
        KnowledgeAnswerStream::first(self).await
    }

    async fn more(&mut self) -> Option<KnowledgeBatch> {
        KnowledgeAnswerStream::more(self).await
    }
}

// spec: ordin-desktop/components/MemoryService#SA-MKA-07
/// The outside facts of one recall, held for **More found** (SA-MKA-07).
pub(crate) struct HeldAnswer {
    pub(crate) late: Vec<WiderFact>,
    pub(crate) unanswered: Vec<UnansweredSource>,
    pub(crate) done: bool,
    pub(crate) unchecked: bool,
    pub(crate) answered: Vec<String>,
    pub(crate) held_at: Instant,
}

/// What §AG adds to a recall's result and log.
#[derive(Default)]
pub(crate) struct WiderOutcome {
    pub(crate) wider: Vec<WiderFact>,
    pub(crate) order: Vec<RecallSlot>,
    pub(crate) status: WiderStatus,
    pub(crate) outside_rows: Vec<OutsideRow>,
    pub(crate) log_json: Option<String>,
    pub(crate) follow: Option<Follow>,
}

fn run_of(who: &MemoryCaller) -> Option<RunId> {
    match who {
        MemoryCaller::Ordin { run_id, .. }
        | MemoryCaller::CrewMember { run_id, .. }
        | MemoryCaller::Agent { run_id, .. } => Some(*run_id),
        _ => None,
    }
}

/// The rest of an answer, followed after the recall returned (AG step 6).
pub(crate) struct Follow {
    stream: Box<dyn WiderStream>,
    recall_id: MemoryRecallId,
    workspace_id: WorkspaceId,
}

fn wider_of(hit: &KnowledgeHit) -> WiderFact {
    WiderFact {
        statement: hit.statement.clone(),
        agent: hit.agent.clone(),
        repository: hit.repository.clone(),
        path: hit.path.clone(),
        commit: hit.commit.clone(),
        section: hit.section.clone(),
        page_title: hit.page_title.clone(),
        kind: hit.kind,
        tier: hit.tier,
        page_status: (hit.status != PageStatus::Stable).then_some(hit.status),
        as_of: hit.as_of,
        answer_id: hit.answer_id,
        shared_identity: hit.shared_identity.clone(),
        may_be_out_of_date: hit.may_be_out_of_date,
    }
}

fn outside_row(fact: &WiderFact, more_found: bool) -> OutsideRow {
    OutsideRow {
        agent: fact.agent.clone(),
        repository: fact.repository.clone(),
        path: fact.path.clone(),
        commit: fact.commit.clone(),
        section: fact.section.clone(),
        page_title: fact.page_title.clone(),
        as_of: fact.as_of,
        answer_id: fact.answer_id.map(|a| a.0),
        more_found,
    }
}

// spec: ordin-desktop/components/MemoryService#SA-MPD-06
fn log_json(answered: &[String], unanswered: &[UnansweredSource], unchecked: bool) -> String {
    let names: Vec<&str> = unanswered.iter().map(|u| u.agent.as_str()).collect();
    let mut value = json!({ "answered": answered, "unanswered": names });
    if unchecked {
        value["unchecked"] = Value::Bool(true);
    }
    value.to_string()
}

fn tier_rank(tier: Option<PageTier>) -> u8 {
    match tier {
        Some(PageTier::HumanReviewed) => 0,
        Some(PageTier::MachineConfirmed) => 1,
        Some(PageTier::Unverified) => 2,
        None => 3,
    }
}

/// SA-MKA-05 tie-break: page before document, anything else after.
fn kind_rank(page: bool, document: bool) -> u8 {
    if page {
        0
    } else if document {
        1
    } else {
        2
    }
}

/// One entry of the merge.
struct Merged {
    fused: f64,
    tier: u8,
    kind: u8,
    local: bool,
    rank: usize,
    deprecated: bool,
}

// spec: ordin-desktop/components/MemoryService#SA-MKA-05
/// SA-MKA-05, then AG step 4's use of SA-MKA-02: the order, the kept facts and the kept outside
/// facts.
pub(crate) fn merge(
    facts: Vec<FactView>,
    outside: Vec<WiderFact>,
    cap: usize,
) -> (Vec<FactView>, Vec<WiderFact>, Vec<RecallSlot>) {
    let mut all: Vec<Merged> = Vec::with_capacity(facts.len() + outside.len());
    for (rank, fact) in facts.iter().enumerate() {
        all.push(Merged {
            fused: 1.0 / (WIDER_FUSION_K + rank as f64 + 1.0),
            tier: tier_rank(fact.page_tier),
            kind: kind_rank(
                fact.page_id.is_some(),
                matches!(fact.came_from, FactOrigin::Document { .. }),
            ),
            local: true,
            rank,
            deprecated: fact.page_status == Some(PageStatus::Deprecated),
        });
    }
    for (rank, fact) in outside.iter().enumerate() {
        all.push(Merged {
            fused: 1.0 / (WIDER_FUSION_K + rank as f64 + 1.0),
            tier: tier_rank(fact.tier),
            kind: kind_rank(fact.kind == KnowledgeHitKind::Page, true),
            local: false,
            rank,
            deprecated: fact.page_status == Some(PageStatus::Deprecated),
        });
    }
    if all.iter().any(|m| !m.deprecated) {
        all.retain(|m| !m.deprecated);
    }
    all.sort_by(|a, b| {
        b.fused
            .partial_cmp(&a.fused)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.tier.cmp(&b.tier))
            .then(a.kind.cmp(&b.kind))
            .then(b.local.cmp(&a.local))
            .then(a.rank.cmp(&b.rank))
    });
    all.truncate(cap);

    let mut kept_facts = Vec::new();
    let mut kept_outside = Vec::new();
    let mut order = Vec::with_capacity(all.len());
    let mut facts: Vec<Option<FactView>> = facts.into_iter().map(Some).collect();
    let mut outside: Vec<Option<WiderFact>> = outside.into_iter().map(Some).collect();
    for entry in all {
        if entry.local {
            if let Some(fact) = facts.get_mut(entry.rank).and_then(Option::take) {
                order.push(RecallSlot::Fact(kept_facts.len() as u8));
                kept_facts.push(fact);
            }
        } else if let Some(fact) = outside.get_mut(entry.rank).and_then(Option::take) {
            order.push(RecallSlot::Wider(kept_outside.len() as u8));
            kept_outside.push(fact);
        }
    }
    (kept_facts, kept_outside, order)
}

// spec: ordin-desktop/components/MemoryService#SA-MKA-04
/// SA-MKA-04: `name` occurs in `query`, both normalised, bounded by the ends or a character that is
/// neither a letter nor a digit.
pub(crate) fn name_in_query(name: &str, query: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    query.match_indices(name).any(|(at, _)| {
        let before = query[..at].chars().next_back();
        let after = query[at + name.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

impl Inner {
    /// §5 step 3 (ADR-112): whether to ask, and the asking.
    pub(crate) async fn wider_step(
        &self,
        who: &MemoryCaller,
        input: &RecallInput,
        mode: WiderMode,
        recall_id: MemoryRecallId,
        facts: &mut Vec<FactView>,
    ) -> WiderOutcome {
        let workspace_id = match who {
            MemoryCaller::Ordin { workspace_id, .. }
            | MemoryCaller::CrewMember { workspace_id, .. }
            | MemoryCaller::Agent { workspace_id, .. }
            | MemoryCaller::Person { workspace_id }
                if self.is_workspace_program() =>
            {
                Some(*workspace_id)
            }
            _ => None,
        };
        let skip = |reason| WiderOutcome {
            status: not_asked(reason),
            ..WiderOutcome::default()
        };
        let Some(workspace_id) = workspace_id else {
            return skip(WiderReason::NotAsked);
        };
        if mode == WiderMode::Never || input.folders.is_some() {
            return skip(WiderReason::NotAsked);
        }
        let Some(client) = self.wider_client() else {
            return skip(WiderReason::NoConnection);
        };
        let connections = client.connection_names(workspace_id).await;
        if connections.is_empty() {
            return skip(WiderReason::NoConnection);
        }
        if mode == WiderMode::Auto
            && self
                .confident(client.as_ref(), workspace_id, input, facts)
                .await
        {
            tracing::debug!(target: "ordin::memory", reason = "confident", "memory.wider_skipped");
            return skip(WiderReason::Confident);
        }
        self.ask(
            client,
            workspace_id,
            run_of(who),
            input,
            recall_id,
            connections,
            facts,
        )
        .await
    }

    // spec: ordin-desktop/components/MemoryService#SA-MKA-03
    /// AG step 1.
    async fn confident(
        &self,
        client: &dyn WiderClient,
        workspace_id: WorkspaceId,
        input: &RecallInput,
        facts: &[FactView],
    ) -> bool {
        let strong = facts
            .iter()
            .filter(|f| f.relevance_permille.unwrap_or(0) >= WIDER_CONFIDENT_PERMILLE)
            .count();
        if strong < WIDER_CONFIDENT_COUNT {
            return false;
        }
        let query = match (&input.query, &input.searches) {
            (Some(query), _) => query.clone(),
            (None, Some(searches)) => searches
                .first()
                .map(|s| s.query.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let query = memory::text::normalise_name(&query);
        let mut named: Vec<String> = client
            .contents_names(workspace_id)
            .await
            .iter()
            .map(|n| memory::text::normalise_name(n))
            .filter(|n| name_in_query(n, &query))
            .collect();
        named.sort_by_key(|n| std::cmp::Reverse(n.chars().count()));
        named.dedup();
        for name in named.into_iter().take(WIDER_NAMES_CHECKED) {
            if !self.knows_name(workspace_id, &name).await {
                return false;
            }
        }
        true
    }

    /// SA-MKA-04: the workspace has a thing of that name or other name.
    async fn knows_name(&self, workspace_id: WorkspaceId, name: &str) -> bool {
        let context = self.place_context(MemoryPlace::Workspace(workspace_id));
        let Ok(guard) = self.auth.mint(context).await else {
            return false;
        };
        let correlation_id = new_correlation_id();
        let page = self
            .service
            .list_entities(
                guard.cx(&correlation_id, None),
                ListEntitiesRequest {
                    search: Some(name.chars().take(200).collect()),
                    cursor: None,
                },
            )
            .await;
        match page {
            Ok(page) => page.data.items.iter().any(|item| {
                memory::text::normalise_name(&item.entity.canonical_name) == name
                    || item
                        .entity
                        .aliases
                        .iter()
                        .any(|a| memory::text::normalise_name(a) == name)
            }),
            Err(_) => false,
        }
    }

    // spec: ordin-desktop/components/MemoryService#SA-MKA-06
    /// AG steps 2 to 5.
    #[allow(clippy::too_many_arguments)]
    async fn ask(
        &self,
        client: Arc<dyn WiderClient>,
        workspace_id: WorkspaceId,
        run_id: Option<RunId>,
        input: &RecallInput,
        recall_id: MemoryRecallId,
        connections: Vec<String>,
        facts: &mut Vec<FactView>,
    ) -> WiderOutcome {
        let started = Instant::now();
        let searches = input.searches.as_ref().map(|items| {
            items
                .iter()
                .map(|s| KnowledgeSearch {
                    kind: match s.kind {
                        RecallSearchKind::Lex => KnowledgeSearchKind::Lex,
                        RecallSearchKind::Vec => KnowledgeSearchKind::Vec,
                    },
                    query: s.query.clone(),
                })
                .collect()
        });
        let now = tokio::time::Instant::now();
        let ask = KnowledgeAsk {
            target: AskTarget::Workspace(workspace_id),
            query: input.query.clone(),
            searches,
            limit: input.result_cap,
            access_group: None,
            hop_budget: 1,
            visited: Vec::new(),
            first_deadline: now + FIRST_DEADLINE,
            more_until: now + MORE_UNTIL,
            request_id: uuid::Uuid::now_v7(),
            asker: None,
        };
        let mut stream = match client.ask(ask).await {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(target: "ordin::memory", code = error.code(), "memory.wider_failed");
                let unanswered: Vec<UnansweredSource> = connections
                    .iter()
                    .map(|agent| UnansweredSource {
                        agent: agent.clone(),
                        reason: UnansweredReason::Unreachable,
                    })
                    .collect();
                let json = log_json(&[], &unanswered, false);
                return WiderOutcome {
                    status: WiderStatus {
                        asked: true,
                        reason: WiderReason::Failed,
                        unanswered,
                        ..WiderStatus::default()
                    },
                    log_json: Some(json),
                    ..WiderOutcome::default()
                };
            }
        };
        let batch = stream.first().await;
        let hits: Vec<WiderFact> = batch.hits.iter().map(wider_of).collect();
        let (kept_facts, kept_outside, order) =
            merge(std::mem::take(facts), hits, usize::from(input.result_cap));
        *facts = kept_facts;
        // SA-MKA-02 once more over what the cut left.
        suppress_deprecated(facts, !kept_outside.is_empty());

        let pending = !batch.unanswered.is_empty() && !batch.final_batch;
        let status = WiderStatus {
            asked: true,
            reason: WiderReason::Asked,
            answered: batch.answered.clone(),
            unanswered: batch.unanswered.clone(),
            more_found_pending: pending,
            unchecked_sources: batch.unchecked_sources,
        };
        if let Ok(mut held) = self.held.lock() {
            held.retain(|_, answer| answer.held_at.elapsed() < MORE_FOUND_HOLD);
            held.insert(
                recall_id,
                HeldAnswer {
                    late: Vec::new(),
                    unanswered: batch.unanswered.clone(),
                    done: !pending,
                    unchecked: batch.unchecked_sources,
                    answered: batch.answered.clone(),
                    held_at: Instant::now(),
                },
            );
        }
        self.journal(
            "wider_asked",
            None,
            None,
            Some(workspace_id),
            "A recall asked the workspace's knowledge agents.",
            json!({
                "recall_id": recall_id.to_string(),
                "connections": connections.len(),
                "answered": batch.answered.len(),
                "unanswered": batch.unanswered.len(),
                "hits": batch.hits.len(),
                "elapsed_ms": started.elapsed().as_millis() as u64,
            }),
        )
        .await;
        tracing::info!(
            target: "ordin::memory",
            recall_id = recall_id.to_string().as_str(),
            connections = connections.len(),
            answered = batch.answered.len(),
            unanswered = batch.unanswered.len(),
            hits = batch.hits.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "memory.wider_asked"
        );
        let answer_ids: Vec<String> = {
            let mut seen = HashSet::new();
            batch
                .hits
                .iter()
                .filter_map(|h| h.answer_id)
                .filter(|a| seen.insert(*a))
                .map(|a| a.to_string())
                .collect()
        };
        if !answer_ids.is_empty() {
            self.journal(
                "wider_answer_ids",
                run_id,
                None,
                Some(workspace_id),
                "Knowledge agents kept a sources record of this recall.",
                json!({
                    "recall_id": recall_id.to_string(),
                    "run_id": run_id.map(|r| r.to_string()),
                    "answer_ids": answer_ids,
                }),
            )
            .await;
        }
        let outside_rows = kept_outside.iter().map(|f| outside_row(f, false)).collect();
        WiderOutcome {
            log_json: Some(log_json(
                &batch.answered,
                &batch.unanswered,
                batch.unchecked_sources,
            )),
            follow: pending.then_some(Follow {
                stream,
                recall_id,
                workspace_id,
            }),
            wider: kept_outside,
            order,
            status,
            outside_rows,
        }
    }

    /// AG step 6, under the service's shutdown token.
    pub(crate) fn follow_more_found(self: &Arc<Self>, follow: Follow) {
        let this = self.clone();
        let shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            let Follow {
                mut stream,
                recall_id,
                workspace_id,
            } = follow;
            loop {
                let batch = tokio::select! {
                    _ = shutdown.cancelled() => return,
                    batch = stream.more() => batch,
                };
                let Some(batch) = batch else {
                    break;
                };
                this.take_late_batch(recall_id, workspace_id, batch).await;
            }
            if let Ok(mut held) = this.held.lock() {
                if let Some(answer) = held.get_mut(&recall_id) {
                    answer.done = true;
                }
            }
        });
    }

    async fn take_late_batch(
        &self,
        recall_id: MemoryRecallId,
        workspace_id: WorkspaceId,
        batch: KnowledgeBatch,
    ) {
        let hits: Vec<WiderFact> = batch.hits.iter().map(wider_of).collect();
        let added = hits.len();
        let rows: Vec<OutsideRow> = hits.iter().map(|f| outside_row(f, true)).collect();
        let mut json_now = None;
        if let Ok(mut held) = self.held.lock() {
            if let Some(answer) = held.get_mut(&recall_id) {
                answer.late.extend(hits);
                answer.unanswered = batch.unanswered.clone();
                answer.unchecked |= batch.unchecked_sources;
                for agent in &batch.answered {
                    if !answer.answered.contains(agent) {
                        answer.answered.push(agent.clone());
                    }
                }
                if batch.unchecked_sources {
                    json_now = Some(log_json(
                        &answer.answered,
                        &answer.unanswered,
                        answer.unchecked,
                    ));
                }
            }
        }
        if let Err(error) = store::append_outside(&self.db, recall_id, &rows).await {
            tracing::warn!(target: "ordin::memory", code = error.code(), "memory.recall_log_failed");
        }
        if let Some(json) = json_now {
            if let Err(error) = store::set_recall_wider(&self.db, recall_id, &json).await {
                tracing::warn!(target: "ordin::memory", code = error.code(), "memory.recall_log_failed");
            }
        }
        let data = json!({
            "workspace_id": workspace_id.to_string(),
            "recall_id": recall_id.to_string(),
            "added": added,
            "unanswered": batch.unanswered,
        });
        if let Err(error) = self.journal.append_event("more_found", data).await {
            tracing::warn!(
                target: "ordin::memory",
                code = ordin_types::CodedError::code(&error),
                "memory.journal_failed"
            );
        }
        tracing::info!(
            target: "ordin::memory",
            recall_id = recall_id.to_string().as_str(),
            count = added,
            done = batch.final_batch,
            "memory.more_found"
        );
    }
}

impl MemoryService {
    /// Late outside facts of one recall, held SA-MKA-07's 30 minutes (`memory.more_found`).
    pub async fn more_found(
        &self,
        workspace_id: WorkspaceId,
        recall_id: MemoryRecallId,
    ) -> Result<MoreFoundView, MemoryError> {
        self.inner.gate()?;
        let inner = &self.inner;
        let rows = store::recall_rows(&inner.db, &[recall_id]).await?;
        if !rows.iter().any(|r| r.workspace_id == Some(workspace_id)) {
            return Err(MemoryError::invalid("MEM_RECALL_IDS_INVALID", "recall_id"));
        }
        let held = inner.held.lock().map_err(|_| MemoryError::internal())?;
        Ok(
            match held
                .get(&recall_id)
                .filter(|h| h.held_at.elapsed() < MORE_FOUND_HOLD)
            {
                Some(answer) => MoreFoundView {
                    recall_id,
                    wider: answer.late.clone(),
                    unanswered: answer.unanswered.clone(),
                    done: answer.done,
                    expired: false,
                },
                None => MoreFoundView {
                    recall_id,
                    wider: Vec::new(),
                    unanswered: Vec::new(),
                    done: true,
                    expired: true,
                },
            },
        )
    }
}
