//! The run hook (TW-F step 1): an `EventSink` that forwards each ended run to the service's queue
//! and returns at once, so the Journal's delivery is never held.

use std::sync::Weak;

use async_trait::async_trait;
use ordin_journal::{EventSink, JournalEvent};
use ordin_types::wire::events::EventKind;
use ordin_types::RunId;

use crate::service::TeamKnowledgeService;

/// The ended states of TW-F step 1 (`interrupted` is recovered, not ended).
const ENDED: &[&str] = &[
    "completed",
    "failed",
    "stopped_at_limit",
    "cancelled",
    "abandoned",
    "no_reply",
];

/// Holds the service weakly: the Journal holds the sink, and the service holds the Journal.
pub(crate) struct RunSink {
    service: Weak<TeamKnowledgeService>,
}

impl RunSink {
    pub(crate) fn new(service: Weak<TeamKnowledgeService>) -> Self {
        RunSink { service }
    }
}

/// The run of a `run_state_changed` event whose state is an ended one.
pub(crate) fn ended_run(event: &JournalEvent) -> Option<RunId> {
    if EventKind::parse(&event.kind) != Some(EventKind::RunStateChanged) {
        return None;
    }
    let state = event.data.get("state").and_then(|v| v.as_str())?;
    if !ENDED.contains(&state) {
        return None;
    }
    event
        .data
        .get("run_id")
        .and_then(|v| v.as_str())
        .and_then(|v| RunId::parse(v).ok())
}

#[async_trait]
impl EventSink for RunSink {
    fn name(&self) -> &'static str {
        "team_workspace"
    }

    async fn deliver(&self, event: &JournalEvent) {
        let (Some(service), Some(run)) = (self.service.upgrade(), ended_run(event)) else {
            return;
        };
        if service.runs_tx.try_send(run).is_err() {
            tracing::warn!(target: "ordin::team_knowledge", seq = event.seq, "team.run_queue_full");
        }
    }

    /// Re-reads with `events_since(next_seq - 1, 1000)` until the head (WorkspaceService.md §8.3).
    async fn lagged(&self, next_seq: u64) {
        let Some(service) = self.service.upgrade() else {
            return;
        };
        let mut after = next_seq.saturating_sub(1);
        loop {
            let Ok(events) = service.deps.journal.events_since(after, 1000).await else {
                return;
            };
            let Some(last) = events.last().map(|e| e.seq) else {
                return;
            };
            for event in &events {
                if let Some(run) = ended_run(event) {
                    let _ = service.runs_tx.send(run).await;
                }
            }
            after = last;
        }
    }
}
