//! C-17 and C-23 spike (plan 08, Phase 1).
//!
//! C-17: `sandboxd`'s library target embeds in the core without its daemon. The test
//! builds the crate's own components (`PolicyEngine`, `SandboxRuntime`, `SessionManager`,
//! `SandboxController`, with stand-ins for the interaction surface and broker that ordin
//! replaces) directly, runs one Python snippet in-process, and never starts `sandboxd`'s
//! `main`, control socket or `mcp-stdio` front door.
//!
//! C-23: the cold start (guest compile plus first run) and a warm run are timed and logged.

// spec: ordin-desktop/components/Sandbox#C-17

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sandboxd::broker::{BrokerCall, BrokerError};
use sandboxd::controller::{
    CapabilityMinter, IdTokenSink, Interactor, RunOutcome, SandboxController,
};
use sandboxd::interaction::{InteractionError, InteractionOutcome};
use sandboxd::policy::PolicyEngine;
use sandboxd::runtime::{Limits, SandboxRuntime};
use sandboxd::session::SessionManager;
use sandboxd::types::{
    CapabilityHandle, ClientIdentity, InteractionRequired, Params, Principal, ResolvedCapability,
    RunRequest, SessionHandle, UntrustedMcpResult, UntrustedResponse,
};

const GUEST: &[u8] = include_bytes!("../../../sandbox/sandbox-host/guest/pysandbox-guest.wasm");

/// No capabilities: the snippet makes no brokered call.
const MANIFEST: &str = r#"{"capabilities":{}}"#;

/// The daemon's interaction surface, which ordin replaces; never reached by this snippet.
struct NoInteraction;
impl Interactor for NoInteraction {
    fn require_boxed<'a>(
        &'a self,
        _who: &'a ClientIdentity,
        _what: InteractionRequired,
    ) -> Pin<Box<dyn Future<Output = Result<InteractionOutcome, InteractionError>> + Send + 'a>>
    {
        Box::pin(async { Err(InteractionError::Denied) })
    }
}

/// The daemon's broker, which ordin replaces; never reached by this snippet.
struct NoBroker;
impl CapabilityMinter for NoBroker {
    fn mint_caps(
        &self,
        _p: &Principal,
        _run_id: &str,
        _client_label: &str,
        _caps: &[ResolvedCapability],
    ) -> Vec<CapabilityHandle> {
        Vec::new()
    }
}
impl BrokerCall for NoBroker {
    fn call_boxed<'a>(
        &'a self,
        _cap_id: &'a [u8; 16],
        _verb: &'a str,
        _path: &'a str,
        _params: &'a Params,
        _body: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<UntrustedResponse, BrokerError>> + Send + 'a>> {
        panic!("the spike snippet makes no brokered call")
    }

    fn call_tool_boxed<'a>(
        &'a self,
        _cap_id: &'a [u8; 16],
        _tool: &'a str,
        _arguments: &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<UntrustedMcpResult, BrokerError>> + Send + 'a>> {
        panic!("the spike snippet makes no brokered tool call")
    }
}

struct NoSink;
impl IdTokenSink for NoSink {
    fn set_id_token(&self, _id_token: String) {}
    fn set_access_token(&self, _access_token: String) {}
}

fn request(run_id: &str) -> RunRequest {
    RunRequest {
        code: "print(1 + 1)".into(),
        requested_capabilities: Vec::new(),
        timeout_ms: None,
        dry_run: false,
        workspace_id: "ws".into(),
        run_id: Some(run_id.into()),
        files: None,
    }
}

fn session() -> SessionHandle {
    SessionHandle {
        client: ClientIdentity {
            principal: "ordin".into(),
            client_label: "c17-spike".into(),
        },
        workspace_id: "ws".into(),
    }
}

async fn run_once(controller: &SandboxController, run_id: &str) -> String {
    match controller
        .run(request(run_id), session())
        .await
        .expect("run ok")
    {
        RunOutcome::Run(r) => {
            assert_eq!(r.exit_code, 0, "stderr={:?}", r.stderr);
            r.stdout
        }
        other => panic!("expected Run, got {other:?}"),
    }
}

/// True when a process named `sandboxd` is running (Linux `/proc`; elsewhere unchecked).
fn sandboxd_process_running() -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|e| {
        std::fs::read_to_string(e.path().join("comm"))
            .map(|c| c.trim() == "sandboxd")
            .unwrap_or(false)
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn c17_python_runs_in_process_without_the_daemon() {
    assert!(
        !sandboxd_process_running(),
        "a sandboxd daemon is already running"
    );

    let cold_start = Instant::now();
    let broker = Arc::new(NoBroker);
    let runtime = Arc::new(
        SandboxRuntime::new(
            &SandboxRuntime::digest_of(GUEST),
            GUEST,
            broker.clone() as Arc<dyn BrokerCall>,
        )
        .expect("runtime builds"),
    );
    let controller = SandboxController::new(
        Arc::new(PolicyEngine::load(MANIFEST, None, &|_, _| true).expect("manifest loads")),
        Arc::new(NoInteraction),
        broker as Arc<dyn CapabilityMinter>,
        runtime,
        Arc::new(SessionManager::new(50)),
        Arc::new(NoSink),
        "https://idp.invalid".to_string(),
        Limits {
            fuel: Some(u64::MAX),
            epoch_deadline: Duration::from_secs(60),
            ..Limits::default()
        },
    );
    let cold_out = run_once(&controller, "r-cold").await;
    let cold = cold_start.elapsed();

    let warm_start = Instant::now();
    let warm_out = run_once(&controller, "r-warm").await;
    let warm = warm_start.elapsed();

    assert_eq!(cold_out.trim(), "2");
    assert_eq!(warm_out.trim(), "2");
    assert!(
        !sandboxd_process_running(),
        "the spike started a sandboxd daemon"
    );

    // C-23 measurement, read from the test output (`--nocapture`).
    println!("C-23 cold start (compile + first run): {cold:?}");
    println!("C-23 warm run: {warm:?}");
}
