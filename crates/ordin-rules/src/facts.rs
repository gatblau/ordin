//! `RunFacts`: per-run facts kept on every decision so a run paused on one instance resumes on
//! another (RulesEngine.md *Approach*, *Internal Logic* B.1 and G.1; C-80).

use std::collections::BTreeMap;

use ordin_types::{AgentInstanceId, ApprovalId, CrewCharterId, RunId, WorkspaceId};
use serde::{Deserialize, Serialize};

use crate::types::{RunKey, RunRestriction};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RunFacts {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub crew_charter_id: Option<CrewCharterId>,
    pub commands: Vec<String>,
    /// No table holds a message: the text is kept in memory for the start's own matching and is
    /// never written to `rules_run_facts`.
    #[serde(skip)]
    pub text_cut: String,
    pub scope: Option<String>,
    pub asker: Option<String>,
    /// The asker, or the started-by identifier when there is none (`repeated-message`).
    pub asker_key: String,
    pub subset: Option<Vec<String>>,
    pub single_call: bool,
    /// The start decision's `limit_seconds`, for a second instance (SA-RECON-12). Absent from a
    /// record written before it was kept, which reads as `None`.
    #[serde(default)]
    pub limit_seconds: Option<u32>,
    /// The control that decided the start (named by D.4 and D.5.5 denials).
    pub deciding_control: Option<String>,
    pub set_aside: Vec<String>,
    pub leases: Vec<String>,
    pub tools_called: BTreeMap<String, u32>,
    pub tool_calls: Vec<String>,
    pub changed_work_folder: bool,
    pub ended: bool,
    /// Unix milliseconds; reset on resume (`elapsed`).
    pub started_at: i64,
    /// End gate → refusals so far (E.4).
    pub refusals: BTreeMap<String, u8>,
    pub restriction: Option<RunRestriction>,
    /// The open approval of an `EndDecision::Ask`.
    pub end_approval: Option<ApprovalId>,
}

impl RunFacts {
    pub(crate) fn key(&self) -> RunKey {
        RunKey {
            run_id: self.run_id,
            agent_instance_id: self.agent_instance_id,
            workspace_id: self.workspace_id,
        }
    }

    /// The scope text (`agent-wide` when none).
    pub(crate) fn scope_text(&self) -> String {
        self.scope
            .clone()
            .unwrap_or_else(|| "agent-wide".to_owned())
    }

    /// `called` counts: `task:<name>` for a sub-agent, with a call naming none under
    /// `task:general-purpose`; the bare `task` sums every `task:*`.
    pub(crate) fn called(&self, key: &str) -> u32 {
        if key == "task" {
            self.tools_called
                .iter()
                .filter(|(k, _)| k.as_str() == "task" || k.starts_with("task:"))
                .map(|(_, n)| *n)
                .sum()
        } else {
            let unnamed = if key == crate::decide::DEFAULT_SUB_AGENT {
                self.tools_called.get("task").copied().unwrap_or(0)
            } else {
                0
            };
            self.tools_called.get(key).copied().unwrap_or(0) + unnamed
        }
    }
}
