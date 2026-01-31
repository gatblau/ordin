//! Crews and workflow runs (steps 10, 17, 18, 39, 40, 45 and 47): charters within the guardrail
//! defaults and maxima, the crew plan, `crews.start`, the Trigger Service's immediate start and the
//! programs' `CrewHost` reads.

use std::collections::{BTreeMap, HashMap};

use ordin_crew_orchestrator::{CrewInput, CrewInputValue, CrewStart};
use ordin_journal::NewArtefact;
use ordin_mcp_front::{ToolFailure, ToolOutput};
use ordin_memory_service::AgentMemoryRules;
use ordin_service_api::blocks::{Block, CrewPlanBlock, UsageEstimate as BlockEstimate};
use ordin_service_api::commands::dashboards::DashboardRefreshStarted;
use ordin_service_api::commands::ordin::MessageAuthor;
use ordin_service_api::commands::workflows::{CheckProblem, WorkflowCheckReport};
use ordin_types::common::{AgentRef, InputUsed};
use ordin_types::domain::places::ArtefactLocation;
use ordin_types::domain::runs::{
    CoordinatorKind, CrewCharter, CrewCharterState, CrewMember, CrewRunState, DashboardSource,
    Guardrails, RunOrigin, Task, TaskTarget, WorkflowInput, WorkflowStep,
};
use ordin_types::domain::WorkflowState;
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::{WorkflowFileLimits, WorkflowInputKind};
use ordin_types::wire::{ContentRef, WorkflowFile};
use ordin_types::{
    AgentInstanceId, CrewCharterId, CrewRunId, DashboardId, TaskId, WorkflowId, WorkspaceId,
};
use ordin_usage_planner::{EstimateRequest, WorkKind, WorkShape};
use serde_json::json;

use crate::error::OhError;
use crate::host::{lock, OrdinHost, Turn, COMPONENT};
use crate::store;
use crate::tools::fail;
use crate::types::*;
use crate::wff;

/// 2D-4: crews of up to 8 members plus the coordinator; failure repeat 2.
const MAX_MEMBERS: usize = 8;
const FAILURE_REPEAT: u32 = 2;

fn estimate_block(e: ordin_usage_planner::UsageEstimate) -> Option<BlockEstimate> {
    serde_json::to_value(e)
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
}

/// Guardrails from asked limits, each clamped to the maxima; the names of the lowered ones.
fn guardrails(
    asked: &CrewLimitsArg,
    defaults: &CrewLimitPolicy,
    maxima: &CrewLimitPolicy,
) -> (Guardrails, Vec<&'static str>) {
    let mut lowered = Vec::new();
    let mut clamp = |value: u32, max: u32, name: &'static str| {
        if value > max {
            lowered.push(name);
            max
        } else {
            value.max(1)
        }
    };
    let rounds = clamp(
        u32::from(asked.rounds.unwrap_or(defaults.rounds)),
        u32::from(maxima.rounds),
        "rounds",
    );
    let minutes = clamp(
        u32::from(
            asked
                .wall_clock_minutes
                .unwrap_or(defaults.wall_clock_minutes),
        ),
        u32::from(maxima.wall_clock_minutes),
        "wall_clock_minutes",
    );
    let at_once = clamp(
        u32::from(asked.members_at_once.unwrap_or(defaults.members_at_once)),
        u32::from(maxima.members_at_once),
        "members_at_once",
    );
    let budget = match (
        asked.request_budget.or(defaults.request_budget),
        maxima.request_budget,
    ) {
        (Some(b), Some(max)) if b > max => {
            lowered.push("request_budget");
            max
        }
        (Some(b), _) => b,
        (None, Some(max)) => max,
        (None, None) => 0,
    };
    (
        Guardrails {
            max_rounds: rounds,
            wall_clock_secs: minutes * 60,
            request_budget: budget,
            max_members_running: at_once,
            members_allowed_external_actions: vec![],
            approval_points: vec![],
            failure_repeat_limit: FAILURE_REPEAT,
        },
        lowered,
    )
}

fn file_guardrails(
    limits: &WorkflowFileLimits,
    maxima: &CrewLimitPolicy,
    members: &HashMap<String, AgentInstanceId>,
) -> Guardrails {
    let (mut g, _) = guardrails(
        &CrewLimitsArg {
            rounds: Some(limits.max_rounds),
            wall_clock_minutes: Some(limits.wall_clock_minutes),
            members_at_once: Some(limits.agents_at_once),
            request_budget: limits.request_budget,
        },
        maxima,
        maxima,
    );
    g.failure_repeat_limit = u32::from(limits.failure_repeat_limit);
    g.members_allowed_external_actions = limits
        .external_action_agents
        .iter()
        .filter_map(|k| members.get(k))
        .map(|id| AgentRef::AgentInstance {
            agent_instance_id: *id,
        })
        .collect();
    g
}

impl OrdinHost {
    /// Step 10 (`create_crew`).
    pub(crate) async fn create_crew(
        &self,
        ws: WorkspaceId,
        c: CreateCrewArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        if c.members.len() > MAX_MEMBERS {
            return Err(fail(
                "OH_CREW_TOO_MANY_MEMBERS",
                "A crew has at most 8 members.",
                false,
            ));
        }
        let mut members = Vec::new();
        for m in &c.members {
            let agent = self.directory_hit(ws, &m.agent).await?.ok_or_else(|| {
                fail("OH_AGENT_UNKNOWN", "No agent with that key is in this workspace; use a key from the agent directory.", false)
            })?;
            members.push(CrewMember {
                member: AgentRef::AgentInstance {
                    agent_instance_id: agent.agent_instance_id,
                },
                role: m.role.clone(),
            });
        }
        let policy = self.policy();
        let asked = c.limits.clone().unwrap_or(CrewLimitsArg {
            rounds: None,
            wall_clock_minutes: None,
            members_at_once: None,
            request_budget: None,
        });
        let (guardrails, lowered) = guardrails(&asked, &policy.crew_defaults, &policy.crew_maxima);
        let mut charter = CrewCharter {
            id: CrewCharterId::new(),
            workspace_id: ws,
            name: None,
            purpose: c.purpose,
            charter_version: 1,
            coordinator: CoordinatorKind::Ordin,
            members,
            allowed_handoffs: vec![],
            guardrails,
            input_fields: c.inputs,
            workflow_id: None,
            workflow_version: None,
            steps: vec![],
            state: CrewCharterState::Written,
            written_at: Timestamp::now(),
        };
        if self.ports.orchestrator.check_plan(&charter, &[]).await.fits {
            charter.state = CrewCharterState::Valid;
        }
        let stored = async {
            let mut tx = self.pool.begin().await?;
            store::discard_unstarted(&mut tx, ws).await?;
            store::insert_charter(&mut tx, &charter, None).await?;
            tx.commit().await
        }
        .await;
        stored.map_err(|_| {
            fail(
                "OH_DATASTORE_UNAVAILABLE",
                "Something went wrong; try again.",
                true,
            )
        })?;
        tracing::info!(
            component = COMPONENT,
            workspace_id = ws.to_string().as_str(),
            crew_charter_id = charter.id.to_string().as_str(),
            members = charter.members.len(),
            clamped = lowered.join(",").as_str(),
            "ordin_host.charter_written"
        );
        let estimate = match self
            .ports
            .usage
            .estimate(EstimateRequest {
                work_key: format!("crew:{}", charter.id),
                shape: WorkShape::Crew {
                    members: u32::try_from(charter.members.len()).unwrap_or(0),
                    max_rounds: charter.guardrails.max_rounds,
                },
            })
            .await
        {
            Ok(e) => estimate_block(e),
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.estimate_unavailable"
                );
                None
            }
        };
        self.show_block(
            ws,
            MessageAuthor::Ordin,
            Block::CrewPlan(CrewPlanBlock {
                crew_charter_id: charter.id,
                estimate,
            }),
            true,
        )
        .await
        .map_err(|e| fail(&e.code(), "That did not work; try again.", e.retryable()))?;
        Ok(ToolOutput {
            text: "Crew plan shown; nothing starts until the person presses Start.".into(),
            structured: Some(json!({ "crew_charter_id": charter.id })),
        })
    }

    /// The crew plan block of a charter stops waiting.
    async fn plan_settled(
        &self,
        ws: WorkspaceId,
        charter_id: CrewCharterId,
    ) -> Result<(), OhError> {
        let rows = {
            let mut c = self.pool.acquire().await?;
            store::waiting_blocks(&mut c, Some(ws), 500).await?
        };
        for row in rows {
            if matches!(&row.block, Block::CrewPlan(p) if p.crew_charter_id == charter_id) {
                let mut c = self.pool.acquire().await?;
                store::set_block_waiting(&mut c, row.block_id, false).await?;
                self.unmirror(row.block_id);
            }
        }
        self.needs_you_changed(ws).await;
        Ok(())
    }

    /// `crews.start` (step 17): the person pressed Start.
    pub async fn crews_start(&self, crew_charter_id: CrewCharterId) -> Result<CrewRunId, OhError> {
        let row = {
            let mut c = self.pool.acquire().await?;
            store::charter(&mut c, crew_charter_id).await?
        }
        .ok_or(OhError::CharterUnknown)?;
        if row.charter.state != CrewCharterState::Valid {
            return Err(OhError::input(
                "CHRT_NOT_STARTED_BY_PERSON",
                "/crew_charter_id",
            ));
        }
        let inputs = lock(&self.state)
            .filled
            .remove(&crew_charter_id)
            .unwrap_or_default();
        let crew_inputs = self.crew_inputs(&inputs).await?;
        self.start_crew(row.charter, crew_inputs, &inputs, true, None, None)
            .await?
            .ok_or(OhError::CrewRunning)
    }

    /// Step 17 from the memory rules on: shared by `crews.start` and the immediate start (step 39).
    async fn start_crew(
        &self,
        charter: CrewCharter,
        inputs: Vec<CrewInput>,
        run_inputs: &[RunInput],
        started_by_person: bool,
        trigger_id: Option<ordin_types::TriggerId>,
        started_by: Option<&RunStartedBy>,
    ) -> Result<Option<CrewRunId>, OhError> {
        let ws = charter.workspace_id;
        match self.ports.orchestrator.running_crew(ws).await {
            Ok(Some(_)) => return Err(OhError::CrewRunning),
            Ok(None) => {}
            Err(e) => return Err(self.dep("orchestrator", e)),
        }
        // SA-CLOSE-04: the rules the agent declares in its Memory part.
        for m in &charter.members {
            if let AgentRef::AgentInstance { agent_instance_id } = m.member {
                let contract = self
                    .ports
                    .inspector
                    .memory_contract(agent_instance_id)
                    .await
                    .map_err(|e| self.dep("inspector", e))?;
                if let Some(c) = contract {
                    let rules = AgentMemoryRules {
                        retains: c.retains,
                        excludes: c.excludes,
                        max_age_days: if c.unbounded { None } else { c.max_age_days },
                        store_location: c.store_location,
                    };
                    self.ports
                        .memory
                        .set_agent_rules(agent_instance_id, rules)
                        .map_err(|e| self.dep("memory", e))?;
                }
            }
        }
        let Some(me) = self.arc() else {
            return Ok(None);
        };
        let charter_id = charter.id;
        // OH-CL-1: a crew that ends at once finds its charter linked; `?` below releases the lock.
        // spec: ordin-desktop/components/OrdinHost#OH-CL-1
        let link = self.crew_start_lock(ws);
        let linked = link.lock().await;
        let crew_run_id = self
            .ports
            .orchestrator
            .start(CrewStart {
                charter,
                inputs,
                coordinator: me.coordinator(),
                started_by_person,
                trigger_id,
            })
            .await
            .map_err(|e| self.dep("orchestrator", e))?;
        let value = serde_json::to_value(run_inputs).unwrap_or(serde_json::Value::Array(vec![]));
        {
            let mut c = self.pool.acquire().await?;
            store::set_charter_started(&mut c, charter_id, crew_run_id, &value).await?;
        }
        drop(linked);
        let _ = started_by;
        self.plan_settled(ws, charter_id).await?;
        Ok(Some(crew_run_id))
    }

    /// `crews.discard_plan`.
    pub async fn crews_discard_plan(&self, crew_charter_id: CrewCharterId) -> Result<(), OhError> {
        let row = {
            let mut c = self.pool.acquire().await?;
            store::charter(&mut c, crew_charter_id).await?
        }
        .ok_or(OhError::CharterUnknown)?;
        {
            let mut c = self.pool.acquire().await?;
            store::set_charter_state(&mut c, crew_charter_id, CrewCharterState::Discarded).await?;
        }
        self.plan_settled(row.charter.workspace_id, crew_charter_id)
            .await
    }

    /// `crews.charter` (OH-WR-2): the charter as it is stored, whatever its state. It reads only: no
    /// state change, no event and no estimate.
    // spec: ordin-desktop/components/OrdinHost#OH-WR-2
    pub async fn crews_charter(
        &self,
        crew_charter_id: CrewCharterId,
    ) -> Result<CrewCharter, OhError> {
        let row = {
            let mut c = self.pool.acquire().await?;
            store::charter(&mut c, crew_charter_id).await?
        }
        .ok_or(OhError::CharterUnknown)?;
        Ok(row.charter)
    }

    /// Inputs filled in a form (step 18): kept for the workspace's newest unstarted workflow charter
    /// whose fields the form answers.
    // OrdinHost.md Internal Logic step 18: the latest form answer naming every input field of an
    // unstarted charter of the workspace is kept for it in memory, for `crews.start`.
    pub(crate) fn form_filled(
        &self,
        ws: WorkspaceId,
        fields: &BTreeMap<String, serde_json::Value>,
    ) {
        let me = self.me.clone();
        let fields = fields.clone();
        tokio::spawn(async move {
            let Some(host) = me.upgrade() else { return };
            let Ok(mut c) = host.pool.acquire().await else {
                return;
            };
            let charters: Vec<(String,)> = sqlx::query_as(
                "SELECT id FROM crew_charters WHERE workspace_id = ? AND state = 'valid' ORDER BY written_at DESC",
            )
            .bind(ws.to_string())
            .fetch_all(&mut *c)
            .await
            .unwrap_or_default();
            for (id,) in charters {
                let Ok(id) = id.parse::<CrewCharterId>() else {
                    continue;
                };
                let Ok(Some(row)) = store::charter(&mut c, id).await else {
                    continue;
                };
                if row.charter.input_fields.is_empty()
                    || !row
                        .charter
                        .input_fields
                        .iter()
                        .all(|f| fields.contains_key(f))
                {
                    continue;
                }
                let inputs: Vec<RunInput> = row
                    .charter
                    .input_fields
                    .iter()
                    .filter_map(|label| {
                        let value = fields.get(label)?;
                        let value = match value {
                            serde_json::Value::Array(items) => RunInputValue::List {
                                items: items
                                    .iter()
                                    .filter_map(|i| i.as_str().map(str::to_owned))
                                    .collect(),
                            },
                            serde_json::Value::String(s) => RunInputValue::Text { text: s.clone() },
                            other => serde_json::from_value::<ContentRef>(other.clone())
                                .map(|content| RunInputValue::Document { content })
                                .unwrap_or(RunInputValue::Text {
                                    text: other.to_string(),
                                }),
                        };
                        Some(RunInput {
                            label: label.clone(),
                            value,
                        })
                    })
                    .collect();
                lock(&host.state).filled.insert(id, inputs);
                return;
            }
        });
    }

    /// Inputs to the Crew Orchestrator's form (step 39.3, SA-CLOSE-02).
    async fn crew_inputs(&self, inputs: &[RunInput]) -> Result<Vec<CrewInput>, OhError> {
        let mut out = Vec::new();
        for input in inputs {
            let value = match &input.value {
                RunInputValue::Text { text } => CrewInputValue::Text { text: text.clone() },
                RunInputValue::List { items } => CrewInputValue::List {
                    items: items.clone(),
                },
                RunInputValue::Document {
                    content: ContentRef::Artefact { artefact_id },
                } => CrewInputValue::Documents {
                    artefacts: vec![*artefact_id],
                },
                // spec: ordin-desktop/components/OrdinHost#SA-CLOSE-02
                RunInputValue::Document {
                    content:
                        ContentRef::PlaceFile {
                            place_id,
                            relative_path,
                        },
                } => {
                    let file = self
                        .ports
                        .files
                        .read_file(*place_id, relative_path)
                        .await
                        .map_err(|_| OhError::InputUnreadable {
                            label: input.label.clone(),
                        })?;
                    let artefact = self
                        .ports
                        .journal
                        .record_artefact(NewArtefact {
                            location: ArtefactLocation::Place {
                                place_id: *place_id,
                            },
                            path: relative_path.clone(),
                            hash: file.hash,
                            media_type: file.media_type,
                            producing_run_id: None,
                        })
                        .await
                        .map_err(|e| self.dep("journal", e))?;
                    CrewInputValue::Documents {
                        artefacts: vec![artefact],
                    }
                }
                RunInputValue::Document { .. } => {
                    return Err(OhError::InputUnreadable {
                        label: input.label.clone(),
                    })
                }
            };
            out.push(CrewInput {
                label: input.label.clone(),
                value,
                used: Vec::<InputUsed>::new(),
            });
        }
        Ok(out)
    }

    /// The agents of a saved file, by key, and the workspace's workflows by name.
    async fn resolve(
        &self,
        ws: WorkspaceId,
        file: &WorkflowFile,
    ) -> (
        HashMap<String, AgentInstanceId>,
        HashMap<String, WorkflowId>,
    ) {
        let mut agents = HashMap::new();
        for a in &file.agents {
            if let Ok(m) = self
                .ports
                .catalogue
                .match_workflow_agent(ws, &a.repository, &a.id, &a.version)
                .await
            {
                match m {
                    ordin_package_catalogue::AgentMatch::Present {
                        agent_instance_id, ..
                    }
                    | ordin_package_catalogue::AgentMatch::OtherVersion {
                        agent_instance_id, ..
                    } => {
                        agents.insert(a.key.clone(), agent_instance_id);
                    }
                    ordin_package_catalogue::AgentMatch::Missing => {}
                }
            }
        }
        let mut called = HashMap::new();
        if let Ok(mut c) = self.pool.acquire().await {
            if let Ok(rows) = store::workflows_of(&mut c, ws).await {
                for r in rows {
                    called.insert(r.name, r.id);
                }
            }
        }
        (agents, called)
    }

    /// A charter for a saved workflow version (steps 18 and 39.3).
    async fn workflow_charter(
        &self,
        ws: WorkspaceId,
        workflow_id: WorkflowId,
        version: u32,
        file: &WorkflowFile,
    ) -> CrewCharter {
        let policy = self.policy();
        let (agents, called) = self.resolve(ws, file).await;
        let members: Vec<CrewMember> = file
            .agents
            .iter()
            .filter_map(|a| {
                agents.get(&a.key).map(|id| CrewMember {
                    member: AgentRef::AgentInstance {
                        agent_instance_id: *id,
                    },
                    role: a.role.clone(),
                })
            })
            .collect();
        CrewCharter {
            id: CrewCharterId::new(),
            workspace_id: ws,
            name: Some(file.name.clone()),
            purpose: file.purpose.clone(),
            charter_version: 1,
            coordinator: CoordinatorKind::Ordin,
            members,
            allowed_handoffs: vec![],
            guardrails: file_guardrails(&file.limits, &policy.crew_maxima, &agents),
            input_fields: file.inputs.iter().map(|i| i.label.clone()).collect(),
            workflow_id: Some(workflow_id),
            workflow_version: Some(version),
            steps: wff::to_domain(&file.steps, &agents, &called),
            state: CrewCharterState::Written,
            written_at: Timestamp::now(),
        }
    }

    async fn saved(
        &self,
        workflow_id: WorkflowId,
        version: Option<u32>,
    ) -> Result<(store::WorkflowRow, u32, WorkflowFile), OhError> {
        let mut c = self.pool.acquire().await?;
        let row = store::workflow(&mut c, workflow_id)
            .await?
            .ok_or(OhError::WorkflowUnknown)?;
        let latest = store::latest_version(&mut c, workflow_id)
            .await?
            .ok_or(OhError::WorkflowUnknown)?;
        let v = version.unwrap_or(latest);
        let version = store::version(&mut c, workflow_id, v)
            .await?
            .ok_or(OhError::VersionUnknown)?;
        Ok((row, v, version.file))
    }

    /// `workflows.run` (step 18): the crew plan; the run starts on `crews.start`.
    pub async fn workflows_run(
        &self,
        workflow_id: WorkflowId,
        version: Option<u32>,
        request: Option<String>,
    ) -> Result<CrewCharterId, OhError> {
        self.workflow_plan(workflow_id, version, request, None)
            .await
    }

    async fn workflow_plan(
        &self,
        workflow_id: WorkflowId,
        version: Option<u32>,
        request: Option<String>,
        slack_channel: Option<String>,
    ) -> Result<CrewCharterId, OhError> {
        let (row, v, file) = self.saved(workflow_id, version).await?;
        let ws = row.workspace_id;
        let mut charter = self.workflow_charter(ws, workflow_id, v, &file).await;
        let check = self.ports.orchestrator.check_plan(&charter, &[]).await;
        // Inputs are filled later, so a plan missing only its inputs still waits for Start.
        let only_inputs = check
            .problems
            .iter()
            .all(|p| matches!(p, ordin_crew_orchestrator::PlanProblem::InputMissing { .. }));
        if row.state != WorkflowState::NeedsAgents && (check.fits || only_inputs) {
            charter.state = CrewCharterState::Valid;
        }
        {
            let mut c = self.pool.acquire().await?;
            store::insert_charter(&mut c, &charter, None).await?;
        }
        if !file.inputs.is_empty() && self.current_run(ws).is_some() {
            let labels: Vec<&str> = file.inputs.iter().map(|i| i.label.as_str()).collect();
            let mut text = format!(
                "Fill the inputs {} from the request and the workspace; reply with show(form)",
                labels.join(", ")
            );
            if let Some(r) = &request {
                text.push_str(&format!(
                    ". The request, quoted as data: {}",
                    serde_json::to_string(r).unwrap_or_default()
                ));
            }
            let _ = self.send_turn(ws, Turn::fixed(text)).await;
        }
        let (agent_steps, ordin_turns) = wff::estimate_counts(&file.steps);
        let estimate = match self
            .ports
            .usage
            .estimate(EstimateRequest {
                work_key: format!("workflow:{workflow_id}:{v}"),
                shape: WorkShape::Workflow {
                    agent_steps,
                    ordin_turns,
                },
            })
            .await
        {
            Ok(e) => estimate_block(e),
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.estimate_unavailable"
                );
                None
            }
        };
        if let Some(channel) = slack_channel {
            // Step 47.4 (chosen there): the plan says where it was suggested from.
            let _ = self
                .show_message(ws, &format!("Suggested from a message in #{channel}"))
                .await;
        }
        self.show_block(
            ws,
            MessageAuthor::Ordin,
            Block::CrewPlan(CrewPlanBlock {
                crew_charter_id: charter.id,
                estimate,
            }),
            true,
        )
        .await?;
        Ok(charter.id)
    }

    /// Step 47 (`propose_workflow_run`).
    pub(crate) async fn propose_workflow_run(
        &self,
        ws: WorkspaceId,
        a: ProposeWorkflowRunArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        let request = a.request.trim().to_owned();
        if request.is_empty() || request.chars().count() > 4_000 {
            return Err(fail(
                "OH_MESSAGE_INVALID",
                "Say in 1 to 4,000 characters what the run is for.",
                false,
            ));
        }
        let unknown = || {
            fail(
                "OH_WORKFLOW_UNKNOWN",
                "There's no saved workflow with that id in this workspace.",
                false,
            )
        };
        let saved = {
            let mut c = self.pool.acquire().await.map_err(|_| unknown())?;
            let row = store::workflow(&mut c, a.workflow_id).await.ok().flatten();
            let latest = store::latest_version(&mut c, a.workflow_id)
                .await
                .ok()
                .flatten();
            row.filter(|r| r.workspace_id == ws).is_some() && latest.is_some()
        };
        if !saved {
            return Err(unknown());
        }
        let channel = lock(&self.state)
            .convs
            .get(&ws)
            .and_then(|c| c.slack.as_ref().map(|s| s.1.clone()));
        let id = self
            .workflow_plan(a.workflow_id, None, Some(request), channel)
            .await
            .map_err(|e| fail(&e.code(), e.message(), e.retryable()))?;
        Ok(ToolOutput {
            text: "Crew plan shown; the run starts only when the person presses Start.".into(),
            structured: Some(json!({ "crew_charter_id": id, "state": "waiting_for_you" })),
        })
    }

    // ---------------------------------------------------------------- Trigger Service (step 39)

    fn inputs_view(
        row: &store::WorkflowRow,
        version: u32,
        latest: u32,
        file: &WorkflowFile,
    ) -> WorkflowInputsView {
        WorkflowInputsView {
            workflow_id: row.id,
            name: row.name.clone(),
            state: row.state,
            version,
            latest_version: latest,
            inputs: file
                .inputs
                .iter()
                .map(|i| WorkflowInput {
                    label: i.label.clone(),
                    kind: i.kind,
                    required: i.required,
                })
                .collect(),
        }
    }

    /// Every workflow of the workspace with a saved version, at its latest version.
    pub async fn workflows(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<WorkflowInputsView>, OhError> {
        self.ports
            .workspaces
            .ensure_active(workspace_id)
            .await
            .map_err(|_| OhError::WorkspaceUnknown)?;
        let mut c = self.pool.acquire().await?;
        let mut out = Vec::new();
        for row in store::workflows_of(&mut c, workspace_id).await? {
            if row.state == WorkflowState::NotSavedYet {
                continue;
            }
            if let Some(latest) = store::latest_version(&mut c, row.id).await? {
                if let Some(v) = store::version(&mut c, row.id, latest).await? {
                    out.push(Self::inputs_view(&row, latest, latest, &v.file));
                }
            }
        }
        Ok(out)
    }

    /// One saved version's inputs; `None` when the workflow or that version does not exist.
    pub async fn workflow_version(
        &self,
        workflow_id: WorkflowId,
        version: u32,
    ) -> Result<Option<WorkflowInputsView>, OhError> {
        let mut c = self.pool.acquire().await?;
        let Some(row) = store::workflow(&mut c, workflow_id).await? else {
            return Ok(None);
        };
        let Some(latest) = store::latest_version(&mut c, workflow_id).await? else {
            return Ok(None);
        };
        Ok(store::version(&mut c, workflow_id, version)
            .await?
            .map(|v| Self::inputs_view(&row, version, latest, &v.file)))
    }

    /// Step 39.3: a saved workflow version started at once (R11; SA-RECON-02).
    // spec: ordin-desktop/components/OrdinHost#SA-RECON-02
    pub async fn start_workflow_run(&self, req: StartWorkflowRun) -> Result<CrewRunId, OhError> {
        self.ports
            .workspaces
            .ensure_active(req.workspace_id)
            .await
            .map_err(|_| OhError::WorkspaceUnknown)?;
        let (row, v, file) = match self.saved(req.workflow_id, Some(req.version)).await {
            Ok(x) => x,
            Err(OhError::VersionUnknown) => return Err(OhError::VersionUnknown),
            Err(_) => return Err(OhError::WorkflowUnknown),
        };
        if row.workspace_id != req.workspace_id {
            return Err(OhError::WorkflowUnknown);
        }
        if row.state == WorkflowState::NeedsAgents {
            return Err(OhError::NeedsAgents);
        }
        for input in file.inputs.iter().filter(|i| i.required) {
            let given = req.inputs.iter().find(|r| r.label == input.label);
            let fits = match (input.kind, given.map(|g| &g.value)) {
                (WorkflowInputKind::Text, Some(RunInputValue::Text { text })) => !text.is_empty(),
                (WorkflowInputKind::Document, Some(RunInputValue::Document { .. })) => true,
                (WorkflowInputKind::List, Some(RunInputValue::List { items })) => !items.is_empty(),
                _ => false,
            };
            if !fits {
                return Err(OhError::InputMissing {
                    label: input.label.clone(),
                });
            }
        }
        let mut charter = self
            .workflow_charter(req.workspace_id, req.workflow_id, v, &file)
            .await;
        charter.state = CrewCharterState::Valid;
        let inputs = self.crew_inputs(&req.inputs).await?;
        let check = self.ports.orchestrator.check_plan(&charter, &inputs).await;
        if !check.fits {
            let problems: Vec<CheckProblem> = check
                .problems
                .iter()
                .map(|p| {
                    wff::problem(
                        "OH_CHECK_FAILED",
                        "/plan",
                        &[("problem", crate::store::word(p))],
                    )
                })
                .collect();
            return Err(OhError::CheckFailed {
                report: WorkflowCheckReport {
                    problems,
                    notices: vec![],
                },
            });
        }
        let started_by = serde_json::to_value(&req.started_by).unwrap_or_default();
        {
            let mut c = self.pool.acquire().await?;
            store::insert_charter(&mut c, &charter, Some(&started_by)).await?;
        }
        // SA-CLOSE-08: a trigger passes started_by_person false with its id; a person true and none.
        let (by_person, trigger) = match &req.started_by {
            RunStartedBy::Person { .. } => (true, None),
            RunStartedBy::ChatFileTrigger { trigger_id, .. }
            | RunStartedBy::EventTrigger { trigger_id, .. } => (false, Some(*trigger_id)),
        };
        let kind = match &req.started_by {
            RunStartedBy::Person { .. } => "person",
            RunStartedBy::ChatFileTrigger { .. } => "chat_file_trigger",
            RunStartedBy::EventTrigger { .. } => "event_trigger",
        };
        let id = self
            .start_crew(
                charter,
                inputs,
                &req.inputs,
                by_person,
                trigger,
                Some(&req.started_by),
            )
            .await?
            .ok_or(OhError::CrewRunning)?;
        tracing::info!(
            component = COMPONENT,
            workflow_id = req.workflow_id.to_string().as_str(),
            version = v,
            crew_run_id = id.to_string().as_str(),
            started_by = kind,
            "ordin_host.workflow_started_at_once"
        );
        Ok(id)
    }

    /// Pass-through to `CrewOrchestrator::kill`.
    pub async fn stop_workflow_run(&self, crew_run_id: CrewRunId) -> Result<(), OhError> {
        self.ports
            .orchestrator
            .kill(crew_run_id)
            .await
            .map_err(|e| self.dep("orchestrator", e))
    }

    /// Pass-through to `CrewOrchestrator::get`; `None` when the crew run is unknown.
    pub async fn crew_run_state(
        &self,
        crew_run_id: CrewRunId,
    ) -> Result<Option<CrewRunState>, OhError> {
        self.ports
            .orchestrator
            .state(crew_run_id)
            .await
            .map_err(|e| self.dep("orchestrator", e))
    }

    /// Step 39.5: the inputs of an earlier crew run, for **Run again**.
    pub async fn run_again_inputs(
        &self,
        crew_run_id: CrewRunId,
    ) -> Result<RunAgainInputs, OhError> {
        let mut c = self.pool.acquire().await?;
        let row = store::charter_by_run(&mut c, crew_run_id)
            .await?
            .ok_or(OhError::CrewRunUnknown)?;
        let (Some(workflow_id), Some(version)) =
            (row.charter.workflow_id, row.charter.workflow_version)
        else {
            return Err(OhError::CrewRunUnknown);
        };
        let latest = store::latest_version(&mut c, workflow_id).await?;
        let file = store::version(&mut c, workflow_id, version)
            .await?
            .ok_or(OhError::VersionUnknown)?
            .file;
        let stored: Vec<RunInput> = row
            .run_inputs
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Ok(RunAgainInputs {
            workspace_id: row.charter.workspace_id,
            workflow_id,
            version,
            newer_version: latest.filter(|l| *l > version),
            inputs: file
                .inputs
                .iter()
                .map(|i| WorkflowRunAgainInput {
                    input: WorkflowInput {
                        label: i.label.clone(),
                        kind: i.kind,
                        required: i.required,
                    },
                    value: stored
                        .iter()
                        .find(|s| s.label == i.label)
                        .map(|s| s.value.clone()),
                    // R1 limit: no interaction gives the version a document's location reports now.
                    // spec: ordin-desktop/components/OrdinHost#OH-R1-3
                    newer_input_note: None,
                })
                .collect(),
        })
    }

    // ---------------------------------------------------------------- CrewHost reads (step 45)

    /// The version each agent of a saved workflow version was saved with.
    pub async fn saved_agent_versions(
        &self,
        workflow_id: WorkflowId,
        version: u32,
    ) -> Result<Vec<(AgentRef, String)>, OhError> {
        let (row, _, file) = self.saved(workflow_id, Some(version)).await?;
        let (agents, _) = self.resolve(row.workspace_id, &file).await;
        Ok(file
            .agents
            .iter()
            .filter_map(|a| {
                agents.get(&a.key).map(|id| {
                    (
                        AgentRef::AgentInstance {
                            agent_instance_id: *id,
                        },
                        a.version.clone(),
                    )
                })
            })
            .collect())
    }

    /// Another workflow's steps at its latest version, as 2C `WorkflowStep`s.
    pub async fn workflow_steps(
        &self,
        workspace_id: WorkspaceId,
        workflow_id: WorkflowId,
    ) -> Result<Option<Vec<WorkflowStep>>, OhError> {
        match self.saved(workflow_id, None).await {
            Ok((row, _, file)) if row.workspace_id == workspace_id => {
                let (agents, called) = self.resolve(workspace_id, &file).await;
                Ok(Some(wff::to_domain(&file.steps, &agents, &called)))
            }
            Ok(_) | Err(OhError::WorkflowUnknown) | Err(OhError::VersionUnknown) => Ok(None),
            Err(e) => Err(e),
        }
    }

    // ---------------------------------------------------------------- dashboards (step 40)

    /// `dashboards.refresh`: **Run again** with the same inputs.
    // spec: ordin-desktop/components/OrdinHost#SA-CLOSE-03
    pub async fn dashboards_refresh(
        &self,
        workspace_id: WorkspaceId,
        dashboard_id: DashboardId,
    ) -> Result<DashboardRefreshStarted, OhError> {
        let dashboard = self
            .ports
            .journal
            .dashboard(dashboard_id)
            .await
            .map_err(|e| self.dep("journal", e))?
            .filter(|d| d.workspace_id == workspace_id)
            .ok_or(OhError::DashboardUnknown)?;
        let run_again_of = match dashboard.made_by {
            DashboardSource::Task { task_id } => Some(task_id),
            // OH-KD-2: a report Ordin made in the conversation is asked of Ordin again, as a task
            // of no earlier task.
            // spec: ordin-desktop/components/OrdinHost#OH-KD-2
            DashboardSource::Conversation => None,
            DashboardSource::Workflow {
                workflow_id,
                version,
            } => {
                let id = self
                    .workflow_plan(workflow_id, Some(version), None, None)
                    .await?;
                return Ok(DashboardRefreshStarted::CrewPlan {
                    crew_charter_id: id,
                });
            }
        };
        // R1 limit (step 40): the Supervisor offers no read of an earlier task, so the task
        // runs again on Ordin's own instance with the dashboard's recorded inputs as the
        // request, marked as a run again of the task.
        // spec: ordin-desktop/components/OrdinHost#OH-R1-4
        let names: Vec<String> = dashboard.inputs.iter().map(|i| i.name.clone()).collect();
        let task = Task {
            id: TaskId::new(),
            workspace_id,
            target: TaskTarget::AgentInstance {
                agent_instance_id: self.config.ordin_agent_instance_id,
            },
            command: None,
            free_text: Some(format!(
                "Run the dashboard {} again with: {}",
                dashboard.name,
                names.join(", ")
            )),
            arguments: vec![],
            attached_artefacts: vec![],
            run_again_of,
            requested_at: Timestamp::now(),
        };
        let run_id = self
            .ports
            .supervisor
            .submit_task(task, WorkKind::Documents, true, None)
            .await
            .map_err(|e| self.dep("supervisor", e))?;
        self.ports
            .journal
            .add_dashboard_refresh(dashboard_id, run_id)
            .await
            .map_err(|e| self.dep("journal", e))?;
        Ok(DashboardRefreshStarted::Run { run_id })
    }

    /// Tool `keep_dashboard` (OH-KD-1): keeps one of the workspace's reports as a dashboard — the
    /// one whose title is `report` (any case), else the most recent. `made_by` comes from the run
    /// that made the report: a task run's task, or Ordin's conversation. A crew's report is kept
    /// from its workflow, so it is refused here.
    // spec: ordin-desktop/components/OrdinHost#OH-KD-1
    pub(crate) async fn keep_dashboard_tool(
        &self,
        ws: WorkspaceId,
        a: crate::types::KeepDashboardArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        use crate::tools::fail;
        let gone = || {
            fail(
                "OH_DEPENDENCY_FAILED",
                "Something went wrong; try again.",
                true,
            )
        };
        let reports = self
            .ports
            .journal
            .recent_reports(ws)
            .await
            .map_err(|_| gone())?;
        let wanted = a.report.as_deref().map(str::trim).filter(|t| !t.is_empty());
        let report = match wanted {
            Some(title) => reports
                .iter()
                .find(|r| r.title.to_lowercase() == title.to_lowercase()),
            None => reports.first(),
        }
        .ok_or_else(|| {
            fail(
                "OH_RUN_UNKNOWN",
                "There is no such report in this workspace yet. Make the report first.",
                false,
            )
        })?;
        let runs = self.ports.supervisor.list(ws).await.map_err(|_| gone())?;
        let run = runs.iter().find(|r| r.id == report.run_id);
        let made_by = match run.map(|r| &r.origin) {
            Some(RunOrigin::Task { task_id }) => DashboardSource::Task { task_id: *task_id },
            Some(RunOrigin::CrewRound { .. }) => {
                return Err(fail(
                    "OH_RUN_NOT_REPEATABLE",
                    "That report came from a crew. Keep it from the crew's workflow instead.",
                    false,
                ))
            }
            Some(RunOrigin::Conversation { .. }) | None => DashboardSource::Conversation,
        };
        let inputs = run.map(|r| r.inputs_used.clone()).unwrap_or_default();
        let kept = self
            .ports
            .journal
            .keep_dashboard(ordin_journal::NewDashboard {
                workspace_id: ws,
                name: a.name.trim().to_owned(),
                made_by,
                inputs,
                run_id: report.run_id,
            })
            .await
            .map_err(|e| match e.code.as_str() {
                "REPORT_DASHBOARD_NAME_DUPLICATE" => fail(
                    "REPORT_DASHBOARD_NAME_DUPLICATE",
                    "A dashboard of this workspace already has that name. Choose another name.",
                    false,
                ),
                "JRNL_NOT_FOUND" => fail(
                    "OH_RUN_UNKNOWN",
                    "There is no such report in this workspace yet. Make the report first.",
                    false,
                ),
                _ => gone(),
            })?;
        self.workspace_changed(ws).await;
        Ok(ToolOutput {
            text: format!(
                "Kept \"{}\" as the dashboard {}. It is under Dashboards in this workspace.",
                report.title, kept.name
            ),
            structured: Some(json!({ "dashboard_id": kept.id })),
        })
    }
}
