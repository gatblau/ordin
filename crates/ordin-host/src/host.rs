//! The Ordin Host itself: construction, the per-workspace conversation state machine (section B),
//! the turn queue and the reader of Ordin's run, and the helpers every section shares.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use ordin_runtime_adapter::{PauseReason, ResumeDecision, RunEnd, RunEndReason, RunEvent};
use ordin_service_api::blocks::{Block, MessageBlock};
use ordin_service_api::commands::ordin::{ConversationEnd, MessageAuthor};
use ordin_service_api::{OrdinTurnFailure, OrdinTurnState};
use ordin_session_supervisor::ConversationStart;
use ordin_types::common::{SetBy, Setting};
use ordin_types::domain::{VoiceLength, VoiceRegister, VoiceSettings, VoiceTone};
use ordin_types::time::Timestamp;
use ordin_types::wire::events::EventKind;
use ordin_types::wire::run::{AdapterKind, RunLimits, RunTarget};
use ordin_types::{BlockId, MemoryFactId, MemoryRecallId, RunId, SlackLinkId, WorkspaceId};
use sqlx::SqlitePool;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::error::OhError;
use crate::ids::ConversationId;
use crate::ports::{
    DepError, LocMessage, Ports, RealBlocks, RealCatalogue, RealFiles, RealInspector, RealJournal,
    RealKnowledge, RealLocalisation, RealMemory, RealOrchestrator, RealProjects, RealPush,
    RealSupervisor, RealUsage, RealWorkspaces,
};
use crate::seed;
use crate::store::{self, BlockRow, ConversationRow};
use crate::types::{OrdinHostConfig, OrdinHostDeps, OrdinHostPolicy, TriggerDesk};

/// The whole seed's limit, continued history included: the Supervisor's limit on the start text
/// (ADR-062; 2C `StartInput`; step 3.2).
pub(crate) const SEED_MAX_CHARS: usize = 16_384;
/// Ordin's run outlives single tasks; the 1-hour break rule ends it (step 3.3, assumed there).
const ORDIN_WALL_CLOCK_SECS: u32 = 28_800;
const ORDIN_IDLE_SECS: u32 = 3_600;
/// A message block's text (2C wire §7).
pub(crate) const MESSAGE_MAX_CHARS: usize = 8_000;
/// `waiting_blocks()`: at most 500.
const WAITING_MAX: i64 = 500;
pub(crate) const COMPONENT: &str = "ordin_host";

/// What a turn this crate sent ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TurnEnd {
    /// Ordin's text for the turn (possibly empty).
    Text(String),
    /// Ordin's run ended or could not be resumed.
    Lost,
}

pub(crate) struct Turn {
    pub text: String,
    pub waiter: Option<oneshot::Sender<TurnEnd>>,
    /// A Slack turn holds its link until Ordin's next pause (step 46).
    pub slack: Option<(SlackLinkId, String)>,
    /// The turn carries the person's own message (step 52.1): only such a turn may change a setting.
    pub persons: bool,
    /// OH-SUM-1: the reply goes only to the waiter and is never shown (the summary turn of step 32).
    pub quiet: bool,
}

impl Turn {
    /// A turn started by the person's own message (steps 4, 35, 38 and 53.4).
    pub(crate) fn person(text: String) -> Self {
        Turn {
            text,
            waiter: None,
            slack: None,
            persons: true,
            quiet: false,
        }
    }

    pub(crate) fn fixed(text: String) -> Self {
        Turn {
            text,
            waiter: None,
            slack: None,
            persons: false,
            quiet: false,
        }
    }
}

/// A coordinator call waiting for Ordin's crew tool (step 16.4).
#[derive(Debug, Clone)]
pub(crate) enum Expect {
    Round,
    Check,
    Condition { paths: Vec<String> },
    Question,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PendingCalls {
    pub calls: Vec<ordin_crew_orchestrator::CrewToolCall>,
    pub decision: Option<crate::types::DecideArgs>,
    pub escalated: Option<(String, Option<String>)>,
}

#[derive(Debug, Clone)]
pub(crate) struct Pending {
    pub expect: Expect,
    pub got: PendingCalls,
}

/// The in-memory state of one workspace's current conversation.
pub(crate) struct Conv {
    pub id: ConversationId,
    pub run_id: RunId,
    pub busy: bool,
    pub queue: VecDeque<Turn>,
    pub in_flight: Option<oneshot::Sender<TurnEnd>>,
    /// OH-LG-1/2: `None` after a restart of the program.
    pub stated_locale: Option<String>,
    pub turns_since_summary: u32,
    /// OH-AM-1: the recall ids of the turn now running.
    pub recalls: Vec<MemoryRecallId>,
    pub slack: Option<(SlackLinkId, String)>,
    /// The latest turn sent to the run carries the person's own message (step 52.1); false while
    /// Ordin works on its seed, before any turn.
    pub persons_turn: bool,
    /// OH-SEED-1: the turn now running is Ordin's work on its seed; its reply is never shown.
    pub seed_turn: bool,
    /// OH-SUM-1: the turn now running is a quiet turn; its reply goes only to its waiter.
    pub quiet_turn: bool,
    /// OH-BRK-1: the summary after a break runs in the background until its message is delivered.
    pub breaking: bool,
    /// OH-ACC-1: the agents already asked about folder access in this conversation.
    pub access_asked: std::collections::HashSet<ordin_types::AgentInstanceId>,
    /// OH-TF-5: the person's latest message in this conversation, as they typed it; empty before any.
    pub persons_text: String,
    /// OH-ACC-2: the folders of the files the person's latest message attached, in attachment order.
    pub attached_places: Vec<ordin_types::PlaceId>,
    /// OH-ATT-2: the files the person's latest message attached from linked folders, in order.
    pub attached_files: Vec<(ordin_types::PlaceId, String)>,
    /// OH-ACC-2: the agents and folders already asked about since the person's latest message.
    pub folder_asked:
        std::collections::HashSet<(ordin_types::AgentInstanceId, ordin_types::PlaceId)>,
    pub directory_changed: bool,
    /// OH-WC-1: the changed linked documents the last line named, sorted; empty, none named.
    pub changed_named: Vec<ordin_types::WorkingCopyItemId>,
    /// OH-RC-2: the answers of tasks that completed since the last turn, `(agent name, answer)`, oldest first.
    pub finished_tasks: Vec<(String, String)>,
    pub voice_changed: bool,
    /// A voice change Ordin made itself in the running turn sends no prefix (step 52.2).
    pub voice_changed_by_ordin: bool,
    pub pending: Option<Pending>,
    /// OH-AM-3.1: facts Ordin kept in this run without `made_from`.
    pub own_facts: Vec<MemoryFactId>,
    /// OH-AM step 3: the thing each quoted fact of a consolidation turn is about, so the summary
    /// Ordin writes back names it (step 15.2).
    pub consolidating: HashMap<MemoryFactId, String>,
    pub last_message_at: Timestamp,
    /// The run was ended by this crate (summary, Clear, continue, removal).
    pub ended_by_us: bool,
    /// OH-WK-1: when the screens were last told Ordin is working; `None` while idle.
    pub working_since: Option<Timestamp>,
}

/// A question block kept with what its answer acts on (steps 8.3, 16.3, 53).
pub(crate) enum Kept {
    Delegation(Box<crate::tools::Delegation>),
    Ask(oneshot::Sender<String>),
    LanguageOffer {
        tag: String,
        name: String,
    },
    /// The key-points form of a start fresh, or of a continue when it names the conversation to
    /// continue (OH-WR-1).
    KeyPoints {
        continues: Option<crate::ids::OrdinConversationId>,
    },
    /// OH-PF-1: the failed items of an approved send-back that **Try again** sends again.
    SendBack {
        pending_push_id: ordin_types::PendingPushId,
        items: Vec<ordin_types::WorkingCopyItemId>,
    },
}

#[derive(Default)]
pub(crate) struct State {
    pub convs: HashMap<WorkspaceId, Conv>,
    pub kept: HashMap<BlockId, (WorkspaceId, Kept)>,
    pub draft: Option<crate::language::DraftJob>,
    pub shares: HashMap<String, ordin_types::WorkflowId>,
    pub counts: HashMap<RunId, crate::memory::RunCounts>,
    /// Facts Ordin kept in a run that has ended, until they are counted (OH-AM-3.1).
    pub ended_facts: HashMap<RunId, Vec<MemoryFactId>>,
    /// The mirror of `ordin_blocks.waiting` (`waiting_blocks()`).
    pub waiting: HashMap<BlockId, crate::types::WaitingBlock>,
    /// Workflow-run inputs the person filled in a form, by charter (step 18).
    pub filled: HashMap<ordin_types::CrewCharterId, Vec<crate::types::RunInput>>,
    /// OH-WR-1.2: a pending key-points request by workspace, with the conversation to continue.
    pub key_points: HashMap<WorkspaceId, Option<crate::ids::OrdinConversationId>>,
    /// OH-CL-1: by workspace, held from `orchestrator.start` until the charter holds the crew run.
    pub crew_starts: HashMap<WorkspaceId, Arc<tokio::sync::Mutex<()>>>,
    /// OH-CLS-1: the workspaces already told what happened while ordin was closed.
    pub closed_told: std::collections::HashSet<WorkspaceId>,
    /// OH-MIN-1: the minutes runs in progress, by run.
    #[cfg(feature = "minutes")]
    pub minutes_runs: HashMap<RunId, crate::minutes::MinutesRun>,
}

pub struct OrdinHost {
    pub(crate) me: Weak<OrdinHost>,
    pub(crate) config: OrdinHostConfig,
    pub(crate) policy: RwLock<OrdinHostPolicy>,
    pub(crate) ports: Ports,
    pub(crate) pool: SqlitePool,
    pub(crate) state: Mutex<State>,
    pub(crate) trigger_desk: RwLock<Option<Arc<dyn TriggerDesk>>>,
    /// OH-MIN-5: the Minutes Service, given by the desktop program.
    #[cfg(feature = "minutes")]
    pub(crate) minutes_port: RwLock<Option<Arc<dyn crate::minutes_tools::MinutesPort>>>,
    pub(crate) background: CancellationToken,
    /// OH-CLS-1: when this host was built, before the program's recovery at start.
    pub(crate) started_at: Timestamp,
}

impl OrdinHost {
    /// OH-CL-1: the lock that orders a crew's start (step 17) before its end (step 16.2).
    // spec: ordin-desktop/components/OrdinHost#OH-CL-1
    pub(crate) fn crew_start_lock(&self, workspace_id: WorkspaceId) -> Arc<tokio::sync::Mutex<()>> {
        lock(&self.state)
            .crew_starts
            .entry(workspace_id)
            .or_default()
            .clone()
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// OH-ANS-1: the longest prefix line of step 51, in characters.
// spec: ordin-desktop/components/OrdinHost#OH-ANS-1
pub(crate) const PREFIX_LINE_MAX_CHARS: usize = 1_000;
/// OH-DIR-1: the longest directory change prefix, in characters, cut at a whole line so no key is cut.
pub(crate) const DIRECTORY_PREFIX_MAX_CHARS: usize = 8_000;
/// OH-RC-2: the longest answer of a finished task in a turn's prefix, in characters, and how many tasks' answers
/// one turn carries (the newest).
// spec: ordin-desktop/components/OrdinHost#OH-RC-2
pub(crate) const FINISHED_TASK_MAX_CHARS: usize = 2_000;
pub(crate) const FINISHED_TASKS_MAX: usize = 2;
/// OH-ANS-2: the longest file name in an "Attached:" line of step 4, in characters.
// spec: ordin-desktop/components/OrdinHost#OH-ANS-2
pub(crate) const ATTACHED_NAME_MAX_CHARS: usize = 255;
/// OH-ATT-1: the longest part of an "Attached:" line after "Attached: ", in characters. Twenty such
/// lines, an 8,000-character message and three 1,001-character prefix lines stay within the
/// runtime's 16,384-character turn (OH-ANS-2's bound).
// spec: ordin-desktop/components/OrdinHost#OH-ATT-1
pub(crate) const ATTACHED_LINE_MAX_CHARS: usize = 255;
/// OH-KP-1: the longest path for which an "Attached:" line also gives the file as a page's source, in characters.
pub(crate) const ATTACHED_SOURCE_PATH_MAX_CHARS: usize = 120;
/// OH-WK-1 / SVC-WK-1: `ordin_turn_changed`'s `doing` is 1–80 characters.
pub(crate) const DOING_MAX_CHARS: usize = 80;

pub(crate) fn cut_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Cut at a sentence end within `max` characters (step 5), else at `max`.
pub(crate) fn cut_sentence(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let head = cut_chars(text, max);
    match head.rfind(['.', '!', '?']) {
        Some(i) if i > 0 => head[..=i].to_owned(),
        _ => head,
    }
}

impl OrdinHost {
    pub const MODULE: &'static str = "ordin_host";

    /// `ModuleMigrations { module: "ordin_host", sqlite: [(1, "ordin_host_init", …), (2, "ordin_host_continue", …)],
    /// postgres: [] }`, registered by the program with the Journal's `MigrationRegistry` (X-14).
    // spec: ordin-desktop/components/OrdinHost#SA-RECON-07
    pub fn migrations() -> ordin_journal::ModuleMigrations {
        ordin_journal::ModuleMigrations {
            module: Self::MODULE,
            sqlite: &[
                (1, "ordin_host_init", include_str!("sql/sqlite_v1.sql")),
                (2, "ordin_host_continue", include_str!("sql/sqlite_v2.sql")),
            ],
            postgres: &[],
        }
    }

    /// Builds the host; the migrations have already been applied by the program.
    pub fn new(
        config: OrdinHostConfig,
        policy: OrdinHostPolicy,
        deps: OrdinHostDeps,
    ) -> Result<Arc<Self>, OhError> {
        let pool = deps
            .datastore
            .sqlite()
            .cloned()
            .ok_or(OhError::DatastoreUnavailable)?;
        let ports = Ports {
            supervisor: Arc::new(RealSupervisor(deps.supervisor)),
            orchestrator: Arc::new(RealOrchestrator(deps.orchestrator)),
            inspector: Arc::new(RealInspector(deps.inspector)),
            usage: Arc::new(RealUsage(deps.usage)),
            workspaces: Arc::new(RealWorkspaces(deps.workspaces)),
            journal: Arc::new(RealJournal(deps.journal)),
            catalogue: Arc::new(RealCatalogue(deps.catalogue)),
            memory: Arc::new(RealMemory(deps.memory)),
            knowledge: Arc::new(RealKnowledge(deps.knowledge)),
            push: Arc::new(RealPush(deps.push)),
            projects: Arc::new(RealProjects(deps.projects)),
            files: Arc::new(RealFiles(deps.files)),
            localisation: Arc::new(RealLocalisation(deps.localisation)),
            blocks: Arc::new(RealBlocks {
                check: deps.blocks,
                refs: deps.refs,
            }),
        };
        Ok(Self::build(config, policy, ports, pool))
    }

    /// The same host over stand-in ports (this crate's tests and the `testkit` feature).
    #[cfg(any(test, feature = "testkit"))]
    pub fn with_ports(
        config: OrdinHostConfig,
        policy: OrdinHostPolicy,
        ports: Ports,
        pool: SqlitePool,
    ) -> Arc<Self> {
        Self::build(config, policy, ports, pool)
    }

    fn build(
        config: OrdinHostConfig,
        policy: OrdinHostPolicy,
        ports: Ports,
        pool: SqlitePool,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me| OrdinHost {
            me: me.clone(),
            config,
            policy: RwLock::new(policy),
            ports,
            pool,
            state: Mutex::new(State::default()),
            trigger_desk: RwLock::new(None),
            #[cfg(feature = "minutes")]
            minutes_port: RwLock::new(None),
            background: CancellationToken::new(),
            started_at: Timestamp::now(),
        })
    }

    pub(crate) fn arc(&self) -> Option<Arc<OrdinHost>> {
        self.me.upgrade()
    }

    pub(crate) fn policy(&self) -> OrdinHostPolicy {
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Replaces the policy; a lowered maximum applies to the next charter, check and import.
    pub fn set_policy(&self, policy: OrdinHostPolicy) {
        *self.policy.write().unwrap_or_else(|p| p.into_inner()) = policy;
        // A profile refresh writes company voice values (step 36).
        if let Some(me) = self.arc() {
            tokio::spawn(async move {
                let _ = me.apply_company_voice().await;
            });
        }
    }

    /// Personal profile only: the Trigger Service's desk. Until it is set, `propose_trigger` is not offered.
    pub fn set_trigger_desk(&self, desk: Arc<dyn TriggerDesk>) {
        *self.trigger_desk.write().unwrap_or_else(|p| p.into_inner()) = Some(desk);
    }

    pub(crate) fn trigger_desk(&self) -> Option<Arc<dyn TriggerDesk>> {
        self.trigger_desk
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// The `ToolProvider` the program registers with the MCP Front before `McpFront::start`.
    pub fn tool_provider(self: &Arc<Self>) -> Arc<dyn ordin_mcp_front::ToolProvider> {
        Arc::new(crate::tools::OrdinTools(Arc::downgrade(self)))
    }

    /// The `ShownBlockStore` the program binds with `McpFront::bind_block_store` before
    /// `McpFront::start` (OrdinHost.md amendment 2026-01-24).
    // spec: ordin-desktop/components/OrdinHost#SA-OH-BLOCK-01
    pub fn block_store(self: &Arc<Self>) -> Arc<dyn ordin_mcp_front::ShownBlockStore> {
        Arc::new(crate::conversation::KeptBlocks(Arc::downgrade(self)))
    }

    /// The `Coordinator` passed to the Crew Orchestrator when this crate starts a crew run.
    pub fn coordinator(self: &Arc<Self>) -> Arc<dyn ordin_crew_orchestrator::Coordinator> {
        Arc::new(crate::coordinator::OrdinCoordinator(Arc::downgrade(self)))
    }

    /// The Journal's `PruneParticipant` for this crate's history (X-14).
    pub fn prune_participant(self: &Arc<Self>) -> Arc<dyn ordin_journal::PruneParticipant> {
        Arc::new(Prune)
    }

    /// Starts background tasks (repository re-check, agent-directory change listener, share outcomes).
    pub async fn start(self: &Arc<Self>, shutdown: CancellationToken) -> Result<(), OhError> {
        let mut c = self.pool.acquire().await?;
        let waiting = store::waiting_blocks(&mut c, None, i64::MAX).await?;
        drop(c);
        for row in &waiting {
            self.mirror_waiting(row);
        }
        self.voice_settings().await?;
        let me = Arc::downgrade(self);
        let stop = shutdown.clone();
        let mut changes = self.ports.inspector.changes();
        tokio::spawn(async move {
            use futures::StreamExt;
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    next = changes.next() => {
                        let Some(workspace_id) = next else { break };
                        let Some(host) = me.upgrade() else { break };
                        let mut state = lock(&host.state);
                        if let Some(conv) = state.convs.get_mut(&workspace_id) {
                            conv.directory_changed = true;
                        }
                        drop(state);
                    }
                }
            }
        });
        crate::workflows::spawn_recheck(self, shutdown.clone());
        crate::workflows::spawn_share_listener(self, shutdown.clone());
        let background = self.background.clone();
        tokio::spawn(async move {
            shutdown.cancelled().await;
            background.cancel();
        });
        tracing::info!(component = COMPONENT, "ordin_host.started");
        Ok(())
    }

    /// The workspace's current Ordin conversation run; `None` when Ordin has none there.
    pub fn current_run(&self, workspace_id: WorkspaceId) -> Option<RunId> {
        lock(&self.state).convs.get(&workspace_id).map(|c| c.run_id)
    }

    /// Every question, form, crew-plan and offer block still waiting, oldest first, at most 500.
    /// Read from the in-memory mirror of `ordin_blocks.waiting`, loaded by `start`.
    pub fn waiting_blocks(&self) -> Vec<crate::types::WaitingBlock> {
        let state = lock(&self.state);
        let mut all: Vec<crate::types::WaitingBlock> = state.waiting.values().cloned().collect();
        all.sort_by(|a, b| {
            a.since
                .cmp(&b.since)
                .then(a.block_id.to_string().cmp(&b.block_id.to_string()))
        });
        all.truncate(WAITING_MAX as usize);
        all
    }

    pub(crate) fn mirror_waiting(&self, row: &BlockRow) {
        let mut state = lock(&self.state);
        if row.waiting {
            let (kind, title) = block_title(&row.block);
            state.waiting.insert(
                row.block_id,
                crate::types::WaitingBlock {
                    workspace_id: row.workspace_id,
                    block_id: row.block_id,
                    kind: kind.to_owned(),
                    title: cut_chars(&title, 200),
                    since: row.shown_at,
                },
            );
        } else {
            state.waiting.remove(&row.block_id);
        }
    }

    pub(crate) fn unmirror(&self, block_id: BlockId) {
        lock(&self.state).waiting.remove(&block_id);
    }

    // ---------------------------------------------------------------- events and blocks

    pub(crate) async fn emit(&self, kind: EventKind, data: serde_json::Value) {
        if let Err(e) = self.ports.journal.append_event(kind.as_str(), data).await {
            tracing::warn!(
                component = COMPONENT,
                code = e.code.as_str(),
                kind = kind.as_str(),
                "ordin_host.event_failed"
            );
        }
    }

    pub(crate) async fn needs_you_changed(&self, workspace_id: WorkspaceId) {
        self.emit(
            EventKind::NeedsYouChanged,
            serde_json::json!({ "workspace_id": workspace_id }),
        )
        .await;
    }

    pub(crate) async fn workspace_changed(&self, workspace_id: WorkspaceId) {
        self.emit(
            EventKind::WorkspaceChanged,
            serde_json::json!({ "workspace_id": workspace_id }),
        )
        .await;
    }

    /// Step 14: store a block of the workspace's current conversation and its message line, append
    /// `block_shown` (or `block_available` past the event limit) and `needs_you_changed` when waiting.
    pub(crate) async fn show_block(
        &self,
        workspace_id: WorkspaceId,
        author: MessageAuthor,
        block: Block,
        waiting: bool,
    ) -> Result<BlockId, OhError> {
        let conv = self.ensure_conversation(workspace_id).await?;
        let block_id = BlockId::new();
        let now = Timestamp::now();
        let (_, title) = block_title(&block);
        let text = cut_chars(
            if title.is_empty() { "…" } else { &title },
            MESSAGE_MAX_CHARS,
        );
        let row = BlockRow {
            block_id,
            conversation_id: conv,
            workspace_id,
            author,
            block: block.clone(),
            waiting,
            shown_at: now,
        };
        let mut tx = self.pool.begin().await?;
        store::insert_block(&mut tx, &row).await?;
        store::insert_message(&mut tx, conv, author, None, &text, Some(block_id), now).await?;
        tx.commit().await?;
        self.mirror_waiting(&row);
        let shown = ordin_service_api::ServiceEvent::BlockShown {
            workspace_id,
            block_id,
            block,
            author: Some(author),
        };
        let data = shown.to_journal_data();
        let size = serde_json::to_vec(&data)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        if size > ordin_service_api::MAX_EVENT_DATA_BYTES {
            self.emit(
                EventKind::BlockAvailable,
                serde_json::json!({ "workspace_id": workspace_id, "block_id": block_id }),
            )
            .await;
        } else {
            self.emit(EventKind::BlockShown, data).await;
        }
        if waiting {
            self.needs_you_changed(workspace_id).await;
        }
        Ok(block_id)
    }

    /// A fixed message block (no model turn).
    pub(crate) async fn show_message(
        &self,
        workspace_id: WorkspaceId,
        text: &str,
    ) -> Result<BlockId, OhError> {
        self.show_block(
            workspace_id,
            MessageAuthor::Ordin,
            Block::Message(MessageBlock {
                text: cut_chars(text, MESSAGE_MAX_CHARS),
                attachments: vec![],
                memory_used: vec![],
            }),
            false,
        )
        .await
    }

    /// A waiting question block, kept with what its answer acts on.
    pub(crate) async fn ask(
        &self,
        workspace_id: WorkspaceId,
        question: &str,
        choices: Vec<String>,
        kept: Option<Kept>,
    ) -> Result<BlockId, OhError> {
        let block = Block::Question(ordin_service_api::blocks::QuestionBlock {
            question: question.to_owned(),
            choices,
            allow_other: false,
        });
        let id = self
            .show_block(workspace_id, MessageAuthor::Ordin, block, true)
            .await?;
        if let Some(kept) = kept {
            lock(&self.state).kept.insert(id, (workspace_id, kept));
        }
        Ok(id)
    }

    // ---------------------------------------------------------------- voice (section I)

    pub(crate) async fn voice_settings(&self) -> Result<VoiceSettings, OhError> {
        let mut c = self.pool.acquire().await?;
        if let Some(v) = store::voice(&mut c).await? {
            return Ok(v);
        }
        let policy = self.policy();
        let v = VoiceSettings {
            tone: match policy.voice_defaults.tone {
                Some(t) => Setting {
                    value: t,
                    set_by: SetBy::Company,
                },
                None => Setting {
                    value: VoiceTone::Friendly,
                    set_by: SetBy::Person,
                },
            },
            register: match policy.voice_defaults.register {
                Some(r) => Setting {
                    value: r,
                    set_by: SetBy::Company,
                },
                None => Setting {
                    value: VoiceRegister::Everyday,
                    set_by: SetBy::Person,
                },
            },
            length: Setting {
                value: VoiceLength::Brief,
                set_by: SetBy::Person,
            },
            name_to_use: Setting {
                value: None,
                set_by: SetBy::Person,
            },
            notes: Setting {
                value: None,
                set_by: SetBy::Person,
            },
            updated_at: Timestamp::now(),
        };
        store::put_voice(&mut c, &v).await?;
        Ok(v)
    }

    /// Step 36: a profile that sets `voice.tone` or `voice.register` writes them as the company's; a
    /// value removed from the profile returns to the person keeping the value.
    pub(crate) async fn apply_company_voice(&self) -> Result<(), OhError> {
        let mut v = self.voice_settings().await?;
        let policy = self.policy();
        let before = v.clone();
        match policy.voice_defaults.tone {
            Some(t) => {
                v.tone = Setting {
                    value: t,
                    set_by: SetBy::Company,
                }
            }
            None => v.tone.set_by = SetBy::Person,
        }
        match policy.voice_defaults.register {
            Some(r) => {
                v.register = Setting {
                    value: r,
                    set_by: SetBy::Company,
                }
            }
            None => v.register.set_by = SetBy::Person,
        }
        if v != before {
            v.updated_at = Timestamp::now();
            let mut c = self.pool.acquire().await?;
            store::put_voice(&mut c, &v).await?;
            self.mark_voice_changed(false);
        }
        Ok(())
    }

    pub(crate) fn mark_voice_changed(&self, by_ordin: bool) {
        for conv in lock(&self.state).convs.values_mut() {
            if by_ordin {
                conv.voice_changed_by_ordin = true;
            } else {
                conv.voice_changed = true;
            }
        }
    }

    // ---------------------------------------------------------------- conversations (section B)

    /// The workspace's current conversation, attaching to its run or opening one (step 3).
    pub(crate) async fn ensure_conversation(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<ConversationId, OhError> {
        if let Some(conv) = lock(&self.state).convs.get(&workspace_id) {
            return Ok(conv.id);
        }
        let row = {
            let mut c = self.pool.acquire().await?;
            store::current_conversation(&mut c, workspace_id).await?
        };
        match row {
            None => {
                let (id, _) = self.open_new(workspace_id, Carry::default()).await?;
                Ok(id)
            }
            Some(row) => {
                // The run of a conversation from before a restart: re-attached when the Supervisor
                // still supervises it, else a new run seeded with the last summary (step 5).
                if let Ok(events) = self.ports.supervisor.subscribe(row.run_id) {
                    self.register(&row, events, false, None);
                    return Ok(row.id);
                }
                self.restart_run(workspace_id, &row).await?;
                Ok(row.id)
            }
        }
    }

    /// A new run for a conversation whose run ended without this crate (step 5).
    pub(crate) async fn restart_run(
        &self,
        workspace_id: WorkspaceId,
        row: &ConversationRow,
    ) -> Result<RunId, OhError> {
        let mut carry = Carry {
            summary: row.seed_summary.clone(),
            ..Carry::default()
        };
        // OH-RST-1: the conversation's own messages since it began, so a request that failed
        // before the restart can be tried again. The summary it began with is kept in the
        // history's opening, and a conversation that continues another quotes that one too.
        // spec: ordin-desktop/components/OrdinHost#OH-RST-1
        let (history, _) = self.continued_history(row.id).await?;
        carry.continued =
            Some(history.with_opening(seed::RESTART_OPENING, carry.summary.take().as_deref()));
        let (seed, _) = self.build_seed(workspace_id, &carry).await?;
        let run_id = self.start_ordin_run(workspace_id, seed, None).await?;
        let events =
            self.ports
                .supervisor
                .subscribe(run_id)
                .map_err(|e| OhError::SessionStartFailed {
                    cause: e.code.clone(),
                    retryable: e.retryable,
                })?;
        let mut c = self.pool.acquire().await?;
        store::set_conversation_run(&mut c, row.id, run_id).await?;
        let mut updated = row.clone();
        updated.run_id = run_id;
        // A run started after a restart has no stated locale: the next turn names it once.
        self.register(&updated, events, true, None);
        Ok(run_id)
    }

    /// Steps 3.1–3.3: the checks, the seed and the run. Nothing is stored.
    pub(crate) async fn start_ordin_run(
        &self,
        workspace_id: WorkspaceId,
        seed: String,
        only_servers: Option<Vec<String>>,
    ) -> Result<RunId, OhError> {
        let model = self
            .ports
            .usage
            .choose_talking(adapter_kind(self.config.harness));
        self.ports
            .supervisor
            .start_conversation(ConversationStart {
                workspace_id,
                target: RunTarget::AgentInstance {
                    agent_instance_id: self.config.ordin_agent_instance_id,
                },
                seed,
                limits: RunLimits {
                    wall_clock_secs: ORDIN_WALL_CLOCK_SECS,
                    request_budget: None,
                    idle_timeout_secs: ORDIN_IDLE_SECS,
                },
                model_override: model,
                only_servers,
            })
            .await
            .map_err(|e| OhError::SessionStartFailed {
                cause: e.code.clone(),
                retryable: e.retryable,
            })
    }

    /// Step 3.1: the workspace, the plan and Ordin's package.
    pub(crate) async fn may_open(&self, workspace_id: WorkspaceId) -> Result<(), OhError> {
        self.ports
            .workspaces
            .ensure_active(workspace_id)
            .await
            .map_err(|_| OhError::WorkspaceUnknown)?;
        self.ports
            .usage
            .may_start_ordin_work()
            .await
            .map_err(|e| OhError::DependencyFailed {
                dependency: "usage",
                code: e.code,
                retryable: true,
            })?;
        self.ports
            .catalogue
            .ensure_ready(self.config.ordin_agent_instance_id)
            .await
            .map_err(|e| OhError::dependency("catalogue", &e))?;
        Ok(())
    }

    /// Steps 3.1–3.4: opens a new current conversation (the workspace has none current).
    pub(crate) async fn open_new(
        &self,
        workspace_id: WorkspaceId,
        carry: Carry,
    ) -> Result<(ConversationId, RunId), OhError> {
        self.may_open(workspace_id).await?;
        let (seed, locale) = self.build_seed(workspace_id, &carry).await?;
        let run_id = self.start_ordin_run(workspace_id, seed, None).await?;
        let events = match self.ports.supervisor.subscribe(run_id) {
            Ok(events) => events,
            Err(e) => {
                self.cancel_logged(run_id).await;
                return Err(OhError::SessionStartFailed {
                    cause: e.code,
                    retryable: e.retryable,
                });
            }
        };
        let row = ConversationRow {
            id: ConversationId::new(),
            workspace_id,
            run_id,
            current: true,
            started_at: Timestamp::now(),
            ended_at: None,
            ended_by: None,
            seed_summary: carry.summary.clone().map(|s| cut_chars(&s, 16_000)),
            end_summary: None,
            continues_id: carry.continues,
        };
        let inserted = async {
            let mut tx = self.pool.begin().await?;
            store::insert_conversation(&mut tx, &row).await?;
            store::move_waiting(&mut tx, workspace_id, row.id).await?;
            tx.commit().await
        }
        .await;
        if let Err(e) = inserted {
            self.cancel_logged(run_id).await;
            return Err(e.into());
        }
        self.register(&row, events, true, Some(locale));
        self.log_started(&row);
        Ok((row.id, run_id))
    }

    pub(crate) fn log_started(&self, row: &ConversationRow) {
        tracing::info!(
            component = COMPONENT,
            workspace_id = row.workspace_id.to_string().as_str(),
            conversation_id = row.id.to_string().as_str(),
            run_id = row.run_id.to_string().as_str(),
            harness = crate::store::word(&self.config.harness).as_str(),
            level = "talking",
            "ordin_host.conversation_started"
        );
    }

    /// Holds the conversation in memory and starts its reader. `busy`: the seed turn is running.
    pub(crate) fn register(
        &self,
        row: &ConversationRow,
        events: tokio::sync::broadcast::Receiver<RunEvent>,
        busy: bool,
        stated_locale: Option<String>,
    ) {
        let conv = Conv {
            id: row.id,
            run_id: row.run_id,
            busy,
            queue: VecDeque::new(),
            in_flight: None,
            stated_locale,
            turns_since_summary: 0,
            recalls: vec![],
            slack: None,
            persons_turn: false,
            seed_turn: busy,
            quiet_turn: false,
            access_asked: std::collections::HashSet::new(),
            persons_text: String::new(),
            attached_places: Vec::new(),
            attached_files: Vec::new(),
            folder_asked: std::collections::HashSet::new(),
            breaking: false,
            directory_changed: false,
            changed_named: vec![],
            finished_tasks: Vec::new(),
            voice_changed: false,
            voice_changed_by_ordin: false,
            pending: None,
            own_facts: vec![],
            consolidating: HashMap::new(),
            last_message_at: row.started_at,
            ended_by_us: false,
            working_since: None,
        };
        let previous = lock(&self.state).convs.insert(row.workspace_id, conv);
        if let Some(previous) = previous {
            if let Some(w) = previous.in_flight {
                let _ = w.send(TurnEnd::Lost);
            }
            for t in previous.queue {
                if let Some(w) = t.waiter {
                    let _ = w.send(TurnEnd::Lost);
                }
            }
        }
        let me = self.me.clone();
        let workspace_id = row.workspace_id;
        let run_id = row.run_id;
        let stop = self.background.clone();
        tokio::spawn(async move {
            read_run(me, workspace_id, run_id, events, stop).await;
        });
    }

    /// Steps 4 and 51: sends or queues one turn. Returns the position in the queue (0 = sent now).
    pub(crate) async fn send_turn(
        &self,
        workspace_id: WorkspaceId,
        turn: Turn,
    ) -> Result<u8, OhError> {
        // `Err((position, since))`: queued; `since` when the screens are told Ordin is working now.
        let ready: Result<Turn, (u8, Option<Timestamp>)> = {
            let mut state = lock(&self.state);
            let max = self.config.max_waiting_messages;
            let conv = state
                .convs
                .get_mut(&workspace_id)
                .ok_or(OhError::ConversationUnknown)?;
            if conv.busy {
                if conv.queue.len() >= max {
                    return Err(OhError::TooManyWaiting);
                }
                conv.queue.push_back(turn);
                let position = u8::try_from(conv.queue.len()).unwrap_or(u8::MAX);
                // OH-WK-1: a turn queued behind Ordin's seed, which the screens were not told of,
                // is already Ordin's work for the person.
                let since = if conv.working_since.is_none() {
                    let now = Timestamp::now();
                    conv.working_since = Some(now);
                    Some(now)
                } else {
                    None
                };
                Err((position, since))
            } else {
                conv.busy = true;
                Ok(turn)
            }
        };
        match ready {
            Ok(turn) => {
                self.dispatch(workspace_id, turn).await;
                Ok(0)
            }
            Err((position, since)) => {
                if let Some(since) = since {
                    self.turn_changed(workspace_id, OrdinTurnState::Working, None, since, None)
                        .await;
                }
                Ok(position)
            }
        }
    }

    /// OH-WK-1: appends `ordin_turn_changed` for the workspace. Emitting never fails the turn; a failed
    /// emit is logged at `debug`.
    // spec: ordin-desktop/components/OrdinHost#OH-WK-1
    pub(crate) async fn turn_changed(
        &self,
        workspace_id: WorkspaceId,
        state: OrdinTurnState,
        doing: Option<String>,
        since: Timestamp,
        failure: Option<OrdinTurnFailure>,
    ) {
        let event = ordin_service_api::ServiceEvent::OrdinTurnChanged {
            workspace_id,
            state,
            doing: doing
                .map(|d| cut_chars(d.trim(), DOING_MAX_CHARS))
                .filter(|d| !d.is_empty()),
            since,
            failure,
        };
        if let Err(e) = self
            .ports
            .journal
            .append_event(
                EventKind::OrdinTurnChanged.as_str(),
                event.to_journal_data(),
            )
            .await
        {
            tracing::debug!(
                component = COMPONENT,
                code = e.code.as_str(),
                "ordin_host.turn_event_failed"
            );
        }
    }

    /// OH-WK-1: Ordin called one of its own tools during the turn — `working` again with what it is
    /// doing (none for a tool with no phrase), keeping the turn's first `since`.
    // spec: ordin-desktop/components/OrdinHost#OH-WK-1
    pub(crate) async fn working_on(&self, workspace_id: WorkspaceId, doing: Option<LocMessage>) {
        let since = lock(&self.state)
            .convs
            .get(&workspace_id)
            .and_then(|c| c.working_since);
        let Some(since) = since else { return };
        let doing = doing.map(|m| self.ports.localisation.message(&m));
        self.turn_changed(workspace_id, OrdinTurnState::Working, doing, since, None)
            .await;
    }

    /// OH-WK-1: `idle` when the turn the screens were told of has ended. OH-WK-2: `failure` is the
    /// word logged on `ordin_host.turn_ended_without_answer`, or none for a turn that answered.
    // spec: ordin-desktop/components/OrdinHost#OH-WK-2
    async fn turn_idle(
        &self,
        workspace_id: WorkspaceId,
        was_working: bool,
        failure: Option<OrdinTurnFailure>,
    ) {
        if was_working {
            self.turn_changed(
                workspace_id,
                OrdinTurnState::Idle,
                None,
                Timestamp::now(),
                failure,
            )
            .await;
        }
    }

    /// The prefix lines of step 4 (agent directory, voice, language), then the text.
    async fn compose(&self, workspace_id: WorkspaceId, text: &str) -> String {
        let (directory, voice, by_ordin, stated) = {
            let state = lock(&self.state);
            match state.convs.get(&workspace_id) {
                Some(c) => (
                    c.directory_changed,
                    c.voice_changed,
                    c.voice_changed_by_ordin,
                    c.stated_locale.clone(),
                ),
                None => (false, false, false, None),
            }
        };
        let mut lines = Vec::new();
        let directory_line = if directory {
            let entries = self.directory_names(workspace_id).await;
            Some(cut_at_line(
                &format!("Agent directory changed:\n{entries}"),
                DIRECTORY_PREFIX_MAX_CHARS,
            ))
        } else {
            None
        };
        let changed_line = self.changed_documents_line(workspace_id).await;
        // OH-RC-2: what a task that completed since the last turn said, once.
        // spec: ordin-desktop/components/OrdinHost#OH-RC-2
        let finished_line = {
            let mut state = lock(&self.state);
            state
                .convs
                .get_mut(&workspace_id)
                .filter(|c| !c.finished_tasks.is_empty())
                .map(|c| {
                    let tasks = std::mem::take(&mut c.finished_tasks);
                    let entries: Vec<String> = tasks
                        .iter()
                        .map(|(name, answer)| format!("- {name} said:\n{answer}"))
                        .collect();
                    format!(
                        "Tasks finished since your last turn (the person has seen these answers; use them if asked):\n{}",
                        entries.join("\n")
                    )
                })
        };
        if voice && !by_ordin {
            if let Ok(v) = self.voice_settings().await {
                lines.push(format!("Voice changed: {}", seed::voice_words(&v)));
            }
        }
        // OH-LG-2: one line naming the latest tag only.
        let active = self.ports.localisation.active_locale();
        if stated.as_deref() != Some(active.as_str()) {
            lines.push(format!(
                "Language changed: {active}. Write in {active} from now on."
            ));
        }
        {
            let mut state = lock(&self.state);
            if let Some(c) = state.convs.get_mut(&workspace_id) {
                c.directory_changed = false;
                c.voice_changed = false;
                c.voice_changed_by_ordin = false;
                c.stated_locale = Some(active);
            }
        }
        let lines: Vec<String> = directory_line
            .into_iter()
            .chain(changed_line)
            .chain(finished_line)
            .chain(
                lines
                    .iter()
                    .map(|line| cut_chars(line, PREFIX_LINE_MAX_CHARS)),
            )
            .collect();
        let composed = if lines.is_empty() {
            text.to_owned()
        } else {
            format!("{}\n{text}", lines.join("\n"))
        };
        // OH-ANS-3: a turn over the answer's bound is cut, never refused; a person's turn fits.
        // spec: ordin-desktop/components/OrdinHost#OH-ANS-3
        cut_chars(&composed, ordin_runtime_adapter::MAX_ANSWER_CHARS)
    }

    /// OH-RC-2: remembers what a completed task said, to go with Ordin's next turn. Only the newest
    /// `FINISHED_TASKS_MAX` answers are kept, each cut to `FINISHED_TASK_MAX_CHARS`.
    // spec: ordin-desktop/components/OrdinHost#OH-RC-2
    pub(crate) fn note_finished_task(&self, workspace_id: WorkspaceId, name: &str, answer: &str) {
        let mut state = lock(&self.state);
        if let Some(conv) = state.convs.get_mut(&workspace_id) {
            conv.finished_tasks
                .push((name.to_owned(), cut_chars(answer, FINISHED_TASK_MAX_CHARS)));
            while conv.finished_tasks.len() > FINISHED_TASKS_MAX {
                conv.finished_tasks.remove(0);
            }
        }
    }

    /// OH-WC-1: "Linked documents changed, not sent back:" with one line per changed working-copy item
    /// (its path, its link and its id, the one `propose_push` takes), when the set differs from the one the
    /// last line named. A failed read names nothing and keeps the last set.
    // spec: ordin-desktop/components/OrdinHost#OH-WC-1
    async fn changed_documents_line(&self, workspace_id: WorkspaceId) -> Option<String> {
        let mut changed = match self.ports.workspaces.changed_documents(workspace_id).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.changed_documents_not_read"
                );
                return None;
            }
        };
        changed.sort_by_key(|d| d.item);
        let ids: Vec<_> = changed.iter().map(|d| d.item).collect();
        {
            let mut state = lock(&self.state);
            let conv = state.convs.get_mut(&workspace_id)?;
            if conv.changed_named == ids {
                return None;
            }
            conv.changed_named = ids;
        }
        if changed.is_empty() {
            return None;
        }
        let entries: Vec<String> = changed
            .iter()
            .map(|d| format!("- {} in {} (item {})", d.path, d.link, d.item))
            .collect();
        Some(cut_at_line(
            &format!(
                "Linked documents changed, not sent back (propose_push takes the item):\n{}",
                entries.join("\n")
            ),
            DIRECTORY_PREFIX_MAX_CHARS,
        ))
    }

    /// OH-DIR-1: the directory as the seed lists it — each agent's name, key and description — so Ordin
    /// can give work to an agent added during the conversation. Names alone left it no key for
    /// `delegate` (found on Windows after **Add it**).
    // spec: ordin-desktop/components/OrdinHost#OH-DIR-1
    async fn directory_names(&self, workspace_id: WorkspaceId) -> String {
        match self.ports.inspector.directory(workspace_id).await {
            Ok(agents) => {
                let claude = self.config.harness == ordin_types::domain::HarnessKind::ClaudeCode;
                crate::seed::directory_text(&agents, claude)
                    .trim_end()
                    .to_owned()
            }
            Err(e) => {
                log_directory_not_read(workspace_id, &e.code);
                String::new()
            }
        }
    }

    /// Sends a turn now (the conversation is marked busy by the caller).
    async fn dispatch(&self, workspace_id: WorkspaceId, turn: Turn) {
        let text = self.compose(workspace_id, &turn.text).await;
        let target = {
            let mut state = lock(&self.state);
            match state.convs.get_mut(&workspace_id) {
                Some(conv) => {
                    conv.in_flight = turn.waiter;
                    conv.recalls.clear();
                    conv.slack = turn.slack;
                    conv.persons_turn = turn.persons;
                    conv.seed_turn = false;
                    conv.quiet_turn = turn.quiet;
                    conv.working_since = Some(Timestamp::now());
                    Some((conv.run_id, conv.working_since))
                }
                None => {
                    if let Some(w) = turn.waiter {
                        let _ = w.send(TurnEnd::Lost);
                    }
                    None
                }
            }
        };
        let Some((run_id, since)) = target else {
            return;
        };
        // OH-WK-1: `working` with `since` = now for every turn sent to Ordin's run.
        if let Some(since) = since {
            self.turn_changed(workspace_id, OrdinTurnState::Working, None, since, None)
                .await;
        }
        if let Err(e) = self
            .ports
            .supervisor
            .resume_with_decision(run_id, ResumeDecision::Answer { text })
            .await
        {
            tracing::warn!(
                component = COMPONENT,
                workspace_id = workspace_id.to_string().as_str(),
                run_id = run_id.to_string().as_str(),
                code = e.code.as_str(),
                "ordin_host.turn_failed"
            );
            let was_working = self.turn_lost(workspace_id, run_id);
            self.turn_idle(workspace_id, was_working, Some(OrdinTurnFailure::NotSent))
                .await;
        }
    }

    /// Sends a fixed turn and waits for Ordin's text.
    pub(crate) async fn run_turn(
        &self,
        workspace_id: WorkspaceId,
        text: String,
    ) -> Result<TurnEnd, OhError> {
        self.run_turn_as(workspace_id, text, false).await
    }

    /// OH-SUM-1: a turn whose reply goes only to the caller and is never shown to the person.
    pub(crate) async fn run_quiet_turn(
        &self,
        workspace_id: WorkspaceId,
        text: String,
    ) -> Result<TurnEnd, OhError> {
        self.run_turn_as(workspace_id, text, true).await
    }

    async fn run_turn_as(
        &self,
        workspace_id: WorkspaceId,
        text: String,
        quiet: bool,
    ) -> Result<TurnEnd, OhError> {
        let (tx, rx) = oneshot::channel();
        self.send_turn(
            workspace_id,
            Turn {
                text,
                waiter: Some(tx),
                slack: None,
                persons: false,
                quiet,
            },
        )
        .await?;
        Ok(rx.await.unwrap_or(TurnEnd::Lost))
    }

    /// A turn's text arrived (step 5).
    async fn turn_ended(&self, workspace_id: WorkspaceId, run_id: RunId, text: String) {
        let taken = {
            let mut state = lock(&self.state);
            match state.convs.get_mut(&workspace_id) {
                Some(conv) if conv.run_id == run_id => {
                    let waiter = conv.in_flight.take();
                    let recalls: Vec<MemoryRecallId> = if text.trim().is_empty() {
                        Vec::new()
                    } else {
                        std::mem::take(&mut conv.recalls)
                    };
                    conv.slack = None;
                    conv.turns_since_summary += 1;
                    let seed = std::mem::replace(&mut conv.seed_turn, false);
                    let quiet = std::mem::replace(&mut conv.quiet_turn, false);
                    let since = conv.working_since;
                    Some((
                        waiter,
                        recalls,
                        conv.turns_since_summary,
                        seed,
                        quiet,
                        since,
                    ))
                }
                _ => None,
            }
        };
        let Some((waiter, recalls, turns, seed, quiet, since)) = taken else {
            return;
        };
        // A turn the person waits on that came back with no text: the screens show no answer.
        let failure =
            (!seed && !quiet && text.trim().is_empty()).then_some(OrdinTurnFailure::EmptyReply);
        if let Some(failure) = failure {
            log_turn_without_answer(workspace_id, run_id, failure.as_str(), since, 0);
        }
        // OH-SEED-1: the reply to the seed is Ordin reading its instructions, not an answer to the person.
        // spec: ordin-desktop/components/OrdinHost#OH-SEED-1
        if seed {
            tracing::debug!(
                component = COMPONENT,
                workspace_id = workspace_id.to_string().as_str(),
                "ordin_host.seed_reply_not_shown"
            );
        }
        // OH-SUM-1: the summary of step 32 is Ordin writing for its next session, not an answer.
        // spec: ordin-desktop/components/OrdinHost#OH-SUM-1
        if quiet {
            tracing::debug!(
                component = COMPONENT,
                workspace_id = workspace_id.to_string().as_str(),
                "ordin_host.quiet_reply_not_shown"
            );
        }
        if !seed && !quiet && !text.trim().is_empty() {
            let dropped = recalls.len().saturating_sub(20);
            let memory_used: Vec<MemoryRecallId> = recalls.into_iter().take(20).collect();
            if !memory_used.is_empty() {
                tracing::info!(
                    component = COMPONENT,
                    workspace_id = workspace_id.to_string().as_str(),
                    recalls = memory_used.len(),
                    dropped,
                    "ordin_host.memory_used"
                );
            }
            let block = Block::Message(MessageBlock {
                text: cut_sentence(&text, MESSAGE_MAX_CHARS),
                attachments: vec![],
                memory_used,
            });
            if let Err(e) = self
                .show_block(workspace_id, MessageAuthor::Ordin, block, false)
                .await
            {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code().as_str(),
                    "ordin_host.reply_not_stored"
                );
            }
        }
        // The conversation is free only once Ordin's reply is stored, so a reader that waits for
        // the turn's end finds the reply.
        let mut was_working = false;
        if let Some(conv) = lock(&self.state).convs.get_mut(&workspace_id) {
            if conv.run_id == run_id {
                conv.busy = false;
                // OH-WK-1: idle when no turn waits; a waiting turn is sent at once with its own
                // `working`.
                if conv.queue.is_empty() {
                    was_working = conv.working_since.take().is_some();
                }
            }
        }
        self.turn_idle(workspace_id, was_working, failure).await;
        if let Some(w) = waiter {
            let _ = w.send(TurnEnd::Text(text));
        }
        // SA-CLOSE-07: C-29 unconfirmed → the planner decides on the turn count alone.
        let context = self
            .ports
            .supervisor
            .context_used_percent(run_id)
            .map(|p| u32::from(p) * 10);
        let summarise = self.ports.usage.should_summarise(context, turns);
        self.next_turn(workspace_id).await;
        if summarise {
            if let Some(me) = self.arc() {
                tokio::spawn(async move {
                    let _ = me
                        .summarise_as(workspace_id, ConversationEnd::ContextLimit)
                        .await;
                });
            }
        }
    }

    async fn next_turn(&self, workspace_id: WorkspaceId) {
        let next = {
            let mut state = lock(&self.state);
            match state.convs.get_mut(&workspace_id) {
                Some(conv) if !conv.busy => conv.queue.pop_front().inspect(|_| {
                    conv.busy = true;
                }),
                _ => None,
            }
        };
        if let Some(turn) = next {
            Box::pin(self.dispatch(workspace_id, turn)).await;
        }
    }

    /// Ordin's run ended or a turn could not be sent: every waiter learns it. True when the screens
    /// had been told Ordin was working (OH-WK-1).
    fn turn_lost(&self, workspace_id: WorkspaceId, run_id: RunId) -> bool {
        let mut state = lock(&self.state);
        if let Some(conv) = state.convs.get_mut(&workspace_id) {
            if conv.run_id == run_id {
                conv.busy = false;
                let waiter = conv.in_flight.take();
                // OH-WK-2: the same word the idle event carries, whether a caller or the person waits.
                // spec: ordin-desktop/components/OrdinHost#OH-WK-2
                if waiter.is_some() || conv.working_since.is_some() {
                    log_turn_without_answer(
                        workspace_id,
                        run_id,
                        OrdinTurnFailure::NotSent.as_str(),
                        conv.working_since,
                        0,
                    );
                }
                if let Some(w) = waiter {
                    let _ = w.send(TurnEnd::Lost);
                }
                return conv.working_since.take().is_some();
            }
        }
        false
    }

    /// An `Ended` of Ordin's run not caused by this crate: the conversation stays current and the
    /// next message opens a new run (step 5).
    /// `why` is the end's code: a `RunEndReason` in snake case, `stopped_by_host` or `events_closed`.
    async fn run_ended(&self, workspace_id: WorkspaceId, run_id: RunId, why: &'static str) {
        let removed = {
            let mut state = lock(&self.state);
            let removed = match state.convs.get(&workspace_id) {
                Some(conv) if conv.run_id == run_id => state.convs.remove(&workspace_id),
                _ => None,
            };
            if let Some(conv) = &removed {
                if !conv.own_facts.is_empty() {
                    state.ended_facts.insert(run_id, conv.own_facts.clone());
                }
            }
            removed
        };
        if let Some(conv) = removed {
            // OH-WK-2: the run's own end reason stays in the log line only.
            self.turn_idle(
                workspace_id,
                conv.working_since.is_some(),
                Some(OrdinTurnFailure::RunEnded),
            )
            .await;
            if !conv.ended_by_us {
                tracing::warn!(
                    component = COMPONENT,
                    workspace_id = workspace_id.to_string().as_str(),
                    run_id = run_id.to_string().as_str(),
                    reason = why,
                    "ordin_host.session_lost"
                );
            }
            let waiting = conv.queue.iter().filter(|t| t.waiter.is_some()).count();
            if conv.in_flight.is_some() || waiting > 0 {
                log_turn_without_answer(workspace_id, run_id, why, conv.working_since, waiting);
            }
            if let Some(w) = conv.in_flight {
                let _ = w.send(TurnEnd::Lost);
            }
            for t in conv.queue {
                if let Some(w) = t.waiter {
                    let _ = w.send(TurnEnd::Lost);
                }
            }
        }
    }

    /// Stops the current conversation's run as this crate's own act (summary, Clear, continue, removal).
    pub(crate) async fn stop_current_run(&self, workspace_id: WorkspaceId) -> Option<RunId> {
        let run = {
            let mut state = lock(&self.state);
            state.convs.get_mut(&workspace_id).map(|c| {
                c.ended_by_us = true;
                c.run_id
            })
        };
        if let Some(run_id) = run {
            self.cancel_logged(run_id).await;
            self.run_ended(workspace_id, run_id, "stopped_by_host")
                .await;
        }
        run
    }

    /// The conversation of a workspace, as held in memory, for the tools.
    pub(crate) fn conv_run(&self, workspace_id: WorkspaceId) -> Option<(ConversationId, RunId)> {
        lock(&self.state)
            .convs
            .get(&workspace_id)
            .map(|c| (c.id, c.run_id))
    }

    /// Cancels a run of this crate's own; a refused cancel is logged with its code and otherwise
    /// ignored, as before.
    pub(crate) async fn cancel_logged(&self, run_id: RunId) {
        if let Err(e) = self.ports.supervisor.cancel(run_id).await {
            log_cancel_failed(run_id, &e.code);
        }
    }

    pub(crate) fn dep(&self, dependency: &'static str, e: DepError) -> OhError {
        tracing::warn!(
            component = COMPONENT,
            dependency,
            code = e.code.as_str(),
            "ordin_host.dependency_failed"
        );
        OhError::DependencyFailed {
            dependency,
            code: e.code,
            retryable: e.retryable,
        }
    }
}

/// What a new conversation's seed carries beyond the workspace (step 3.2 (5)).
#[derive(Debug, Clone, Default)]
pub(crate) struct Carry {
    pub summary: Option<String>,
    /// The continued history of step 48.5.
    pub continued: Option<seed::ContinuedHistory>,
    pub continues: Option<ConversationId>,
}

pub(crate) fn adapter_kind(harness: ordin_types::domain::HarnessKind) -> AdapterKind {
    match harness {
        ordin_types::domain::HarnessKind::ClaudeCode => AdapterKind::ClaudeLocal,
        ordin_types::domain::HarnessKind::CopilotCli => AdapterKind::CopilotLocal,
    }
}

/// `(kind, title)` of a block: the kind identifier and a line naming it.
pub(crate) fn block_title(block: &Block) -> (&'static str, String) {
    match block {
        Block::Message(m) => ("message", m.text.clone()),
        Block::Question(q) => ("question", q.question.clone()),
        Block::Form(f) => ("form", f.title.clone()),
        Block::CrewPlan(_) => ("crew_plan", "Crew plan".to_owned()),
        Block::AgentProposal(p) => ("agent_proposal", p.agent.clone()),
        Block::Access(a) => ("access", a.text.clone()),
        Block::Result(ordin_service_api::blocks::ResultBlock::Offer { offer, .. }) => (
            "offer",
            match offer {
                ordin_service_api::blocks::ResultOffer::SaveAsWorkflow => {
                    "Save as a workflow".to_owned()
                }
                ordin_service_api::blocks::ResultOffer::UpdateTheWorkflow => {
                    "Update the workflow".to_owned()
                }
            },
        ),
        Block::Result(_) => ("result", "Result".to_owned()),
        Block::SlackList(_) => ("slack_list", "Slack messages".to_owned()),
        Block::Table(t) => ("table", t.title.clone()),
        Block::Chart(c) => ("chart", c.title.clone()),
        Block::Report(r) => ("report", r.title.clone()),
        _ => ("block", "Shown".to_owned()),
    }
}

/// The reader of one Ordin run (step 5).
async fn read_run(
    me: Weak<OrdinHost>,
    workspace_id: WorkspaceId,
    run_id: RunId,
    mut events: tokio::sync::broadcast::Receiver<RunEvent>,
    stop: CancellationToken,
) {
    use tokio::sync::broadcast::error::RecvError;
    loop {
        let event = tokio::select! {
            _ = stop.cancelled() => return,
            e = events.recv() => e,
        };
        let Some(host) = me.upgrade() else { return };
        match event {
            Ok(RunEvent::Paused {
                reason: PauseReason::Person { question },
            }) => host.turn_ended(workspace_id, run_id, question).await,
            Ok(RunEvent::Ended { end }) => {
                let RunEnd { reason, .. } = end;
                host.run_ended(workspace_id, run_id, end_reason_code(reason))
                    .await;
                return;
            }
            Ok(_) => {}
            // Events were dropped, possibly the pause carrying Ordin's answer.
            Err(RecvError::Lagged(skipped)) => log_events_lagged(workspace_id, run_id, skipped),
            Err(RecvError::Closed) => {
                host.run_ended(workspace_id, run_id, "events_closed").await;
                return;
            }
        }
    }
}

/// Why a waited turn gave no text, for the log: `timeout`, `not_sent` (with the error's code
/// logged where it was raised) or `lost`. Empty for a turn that answered.
pub(crate) fn turn_failure<E>(
    ended: &Result<Result<TurnEnd, E>, tokio::time::error::Elapsed>,
) -> &'static str {
    match ended {
        Err(_) => "timeout",
        Ok(Err(_)) => "not_sent",
        Ok(Ok(TurnEnd::Lost)) => "lost",
        Ok(Ok(TurnEnd::Text(_))) => "",
    }
}

/// A run's end reason as its snake-case code, for the log.
fn end_reason_code(reason: RunEndReason) -> &'static str {
    match reason {
        RunEndReason::Completed => "completed",
        RunEndReason::Failed => "failed",
        RunEndReason::StoppedAtLimit => "stopped_at_limit",
        RunEndReason::Cancelled => "cancelled",
        RunEndReason::Interrupted => "interrupted",
        RunEndReason::Abandoned => "abandoned",
        RunEndReason::NoReply => "no_reply",
    }
}

/// Milliseconds since `since`; 0 when the turn had no start recorded.
fn elapsed_ms(since: Option<Timestamp>) -> u64 {
    since.map_or(0, |s| {
        u64::try_from(Timestamp::now().unix_millis() - s.unix_millis()).unwrap_or(0)
    })
}

/// The one line for a turn that ends without an answer: the run, the end's code, how long the turn
/// ran and how many queued turns were lost with it. Never the turn's text.
fn log_turn_without_answer(
    workspace_id: WorkspaceId,
    run_id: RunId,
    reason: &'static str,
    since: Option<Timestamp>,
    waiting: usize,
) {
    tracing::warn!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        run_id = run_id.to_string().as_str(),
        reason,
        elapsed_ms = elapsed_ms(since),
        waiting = waiting as u64,
        "ordin_host.turn_ended_without_answer"
    );
}

fn log_directory_not_read(workspace_id: WorkspaceId, code: &str) {
    tracing::warn!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        code,
        "ordin_host.directory_not_read"
    );
}

fn log_cancel_failed(run_id: RunId, code: &str) {
    tracing::warn!(
        component = COMPONENT,
        run_id = run_id.to_string().as_str(),
        code,
        "ordin_host.cancel_failed"
    );
}

/// The reader of Ordin's run fell behind and `skipped` events were dropped, possibly the pause
/// that carried Ordin's answer.
fn log_events_lagged(workspace_id: WorkspaceId, run_id: RunId, skipped: u64) {
    tracing::warn!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        run_id = run_id.to_string().as_str(),
        skipped,
        "ordin_host.run_events_lagged"
    );
}

/// The conversation summary turn gave no text; `reason` as [`turn_failure`] gives it, or
/// `empty_reply`.
pub(crate) fn log_summary_not_written(workspace_id: WorkspaceId, reason: &'static str) {
    tracing::warn!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        reason,
        "ordin_host.summary_not_written"
    );
}

/// The turn that tells Ordin a crew ended gave no text.
pub(crate) fn log_crew_end_turn_unanswered(
    crew_run_id: &str,
    reason: &'static str,
    elapsed_ms: u64,
) {
    tracing::warn!(
        component = COMPONENT,
        crew_run_id,
        reason,
        elapsed_ms,
        "ordin_host.crew_end_turn_unanswered"
    );
}

/// The prune participant (step 1).
struct Prune;

#[async_trait::async_trait]
impl ordin_journal::PruneParticipant for Prune {
    fn module(&self) -> &'static str {
        OrdinHost::MODULE
    }

    // spec: ordin-desktop/components/OrdinHost#SA-RECON-07
    async fn prune(
        &self,
        store: &ordin_journal::Datastore,
        request: &ordin_journal::PruneRequest,
    ) -> Result<u64, ordin_journal::PruneFailure> {
        let failed = ordin_journal::PruneFailure {
            code: "OH_DATASTORE_UNAVAILABLE",
        };
        let pool = store.sqlite().ok_or(failed.clone())?;
        let mut tx = pool.begin().await.map_err(|_| failed.clone())?;
        let n = store::prune(&mut tx, request.cutoff, &request.runs_in_progress)
            .await
            .map_err(|_| failed.clone())?;
        tx.commit().await.map_err(|_| failed)?;
        Ok(n)
    }
}

/// Waits at most `within` for a turn; `None` on timeout.
pub(crate) async fn within<T>(
    within: Duration,
    f: impl std::future::Future<Output = T>,
) -> Option<T> {
    tokio::time::timeout(within, f).await.ok()
}

/// `text` cut to at most `max` characters at the end of a whole line (OH-DIR-1).
pub(crate) fn cut_at_line(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut = cut_chars(text, max);
    match cut.rfind('\n') {
        Some(at) => cut[..at].to_owned(),
        None => cut,
    }
}

#[cfg(test)]
mod failure_log_tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lines").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// One failure point: its event, a field it must carry, and the call that logs it.
    type Case<'a> = (&'static str, &'a str, Box<dyn Fn() + 'a>);

    /// The lines `f` writes, in ordin's own log format.
    fn logged(f: impl FnOnce()) -> String {
        let lines = Lines::default();
        let writer = lines.clone();
        let subscriber = ordin_types::log::subscriber(move || writer.clone(), tracing::Level::INFO);
        tracing::subscriber::with_default(subscriber, f);
        let text = String::from_utf8(lines.0.lock().expect("lines").clone()).expect("utf-8");
        text
    }

    fn assert_readable(text: &str, event: &str) {
        assert!(text.contains(&format!(r#""event":"{event}""#)), "{text}");
        assert!(!text.contains("invalid_event_name"), "{text}");
        assert!(!text.contains("<debug>"), "{text}");
    }

    #[test]
    fn a_turn_without_an_answer_logs_its_run_reason_and_elapsed_time() {
        let ws = WorkspaceId::new();
        let run = RunId::new();
        let ten_seconds_ago =
            Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 10_000).expect("time");
        // (reason, since, waiting): every end a turn can meet.
        let cases: [(&'static str, Option<Timestamp>, usize); 5] = [
            ("empty_reply", Some(ten_seconds_ago), 0),
            ("not_sent", Some(ten_seconds_ago), 0),
            ("no_reply", Some(ten_seconds_ago), 2),
            ("events_closed", None, 0),
            ("stopped_by_host", Some(ten_seconds_ago), 1),
        ];
        for (reason, since, waiting) in cases {
            let text = logged(|| log_turn_without_answer(ws, run, reason, since, waiting));
            assert_readable(&text, "ordin_host.turn_ended_without_answer");
            assert!(text.contains(&format!(r#""reason":"{reason}""#)), "{text}");
            assert!(text.contains(&run.to_string()), "{text}");
            assert!(text.contains(&format!(r#""waiting":{waiting}"#)), "{text}");
            let line: serde_json::Value =
                serde_json::from_str(text.lines().next().expect("a line")).expect("json");
            let elapsed = line["elapsed_ms"].as_u64().expect("elapsed_ms is a number");
            match since {
                Some(_) => assert!(elapsed >= 10_000, "{text}"),
                None => assert_eq!(elapsed, 0, "{text}"),
            }
        }
    }

    #[test]
    fn every_end_reason_has_a_readable_code() {
        let cases = [
            (RunEndReason::Completed, "completed"),
            (RunEndReason::Failed, "failed"),
            (RunEndReason::StoppedAtLimit, "stopped_at_limit"),
            (RunEndReason::Cancelled, "cancelled"),
            (RunEndReason::Interrupted, "interrupted"),
            (RunEndReason::Abandoned, "abandoned"),
            (RunEndReason::NoReply, "no_reply"),
        ];
        for (reason, code) in cases {
            assert_eq!(end_reason_code(reason), code);
            // The code is the wire form of the reason.
            assert_eq!(
                serde_json::to_value(reason).expect("json"),
                serde_json::Value::String(code.to_owned())
            );
        }
    }

    #[tokio::test]
    async fn a_waited_turn_names_why_it_gave_no_text() {
        let timed_out = tokio::time::timeout(
            Duration::from_millis(1),
            std::future::pending::<Result<TurnEnd, OhError>>(),
        )
        .await;
        assert_eq!(turn_failure(&timed_out), "timeout");
        let cases: [(Result<TurnEnd, OhError>, &str); 3] = [
            (Err(OhError::ConversationUnknown), "not_sent"),
            (Ok(TurnEnd::Lost), "lost"),
            (Ok(TurnEnd::Text("hello there".into())), ""),
        ];
        for (end, reason) in cases {
            assert_eq!(turn_failure(&Ok(end)), reason);
        }
    }

    #[test]
    fn the_other_failure_points_log_codes_and_counts_only() {
        let ws = WorkspaceId::new();
        let run = RunId::new();
        let cases: [Case<'_>; 5] = [
            (
                "ordin_host.run_events_lagged",
                r#""skipped":7"#,
                Box::new(move || log_events_lagged(ws, run, 7)),
            ),
            (
                "ordin_host.cancel_failed",
                r#""code":"SUP_RUN_NOT_FOUND""#,
                Box::new(move || log_cancel_failed(run, "SUP_RUN_NOT_FOUND")),
            ),
            (
                "ordin_host.directory_not_read",
                r#""code":"AGT_UNAVAILABLE""#,
                Box::new(move || log_directory_not_read(ws, "AGT_UNAVAILABLE")),
            ),
            (
                "ordin_host.summary_not_written",
                r#""reason":"timeout""#,
                Box::new(move || log_summary_not_written(ws, "timeout")),
            ),
            (
                "ordin_host.crew_end_turn_unanswered",
                r#""elapsed_ms":1500"#,
                Box::new(|| log_crew_end_turn_unanswered("crew-1", "lost", 1_500)),
            ),
        ];
        for (event, field, f) in cases {
            let text = logged(f);
            assert_readable(&text, event);
            assert!(text.contains(field), "{event}: {text}");
        }
    }
}
