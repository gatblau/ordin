//! G. Late decisions: a person decides after the hook has given up waiting, and the session is
//! resumed with that decision through the Session Supervisor, so a slow approval never runs a tool
//! unapproved. The Journal sink covers a decision that arrives across a restart.

use std::sync::Weak;

use ordin_approval::WaitOutcome;
use ordin_journal::{EventSink, JournalEvent};
use ordin_session_supervisor::ResumeDecision;
use ordin_types::domain::runs::ApprovalState;
use ordin_types::wire::events::EventKind;
use ordin_types::ApprovalId;

use crate::relay::HookRelay;
use crate::store::{self, LateKind};

/// Told to the model when the person declines after the hook has gone (G.1).
const REASON_LATE_REJECTED: &str = "The person chose not to do this.";

impl HookRelay {
    /// G.1: one watcher per row waits for the approval without a limit, until shutdown.
    // spec: ordin-desktop/components/HookRelay#SA-RECON-03
    pub(crate) fn spawn_late_watcher(&self, approval_id: ApprovalId) {
        let Some(relay) = self.arc() else {
            return;
        };
        tokio::spawn(async move {
            let waited = tokio::select! {
                _ = relay.shutdown_signal.cancelled() => return,
                waited = relay.ports.approvals.wait(approval_id, None) => waited,
            };
            match waited {
                Ok(WaitOutcome::Decided(state)) => relay.settle_late(approval_id, state).await,
                Ok(WaitOutcome::DeadlinePassed) => {}
                Err(error) => hook_log!(
                    warn,
                    approval_id = approval_id.to_string().as_str(),
                    code = "HOOK_APPROVAL_UNAVAILABLE",
                    cause = ordin_types::CodedError::code(&error),
                    "hook_internal"
                ),
            }
        });
    }

    /// G.1, G.2: acts on one decided approval whose row is still in the table, then deletes the row.
    /// A watcher and the Journal sink may both arrive here; the first to claim the approval settles
    /// it and the other finds nothing to do.
    pub(crate) async fn settle_late(&self, approval_id: ApprovalId, state: ApprovalState) {
        if state == ApprovalState::Pending {
            return;
        }
        if !self.claim_settling(approval_id) {
            return;
        }
        self.settle_claimed(approval_id, state).await;
        self.release_settling(approval_id);
    }

    fn claim_settling(&self, approval_id: ApprovalId) -> bool {
        self.settling
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(approval_id)
    }

    fn release_settling(&self, approval_id: ApprovalId) {
        self.settling
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&approval_id);
    }

    async fn settle_claimed(&self, approval_id: ApprovalId, state: ApprovalState) {
        let row = match store::get(&self.datastore, approval_id).await {
            Ok(Some(row)) => row,
            Ok(None) => return,
            Err(error) => {
                hook_log!(
                    warn,
                    approval_id = approval_id.to_string().as_str(),
                    code = "HOOK_STORE_UNAVAILABLE",
                    cause = error.to_string().as_str(),
                    "hook_internal"
                );
                return;
            }
        };
        let run_id = row.run_id;
        // A stop-gate approval settles through the Rules Engine before the resume.
        let decision = match state {
            ApprovalState::Approved => {
                if row.kind == LateKind::StopGate {
                    self.ports.rules.settle_end_gates(run_id, true).await;
                }
                Some(ResumeDecision::Approve { approval_id })
            }
            ApprovalState::Rejected => {
                if row.kind == LateKind::StopGate {
                    self.ports.rules.settle_end_gates(run_id, false).await;
                }
                Some(ResumeDecision::Reject {
                    approval_id,
                    reason: Some(REASON_LATE_REJECTED.to_owned()),
                })
            }
            // An expired approval resumes nothing.
            ApprovalState::Expired | ApprovalState::Pending => None,
        };
        if let Some(decision) = decision {
            // G.2: a resume error (run ended, verb not offered) is logged and the row still goes:
            // the run is gone or cannot be resumed, and the Rules Engine's approved-once record
            // still lets the call through if the session repeats it.
            if let Err(error) = self
                .ports
                .supervisor
                .resume_with_decision(run_id, decision)
                .await
            {
                hook_log!(
                    warn,
                    run_id = run_id.to_string().as_str(),
                    code = error.code(),
                    "hook_late_resume_failed"
                );
            }
        }
        if let Err(error) = store::delete(&self.datastore, approval_id).await {
            hook_log!(
                warn,
                approval_id = approval_id.to_string().as_str(),
                code = "HOOK_STORE_UNAVAILABLE",
                cause = error.to_string().as_str(),
                "hook_internal"
            );
        }
    }
}

/// `journal.subscribe(hook_relay.event_sink())`: an `approval_decided` event for an approval in the
/// table triggers G.1 directly (G.3).
pub(crate) struct LateSink(pub Weak<HookRelay>);

#[async_trait::async_trait]
impl EventSink for LateSink {
    fn name(&self) -> &'static str {
        "hook_relay"
    }

    async fn deliver(&self, event: &JournalEvent) {
        if EventKind::parse(&event.kind) != Some(EventKind::ApprovalDecided) {
            return;
        }
        let Some(relay) = self.0.upgrade() else {
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
        let state = match event.data.get("state").and_then(|v| v.as_str()) {
            Some("approved") => ApprovalState::Approved,
            Some("rejected") => ApprovalState::Rejected,
            Some("expired") => ApprovalState::Expired,
            _ => return,
        };
        relay.settle_late(approval_id, state).await;
    }

    /// Missed events are not read again: the watcher of each row still sees its approval decided.
    async fn lagged(&self, next_seq: u64) {
        hook_log!(warn, next_seq, "hook_events_lagged");
    }
}
