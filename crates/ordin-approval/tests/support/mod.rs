//! Shared test helpers: a temporary SQLite journal with the approval tables, request builders and
//! recording sinks. A missing fixture fails the test; nothing here skips.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ordin_approval::{
    ApprovalConfig, ApprovalRequest, ApprovalService, ApproverPolicy, PressOrigin,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EventSink, Journal, JournalEvent, JournalOptions,
    MigrationRegistry, SystemClock, MIGRATIONS,
};
use ordin_types::wire::approval::{
    AffectedItem, ApprovalAsker, ApprovalCard, ApprovalPress, CardRef, PressDecision, RejectStyle,
};
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{ApprovalId, BackendId, RunId, WorkspaceId};

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-approval-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Fixture {
    pub dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub service: Arc<ApprovalService>,
}

pub const COMPUTER: &str = "Asha's laptop";

pub fn config(journal: &Journal, approvers: Option<ApproverPolicy>) -> ApprovalConfig {
    ApprovalConfig {
        backend: BackendId(journal.backend_uuid()),
        local_computer_name: COMPUTER.to_owned(),
        approvers,
    }
}

/// A migrated journal and approval service on a fresh SQLite file (desktop profile).
pub async fn fixture() -> Fixture {
    fixture_with(None).await
}

pub async fn fixture_with(approvers: Option<ApproverPolicy>) -> Fixture {
    let dir = TempDir::new();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("data").join("journal.db"),
        backup_dir: dir.0.join("data").join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("register the journal");
    registry
        .register(ApprovalService::migrations())
        .expect("register the approval module");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the migrations");
    let journal = Arc::new(
        Journal::start(
            store.clone(),
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("start the journal"),
    );
    let service = ApprovalService::new(store.clone(), journal.clone(), config(&journal, approvers))
        .await
        .expect("start the approval service");
    Fixture {
        dir,
        store,
        journal,
        service,
    }
}

impl Fixture {
    /// A second service over the same file, as after an application restart.
    pub async fn restart(&self) -> Arc<ApprovalService> {
        ApprovalService::new(
            self.store.clone(),
            self.journal.clone(),
            config(&self.journal, None),
        )
        .await
        .expect("restart the approval service")
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("a SQLite datastore")
    }

    pub async fn events(&self) -> Vec<JournalEvent> {
        if self.journal.last_seq() == 0 {
            return Vec::new();
        }
        self.journal
            .events_since(0, 1_000)
            .await
            .expect("read events")
    }

    /// Every event of one kind, in order.
    pub async fn events_of(&self, kind: &str) -> Vec<JournalEvent> {
        self.events()
            .await
            .into_iter()
            .filter(|event| event.kind == kind)
            .collect()
    }
}

pub fn workspace() -> WorkspaceId {
    WorkspaceId::new()
}

/// The push request of the spec's example, asked by `asked_by`.
pub fn request_for(workspace_id: WorkspaceId, asked_by: ApprovalAsker) -> ApprovalRequest {
    let affects = vec![AffectedItem::Record {
        label: "3 commits on main".to_owned(),
    }];
    ApprovalRequest {
        workspace_id,
        asked_by,
        question: "Push 3 commits to acme/supplier-portal?".to_owned(),
        verb: "Push".to_owned(),
        where_line: "acme/supplier-portal, branch main".to_owned(),
        affects: affects.clone(),
        changes: "The three commits become visible to everyone who can open the repository."
            .to_owned(),
        content: None,
        reject_style: RejectStyle::Reject,
        show_details: false,
        proposal: ActionProposal {
            action: "Push to GitHub".to_owned(),
            destination: "acme/supplier-portal, branch main".to_owned(),
            affects,
            consequence:
                "The three commits become visible to everyone who can open the repository."
                    .to_owned(),
        },
        asker: None,
        leaves_company: true,
    }
}

pub fn request() -> ApprovalRequest {
    request_for(
        workspace(),
        ApprovalAsker::Run {
            run_id: RunId::new(),
        },
    )
}

/// The text of a card reference (its serde form is transparent).
pub fn ref_text(card_ref: &CardRef) -> String {
    match serde_json::to_value(card_ref).expect("serialise a card reference") {
        serde_json::Value::String(text) => text,
        other => panic!("a card reference serialises as text, not {other}"),
    }
}

pub fn card_ref(text: &str) -> CardRef {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).expect("card reference")
}

pub fn press(card_ref: &CardRef, decision: PressDecision) -> ApprovalPress {
    ApprovalPress {
        card_ref: card_ref.clone(),
        decision,
    }
}

pub fn approve(card: &ApprovalCard) -> ApprovalPress {
    press(&card.buttons.approve, PressDecision::Approve)
}

pub fn reject(card: &ApprovalCard) -> ApprovalPress {
    press(&card.buttons.reject, PressDecision::Reject)
}

pub fn approver(subject: &str, groups: &[&str]) -> PressOrigin {
    PressOrigin::Approver {
        subject: subject.to_owned(),
        groups: groups.iter().map(|g| (*g).to_owned()).collect(),
        via: ordin_approval::ApproverVia::Chat,
    }
}

pub fn policy(users: &[&str], groups: &[&str], internal: bool) -> ApproverPolicy {
    ApproverPolicy {
        users: users.iter().map(|u| (*u).to_owned()).collect(),
        groups: groups.iter().map(|g| (*g).to_owned()).collect(),
        asker_may_approve_internal: internal,
    }
}

pub fn id_of(card: &ApprovalCard) -> ApprovalId {
    card.approval_id
}

pub fn code_of(error: &ordin_approval::ApprovalError) -> &'static str {
    error.code()
}

/// Records the kind and sequence number of every event it receives.
pub struct RecordingSink {
    pub seen: Mutex<Vec<(u64, String)>>,
}

impl RecordingSink {
    pub fn new() -> Arc<RecordingSink> {
        Arc::new(RecordingSink {
            seen: Mutex::new(Vec::new()),
        })
    }

    pub fn seen(&self) -> Vec<(u64, String)> {
        self.seen.lock().expect("sink lock").clone()
    }
}

#[async_trait]
impl EventSink for RecordingSink {
    fn name(&self) -> &'static str {
        "recording"
    }

    async fn deliver(&self, event: &JournalEvent) {
        self.seen
            .lock()
            .expect("sink lock")
            .push((event.seq, event.kind.clone()));
    }

    async fn lagged(&self, _next_seq: u64) {}
}

/// Waits (up to 5 s) until `check` holds; the Journal delivers to sinks on its own tasks.
pub async fn eventually<F: FnMut() -> bool>(mut check: F) {
    for _ in 0..500 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the condition did not hold within 5 s");
}
