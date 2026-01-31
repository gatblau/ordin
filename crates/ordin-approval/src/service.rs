//! The Approval Service (ApprovalService.md *Internal Logic* A to L).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use ordin_journal::{
    Datastore, EventSink, Journal, MigrationStep, ModuleMigrations, PruneParticipant,
};
use ordin_types::domain::runs::ApprovalState;
use ordin_types::encoding::b64url_encode;
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{
    ApprovalAsker, ApprovalButtons, ApprovalCard, ApprovalPress, CardRef, PressDecision,
};
use ordin_types::wire::events::EventKind;
use ordin_types::{ApprovalId, CodedError, CrewRunId, Cursor, Page, RunId, WorkspaceId};
use sqlx::SqlitePool;
use tokio::sync::{watch, Mutex};

use crate::error::ApprovalError;
use crate::model::{
    ApprovalConfig, ApprovalOutcome, ApprovalRecord, ApprovalRequest, ApproverPolicy, ChatChannel,
    Ids, PressOrigin, WaitOutcome,
};
use crate::sink::{ApprovalPruneParticipant, ApprovalSink};
use crate::store::{self, Asker, Button};
use crate::validate;

/// `decided_on` / `ApprovalDecided.decided_on` is 1 to 80 characters (2C).
const DECIDED_ON_MAX: usize = 80;
/// The retry after a failed Journal emit (B.6).
const EMIT_RETRY: Duration = Duration::from_millis(200);
/// The `decided_on` of an approval expired because its work ended (F.2, *chosen here*).
const DECIDED_ON_RUN_ENDED: &str = "run ended";
/// The `decided_on` of an approval its asker took back (K.4, *chosen here*).
const DECIDED_ON_WITHDRAWN: &str = "withdrawn";

const SQLITE_MIGRATIONS: &[MigrationStep] = &[(
    1,
    "create approval tables",
    include_str!("sql/sqlite_v1.sql"),
)];
const POSTGRES_MIGRATIONS: &[MigrationStep] = &[(
    1,
    "create approval tables",
    include_str!("sql/postgres_v1.sql"),
)];

pub struct ApprovalService {
    pool: SqlitePool,
    journal: Arc<Journal>,
    config: ApprovalConfig,
    /// One channel per pending approval (Approach). A tokio mutex, so `watch` can read the store
    /// while holding it and never races a decision's wake-up.
    channels: Mutex<HashMap<ApprovalId, watch::Sender<ApprovalState>>>,
}

impl ApprovalService {
    pub const MODULE: &'static str = "approval";

    // spec: ordin-desktop/components/ApprovalService#SA-RECON-04
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: Self::MODULE,
            sqlite: SQLITE_MIGRATIONS,
            postgres: POSTGRES_MIGRATIONS,
        }
    }

    /// A. Opens the store, loads every pending approval into memory (recovery), and returns.
    // spec: ordin-desktop/components/ApprovalService#SA-XC-07
    pub async fn new(
        datastore: Datastore,
        journal: Arc<Journal>,
        config: ApprovalConfig,
    ) -> Result<Arc<Self>, ApprovalError> {
        // SPEC-GAP-3: the component spec gives PostgreSQL migrations (agent profile, scaled shape) but
        // its statements are written once, for SQLite. Release 1 builds the desktop profile only, so a
        // PostgreSQL datastore is refused as an unavailable store until the agent profile is built.
        let pool = match datastore {
            Datastore::Sqlite(pool) => pool,
            Datastore::Postgres(_) => {
                return Err(ApprovalError::Store(sqlx::Error::Configuration(
                    "the approval store is built for SQLite only".into(),
                )))
            }
        };
        let pending = store::pending_ids(&pool).await?;
        let mut channels = HashMap::with_capacity(pending.len());
        for id in &pending {
            channels.insert(*id, watch::channel(ApprovalState::Pending).0);
        }
        tracing::info!(count = pending.len(), "approval_recovered");
        Ok(Arc::new(ApprovalService {
            pool,
            journal,
            config,
            channels: Mutex::new(channels),
        }))
    }

    /// B. Creates a pending approval, mints its buttons, journals `approval_requested`.
    pub async fn request(&self, req: ApprovalRequest) -> Result<ApprovalCard, ApprovalError> {
        let req = validate::validate(req)?;
        let approval_id = ApprovalId::new();
        let approve = mint_card_ref()?;
        let reject = mint_card_ref()?;
        let show_details = if req.show_details {
            Some(mint_card_ref()?)
        } else {
            None
        };
        let mut refs = vec![
            (Button::Approve, card_ref_text(&approve)?),
            (Button::Reject, card_ref_text(&reject)?),
        ];
        if let Some(details) = &show_details {
            refs.push((Button::ShowDetails, card_ref_text(details)?));
        }
        let card = ApprovalCard {
            approval_id,
            backend: self.config.backend,
            workspace_id: req.workspace_id,
            asked_by: req.asked_by.clone(),
            question: req.question.clone(),
            verb: req.verb.clone(),
            where_line: req.where_line.clone(),
            affects: req.affects.clone(),
            changes: req.changes.clone(),
            content: req.content.clone(),
            reject_style: req.reject_style,
            buttons: ApprovalButtons {
                approve,
                reject,
                show_details,
            },
            created_at: Timestamp::now(),
        };
        store::insert(&self.pool, &card, &req, &refs).await?;
        self.channels
            .lock()
            .await
            .insert(approval_id, watch::channel(ApprovalState::Pending).0);
        // spec: ordin-desktop/components/ApprovalService#SA-RECON-02
        self.emit(
            EventKind::ApprovalRequested,
            serde_json::json!({ "card": card }),
            approval_id,
        )
        .await;
        self.emit_needs_you(card.workspace_id, approval_id).await;
        match &card.asked_by {
            ApprovalAsker::Run { run_id } => tracing::info!(
                approval_id = approval_id.to_string(),
                workspace_id = card.workspace_id.to_string(),
                asked_by_kind = "run",
                run_id = run_id.to_string(),
                "approval_requested"
            ),
            ApprovalAsker::CrewRound { crew_run_id, .. } => tracing::info!(
                approval_id = approval_id.to_string(),
                workspace_id = card.workspace_id.to_string(),
                asked_by_kind = "crew_round",
                crew_run_id = crew_run_id.to_string(),
                "approval_requested"
            ),
            ApprovalAsker::Person => tracing::info!(
                approval_id = approval_id.to_string(),
                workspace_id = card.workspace_id.to_string(),
                asked_by_kind = "person",
                "approval_requested"
            ),
        }
        Ok(card)
    }

    /// C. Service API `approval.decide`, and chat buttons.
    pub async fn decide(
        &self,
        press: ApprovalPress,
        origin: PressOrigin,
    ) -> Result<ApprovalOutcome, ApprovalError> {
        let card_ref = card_ref_text(&press.card_ref)?;
        // C.1
        let found = store::button_lookup(&self.pool, &card_ref)
            .await?
            .ok_or(ApprovalError::CardRefUnknown)?;
        let wanted = match press.decision {
            PressDecision::Approve => Button::Approve,
            PressDecision::Reject => Button::Reject,
        };
        if found.button != wanted {
            return Err(ApprovalError::CardRefMismatch);
        }
        // C.2
        if !found.button_open || found.state != ApprovalState::Pending {
            return Err(refused_for(found.state, found.decided_on));
        }
        // C.3 and C.4
        self.authorise(&origin, found.asker.as_deref(), found.leaves_company)?;
        // C.5
        let (decided_on, decided_by) = self.decided_labels(&origin);
        let state = state_of_decision(press.decision);
        let outcome = self
            .conclude(found.ids, state, wanted, decided_by, decided_on)
            .await?;
        // C.9
        let origin_kind = origin_kind(&origin);
        match &origin {
            PressOrigin::Approver { subject, .. } if self.config.approvers.is_some() => {
                tracing::info!(
                    approval_id = outcome.approval_id.to_string(),
                    state = store::state_text(outcome.state),
                    origin_kind,
                    approver = subject.as_str(),
                    "approval_decided"
                )
            }
            _ => tracing::info!(
                approval_id = outcome.approval_id.to_string(),
                state = store::state_text(outcome.state),
                origin_kind,
                "approval_decided"
            ),
        }
        Ok(outcome)
    }

    /// D. Agent profile with no approvers named: the operator decides.
    pub async fn decide_by_operator(
        &self,
        approval_id: ApprovalId,
        decision: PressDecision,
        operator: &str,
    ) -> Result<ApprovalOutcome, ApprovalError> {
        // D.1
        let allowed = matches!(
            &self.config.approvers,
            Some(policy) if policy.users.is_empty() && policy.groups.is_empty()
        );
        if !allowed {
            return Err(ApprovalError::OperatorNotAllowed);
        }
        // D.2
        let row = store::state_row(&self.pool, approval_id)
            .await?
            .ok_or(ApprovalError::NotFound)?;
        if row.state != ApprovalState::Pending {
            return Err(refused_for(row.state, row.decided_on));
        }
        let button = match decision {
            PressDecision::Approve => Button::Approve,
            PressDecision::Reject => Button::Reject,
        };
        let operator = bounded_label(operator);
        let outcome = self
            .conclude(
                row.ids,
                state_of_decision(decision),
                button,
                operator.clone(),
                operator.clone(),
            )
            .await?;
        tracing::info!(
            approval_id = outcome.approval_id.to_string(),
            state = store::state_text(outcome.state),
            origin_kind = "operator",
            approver = operator.as_str(),
            "approval_decided"
        );
        Ok(outcome)
    }

    /// K. The asking service takes back an approval it opened, before anyone decides.
    // spec: ordin-desktop/components/ApprovalService#SA-RECON-01
    // spec: ordin-desktop/components/ApprovalService#SA-CLOSE-01
    pub async fn withdraw(&self, approval_id: ApprovalId) -> Result<(), ApprovalError> {
        // K.1
        let row = store::state_row(&self.pool, approval_id)
            .await?
            .ok_or(ApprovalError::NotFound)?;
        // K.2
        match row.state {
            ApprovalState::Expired => return Ok(()),
            ApprovalState::Approved | ApprovalState::Rejected => {
                return Err(refused_for(row.state, row.decided_on))
            }
            ApprovalState::Pending => {}
        }
        // K.3
        if !store::withdraw(&self.pool, approval_id).await? {
            let again = store::state_row(&self.pool, approval_id)
                .await?
                .ok_or(ApprovalError::NotFound)?;
            return match again.state {
                ApprovalState::Expired => Ok(()),
                other => Err(refused_for(other, again.decided_on)),
            };
        }
        // K.4
        self.close_channel(approval_id, ApprovalState::Expired)
            .await;
        self.emit_decided(approval_id, ApprovalState::Expired, DECIDED_ON_WITHDRAWN)
            .await;
        self.emit_needs_you(row.ids.workspace, approval_id).await;
        // K.5
        tracing::info!(approval_id = approval_id.to_string(), "approval_withdrawn");
        Ok(())
    }

    /// E.1. The receiver changes once, from `Pending` to a final state.
    ///
    /// E.1 reads the store for an approval that is not in memory (a decided or expired one returns
    /// a channel already holding its final state), so the call is `async`.
    pub async fn watch(
        &self,
        approval_id: ApprovalId,
    ) -> Result<watch::Receiver<ApprovalState>, ApprovalError> {
        let mut channels = self.channels.lock().await;
        if let Some(sender) = channels.get(&approval_id) {
            return Ok(sender.subscribe());
        }
        let row = store::state_row(&self.pool, approval_id)
            .await?
            .ok_or(ApprovalError::NotFound)?;
        let (sender, receiver) = watch::channel(row.state);
        if row.state == ApprovalState::Pending {
            channels.insert(approval_id, sender);
        }
        Ok(receiver)
    }

    /// E.2. Waits for a decision until `deadline` (`None` waits without limit). A deadline never
    /// decides: the approval stays pending (11 §5.5).
    pub async fn wait(
        &self,
        approval_id: ApprovalId,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<WaitOutcome, ApprovalError> {
        let mut receiver = self.watch(approval_id).await?;
        // The state is copied out inside the future, so no borrow of the channel is held across an
        // await and the future stays `Send` for callers that spawn it.
        let decided = async {
            receiver
                .wait_for(|state| *state != ApprovalState::Pending)
                .await
                .map(|state| *state)
        };
        let waited = match deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, decided).await {
                Ok(result) => result,
                Err(_) => return Ok(WaitOutcome::DeadlinePassed),
            },
            None => decided.await,
        };
        match waited {
            Ok(state) => Ok(WaitOutcome::Decided(state)),
            // The sender went away without a final state: the store is the one truth.
            Err(_) => {
                let record = self.get(approval_id).await?;
                if record.state == ApprovalState::Pending {
                    Ok(WaitOutcome::DeadlinePassed)
                } else {
                    Ok(WaitOutcome::Decided(record.state))
                }
            }
        }
    }

    /// The record with its card and state.
    pub async fn get(&self, approval_id: ApprovalId) -> Result<ApprovalRecord, ApprovalError> {
        store::get(&self.pool, approval_id)
            .await?
            .ok_or(ApprovalError::NotFound)
    }

    /// H. Service API `approval.list_pending`; page size 100, oldest first.
    // spec: ordin-desktop/components/ApprovalService#SA-PAGE-01
    pub async fn list_pending(
        &self,
        workspace: Option<WorkspaceId>,
        cursor: Option<Cursor>,
    ) -> Result<Page<ApprovalRecord>, ApprovalError> {
        store::list_pending(&self.pool, workspace, cursor).await
    }

    /// F. Expires every pending approval of a run that ended first.
    pub async fn expire_for_run(&self, run_id: RunId) -> Result<Vec<ApprovalId>, ApprovalError> {
        let expired = store::expire(&self.pool, Asker::Run, &run_id.to_string()).await?;
        self.after_expiry(expired, Some(run_id), None).await
    }

    /// F. Expires every pending approval of a crew run that ended first.
    pub async fn expire_for_crew_run(
        &self,
        crew_run_id: CrewRunId,
    ) -> Result<Vec<ApprovalId>, ApprovalError> {
        let expired = store::expire(&self.pool, Asker::CrewRun, &crew_run_id.to_string()).await?;
        self.after_expiry(expired, None, Some(crew_run_id)).await
    }

    /// The `EventSink` the program registers with the Journal.
    pub fn event_sink(self: &Arc<Self>) -> Arc<dyn EventSink> {
        Arc::new(ApprovalSink::new(Arc::downgrade(self)))
    }

    /// The Journal's pruning hook for decided approvals.
    // spec: ordin-desktop/components/ApprovalService#A-08
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn PruneParticipant> {
        Arc::new(ApprovalPruneParticipant)
    }

    // ---- internals

    /// C.3 and C.4: who may press.
    fn authorise(
        &self,
        origin: &PressOrigin,
        asker: Option<&str>,
        leaves_company: bool,
    ) -> Result<(), ApprovalError> {
        match &self.config.approvers {
            Some(policy) => authorise_agent(policy, origin, asker, leaves_company),
            None => match origin {
                PressOrigin::Approver { .. } => Err(ApprovalError::NotAnApprover),
                _ => Ok(()),
            },
        }
    }

    /// C.5: `(decided_on, decided_by)` for a press.
    fn decided_labels(&self, origin: &PressOrigin) -> (String, String) {
        let (on, by) = match origin {
            PressOrigin::LocalScreen => (self.config.local_computer_name.clone(), "person".into()),
            PressOrigin::PairedDesktop { computer_name } => {
                (computer_name.clone(), "person".to_owned())
            }
            PressOrigin::PairedChat { channel } => (
                match channel {
                    ChatChannel::Telegram => "Telegram",
                    ChatChannel::Slack => "Slack",
                }
                .to_owned(),
                "person".to_owned(),
            ),
            PressOrigin::Approver { subject, .. } => (subject.clone(), subject.clone()),
        };
        (bounded_label(&on), by)
    }

    /// C.6 to C.8 (also D): decide once, wake the waiters, journal.
    async fn conclude(
        &self,
        ids: Ids,
        state: ApprovalState,
        button: Button,
        decided_by: String,
        decided_on: String,
    ) -> Result<ApprovalOutcome, ApprovalError> {
        let won = store::decide(
            &self.pool,
            ids.approval,
            state,
            &decided_by,
            &decided_on,
            button,
        )
        .await?;
        if !won {
            let again = store::state_row(&self.pool, ids.approval)
                .await?
                .ok_or(ApprovalError::NotFound)?;
            return Err(refused_for(again.state, again.decided_on));
        }
        // C.7
        self.close_channel(ids.approval, state).await;
        // C.8 and L
        self.emit_decided(ids.approval, state, &decided_on).await;
        self.emit_needs_you(ids.workspace, ids.approval).await;
        Ok(ApprovalOutcome {
            approval_id: ids.approval,
            state,
            decided_on,
        })
    }

    /// F.2 and L, after the statement of F.1.
    async fn after_expiry(
        &self,
        expired: Vec<Ids>,
        run_id: Option<RunId>,
        crew_run_id: Option<CrewRunId>,
    ) -> Result<Vec<ApprovalId>, ApprovalError> {
        let mut workspaces: BTreeSet<WorkspaceId> = BTreeSet::new();
        for ids in &expired {
            self.close_channel(ids.approval, ApprovalState::Expired)
                .await;
            self.emit_decided(ids.approval, ApprovalState::Expired, DECIDED_ON_RUN_ENDED)
                .await;
            if workspaces.insert(ids.workspace) {
                self.emit_needs_you(ids.workspace, ids.approval).await;
            }
            match (run_id, crew_run_id) {
                (Some(run_id), _) => tracing::info!(
                    approval_id = ids.approval.to_string(),
                    run_id = run_id.to_string(),
                    "approval_expired"
                ),
                (None, Some(crew_run_id)) => tracing::info!(
                    approval_id = ids.approval.to_string(),
                    crew_run_id = crew_run_id.to_string(),
                    "approval_expired"
                ),
                (None, None) => {
                    tracing::info!(approval_id = ids.approval.to_string(), "approval_expired")
                }
            }
        }
        Ok(expired.iter().map(|ids| ids.approval).collect())
    }

    /// Sends the final state to every waiter and drops the channel.
    async fn close_channel(&self, approval_id: ApprovalId, state: ApprovalState) {
        let mut channels = self.channels.lock().await;
        if let Some(sender) = channels.remove(&approval_id) {
            sender.send_replace(state);
        }
    }

    async fn emit_decided(&self, approval_id: ApprovalId, state: ApprovalState, decided_on: &str) {
        self.emit(
            EventKind::ApprovalDecided,
            serde_json::json!({
                "approval_id": approval_id,
                "state": state,
                "decided_on": decided_on,
            }),
            approval_id,
        )
        .await;
    }

    async fn emit_needs_you(&self, workspace_id: WorkspaceId, approval_id: ApprovalId) {
        self.emit(
            EventKind::NeedsYouChanged,
            serde_json::json!({ "workspace_id": workspace_id }),
            approval_id,
        )
        .await;
    }

    /// The Journal emit with the retry rule of B.6: one retry after 200 ms; a second failure is
    /// logged and never fails the call.
    async fn emit(&self, kind: EventKind, data: serde_json::Value, approval_id: ApprovalId) {
        for attempt in 0..2 {
            match self.journal.append_event(kind.as_str(), data.clone()).await {
                Ok(_) => return,
                Err(error) => {
                    tracing::warn!(
                        approval_id = approval_id.to_string(),
                        error_code = CodedError::code(&error),
                        code = "APPR_EMIT_FAILED",
                        "approval_emit_failed"
                    );
                    if attempt == 0 {
                        tokio::time::sleep(EMIT_RETRY).await;
                    }
                }
            }
        }
    }
}

/// C.3 for the agent profile.
fn authorise_agent(
    policy: &ApproverPolicy,
    origin: &PressOrigin,
    asker: Option<&str>,
    leaves_company: bool,
) -> Result<(), ApprovalError> {
    let PressOrigin::Approver {
        subject, groups, ..
    } = origin
    else {
        return Err(ApprovalError::NotAnApprover);
    };
    // No approvers named: every press is refused; only `decide_by_operator` decides (ADR-097 item 4).
    if policy.users.is_empty() && policy.groups.is_empty() {
        return Err(ApprovalError::NotAnApprover);
    }
    let named = policy.users.iter().any(|user| user == subject)
        || groups.iter().any(|group| policy.groups.contains(group));
    if !named {
        return Err(ApprovalError::NotAnApprover);
    }
    if asker == Some(subject.as_str()) && (leaves_company || !policy.asker_may_approve_internal) {
        return Err(ApprovalError::AskerMayNotApprove);
    }
    Ok(())
}

fn state_of_decision(decision: PressDecision) -> ApprovalState {
    match decision {
        PressDecision::Approve => ApprovalState::Approved,
        PressDecision::Reject => ApprovalState::Rejected,
    }
}

/// The refusal for an approval that is no longer pending (C.2, C.6, K.2).
fn refused_for(state: ApprovalState, decided_on: Option<String>) -> ApprovalError {
    match state {
        ApprovalState::Approved | ApprovalState::Rejected => ApprovalError::AlreadyDecided {
            state,
            decided_on: decided_on.unwrap_or_default(),
        },
        // A pending state here means the row changed under the caller; the card is closed to it.
        ApprovalState::Expired | ApprovalState::Pending => ApprovalError::CardClosed,
    }
}

fn origin_kind(origin: &PressOrigin) -> &'static str {
    match origin {
        PressOrigin::LocalScreen => "local_screen",
        PressOrigin::PairedDesktop { .. } => "paired_desktop",
        PressOrigin::PairedChat { .. } => "paired_chat",
        PressOrigin::Approver { .. } => "approver",
    }
}

/// `decided_on` truncated to 80 characters (C.5).
///
/// The store refuses an empty `decided_on` (1 to 80 characters), but nothing stops a program
/// passing an empty computer name or operator. An empty value is stored as `unknown`.
fn bounded_label(raw: &str) -> String {
    let label: String = raw.chars().take(DECIDED_ON_MAX).collect();
    if label.is_empty() {
        "unknown".to_owned()
    } else {
        label
    }
}

/// B.3: 16 bytes from the operating system's random source, base64url without padding.
///
/// `CardRef` has no constructor in `ordin-types` (its field is private), so the minted text goes in
/// through its serde form, which is transparent.
fn mint_card_ref() -> Result<CardRef, ApprovalError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| {
        ApprovalError::Store(sqlx::Error::Io(std::io::Error::other("no random source")))
    })?;
    serde_json::from_value(serde_json::Value::String(b64url_encode(&bytes)))
        .map_err(|_| ApprovalError::corrupt("card reference"))
}

/// The text of a `CardRef`, through its transparent serde form.
fn card_ref_text(card_ref: &CardRef) -> Result<String, ApprovalError> {
    match serde_json::to_value(card_ref) {
        Ok(serde_json::Value::String(text)) => Ok(text),
        _ => Err(ApprovalError::corrupt("card reference")),
    }
}
