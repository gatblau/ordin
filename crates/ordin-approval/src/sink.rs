//! The Journal-facing parts: the `EventSink` for run-ended and crew-ended events (F.3) and the
//! `PruneParticipant` for decided approvals (Data Model, *Pruning*).

use std::sync::Weak;

use async_trait::async_trait;
use ordin_journal::{
    Datastore, EventSink, JournalEvent, PruneFailure, PruneParticipant, PruneRequest,
};
use ordin_types::domain::runs::{CrewRunState, RunState};
use ordin_types::wire::events::EventKind;
use ordin_types::{CrewRunId, RunId};
use serde::Deserialize;

use crate::service::ApprovalService;
use crate::store;

/// `RunState` values after which a run asks for nothing more (ApprovalService.md Shared Context).
fn run_ended(state: RunState) -> bool {
    matches!(
        state,
        RunState::Completed
            | RunState::Failed
            | RunState::StoppedAtLimit
            | RunState::Cancelled
            | RunState::Interrupted
            | RunState::Abandoned
            | RunState::NoReply
    )
}

/// `CrewRunState` values after which a crew run asks for nothing more.
fn crew_ended(state: CrewRunState) -> bool {
    matches!(
        state,
        CrewRunState::Finished
            | CrewRunState::Escalated
            | CrewRunState::StoppedAtLimit
            | CrewRunState::Killed
            | CrewRunState::Failed
    )
}

#[derive(Deserialize)]
struct RunChanged {
    run_id: RunId,
    state: RunState,
}

#[derive(Deserialize)]
struct CrewChanged {
    crew_run_id: CrewRunId,
    state: CrewRunState,
}

/// Holds the service weakly: the Journal holds the sink, and the service holds the Journal.
pub(crate) struct ApprovalSink {
    service: Weak<ApprovalService>,
}

impl ApprovalSink {
    pub(crate) fn new(service: Weak<ApprovalService>) -> Self {
        ApprovalSink { service }
    }
}

#[async_trait]
impl EventSink for ApprovalSink {
    fn name(&self) -> &'static str {
        "approval"
    }

    /// F.3: `run_state_changed` with an ended state expires that run's approvals;
    /// `crew_round_changed` with an ended crew state expires that crew run's; every other kind is ignored.
    async fn deliver(&self, event: &JournalEvent) {
        let Some(service) = self.service.upgrade() else {
            return;
        };
        let result = if event.kind == EventKind::RunStateChanged.as_str() {
            match serde_json::from_value::<RunChanged>(event.data.clone()) {
                Ok(changed) if run_ended(changed.state) => {
                    service.expire_for_run(changed.run_id).await.map(|_| ())
                }
                _ => return,
            }
        } else if event.kind == EventKind::CrewRoundChanged.as_str() {
            match serde_json::from_value::<CrewChanged>(event.data.clone()) {
                Ok(changed) if crew_ended(changed.state) => service
                    .expire_for_crew_run(changed.crew_run_id)
                    .await
                    .map(|_| ()),
                _ => return,
            }
        } else {
            return;
        };
        if let Err(error) = result {
            // The approval stays pending (nothing is expired on a guess) and the failure is logged
            // under its own event name.
            tracing::warn!(code = error.code(), "approval_expiry_failed");
        }
    }

    /// F.3: log the overflow and run one sweep. The sweep expires nothing: an approval whose run is not
    /// known as running is left pending, because the asker re-checks on its own restart.
    async fn lagged(&self, next_seq: u64) {
        tracing::warn!(next_seq, "approval_sink_lagged");
    }
}

/// The Journal's pruning of decided approvals.
pub(crate) struct ApprovalPruneParticipant;

#[async_trait]
impl PruneParticipant for ApprovalPruneParticipant {
    fn module(&self) -> &'static str {
        ApprovalService::MODULE
    }

    async fn prune(
        &self,
        datastore: &Datastore,
        request: &PruneRequest,
    ) -> Result<u64, PruneFailure> {
        let failed = || PruneFailure {
            code: "APPR_STORE_UNAVAILABLE",
        };
        let Datastore::Sqlite(pool) = datastore else {
            return Err(failed());
        };
        let runs: Vec<String> = request
            .runs_in_progress
            .iter()
            .map(|run| run.to_string())
            .collect();
        store::prune(pool, request.cutoff, &runs)
            .await
            .map_err(|_| failed())
    }
}
