//! The two Journal hooks of the supervisor: the event sink that resumes agent-profile runs when
//! their approval is decided (H.4), and the pruning participant for `runs` and `tasks`
//! (SessionSupervisor.md *Data Model*, pruning).

use std::sync::Weak;

use ordin_journal::{
    Datastore, EventSink, JournalEvent, PruneFailure, PruneParticipant, PruneRequest,
};
use ordin_types::wire::events::EventKind;
use ordin_types::ApprovalId;

use crate::store;
use crate::supervisor::SessionSupervisor;

/// `journal.subscribe(supervisor.event_sink())`.
pub(crate) struct SupervisorSink(pub Weak<SessionSupervisor>);

#[async_trait::async_trait]
impl EventSink for SupervisorSink {
    fn name(&self) -> &'static str {
        "session_supervisor"
    }

    /// H.4: `approval_decided` for a run paused on that approval → `run_queue.resume`. Every other
    /// event, and every program outside the agent profile, is ignored.
    async fn deliver(&self, event: &JournalEvent) {
        if EventKind::parse(&event.kind) != Some(EventKind::ApprovalDecided) {
            return;
        }
        let Some(sup) = self.0.upgrade() else {
            return;
        };
        if !sup.config.program.is_agent() {
            return;
        }
        let Some(queue) = sup.run_queue.clone() else {
            return;
        };
        let Some(approval_id) = event
            .data
            .get("approval_id")
            .and_then(|v| v.as_str())
            .and_then(|t| ApprovalId::parse(t).ok())
        else {
            return;
        };
        let state = event
            .data
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !matches!(state, "approved" | "rejected") {
            return;
        }
        let run_id = match store::paused_with_approval(&sup.datastore, approval_id).await {
            Ok(Some(run_id)) => run_id,
            Ok(None) => return,
            Err(_) => {
                tracing::error!(code = "SUP_STORE_UNAVAILABLE", "sup_store_unavailable");
                return;
            }
        };
        let decision = serde_json::json!({ "approval_id": approval_id, "state": state });
        if let Err(e) = queue.resume(&run_id.to_string(), decision).await {
            tracing::warn!(
                run_id = run_id.to_string().as_str(),
                code = e.code(),
                "sup_record_not_resumed"
            );
        }
    }

    /// Missed events are not read again: a paused record whose decision was missed stays paused
    /// until the approval is decided again or the run is cancelled.
    async fn lagged(&self, next_seq: u64) {
        tracing::warn!(next_seq, "sup_events_lagged");
    }
}

/// `journal.register_prune_participant(supervisor.prune_participant())`.
pub(crate) struct SupervisorPrune;

#[async_trait::async_trait]
impl PruneParticipant for SupervisorPrune {
    fn module(&self) -> &'static str {
        SessionSupervisor::MODULE
    }

    /// Runs ended before the cutoff, then tasks whose runs are all deleted; never a run in progress
    /// or listed in `runs_in_progress` (04:386, A-08).
    async fn prune(&self, store: &Datastore, request: &PruneRequest) -> Result<u64, PruneFailure> {
        store::prune(store, request.cutoff, &request.runs_in_progress)
            .await
            .map_err(|_| PruneFailure {
                code: "SUP_STORE_UNAVAILABLE",
            })
    }
}
