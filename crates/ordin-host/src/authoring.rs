//! The Authoring Service's and the Handover Service's hosts (steps 42 and 43): single-turn runs of
//! the Ordin instance with fixed templates (SA-CLOSE-01), rehearsals and trials, handover notes.

use ordin_runtime_adapter::{PauseReason, RunEndReason, RunEvent};
use ordin_types::domain::runs::{Task, TaskTarget};
use ordin_types::domain::{HandoverNote, NoteSection, StageChoice};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, RunId, TaskId, WorkspaceId};
use ordin_usage_planner::WorkKind;

use crate::error::OhError;
use crate::host::{lock, OrdinHost, Turn, TurnEnd};
use crate::types::*;

/// A generated reply is at most 64 KiB (step 42.2).
const REPLY_MAX_BYTES: usize = 65_536;
/// The note's four headings (HandoverService.md, *The note's four headings*).
const HEADINGS: [&str; 4] = [
    "What's done",
    "Decisions made",
    "Open questions",
    "What's needed next",
];

/// SA-CLOSE-01: the fixed text of each template; `language-draft` takes the tag and name from the data.
pub(crate) fn template(id: &str, data: &serde_json::Value) -> Option<String> {
    Some(match id {
        "foundry-part" => "Write one agent part of the kind and for the purpose given in the data below. Reply with the part's file content only, in the format the data names, with no other text.".to_owned(),
        "fix-finding" => "A check raised the finding in the data below against an agent draft. Reply with the whole new content of the file the finding names, changed only as far as needed to remove the finding, with no other text.".to_owned(),
        "rehearsal-review" => "Below are a rehearsal situation and what the agent did in it. In at most five sentences, give your opinion of whether the result meets what the situation asked. You decide nothing; your reply is shown as a model's opinion.".to_owned(),
        "language-draft" => crate::language::draft_template(
            data.get("locale").and_then(serde_json::Value::as_str).unwrap_or(""),
            data.get("name").and_then(serde_json::Value::as_str).unwrap_or(""),
        ),
        _ => return None,
    })
}

impl OrdinHost {
    /// The workspace a run with no workspace of its own is recorded in: one whose conversation is
    /// current (R1 limit, steps 42.2 and 54.3).
    // spec: ordin-desktop/components/OrdinHost#OH-R1-5
    fn any_workspace(&self) -> Option<WorkspaceId> {
        lock(&self.state).convs.keys().next().copied()
    }

    /// A single-turn run: the template, then `Data:` and the quoted JSON (ADR-088). Returns the
    /// turn's text or the failing code.
    pub(crate) async fn generate_text(
        &self,
        template: &str,
        data: &serde_json::Value,
        ws: Option<WorkspaceId>,
    ) -> Result<String, String> {
        let ws = ws
            .or_else(|| self.any_workspace())
            .ok_or_else(|| "OH_WORKSPACE_UNKNOWN".to_owned())?;
        let quoted = serde_json::to_string_pretty(data).unwrap_or_default();
        let text = format!("{template}\nData:\n```json\n{quoted}\n```");
        let task = Task {
            id: TaskId::new(),
            workspace_id: ws,
            target: TaskTarget::AgentInstance {
                agent_instance_id: self.config.ordin_agent_instance_id,
            },
            command: None,
            free_text: Some(text),
            arguments: vec![],
            attached_artefacts: vec![],
            run_again_of: None,
            requested_at: Timestamp::now(),
        };
        let (run_id, end) = self.run_task(task).await?;
        let _ = run_id;
        match end {
            (RunEndReason::Completed, Some(text), _) => {
                let mut t = text;
                if t.len() > REPLY_MAX_BYTES {
                    let mut cut = REPLY_MAX_BYTES;
                    while !t.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    t.truncate(cut);
                }
                Ok(t)
            }
            _ => Err("SUP_RUN_FAILED".to_owned()),
        }
    }

    /// Submits a task and waits for its end: (reason, final message, approvals asked).
    async fn run_task(
        &self,
        task: Task,
    ) -> Result<(RunId, (RunEndReason, Option<String>, u32)), String> {
        let run_id = self
            .ports
            .supervisor
            .submit_task(task, WorkKind::Documents, false, None)
            .await
            .map_err(|e| e.code)?;
        let mut events = self
            .ports
            .supervisor
            .subscribe(run_id)
            .map_err(|e| e.code)?;
        let mut approvals = 0;
        loop {
            match events.recv().await {
                Ok(RunEvent::Paused {
                    reason: PauseReason::Approval { .. },
                }) => approvals += 1,
                Ok(RunEvent::Ended { end }) => {
                    return Ok((run_id, (end.reason, end.final_message, approvals)))
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => return Err("SUP_RUN_NOT_FOUND".to_owned()),
            }
        }
    }

    /// A fixed turn in the workspace's conversation, its text or `OH_DEPENDENCY_FAILED`.
    async fn ordin_turn(&self, ws: WorkspaceId, text: String) -> Result<String, OhError> {
        self.ensure_conversation(ws).await?;
        match self.run_turn(ws, text).await? {
            TurnEnd::Text(t) => Ok(t),
            TurnEnd::Lost => Err(OhError::DependencyFailed {
                dependency: "supervisor",
                code: "OH_COORDINATOR_SESSION_LOST".into(),
                retryable: true,
            }),
        }
    }
}

#[async_trait::async_trait]
impl AuthoringHost for OrdinHost {
    /// Step 42.1.
    // spec: ordin-desktop/components/OrdinHost#OH-REC-3
    async fn start_agent_maker(&self, req: AgentMakerStart) -> Result<RunId, OhError> {
        // The Package Catalogue's §F check is due before a session starts on the instance.
        self.ports
            .catalogue
            .ensure_ready(self.config.agent_maker_instance_id)
            .await
            .map_err(|e| self.dep("catalogue", e))?;
        let mode = match &req.mode {
            AgentMakerMode::WholeAgent { agent_draft_id } => {
                format!("Mode: whole agent, draft {agent_draft_id}.")
            }
            AgentMakerMode::OnePart { part_draft_id } => {
                format!("Mode: one part, draft {part_draft_id}.")
            }
            AgentMakerMode::Lifecycle { part_draft_id } => {
                format!("Mode: lifecycle, draft {part_draft_id}.")
            }
            AgentMakerMode::Fix { finding } => format!("Mode: fix the finding {finding}."),
        };
        let mut text = req.request.clone();
        for r in &req.requirements {
            text.push_str(&format!("\n{r}"));
        }
        text.push_str(&format!("\n{mode}"));
        let task = Task {
            id: TaskId::new(),
            workspace_id: req.workspace_id,
            target: TaskTarget::AgentInstance {
                agent_instance_id: self.config.agent_maker_instance_id,
            },
            command: None,
            free_text: Some(text),
            arguments: vec![],
            attached_artefacts: vec![],
            run_again_of: None,
            requested_at: Timestamp::now(),
        };
        self.ports
            .supervisor
            .submit_task(task, WorkKind::Documents, true, None)
            .await
            .map_err(|e| self.dep("supervisor", e))
    }

    /// Step 42.2: unknown template → `OH_TEMPLATE_UNKNOWN`, and no run starts.
    async fn generate(&self, req: GenerationRequest) -> Result<String, OhError> {
        let text = template(req.template_id, &req.data).ok_or(OhError::TemplateUnknown)?;
        self.generate_text(&text, &req.data, Some(req.workspace_id))
            .await
            .map_err(|code| OhError::DependencyFailed {
                dependency: "supervisor",
                code,
                retryable: true,
            })
    }

    /// Step 42.3 (SA-CLOSE-06): the rehearsal runs on a trial instance of the draft (`add_trial`),
    /// and `tools_used` is empty in R1 because no run event names a tool.
    // spec: ordin-desktop/components/OrdinHost#SA-CLOSE-06
    // spec: ordin-desktop/components/OrdinHost#OH-R1-2
    async fn rehearse(
        &self,
        req: RehearsalRunRequest,
    ) -> Result<ordin_checker::RunEvidence, OhError> {
        let instance = self
            .add_trial(TrialRequest {
                agent_draft_id: req.agent_draft_id,
                agent_repository_id: req.agent_repository_id,
                worktree: req.worktree.clone(),
                agent: String::new(),
                workspace_id: req.workspace_id,
            })
            .await?;
        let mut text = req.situation.clone();
        for d in &req.documents {
            text.push_str(&format!("\nAttached: {d}"));
        }
        let task = Task {
            id: TaskId::new(),
            workspace_id: req.workspace_id,
            target: TaskTarget::AgentInstance {
                agent_instance_id: instance,
            },
            command: None,
            free_text: Some(text),
            arguments: vec![],
            attached_artefacts: vec![],
            run_again_of: None,
            requested_at: Timestamp::now(),
        };
        let (run_id, (_, message, approvals)) =
            self.run_task(task)
                .await
                .map_err(|code| OhError::DependencyFailed {
                    dependency: "supervisor",
                    code,
                    retryable: true,
                })?;
        let external = self
            .ports
            .journal
            .external_actions_for_run(run_id)
            .await
            .map_err(|e| self.dep("journal", e))?;
        let names = self
            .ports
            .journal
            .artefact_names(run_id)
            .await
            .map_err(|e| self.dep("journal", e))?;
        Ok(ordin_checker::RunEvidence {
            run_id,
            final_message: message.unwrap_or_default(),
            tools_used: vec![],
            approvals_asked: approvals,
            external_actions: external,
            artefact_names: names,
        })
    }

    /// Step 42.4.
    async fn add_trial(&self, req: TrialRequest) -> Result<AgentInstanceId, OhError> {
        let locale = self.ports.localisation.active_locale();
        self.ports
            .catalogue
            .add_trial(ordin_package_catalogue::TrialInput {
                agent_draft_id: req.agent_draft_id,
                agent_repository_id: req.agent_repository_id,
                worktree: req.worktree,
                agent: req.agent,
                workspace_id: req.workspace_id,
                workspace_languages: vec![locale.clone()],
                interface_locale: locale,
            })
            .await
            .map_err(|e| self.dep("catalogue", e))
    }
}

#[async_trait::async_trait]
impl HandoverHost for OrdinHost {
    /// Step 43: one Ordin turn; a reply without the four sections → `OH_DEPENDENCY_FAILED`.
    async fn draft_note(
        &self,
        workspace: WorkspaceId,
        results: &[String],
        stage: Option<&str>,
    ) -> Result<[NoteSection; 4], OhError> {
        let text = format!(
            "Draft a handover note for stage {} with four sections: {}. Results: {}.",
            stage.unwrap_or("the next stage"),
            HEADINGS.join(", "),
            serde_json::to_string(results).unwrap_or_default()
        );
        let reply = self.ordin_turn(workspace, text).await?;
        let failed = || OhError::DependencyFailed {
            dependency: "ordin",
            code: "OH_DEPENDENCY_FAILED".into(),
            retryable: true,
        };
        let mut sections: Vec<NoteSection> = Vec::new();
        for (i, heading) in HEADINGS.iter().enumerate() {
            let start = reply.find(heading).ok_or_else(failed)? + heading.len();
            let end = HEADINGS
                .get(i + 1)
                .and_then(|next| reply[start..].find(next).map(|p| start + p))
                .unwrap_or(reply.len());
            let body = reply[start..end]
                .trim_matches(|c: char| c == '#' || c == ':' || c.is_whitespace())
                .to_owned();
            sections.push(NoteSection {
                heading: (*heading).to_owned(),
                body,
                drafted_by_ordin: true,
            });
        }
        sections.try_into().map_err(|_| failed())
    }

    /// Step 43: a stage not in `choices` → `OH_DEPENDENCY_FAILED`.
    async fn recommend_next_stage(
        &self,
        workspace: WorkspaceId,
        choices: &[StageChoice],
    ) -> Result<(String, String), OhError> {
        let list: Vec<String> = choices
            .iter()
            .map(|c| format!("{} — {}", c.stage, c.when))
            .collect();
        let text = format!(
            "Recommend one of these stages and give a one-line reason: {}.",
            list.join("; ")
        );
        let reply = self.ordin_turn(workspace, text).await?;
        let stage =
            choices
                .iter()
                .find(|c| reply.contains(&c.stage))
                .ok_or(OhError::DependencyFailed {
                    dependency: "ordin",
                    code: "OH_DEPENDENCY_FAILED".into(),
                    retryable: true,
                })?;
        let reason = reply.lines().last().unwrap_or("").trim().to_owned();
        Ok((stage.stage.clone(), reason))
    }

    /// Step 43: the note as the first turn of the workspace's conversation.
    async fn start_from_handover(
        &self,
        workspace: WorkspaceId,
        note: &HandoverNote,
        stage_agent: Option<String>,
    ) -> Result<(), OhError> {
        self.ensure_conversation(workspace).await?;
        let mut text = String::from("A handover note, quoted as data:\n");
        for s in &note.sections {
            text.push_str(&format!("## {}\n{}\n", s.heading, s.body));
        }
        if let Some(agent) = stage_agent {
            text.push_str(&format!("The next stage's agent is {agent}."));
        }
        self.send_turn(workspace, Turn::fixed(text)).await?;
        Ok(())
    }
}
