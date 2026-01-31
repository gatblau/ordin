//! Shared helpers of the integration tests: the Crew Orchestrator's own harness of real components
//! (Journal, Approval Service, Rules Engine, Usage Planner, Session Supervisor, Crew Orchestrator on
//! a temporary SQLite file), and an Ordin Host whose Supervisor and Orchestrator are those real ones,
//! every other dependency a stand-in of `ordin_host::testkit`.
#![allow(dead_code)]

/// Reused, not copied: the same harness the Crew Orchestrator's integration tests run on.
#[path = "../../../ordin-crew-orchestrator/tests/support/mod.rs"]
pub mod orch;

use std::sync::Arc;
use std::time::Duration;

use ordin_host::ports::{RealOrchestrator, RealSupervisor};
use ordin_host::testkit::{config, policy, Fakes};
use ordin_host::{OrdinHost, OrdinHostConfig};
use ordin_journal::{ApplyMode, MigrationRegistry, MIGRATIONS};
use ordin_runtime_adapter::{PauseReason, ResumeDecision, RunEvent};
use ordin_types::wire::run::RunTarget;
use ordin_types::{AgentInstanceId, RunId};
use tokio_util::sync::CancellationToken;

pub struct World {
    pub h: orch::Harness,
    pub fakes: Fakes,
    pub host: Arc<OrdinHost>,
    pub config: OrdinHostConfig,
    pub stop: CancellationToken,
}

impl Drop for World {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// Applies this crate's migrations to the harness's datastore.
async fn migrate(h: &orch::Harness) {
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("register the journal");
    // Every module the harness applied, so the registry knows each one's version.
    registry
        .register(ordin_approval::ApprovalService::migrations())
        .unwrap();
    registry
        .register(ordin_rules::RulesEngine::migrations())
        .unwrap();
    registry
        .register(ordin_usage_planner::UsagePlanner::migrations())
        .unwrap();
    registry
        .register(ordin_session_supervisor::SessionSupervisor::migrations())
        .unwrap();
    registry
        .register(ordin_crew_orchestrator::CrewOrchestrator::migrations())
        .unwrap();
    registry
        .register(OrdinHost::migrations())
        .expect("register the ordin host module");
    registry
        .apply(&h.store, ApplyMode::ApplyPending)
        .await
        .expect("apply the ordin host migrations");
}

impl World {
    /// Members' runs follow the harness's `FakeAdapter`; Ordin's run is driven by the test.
    pub async fn new() -> World {
        World::over(orch::Harness::new().await).await
    }

    /// Local runs replay `script` through the Runtime Adapter interface's `ScriptedAdapter`.
    pub async fn scripted(script: Vec<RunEvent>) -> World {
        World::over(orch::Harness::scripted(script).await).await
    }

    async fn over(h: orch::Harness) -> World {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        migrate(&h).await;
        let fakes = Fakes::new();
        let mut ports = fakes.ports.clone();
        ports.supervisor = Arc::new(RealSupervisor(h.supervisor.clone()));
        ports.orchestrator = Arc::new(RealOrchestrator(h.orch.clone()));
        let mut cfg = config();
        cfg.coordinator_turn_timeout = Duration::from_secs(10);
        let host = OrdinHost::with_ports(cfg.clone(), policy(), ports, h.pool().clone());
        let stop = CancellationToken::new();
        host.start(stop.clone()).await.expect("the host starts");
        World {
            h,
            fakes,
            host,
            config: cfg,
            stop,
        }
    }

    pub fn ordin(&self) -> AgentInstanceId {
        self.config.ordin_agent_instance_id
    }

    /// The runs the adapter started for Ordin.
    pub fn ordin_runs(&self) -> Vec<RunId> {
        let ordin = self.ordin();
        self.h
            .local
            .starts
            .lock()
            .unwrap()
            .iter()
            .filter(|s| matches!(s.target, RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == ordin))
            .map(|s| s.run_id)
            .collect()
    }

    /// Ordin answers on its run (a pause for the person, as Claude Code's end of turn).
    pub fn ordin_says(&self, run_id: RunId, text: &str) {
        self.h.local.push(
            run_id,
            RunEvent::Paused {
                reason: PauseReason::Person {
                    question: text.to_owned(),
                },
            },
        );
    }

    /// The turns the host sent to Ordin's run, in order.
    pub fn turns_to(&self, run_id: RunId) -> Vec<String> {
        self.h
            .local
            .resumes
            .lock()
            .unwrap()
            .iter()
            .filter(|(r, _)| *r == run_id)
            .filter_map(|(_, d)| match d {
                ResumeDecision::Answer { text } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }
}

/// Waits up to `secs` seconds for `cond`.
pub async fn until(secs: u64, mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..(secs * 50) {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cond()
}

/// Ordin's session as a script: each answer is given after a short wait (the host reads the run
/// once `start_conversation` returns, as Claude Code's first answer takes seconds), then the session
/// waits for the next turn. After the last answer it waits until it is stopped.
pub fn ordin_script(answers: &[&str]) -> orch::Plan {
    let mut plan = vec![orch::started()];
    for (i, text) in answers.iter().enumerate() {
        if i > 0 {
            plan.push(orch::Act::AwaitResume);
            plan.push(orch::Act::Event(RunEvent::Resumed));
        }
        plan.push(orch::Act::Sleep(50));
        plan.push(orch::Act::Event(RunEvent::Paused {
            reason: PauseReason::Person {
                question: (*text).to_owned(),
            },
        }));
    }
    plan.push(orch::Act::Hang);
    plan
}
