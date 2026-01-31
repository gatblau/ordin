//! OH-CLS-1: after a restart, the person is told once, in one message in each workspace's
//! conversation, what the restart left behind there: tasks interrupted, crews paused and sends whose
//! outcome is not known (11 *Recovery after a restart*), followed by one progress card per interrupted
//! task, whose run screen offers Try again.

use std::collections::HashMap;
use std::time::Duration;

use ordin_service_api::blocks::{Block, PlanProgressBlock};
use ordin_service_api::commands::ordin::MessageAuthor;
use ordin_types::domain::runs::{RunOrigin, RunState};
use ordin_types::time::Timestamp;
use ordin_types::wire::StartUpSubject;
use ordin_types::{RunId, WorkspaceId};

use crate::host::{lock, within, OrdinHost, COMPONENT};
use crate::ports::DepError;

/// The longest the reads of OH-CLS-1 may take before nothing is shown.
const CLOSED_READ_SECS: u64 = 5;

/// The most interrupted tasks given a card of their own at one start.
const CLOSED_CARDS_MAX: usize = 10;

/// What the restart left behind in one workspace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Closed {
    pub interrupted: usize,
    pub paused_crews: u32,
    pub unsure: usize,
}

/// "While ordin was closed: …." naming each non-zero count, or `None` when there is nothing to tell.
// spec: ordin-desktop/components/OrdinHost#OH-CLS-1
pub(crate) fn closed_text(c: Closed) -> Option<String> {
    let mut parts = Vec::new();
    match c.interrupted {
        0 => {}
        1 => parts.push("1 task was interrupted".to_owned()),
        n => parts.push(format!("{n} tasks were interrupted")),
    }
    match c.paused_crews {
        0 => {}
        1 => parts.push("1 crew is paused".to_owned()),
        n => parts.push(format!("{n} crews are paused")),
    }
    match c.unsure {
        0 => {}
        1 => parts.push("I'm not sure 1 message was sent".to_owned()),
        n => parts.push(format!("I'm not sure {n} messages were sent")),
    }
    let joined = match parts.as_slice() {
        [] => return None,
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [head @ .., last] => format!("{}, and {last}", head.join(", ")),
    };
    Some(format!("While ordin was closed: {joined}."))
}

impl OrdinHost {
    /// OH-CLS-1: the first time a workspace's conversation is opened, read or sent a message after
    /// this program started, one message says what the restart left behind, and each interrupted task
    /// gets a progress card that opens its run; never again until the next start. No model turn; a
    /// failed read shows nothing.
    // spec: ordin-desktop/components/OrdinHost#OH-CLS-1
    pub(crate) async fn tell_closed(&self, workspace_id: WorkspaceId) {
        if !lock(&self.state).closed_told.insert(workspace_id) {
            return;
        }
        let read = within(
            Duration::from_secs(CLOSED_READ_SECS),
            self.closed_counts(workspace_id),
        )
        .await;
        let (counts, interrupted) = match read {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.closed_summary_not_read"
                );
                return;
            }
            None => {
                tracing::warn!(
                    component = COMPONENT,
                    timed_out = true,
                    "ordin_host.closed_summary_not_read"
                );
                return;
            }
        };
        let Some(text) = closed_text(counts) else {
            return;
        };
        if let Err(e) = self.show_message(workspace_id, &text).await {
            tracing::warn!(
                component = COMPONENT,
                code = e.code().as_str(),
                "ordin_host.closed_summary_not_shown"
            );
            return;
        }
        // Found in the Phase 5 walk: the interrupted task's follower ended with the program, so
        // nothing else led to its run screen and its Try again (ScreensModule.md SM-RS-8).
        for run_id in interrupted.into_iter().take(CLOSED_CARDS_MAX) {
            let card = Block::PlanProgress(PlanProgressBlock {
                subject: StartUpSubject::Run { run_id },
            });
            if let Err(e) = self
                .show_block(workspace_id, MessageAuthor::Ordin, card, false)
                .await
            {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code().as_str(),
                    "ordin_host.closed_summary_not_shown"
                );
                return;
            }
        }
    }

    /// The three counts of OH-CLS-1, each for runs ended at or after this program's start, and the
    /// interrupted task runs in the order `list` gives them.
    async fn closed_counts(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<(Closed, Vec<RunId>), DepError> {
        let since = self.started_at;
        let ended_since = |at: Option<Timestamp>| at.is_some_and(|at| at >= since);
        let interrupted: Vec<RunId> = self
            .ports
            .supervisor
            .list(workspace_id)
            .await?
            .iter()
            .filter(|r| {
                matches!(r.origin, RunOrigin::Task { .. })
                    && r.state == RunState::Interrupted
                    && ended_since(r.ended_at)
            })
            .map(|r| r.id)
            .collect();
        let paused_crews = self
            .ports
            .orchestrator
            .paused_for_restart(workspace_id)
            .await?;
        let mut ours: HashMap<RunId, bool> = HashMap::new();
        let mut unsure = 0;
        for run_id in self.ports.journal.unknown_outcome_runs().await? {
            let mine = match ours.get(&run_id) {
                Some(mine) => *mine,
                None => {
                    let mine = match self.ports.supervisor.run_task(run_id).await? {
                        Some((run, ws, _)) => ws == workspace_id && ended_since(run.ended_at),
                        None => false,
                    };
                    ours.insert(run_id, mine);
                    mine
                }
            };
            if mine {
                unsure += 1;
            }
        }
        Ok((
            Closed {
                interrupted: interrupted.len(),
                paused_crews,
                unsure,
            },
            interrupted,
        ))
    }
}
