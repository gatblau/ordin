//! The Push Service: construction, the event loops and recovery (Public Interface; Internal Logic B,
//! D, E3 and G). The sections that raise cards and send live in `documents`, `copies` and `pulls`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_journal::{EventSink, ModuleMigrations};
use ordin_types::WorkspaceId;
use sqlx::SqlitePool;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::error::PushError;
use crate::model::{PullRequestProposal, PushDeps, PushNotice, SubmissionId};
use crate::sink::PushSink;

/// The decision queue's capacity (Internal Logic B).
const QUEUE_CAPACITY: usize = 1_024;
/// The comparison interval for originals this service follows (Configuration; 2D-4).
const COMPARE_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// The notice channel's capacity.
const NOTICE_CAPACITY: usize = 256;

/// A decision read from an `approval_decided` event.
#[derive(Debug, Clone)]
pub(crate) struct Decision {
    pub approval_id: ordin_types::ApprovalId,
    /// `approved`, `rejected` or `expired`.
    pub state: String,
}

pub struct PushService {
    pub(crate) deps: PushDeps,
    pub(crate) pool: Option<SqlitePool>,
    pub(crate) notices: broadcast::Sender<PushNotice>,
    pub(crate) queue: mpsc::Sender<Decision>,
    queue_rx: tokio::sync::Mutex<Option<mpsc::Receiver<Decision>>>,
    /// The whole proposal of a pull request, file bytes included, until it is sent. The bytes are also
    /// stored in `pull_request_files`, so a restart before the send still sends them.
    pub(crate) proposals: Mutex<HashMap<SubmissionId, PullRequestProposal>>,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Waits for the next value of an optional receiver; a closed receiver is dropped and never wakes again.
async fn next<T: Clone>(receiver: &mut Option<broadcast::Receiver<T>>) -> Option<T> {
    loop {
        let Some(inner) = receiver.as_mut() else {
            std::future::pending::<()>().await;
            continue;
        };
        match inner.recv().await {
            Ok(value) => return Some(value),
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(component = "push", skipped, "push.channel_lagged");
            }
            Err(broadcast::error::RecvError::Closed) => {
                *receiver = None;
            }
        }
    }
}

impl PushService {
    pub const MODULE: &'static str = "push";

    /// Module `push`; the program passes it to `MigrationRegistry::register` (R3). The component does not
    /// run in the agent profile, so the PostgreSQL slice is empty.
    // spec: ordin-desktop/components/PushService#SA-RECON-07
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: Self::MODULE,
            sqlite: &[
                (1, "push tables", include_str!("sql/sqlite_v1.sql")),
                (
                    2,
                    "team knowledge shares",
                    include_str!("sql/sqlite_v2.sql"),
                ),
                (
                    3,
                    "copy requests, knowledge page files, pull request files",
                    include_str!("sql/sqlite_v3.sql"),
                ),
            ],
            postgres: &[],
        }
    }

    pub fn new(deps: PushDeps) -> Arc<Self> {
        let pool = deps.journal.datastore().sqlite().cloned();
        let (notices, _) = broadcast::channel(NOTICE_CAPACITY);
        let (queue, queue_rx) = mpsc::channel(QUEUE_CAPACITY);
        Arc::new(PushService {
            deps,
            pool,
            notices,
            queue,
            queue_rx: tokio::sync::Mutex::new(Some(queue_rx)),
            proposals: Mutex::new(HashMap::new()),
        })
    }

    /// The sink the program registers with `Journal::subscribe`; it forwards `approval_decided` events to §B.
    // spec: ordin-desktop/components/PushService#SA-RECON-09
    pub fn event_sink(self: &Arc<Self>) -> Arc<dyn EventSink> {
        Arc::new(PushSink::new(Arc::downgrade(self)))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PushNotice> {
        self.notices.subscribe()
    }

    pub(crate) fn pool(&self) -> Result<&SqlitePool, PushError> {
        self.pool
            .as_ref()
            .ok_or_else(|| PushError::store(false, "the push tables are built for SQLite only"))
    }

    pub(crate) fn notify(&self, notice: PushNotice) {
        // No subscriber is not a failure.
        let _ = self.notices.send(notice);
    }

    /// H: one `needs_you_changed` per affected workspace; a failed append changes nothing else.
    pub(crate) async fn needs_you(&self, workspace: WorkspaceId) {
        let data = serde_json::json!({ "workspace_id": workspace });
        if let Err(error) = self
            .deps
            .journal
            .append_event("needs_you_changed", data)
            .await
        {
            tracing::warn!(
                component = "push",
                code = ordin_types::CodedError::code(&error),
                "push.event_failed"
            );
        }
    }

    /// Recovery (§G), then the event loops (§B, §D, §E). Returns when `shutdown` fires.
    pub async fn run(&self, shutdown: CancellationToken) -> Result<(), PushError> {
        let Some(mut queue) = self.queue_rx.lock().await.take() else {
            return Err(PushError::store(false, "run was already started"));
        };
        let mut local = Some(self.deps.working_copy.local_changes());
        let mut originals = Some(self.deps.projects.subscribe_original_changes());
        let mut places = Some(self.deps.files.subscribe_changes());
        self.recover().await?;
        let mut compare = tokio::time::interval(COMPARE_INTERVAL);
        compare.tick().await;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                Some(decision) = queue.recv() => self.handle_decision(decision).await,
                Some(item) = next(&mut local) => self.on_local_change(item).await,
                Some(change) = next(&mut originals) => self.on_repository_change(change).await,
                Some(change) = next(&mut places) => self.on_place_change(change).await,
                _ = compare.tick() => self.compare_originals().await,
            }
        }
    }

    /// §G: after the program's `Journal::recover_external_actions` and before accepting new work.
    pub(crate) async fn recover(&self) -> Result<(), PushError> {
        self.recover_external_actions().await?;
        self.recover_pull_requests().await?;
        self.recover_team_shares().await?;
        Ok(())
    }

    /// B: routes one decision to the push, submission, copy operation or team share that holds the card.
    pub(crate) async fn handle_decision(&self, decision: Decision) {
        let Ok(pool) = self.pool() else {
            return;
        };
        let id = decision.approval_id;
        let routed = async {
            if let Some(push) = crate::store::push_by_approval(pool, id).await? {
                return self.decide_push(push, &decision.state).await;
            }
            if let Some(submission) = crate::store::submission_by_approval(pool, id).await? {
                return self.decide_pull_request(submission, &decision.state).await;
            }
            let ops = crate::store::copy_ops_by_approval(pool, id).await?;
            if !ops.is_empty() {
                return self.decide_copies(ops, &decision.state).await;
            }
            if let Some(share) = crate::store::team_share_by_approval(pool, id).await? {
                return self.decide_team_share(share, &decision.state).await;
            }
            // 4: a decision for an id this component does not hold is ignored.
            Ok(())
        }
        .await;
        if let Err(error) = routed {
            tracing::warn!(
                component = "push",
                code = error.code,
                "push.decision_failed"
            );
        }
    }
}
