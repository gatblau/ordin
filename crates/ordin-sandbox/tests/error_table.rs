//! One test per row of Sandbox.md's Error Table that `tests/scenarios.rs` does not already cover
//! (X1). Rows reached only through a `sandboxd` fault this harness cannot cause (guest start
//! failure, run timeout, an interaction refused without a recorded fault, a retryable credential
//! fault, the in-process configuration refused) are covered by the mapping unit tests in
//! `src/call.rs` and `src/error.rs`.

mod support;

use std::time::Duration;

use ordin_mcp_front::{ServerName, ToolKey};
use ordin_sandbox::{
    KeyPlacementSpec, Sandbox, SandboxCapability, SandboxCapabilityKind, SandboxCredential,
    SandboxStatus,
};
use ordin_types::{PlaceId, RunId};
use serde_json::json;
use support::*;

fn capability(name: &str, path_allow: &str, credential: SandboxCredential) -> SandboxCapability {
    SandboxCapability {
        name: name.to_owned(),
        kind: SandboxCapabilityKind::Rest {
            host: "crm.example.com".to_owned(),
            path_allow: vec![path_allow.to_owned()],
            methods: vec!["GET".to_owned()],
        },
        allow_write: false,
        allow_private_network: false,
        credential,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_name_not_offered() {
    let f = fixture().await;
    let key = ToolKey {
        server: ServerName::new("sandbox").unwrap(),
        tool: "shell_sandbox".to_owned(),
    };
    let e = f
        .sandbox
        .tool_provider()
        .call(f.ctx(RunId::new()), &key, json!({"code": "echo"}))
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_TOOL_UNKNOWN");
    assert_eq!(
        e.message,
        "Use python_sandbox; that tool is not offered here."
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arguments_malformed_are_an_argument_problem() {
    let f = fixture().await;
    let provider = f.sandbox.tool_provider();
    let p = provider
        .check_arguments(
            &python(),
            &json!({"code": "x", "requestedCapabilities": ["ordin_tools"]}),
        )
        .expect_err("refused");
    assert_eq!(p.at, "/requestedCapabilities/0");
    assert!(provider
        .check_arguments(&python(), &json!({"code": "print(1)"}))
        .is_ok());
    // `call` refuses the same arguments with SBX_ARGS_INVALID.
    let e = f
        .call(RunId::new(), json!({"code": ""}))
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_ARGS_INVALID");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sixteen_work_sessions_open() {
    let f = fixture().await;
    for _ in 0..16 {
        f.call(
            RunId::new(),
            json!({"code": "print(1)", "workSession": true}),
        )
        .await
        .expect("a work session opens");
    }
    let e = f
        .call(
            RunId::new(),
            json!({"code": "print(1)", "workSession": true}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_SESSIONS_FULL");
    assert!(e.retryable);
    // A run without a work session still runs.
    f.call(RunId::new(), json!({"code": "print(1)"}))
        .await
        .expect("runs without a work session");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_guest_slot_after_shutdown_and_folders_deleted() {
    let f = fixture().await;
    let run = RunId::new();
    f.call(run, json!({"code": "print(1)", "workSession": true}))
        .await
        .expect("a work session opens");
    assert!(f.session_dir(run).exists());
    f.sandbox.shutdown().await;
    assert!(!f.session_dir(run).exists());
    let e = f
        .call(RunId::new(), json!({"code": "print(1)"}))
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_BUSY");
    assert!(e.retryable);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manifest_refused_by_the_policy_engine() {
    let f = fixture().await;
    // An invalid regular expression in `pathAllow`.
    f.grants
        .set(vec![capability("crm", "([", SandboxCredential::None)]);
    let e = f
        .call(RunId::new(), json!({"code": "print(1)"}))
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_POLICY_INVALID");
    assert!(log_lines(&["sandbox.policy_invalid", &f.agent.to_string()]).len() == 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_key_not_entered() {
    let f = fixture().await;
    f.grants.set(vec![capability(
        "crm",
        "^/v1/.*$",
        SandboxCredential::AgentKey {
            key_name: "crm_key".to_owned(),
            placement: KeyPlacementSpec::Header {
                name: "X-Api-Key".to_owned(),
                scheme: None,
            },
        },
    )]);
    let e = f
        .call(RunId::new(), json!({"code": "print(1)"}))
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_KEY_MISSING");
    assert_eq!(
        e.message,
        "The key `crm_key` for this agent has not been entered."
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn input_file_not_found_too_large_and_unreadable() {
    let f = fixture().await;
    let place = PlaceId::new();
    f.places.put(place, "small.csv", b"x");
    f.places
        .put(place, "big.bin", &vec![0u8; 16 * 1024 * 1024 + 1]);
    let input = |path: &str| json!({"code": "print(1)", "inputs": [{"placeId": place.to_string(), "path": path}]});

    let e = f
        .call(RunId::new(), input("missing.csv"))
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_INPUT_NOT_FOUND");
    assert_eq!(e.message, "`missing.csv` was not found in that place.");

    let e = f
        .call(RunId::new(), input("big.bin"))
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_INPUTS_TOO_LARGE");

    *f.places.unreadable.lock().unwrap() = true;
    let e = f
        .call(RunId::new(), input("small.csv"))
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_INPUT_UNREADABLE");
    assert!(e.retryable);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_file_entries_are_refused() {
    let f = fixture().await;
    let e = f
        .call(
            RunId::new(),
            json!({"code": "print(1)", "files": [
                {"path": "a.txt", "contentBase64": "eA=="},
                {"path": "a.txt", "contentBase64": "eQ=="}
            ]}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_FILE_INVALID");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn output_artefact_not_saved() {
    let f = fixture().await;
    *f.places.fail_writes.lock().unwrap() = true;
    let run = RunId::new();
    let e = f
        .call(
            run,
            json!({"code": "open('/work/out.txt','w').write('x')", "workSession": true}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_OUTPUT_SAVE_FAILED");
    assert!(e.retryable);
    // The work session still holds the output, so nothing is lost.
    assert_eq!(
        std::fs::read(f.session_dir(run).join("out.txt")).unwrap(),
        b"x"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn module_calls_over_fifty_exhaust_the_run_budget() {
    let f = fixture().await;
    f.dispatch.offer(vec![tool("mail", "email_search")]);
    let result = f
        .call(
            RunId::new(),
            json!({"code": "for i in range(51):\n    r = ordin_tools.email_search(query='x')\nprint(r['is_error'], r['parts'][0]['json']['code'])"}),
        )
        .await
        .expect("the run returns");
    assert_eq!(
        result["stdout"], "True SBX_RUN_BUDGET_EXHAUSTED\n",
        "{result}"
    );
    assert_eq!(f.dispatch.calls.lock().unwrap().len(), 50);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_run_returns_only_the_planned_calls() {
    let f = fixture().await;
    f.grants
        .set(vec![capability("crm", "^/v1/.*$", SandboxCredential::None)]);
    let result = f
        .call(
            RunId::new(),
            json!({"code": "api.crm.get('/v1/x')", "requestedCapabilities": ["crm"], "dryRun": true}),
        )
        .await
        .expect("a plan");
    assert_eq!(result["plannedCalls"][0]["host"], "crm.example.com");
    assert!(result.get("stdout").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_closed_ends_the_work_session() {
    let f = fixture().await;
    let run = RunId::new();
    f.call(run, json!({"code": "print(1)", "workSession": true}))
        .await
        .expect("a work session opens");
    assert!(f.session_dir(run).exists());
    f.sandbox.tool_provider().session_closed(run).await;
    assert!(!f.session_dir(run).exists());
}

// The secret store cannot be written: the audit key cannot be created (A.1).
#[cfg(unix)]
#[tokio::test]
async fn audit_key_unavailable_at_start() {
    use std::os::unix::fs::PermissionsExt;
    let p = parts().await;
    let secrets = p.dir.0.join("identity").join("secrets");
    std::fs::create_dir_all(&secrets).expect("secrets folder");
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o500)).expect("read-only");
    let sandbox = Sandbox::start(p.deps(Duration::from_secs(60))).await;
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o700)).expect("restore");
    assert_eq!(
        sandbox.status(),
        SandboxStatus::Unavailable {
            code: "SBX_AUDIT_KEY_UNAVAILABLE"
        }
    );
    assert!(sandbox.tool_provider().servers()[0].tools.is_empty());
}
