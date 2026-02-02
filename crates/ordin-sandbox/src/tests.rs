//! Scenarios that need the crate's private seams: a wrong compiled-in digest, a sweep without the
//! 60-second tick, and the module route's handle checks.

#[path = "../tests/support/mod.rs"]
mod support;

use std::sync::Arc;
use std::time::Duration;

use ordin_mcp_front::ToolCaller;
use ordin_types::{RunId, WorkspaceId};
use sandboxd::broker::BrokerCall;
use serde_json::json;
use support::*;

use crate::agent::OrdinBroker;
use crate::{Sandbox, SandboxStatus};

// Scenario: Error — guest digest mismatch at start
#[tokio::test]
async fn guest_digest_mismatch_at_start() {
    let p = parts().await;
    let wrong = "0".repeat(64);
    let wrong: &'static str = Box::leak(wrong.into_boxed_str());
    let sandbox =
        Sandbox::start_with(p.deps(Duration::from_secs(60)), super::GUEST_PY, wrong).await;
    assert_eq!(
        sandbox.status(),
        SandboxStatus::Unavailable {
            code: "SBX_GUEST_DIGEST_MISMATCH"
        }
    );
    let provider = sandbox.tool_provider();
    assert!(provider.servers()[0].tools.is_empty());
    let grant = ordin_mcp_front::SessionGrant {
        run_id: RunId::new(),
        workspace_id: WorkspaceId::new(),
        agent_instance_id: ordin_types::AgentInstanceId::new(),
        harness: ordin_types::domain::identity::HarnessKind::ClaudeCode,
        profile: ordin_mcp_front::ProgramProfile::Desktop,
        caller: ToolCaller::Agent,
        scope_id: None,
        coding_task_id: None,
        only_servers: None,
    };
    assert!(provider.offer(&grant).await.unwrap().is_empty());
}

// Scenario: Edge — idle work session is deleted after the profile limit
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_work_session_is_deleted_after_the_profile_limit() {
    let p = parts().await;
    let sandbox = Sandbox::start(p.deps(Duration::from_secs(1))).await;
    let f = Fixture {
        dir: p.dir,
        journal: p.journal,
        identity: p.identity,
        grants: p.grants,
        places: p.places,
        dispatch: p.dispatch,
        sandbox,
        agent: ordin_types::AgentInstanceId::new(),
        workspace: WorkspaceId::new(),
    };
    let run = RunId::new();
    f.call(
        run,
        json!({"code": "open('/work/a.txt','w').write('x')", "workSession": true}),
    )
    .await
    .expect("the run succeeds");
    assert!(f.session_dir(run).exists());
    tokio::time::sleep(Duration::from_millis(1100)).await;
    f.sandbox.sweep_now().await;
    assert!(!f.session_dir(run).exists());
    let lines = log_lines(&["sandbox.session_deleted", &run.to_string()]);
    assert!(lines.iter().any(|l| l.contains("idle")), "{lines:?}");
}

// Error Table: module call with an unknown or expired capability reference
#[tokio::test]
async fn a_module_call_with_an_unknown_reference_is_refused_inside_the_room() {
    let f = fixture().await;
    let core = &f.sandbox.core;
    let broker = OrdinBroker {
        inner: Arc::new(sandboxd::broker::IdentityBroker::new(
            Arc::new(sandboxd::policy::PolicyEngine::load("{}", None, &|_, _| false).unwrap()),
            Arc::new(sandboxd::audit::AuditLogger::new(
                vec![1; 32],
                Box::new(crate::agent::JournalAuditSink {
                    tx: core.audit_tx.clone(),
                    workspace: WorkspaceId::new(),
                }),
            )),
            Arc::new(sandboxd::obo::OboClient::new(String::new()).unwrap()),
            Arc::new(
                sandboxd::downstream::DownstreamClient::new(1024, Duration::from_secs(1), false)
                    .unwrap(),
            ),
            Arc::new(crate::agent::RunCredentials::default()),
            1024,
            Arc::new(std::collections::HashMap::<String, String>::new()),
        )),
        module: core.module.clone(),
    };
    // An expired reference.
    let expired = [9u8; 16];
    core.module.refs.lock().unwrap().insert(
        expired,
        (
            "nobody".to_owned(),
            std::time::Instant::now() - Duration::from_secs(1),
        ),
    );
    let r = broker
        .call_tool_boxed(&expired, "email_search", &json!({}))
        .await
        .expect("an envelope");
    assert!(r.untrusted && r.is_error);
    assert_eq!(
        r.parts,
        vec![sandboxd::types::UntrustedPart::Json {
            body: br#"{"code":"SBX_MODULE_HANDLE_INVALID"}"#.to_vec()
        }]
    );
    // A reference this route did not mint goes to the broker, which refuses it.
    assert!(broker
        .call_tool_boxed(&[1u8; 16], "x", &json!({}))
        .await
        .is_err());
}
