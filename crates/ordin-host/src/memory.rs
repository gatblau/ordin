//! Summaries at run end (ADR-111 amendment, OH-AM-3 and OH-AM-4): this crate counts the episodic
//! facts a run wrote per thing and asks Ordin, in a fixed turn, to summarise a thing with five or more.
//! The counters live in memory (SA-OH-AMEM-02).

use std::collections::BTreeMap;

use ordin_memory_service::{FactView, MemoryKind};
use ordin_service_api::commands::ordin::MessageAuthor;
use ordin_types::time::Timestamp;
use ordin_types::{MemoryFactId, MemoryThingId, RunId, WorkspaceId};

use crate::host::{lock, OrdinHost, Turn, COMPONENT, MESSAGE_MAX_CHARS};
use crate::store;

/// SA-OH-AMEM-01: at most 3 consolidation turns follow one ended run.
const TURNS_PER_RUN: usize = 3;
const FACTS_TO_SUMMARISE: usize = 5;
const FACTS_QUOTED: usize = 20;
const ONCE_A_DAY_MS: i64 = 24 * 3_600 * 1_000;

/// One episodic fact as consolidation quotes it: its id, its statement and when it was written.
type Quoted = (MemoryFactId, String, Timestamp);

/// The episodic facts one run wrote, per thing.
#[derive(Debug, Clone, Default)]
pub(crate) struct RunCounts {
    pub things: BTreeMap<String, (String, Vec<Quoted>)>,
}

impl RunCounts {
    fn add(&mut self, fact: &FactView) {
        if fact.kind != MemoryKind::Happened {
            return;
        }
        for thing in &fact.things {
            let entry = self
                .things
                .entry(thing.id.0.clone())
                .or_insert_with(|| (thing.name.clone(), Vec::new()));
            if !entry.1.iter().any(|(id, _, _)| *id == fact.id) {
                entry
                    .1
                    .push((fact.id, fact.statement.clone(), fact.written_at));
            }
        }
    }
}

impl OrdinHost {
    /// OH-AM-3.2 and 3.3: what a member's or a delegated task's run wrote.
    // spec: ordin-desktop/components/OrdinHost#SA-OH-AMEM-02
    pub(crate) async fn count_run(&self, _workspace_id: WorkspaceId, run_id: RunId) {
        match self.ports.memory.remembered_in_run(run_id).await {
            Ok(items) => {
                let mut counts = RunCounts::default();
                for item in &items.items {
                    counts.add(&item.fact);
                }
                lock(&self.state).counts.insert(run_id, counts);
            }
            Err(e) => tracing::warn!(
                component = COMPONENT,
                code = e.code.as_str(),
                "ordin_host.memory_count_failed"
            ),
        }
    }

    /// OH-AM-3.1 then OH-AM-4 for Ordin's own run, when its conversation is summarised, cleared or put away.
    pub(crate) async fn ordin_run_ended(&self, workspace_id: WorkspaceId, run_id: RunId) {
        let facts = lock(&self.state)
            .ended_facts
            .remove(&run_id)
            .unwrap_or_default();
        if facts.is_empty() {
            return;
        }
        match self.ports.memory.get_facts(workspace_id, facts).await {
            Ok(views) => {
                let mut counts = RunCounts::default();
                for f in &views {
                    counts.add(f);
                }
                lock(&self.state).counts.insert(run_id, counts);
                self.consolidate(workspace_id, run_id).await;
            }
            Err(e) => tracing::warn!(
                component = COMPONENT,
                code = e.code.as_str(),
                "ordin_host.memory_count_failed"
            ),
        }
    }

    pub(crate) async fn consolidate(&self, workspace_id: WorkspaceId, run_id: RunId) {
        self.consolidate_many(workspace_id, vec![run_id]).await;
    }

    /// OH-AM-4: candidates of at least five facts, not consolidated in the last day, at most three,
    /// by count descending then name ascending; one fixed turn each.
    // spec: ordin-desktop/components/OrdinHost#SA-OH-AMEM-01
    pub(crate) async fn consolidate_many(&self, workspace_id: WorkspaceId, runs: Vec<RunId>) {
        let mut merged = RunCounts::default();
        {
            let mut state = lock(&self.state);
            for run in &runs {
                if let Some(c) = state.counts.remove(run) {
                    for (id, (name, facts)) in c.things {
                        let entry = merged
                            .things
                            .entry(id)
                            .or_insert_with(|| (name, Vec::new()));
                        for f in facts {
                            if !entry.1.iter().any(|(x, _, _)| *x == f.0) {
                                entry.1.push(f);
                            }
                        }
                    }
                }
            }
        }
        let mut candidates: Vec<(String, String, Vec<Quoted>)> = Vec::new();
        let now = Timestamp::now();
        for (id, (name, facts)) in merged.things {
            if facts.len() < FACTS_TO_SUMMARISE {
                continue;
            }
            let thing = MemoryThingId(id.clone());
            match self
                .ports
                .memory
                .last_consolidated(workspace_id, thing)
                .await
            {
                Ok(Some(t)) if now.unix_millis() - t.unix_millis() < ONCE_A_DAY_MS => continue,
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        component = COMPONENT,
                        code = e.code.as_str(),
                        "ordin_host.memory_count_failed"
                    );
                    continue;
                }
            }
            candidates.push((id, name, facts));
        }
        if candidates.is_empty() {
            return;
        }
        candidates.sort_by(|a, b| b.2.len().cmp(&a.2.len()).then(a.1.cmp(&b.1)));
        candidates.truncate(TURNS_PER_RUN);
        if self.ports.usage.may_start_ordin_work().await.is_err() {
            tracing::info!(
                component = COMPONENT,
                reason = "plan",
                "ordin_host.consolidation_skipped"
            );
            return;
        }
        let Some((conv, ordin_run)) = self.conv_run(workspace_id) else {
            tracing::info!(
                component = COMPONENT,
                reason = "no_conversation",
                "ordin_host.consolidation_skipped"
            );
            return;
        };
        for (_, name, mut facts) in candidates {
            facts.sort_by_key(|f| std::cmp::Reverse(f.2));
            let quoted: Vec<String> = facts
                .iter()
                .take(FACTS_QUOTED)
                .map(|(id, statement, _)| {
                    format!(
                        "{id}: {}",
                        serde_json::to_string(statement).unwrap_or_default()
                    )
                })
                .collect();
            let text = format!(
                "Write one short summary of what these facts say about {name}. Reply by calling remember with the summary as fact, \
                 entities naming {name}, and made_from set to the ids of the facts you used (at least two).\n{}",
                quoted.join("\n")
            );
            let stored = async {
                let mut c = self.pool.acquire().await?;
                store::insert_message(
                    &mut c,
                    conv,
                    MessageAuthor::System,
                    Some("consolidation_turn"),
                    &crate::host::cut_chars(&text, MESSAGE_MAX_CHARS),
                    None,
                    Timestamp::now(),
                )
                .await
            }
            .await;
            if stored.is_err() {
                tracing::warn!(
                    component = COMPONENT,
                    code = "OH_DATASTORE_UNAVAILABLE",
                    "ordin_host.consolidation_skipped"
                );
                continue;
            }
            let turn = Turn {
                text,
                waiter: None,
                slack: None,
                persons: false,
                quiet: false,
            };
            match self.send_turn(workspace_id, turn).await {
                Ok(_) => {
                    let ids: Vec<String> = facts
                        .iter()
                        .take(FACTS_QUOTED)
                        .map(|f| f.0.to_string())
                        .collect();
                    tracing::info!(component = COMPONENT, workspace_id = workspace_id.to_string().as_str(), run_id = ordin_run.to_string().as_str(), facts = ?ids, "ordin_host.consolidation_turn");
                }
                Err(_) => {
                    tracing::info!(
                        component = COMPONENT,
                        reason = "queue_full",
                        "ordin_host.consolidation_skipped"
                    );
                }
            }
        }
    }
}
