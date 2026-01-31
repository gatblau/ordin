//! The Journal-facing part: the `EventSink` that forwards `approval_decided` events to the decision
//! loop (Internal Logic B). It returns at once so the Journal's delivery is never held.

use std::sync::Weak;

use async_trait::async_trait;
use ordin_journal::{EventSink, JournalEvent};
use ordin_types::ApprovalId;

use crate::service::{Decision, PushService};

/// Holds the service weakly: the Journal holds the sink, and the service holds the Journal.
pub(crate) struct PushSink {
    service: Weak<PushService>,
}

impl PushSink {
    pub(crate) fn new(service: Weak<PushService>) -> Self {
        PushSink { service }
    }
}

/// `data { approval_id, state, decided_on }` of an `approval_decided` event.
pub(crate) fn decision_of(event: &JournalEvent) -> Option<Decision> {
    if event.kind != "approval_decided" {
        return None;
    }
    let approval_id = event
        .data
        .get("approval_id")
        .and_then(|v| v.as_str())
        .and_then(|v| ApprovalId::parse(v).ok())?;
    let state = event.data.get("state").and_then(|v| v.as_str())?.to_owned();
    Some(Decision { approval_id, state })
}

#[async_trait]
impl EventSink for PushSink {
    fn name(&self) -> &'static str {
        "push"
    }

    async fn deliver(&self, event: &JournalEvent) {
        let (Some(service), Some(decision)) = (self.service.upgrade(), decision_of(event)) else {
            return;
        };
        if service.queue.try_send(decision).is_err() {
            // The queue is full: the decision is re-read from the Journal by `lagged`.
            tracing::warn!(component = "push", seq = event.seq, "push.queue_full");
        }
    }

    /// B: re-read with `events_since(next_seq - 1, 1000)` until the head.
    async fn lagged(&self, next_seq: u64) {
        let Some(service) = self.service.upgrade() else {
            return;
        };
        let mut after = next_seq.saturating_sub(1);
        loop {
            let events = match service.deps.journal.events_since(after, 1000).await {
                Ok(events) => events,
                Err(error) => {
                    tracing::warn!(
                        component = "push",
                        code = ordin_types::CodedError::code(&error),
                        "push.lag_read_failed"
                    );
                    return;
                }
            };
            let Some(last) = events.last().map(|e| e.seq) else {
                return;
            };
            for event in &events {
                if let Some(decision) = decision_of(event) {
                    let _ = service.queue.send(decision).await;
                }
            }
            after = last;
        }
    }
}
