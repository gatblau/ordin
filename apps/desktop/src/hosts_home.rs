//! Program-side implementations for the block check and for Home and Search (OrdinDesktop.md
//! *Program-side implementations and calls*): the `RefResolver` bound with `bind_ref_resolver` and
//! passed as `OrdinHostDeps.refs` (row 3.41), the `JournalReader` composite of row 3.42 and the
//! three `SearchSource`s registered right after it. Each delegates to the module the spec names and
//! adds no decision; a module built later is read from [`Late`], and one not built yet answers as
//! "not found" (a reference) or leaves its rows out (Home).
//!
//! No build switch is on in this build: the Mail Service, Telegram Service, Slack Service, Handover
//! Service and Hosted Agent Adapter do not exist, so `email_message` answers false, a Telegram or
//! Slack `EntityRef` answers false, and *Needs you* has no handover rows and no approver cards. The
//! switched builds add them.
//
// Card words, rules lines, the reference checks and Home's rows follow OrdinDesktop.md, amendment
// January 2026, DSK-REC-14 to DSK-REC-20. A crew charter reference answers false (G-REC-1: no owner
// reads a charter by its id).

use std::sync::Arc;

use ordin_catalogue::CatalogueService;
use ordin_home_search::{
    GoToKind, GoToResult, HomeSourceError, JournalReader, OpenTarget, RunAgainSubject, RunSummary,
    SearchSource, WaitingItem,
};
use ordin_journal::{Journal, RunIndexEntry};
use ordin_knowledge::KnowledgeBlock;
use ordin_localisation::Localisation;
use ordin_service_api::blocks::{EntityRef, PlanState, RefResolver};
use ordin_session_supervisor::{RunView, SupervisorError};
use ordin_types::common::{AgentRef, InputUsed};
use ordin_types::domain::places::ArtefactLocation;
use ordin_types::domain::runs::{ExternalActionState, Run, RunOrigin, RunState};
use ordin_types::time::Timestamp;
use ordin_types::wire::ContentRef;
use ordin_types::{EmailLinkId, RunId, WorkspaceId};
use ordin_workspaces::WorkspaceListFilter;

use crate::hosts::Late;

/// The most pages of one listing read for Home (100 items a page), so that a broken cursor cannot
/// loop.
const MAX_PAGES: usize = 50;

/// The workspaces not in the bin, or none when the Workspace Service is not built yet.
async fn live_workspaces(late: &Late) -> Vec<WorkspaceId> {
    match late.workspaces.get() {
        Some(workspaces) => workspaces
            .list(WorkspaceListFilter::NotInBin)
            .await
            .map(|all| all.into_iter().map(|w| w.id).collect())
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

/// The display name of the agent of `run`: the Package Catalogue's for an agent instance, the
/// Organisation Profile's for a remote agent; any error → `None` (DSK-REC-15).
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-15
async fn agent_name(late: &Late, run: &Run) -> Option<String> {
    match run.agent {
        AgentRef::AgentInstance { agent_instance_id } => {
            let packages = late.packages.get()?;
            packages
                .get(agent_instance_id)
                .await
                .ok()
                .map(|agent| agent.instance.display_name)
        }
        AgentRef::RemoteAgent { remote_agent_id } => {
            let profile = late.profile.get()?;
            profile
                .remote_agent(remote_agent_id)
                .await
                .ok()
                .map(|agent| agent.display_name)
        }
    }
}

/// The snake-case name of a run state, as the screens read it.
fn state_name(state: RunState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The Session Supervisor's view of `run_id`, or `None` when it is not built or cannot read it.
async fn run_view(late: &Late, run_id: RunId) -> Option<RunView> {
    late.supervisor.get()?.get(run_id).await.ok()
}

// ---------------------------------------------------------------- RefResolver (SA-CLOSE-01)

/// `RefResolver` over the File Service, the Knowledge Service, the Journal, the Session Supervisor,
/// the Package Catalogue, the Workspace Service, the Memory Service and the Catalogue Service.
pub struct DesktopRefResolver {
    pub journal: Arc<Journal>,
    pub catalogue: Arc<CatalogueService>,
    /// The approve-button words of every bundled locale, read once at construction (DSK-REC-14).
    pub approval_verbs: Vec<String>,
    pub late: Arc<Late>,
}

impl DesktopRefResolver {
    pub fn new(
        journal: Arc<Journal>,
        catalogue: Arc<CatalogueService>,
        localisation: &Localisation,
        late: Arc<Late>,
    ) -> DesktopRefResolver {
        DesktopRefResolver {
            journal,
            catalogue,
            approval_verbs: crate::words::approval_verbs(localisation),
            late,
        }
    }

    /// Whether the instance is one of the two bundled agents (PackageCatalogue.md PKG-REC-1).
    fn is_bundled(id: ordin_types::AgentInstanceId) -> bool {
        use ordin_package_catalogue::BundledAgent;
        id == BundledAgent::Ordin.instance_id() || id == BundledAgent::AgentMaker.instance_id()
    }

    /// Whether the artefact `id` belongs to `workspace_id` (DSK-REC-16).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-16
    async fn artefact_in(&self, workspace_id: WorkspaceId, id: ordin_types::ArtefactId) -> bool {
        let Ok(Some(artefact)) = self.journal.artefact(id).await else {
            return false;
        };
        match artefact.location {
            ArtefactLocation::Place { place_id } => match self.late.workspaces.get() {
                Some(workspaces) => workspaces
                    .places(workspace_id)
                    .await
                    .is_ok_and(|places| places.iter().any(|p| p.id == place_id)),
                None => false,
            },
            // DSK-REC-25: a bundled instance serves no workspace, so its artefact belongs to the
            // workspace of the run that produced it.
            // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-25
            ArtefactLocation::AgentFolder { agent_instance_id }
                if Self::is_bundled(agent_instance_id) =>
            {
                let (Some(run_id), Some(supervisor)) =
                    (artefact.producing_run_id, self.late.supervisor.get())
                else {
                    return false;
                };
                supervisor
                    .get(run_id)
                    .await
                    .is_ok_and(|view| view.workspace_id == workspace_id)
            }
            ArtefactLocation::AgentFolder { agent_instance_id } => match self.late.packages.get() {
                Some(packages) => packages
                    .get(agent_instance_id)
                    .await
                    .is_ok_and(|agent| agent.workspaces.contains(&workspace_id)),
                None => false,
            },
        }
    }
}

#[async_trait::async_trait]
impl RefResolver for DesktopRefResolver {
    async fn content(&self, workspace_id: WorkspaceId, item: &ContentRef) -> bool {
        match item {
            ContentRef::PlaceFile {
                place_id,
                relative_path,
            } => match self.late.files.get() {
                Some(files) => files.read_file(*place_id, relative_path).await.is_ok(),
                None => false,
            },
            ContentRef::FileVersion { file_version_id } => match self.late.files.get() {
                Some(files) => files.read_version(*file_version_id).await.is_ok(),
                None => false,
            },
            ContentRef::Artefact { artefact_id } => {
                self.artefact_in(workspace_id, *artefact_id).await
            }
        }
    }

    /// The Mail Service is not built in this build.
    async fn email_message(
        &self,
        _workspace_id: WorkspaceId,
        _email_link_id: EmailLinkId,
        _message_id: &str,
    ) -> bool {
        false
    }

    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-17
    async fn entity(&self, workspace_id: WorkspaceId, entity: EntityRef) -> bool {
        match entity {
            EntityRef::KnowledgePage(page_id) => match self.late.knowledge.get() {
                Some(knowledge) => knowledge
                    .resolve_block(&KnowledgeBlock {
                        workspace_id,
                        page: Some(page_id),
                    })
                    .await
                    .is_ok(),
                None => false,
            },
            EntityRef::Run(run_id) => run_view(&self.late, run_id)
                .await
                .is_some_and(|view| view.workspace_id == workspace_id),
            EntityRef::MemoryFact(id) => match self.late.memory.get() {
                Some(memory) => memory.get_facts(workspace_id, vec![id]).await.is_ok(),
                None => false,
            },
            EntityRef::AgentRepository(id) => self.catalogue.repository_ref(id).await.is_ok(),
            // G-REC-1: no owner reads a charter by its id.
            EntityRef::CrewCharter(_) => false,
            // A Ready to publish checklist's linked folder: one of the workspace's Confluence links
            // (2C wire §13).
            // spec: ordin-desktop/phase-2c-shared-types-wire#confluence_link_id
            EntityRef::ConfluenceLink(link) => match self.late.workspaces.get() {
                Some(workspaces) => workspaces
                    .confluence_links(workspace_id)
                    .await
                    .is_ok_and(|links| links.iter().any(|l| l.id == link)),
                None => false,
            },
            // The Telegram and Slack Services and the Mail Service are not built in this build.
            EntityRef::TelegramLink(_) | EntityRef::SlackLink(_) | EntityRef::Draft(_) => false,
        }
    }

    async fn run_agent_name(&self, run_id: RunId) -> Option<String> {
        let view = run_view(&self.late, run_id).await?;
        agent_name(&self.late, &view.run).await
    }

    /// The highest plan version recorded and the length of that plan (Journal `current_plan`).
    async fn plan_state(&self, run_id: RunId) -> PlanState {
        match self.journal.current_plan(run_id).await {
            Ok(steps) => PlanState {
                versions: steps.iter().map(|s| s.plan_version).max().unwrap_or(0),
                current_len: u8::try_from(steps.len()).unwrap_or(u8::MAX),
            },
            Err(_) => PlanState::default(),
        }
    }

    fn approval_verbs(&self) -> &[String] {
        &self.approval_verbs
    }
}

// ---------------------------------------------------------------- JournalReader (row 3.42)

/// Home's `JournalReader` composite over the Approval Service, the Journal, the Push Service, the
/// Ordin Host, the Session Supervisor and the Crew Orchestrator (HomeAndSearch.md SA-RECON-02).
pub struct DesktopJournalReader {
    pub journal: Arc<Journal>,
    pub approval: Arc<ordin_approval::ApprovalService>,
    pub late: Arc<Late>,
}

impl DesktopJournalReader {
    /// One run of the Journal's run index as Home's row (DSK-REC-19). `None` when the run cannot be
    /// read.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-19
    async fn summary(&self, entry: &RunIndexEntry) -> Option<RunSummary> {
        let view = run_view(&self.late, entry.run_id).await?;
        let run = &view.run;
        // DSK-HOME-1: Ordin's own conversation is shown in its panel, not as work on Home: paused between
        // messages, it read "Ordin — paused · Waiting for you" under Running now.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-HOME-1
        if matches!(run.origin, RunOrigin::Conversation { .. }) {
            return None;
        }
        let title = match agent_name(&self.late, run).await {
            Some(name) => name,
            None => match run.agent {
                AgentRef::AgentInstance { agent_instance_id } => agent_instance_id.to_string(),
                AgentRef::RemoteAgent { remote_agent_id } => remote_agent_id.to_string(),
            },
        };
        let crew_run_id = match run.origin {
            RunOrigin::CrewRound { crew_run_id, .. } => Some(crew_run_id),
            _ => None,
        };
        Some(RunSummary {
            workspace_id: view.workspace_id,
            run_id: Some(entry.run_id),
            crew_run_id,
            title,
            state: state_name(entry.state),
            at: entry.ended_at.unwrap_or(entry.first_seen_at),
            can_run_again: true,
        })
    }

    async fn summaries(&self, entries: Vec<RunIndexEntry>) -> Vec<RunSummary> {
        let mut out = Vec::with_capacity(entries.len());
        for entry in &entries {
            if let Some(summary) = self.summary(entry).await {
                out.push(summary);
            }
        }
        out
    }
}

#[async_trait::async_trait]
impl JournalReader for DesktopJournalReader {
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-18
    async fn waiting_items(&self) -> Result<Vec<WaitingItem>, HomeSourceError> {
        let mut items = Vec::new();
        // Approvals waiting for the person.
        let mut cursor = None;
        for _ in 0..MAX_PAGES {
            let page = self
                .approval
                .list_pending(None, cursor)
                .await
                .map_err(|_| HomeSourceError::Unavailable)?;
            for record in page.items {
                let card = record.card;
                items.push(WaitingItem {
                    workspace_id: Some(card.workspace_id),
                    kind: "approval".to_owned(),
                    title: card.question,
                    since: card.created_at,
                    opens: OpenTarget::Approval {
                        workspace_id: card.workspace_id,
                        approval_id: card.approval_id,
                    },
                });
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        // External actions whose outcome is unknown, in their run's workspace.
        let mut cursor = None;
        for _ in 0..MAX_PAGES {
            let page = self
                .journal
                .list_external_actions(Some(ExternalActionState::UnknownOutcome), cursor)
                .await
                .map_err(|_| HomeSourceError::Unavailable)?;
            for action in page.items {
                let view = match action.run_id {
                    Some(run_id) => run_view(&self.late, run_id).await,
                    None => None,
                };
                let (Some(run_id), Some(view)) = (action.run_id, view) else {
                    tracing::debug!(
                        component = "desktop",
                        external_action = ?action.action_key,
                        "waiting_item_left_out"
                    );
                    continue;
                };
                items.push(WaitingItem {
                    workspace_id: Some(view.workspace_id),
                    kind: "unknown_outcome".to_owned(),
                    title: action.destination,
                    since: action.sent_at,
                    opens: OpenTarget::Run {
                        workspace_id: view.workspace_id,
                        run_id,
                    },
                });
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        // Held pushes, per workspace not in the bin.
        if let Some(push) = self.late.push.get() {
            for workspace_id in live_workspaces(&self.late).await {
                let pending = push
                    .list_pending(workspace_id)
                    .await
                    .map_err(|_| HomeSourceError::Unavailable)?;
                for held in pending
                    .into_iter()
                    .filter(|p| p.state == ordin_types::domain::places::PendingPushState::Held)
                {
                    items.push(WaitingItem {
                        workspace_id: Some(held.workspace_id),
                        kind: "held_push".to_owned(),
                        title: held.request,
                        since: held.created_at,
                        opens: OpenTarget::Workspace {
                            workspace_id: held.workspace_id,
                        },
                    });
                }
            }
        }
        // Ordin's waiting blocks (SA-CLOSE-01).
        if let Some(host) = self.late.ordin_host.get() {
            for block in host.waiting_blocks() {
                items.push(WaitingItem {
                    workspace_id: Some(block.workspace_id),
                    kind: block.kind,
                    title: block.title,
                    since: block.since,
                    opens: OpenTarget::Block {
                        workspace_id: block.workspace_id,
                        block_id: block.block_id,
                    },
                });
            }
        }
        Ok(items)
    }

    async fn running(&self) -> Result<Vec<RunSummary>, HomeSourceError> {
        let entries = self
            .journal
            .running_runs()
            .await
            .map_err(|_| HomeSourceError::Unavailable)?;
        Ok(self.summaries(entries).await)
    }

    async fn finished_since(
        &self,
        since: Timestamp,
        limit: u32,
    ) -> Result<Vec<RunSummary>, HomeSourceError> {
        let entries = self
            .journal
            .runs_ended_since(since, limit)
            .await
            .map_err(|_| HomeSourceError::Unavailable)?;
        Ok(self.summaries(entries).await)
    }

    async fn run_inputs(&self, run: RunAgainSubject) -> Result<Vec<InputUsed>, HomeSourceError> {
        match run {
            RunAgainSubject::Run { run_id } => {
                let supervisor = self
                    .late
                    .supervisor
                    .get()
                    .ok_or(HomeSourceError::Unavailable)?;
                match supervisor.get(run_id).await {
                    Ok(view) => Ok(view.run.inputs_used),
                    Err(SupervisorError::RunNotFound) => Err(HomeSourceError::NotFound),
                    Err(_) => Err(HomeSourceError::Unavailable),
                }
            }
            RunAgainSubject::CrewRun { crew_run_id } => {
                let orchestrator = self
                    .late
                    .orchestrator
                    .get()
                    .ok_or(HomeSourceError::Unavailable)?;
                match orchestrator.get(crew_run_id).await {
                    Ok((crew, _)) => Ok(crew.inputs),
                    Err(ordin_crew_orchestrator::CrewError::NotFound) => {
                        Err(HomeSourceError::NotFound)
                    }
                    Err(_) => Err(HomeSourceError::Unavailable),
                }
            }
        }
    }
}

// ---------------------------------------------------------------- SearchSources (after row 3.42)

/// Case-insensitive containment.
fn contains_text(text: &str, query: &str) -> bool {
    text.to_lowercase().contains(&query.trim().to_lowercase())
}

fn limit_of(limit: u32) -> usize {
    usize::try_from(limit).unwrap_or(usize::MAX)
}

/// Agents by display name, over the Package Catalogue's `list`.
pub struct AgentSearch {
    pub late: Arc<Late>,
}

#[async_trait::async_trait]
impl SearchSource for AgentSearch {
    fn group(&self) -> GoToKind {
        GoToKind::Agent
    }

    async fn find(&self, query: &str, limit: u32) -> Vec<GoToResult> {
        let Some(packages) = self.late.packages.get() else {
            return Vec::new();
        };
        let Ok(agents) = packages.list(None).await else {
            return Vec::new();
        };
        agents
            .into_iter()
            .filter(|a| contains_text(&a.instance.display_name, query))
            .take(limit_of(limit))
            .map(|a| GoToResult {
                kind: GoToKind::Agent,
                label: a.instance.display_name,
                detail: None,
                workspace_id: None,
                // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-20
                opens: OpenTarget::Item {
                    kind: "agent".to_owned(),
                    id: a.instance.id.to_string(),
                    workspace_id: None,
                },
            })
            .collect()
    }
}

/// Workflows by name, over the Ordin Host's `workflows.list` of each workspace not in the bin.
pub struct WorkflowSearch {
    pub late: Arc<Late>,
}

#[async_trait::async_trait]
impl SearchSource for WorkflowSearch {
    fn group(&self) -> GoToKind {
        GoToKind::Workflow
    }

    async fn find(&self, query: &str, limit: u32) -> Vec<GoToResult> {
        let Some(host) = self.late.ordin_host.get() else {
            return Vec::new();
        };
        let limit = limit_of(limit);
        let mut out = Vec::new();
        for workspace_id in live_workspaces(&self.late).await {
            let mut cursor = None;
            for _ in 0..MAX_PAGES {
                let Ok(page) = host.workflows_list(workspace_id, cursor).await else {
                    break;
                };
                for workflow in page.items {
                    if out.len() >= limit {
                        return out;
                    }
                    if contains_text(&workflow.name, query) {
                        out.push(GoToResult {
                            kind: GoToKind::Workflow,
                            label: workflow.name,
                            detail: None,
                            workspace_id: Some(workspace_id),
                            // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-20
                            opens: OpenTarget::Item {
                                kind: "workflow".to_owned(),
                                id: workflow.workflow_id.to_string(),
                                workspace_id: Some(workspace_id),
                            },
                        });
                    }
                }
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
        }
        out.truncate(limit);
        out
    }
}

/// Conversations by their first request, over the Ordin Host's `ordin.list_conversations` of each
/// workspace not in the bin.
pub struct ConversationSearch {
    pub late: Arc<Late>,
}

#[async_trait::async_trait]
impl SearchSource for ConversationSearch {
    fn group(&self) -> GoToKind {
        GoToKind::Conversation
    }

    async fn find(&self, query: &str, limit: u32) -> Vec<GoToResult> {
        let Some(host) = self.late.ordin_host.get() else {
            return Vec::new();
        };
        let limit = limit_of(limit);
        let mut out = Vec::new();
        for workspace_id in live_workspaces(&self.late).await {
            let mut cursor = None;
            for _ in 0..MAX_PAGES {
                let Ok(page) = host.list_conversations(workspace_id, cursor).await else {
                    break;
                };
                for conversation in page.items {
                    if out.len() >= limit {
                        return out;
                    }
                    let Some(first) = conversation.first_request else {
                        continue;
                    };
                    if contains_text(&first, query) {
                        out.push(GoToResult {
                            kind: GoToKind::Conversation,
                            label: first,
                            detail: None,
                            workspace_id: Some(workspace_id),
                            // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-20
                            opens: OpenTarget::Item {
                                kind: "conversation".to_owned(),
                                id: conversation.conversation_id.as_str().to_owned(),
                                workspace_id: Some(workspace_id),
                            },
                        });
                    }
                }
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
        }
        out.truncate(limit);
        out
    }
}
