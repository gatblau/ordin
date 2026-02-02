//! One test per Gherkin scenario of Sandbox.md, named after it (X2). The guest is the real
//! committed Python guest running in-process; the Journal and Identity are real; the program's
//! seams are doubles. Two scenarios need the crate's private seams and live in `src/tests.rs`
//! (guest digest mismatch, idle work session).

mod support;

use std::time::Duration;

use ordin_mcp_front::ToolCaller;
use ordin_sandbox::{
    GuestKind, SandboxCapability, SandboxCapabilityKind, SandboxCredential, SandboxStatus,
};
use ordin_types::domain::identity::CredentialKind;
use ordin_types::domain::places::ArtefactLocation;
use ordin_types::{CredentialId, PlaceId, RunId};
use serde_json::json;
use support::*;

fn rest(name: &str, credential: SandboxCredential) -> SandboxCapability {
    SandboxCapability {
        name: name.to_owned(),
        kind: SandboxCapabilityKind::Rest {
            host: format!("{name}.example.com"),
            path_allow: vec!["^/v1/.*$".to_owned()],
            methods: vec!["GET".to_owned()],
        },
        allow_write: false,
        allow_private_network: false,
        credential,
    }
}

fn person(kind: CredentialKind, credential_id: CredentialId) -> SandboxCredential {
    SandboxCredential::PersonToken {
        credential_id,
        kind,
    }
}

// Scenario: Happy path — Python with a place input returns an artefact
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn happy_path_python_with_a_place_input_returns_an_artefact() {
    let f = fixture().await;
    let suppliers = PlaceId::new();
    let mut csv = String::from("supplier,rate\n");
    for i in 0..42 {
        csv.push_str(&format!("s{i},{i}\n"));
    }
    f.places.put(suppliers, "Q3/rates.csv", csv.as_bytes());
    let run = RunId::new();
    let code = "rows=[l.split(',') for l in open('/work/rates.csv').read().splitlines()[1:]]\nopen('/work/summary.csv','w').write('supplier,avg\\n'+'\\n'.join(r[0]+','+r[1] for r in rows))\nprint(len(rows))";
    let result = f
        .call(
            run,
            json!({"code": code, "inputs": [{"placeId": suppliers.to_string(), "path": "Q3/rates.csv"}]}),
        )
        .await
        .expect("the run succeeds");
    assert_eq!(result["exitCode"], 0, "{result}");
    assert_eq!(result["stdout"], "42\n");
    let saved_as = format!("sandbox-output/{run}/summary.csv");
    assert_eq!(
        result["changes"],
        json!([{"path": "summary.csv", "kind": "added", "savedAs": saved_as}])
    );
    assert!(result.get("sessionChanges").is_none());
    assert!(f
        .places
        .written(&saved_as)
        .expect("written to the agent's folder")
        .starts_with(b"supplier,avg\n"));

    let artefacts = f.artefacts().await;
    assert_eq!(artefacts.len(), 1);
    assert_eq!(
        artefacts[0].location,
        ArtefactLocation::AgentFolder {
            agent_instance_id: f.agent
        }
    );
    assert_eq!(artefacts[0].path, saved_as);
    assert_eq!(artefacts[0].version, 1);
    assert_eq!(artefacts[0].media_type, "text/csv");
    assert_eq!(artefacts[0].producing_run_id, Some(run));

    let entries = f.entries("sandbox_run").await;
    assert_eq!(entries.len(), 1);
    let data = &entries[0].data;
    assert_eq!(data["code_sha256"], sha_hex(code.as_bytes()));
    assert_eq!(data["files_in"], 1);
    assert_eq!(data["files_out"], 1);
    let text = data.to_string();
    assert!(!text.contains("splitlines") && !text.contains("supplier,avg"));
}

// Scenario: Edge — work session keeps files between calls and ends with the run
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn work_session_keeps_files_between_calls_and_ends_with_the_run() {
    let f = fixture().await;
    let run = RunId::new();
    let first = f
        .call(
            run,
            json!({"code": "open('/work/a.txt','w').write('kept between calls')", "workSession": true}),
        )
        .await
        .expect("first call");
    assert_eq!(first["exitCode"], 0, "{first}");
    let second = f
        .call(
            run,
            json!({"code": "print(open('/work/a.txt').read())", "workSession": true}),
        )
        .await
        .expect("second call");
    assert_eq!(second["stdout"], "kept between calls\n", "{second}");
    assert_eq!(
        second["sessionChanges"],
        json!([{"path": "a.txt", "kind": "added"}])
    );
    let dir = f.session_dir(run);
    assert!(dir.exists());

    f.journal
        .append_event(
            "run_state_changed",
            json!({"run_id": run.to_string(), "state": "completed"}),
        )
        .await
        .expect("event");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while dir.exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!dir.exists(), "the session folder is deleted at run end");
}

// Scenario: Edge — second call on a busy work session
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn second_call_on_a_busy_work_session() {
    let f = std::sync::Arc::new(fixture().await);
    let run = RunId::new();
    let first = {
        let f = f.clone();
        tokio::spawn(async move {
            f.call(
                run,
                json!({"code": "x = 0\nfor i in range(200000): x += i\nprint(x)", "workSession": true}),
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    let second = f
        .call(run, json!({"code": "print(1)", "workSession": true}))
        .await
        .expect_err("refused while busy");
    assert_eq!(second.code, "SBX_SESSION_BUSY");
    assert!(second.retryable);
    first.await.unwrap().expect("the first call finishes");
}

// Scenario: Edge — runtime limit is a result, not an error
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_limit_is_a_result_not_an_error() {
    let f = fixture().await;
    let result = f
        .call(RunId::new(), json!({"code": "while True: pass"}))
        .await
        .expect("a result");
    assert_eq!(result["exitCode"], -2);
    assert_eq!(result["stderr"], "RUNTIME_LIMIT");
}

// Scenario: Edge — ordin_tools call goes through the Rules Engine
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordin_tools_call_goes_through_the_rules_engine() {
    let f = fixture().await;
    f.dispatch.offer(vec![tool("mail", "email_search")]);
    let run = RunId::new();
    let result = f
        .call(
            run,
            json!({"code": "r = ordin_tools.email_search(query='Kestrel')\nprint(r['untrusted'], r['is_error'], r['parts'][0]['json']['hits'])"}),
        )
        .await
        .expect("the run succeeds");
    assert_eq!(result["stdout"], "True False 1\n", "{result}");
    let calls = f.dispatch.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, run);
    assert_eq!(calls[0].1.server.as_str(), "mail");
    assert_eq!(calls[0].1.tool, "email_search");
    assert_eq!(calls[0].2, json!({"query": "Kestrel"}));
    assert_eq!(f.entries("sandbox_run").await[0].data["module_calls"], 1);
}

// Scenario: Edge — a brokered call uses the person's token without exposing it
//
// The outbound request cannot be observed here: `sandboxd` dials only HTTPS hosts on public
// addresses (`PYS_ALLOW_PLAINTEXT_LOOPBACK_EGRESS=false`), so no loopback stand-in can receive it
// (follow-up FU-007). This test checks the half that is observable: the person's token comes from
// Identity through the interactor, the run is not refused, and the token never reaches stdout.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_brokered_call_uses_the_persons_token_without_exposing_it() {
    let f = fixture().await;
    let credential = f
        .identity
        .seed_microsoft_for_tests("refresh", "client", "https://login.invalid/token", &[])
        .await
        .expect("a signed-in credential");
    f.grants.set(vec![rest(
        "tickets",
        person(CredentialKind::Microsoft, credential),
    )]);
    let result = f
        .call(
            RunId::new(),
            json!({
                "code": "try:\n    print(api.tickets.get('/v1/open'))\nexcept Exception as e:\n    print('failed')",
                "requestedCapabilities": ["tickets"]
            }),
        )
        .await
        .expect("the run is not refused");
    let stdout = result["stdout"].as_str().unwrap_or_default();
    assert!(!stdout.contains("seeded"), "{stdout}");
}

// Scenario: Error — requested capability not granted
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requested_capability_not_granted() {
    let f = fixture().await;
    let place = PlaceId::new();
    f.places.put(place, "a.csv", b"x");
    let e = f
        .call(
            RunId::new(),
            json!({"code": "print(1)", "requestedCapabilities": ["crm"],
                   "inputs": [{"placeId": place.to_string(), "path": "a.csv"}]}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_CAPABILITY_UNKNOWN");
    assert_eq!(
        e.message,
        "`crm` is not a system this agent may call; granted: none."
    );
    assert_eq!(*f.places.reads.lock().unwrap(), 0, "no input file is read");
}

// Scenario: Error — place not granted
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn place_not_granted() {
    let f = fixture().await;
    let e = f
        .call(
            RunId::new(),
            json!({"code": "print(1)", "inputs": [{"placeId": PlaceId::new().to_string(), "path": "a.csv"}]}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_INPUT_NOT_GRANTED");
    assert!(!e.retryable);
}

// Scenario: Error — path leaving the work folder
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_leaving_the_work_folder() {
    let f = fixture().await;
    let e = f
        .call(
            RunId::new(),
            json!({"code": "print(1)", "files": [{"path": "../x", "contentBase64": "eA=="}]}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_FILE_INVALID");
    assert!(e.message.contains("../x"), "{}", e.message);
}

// Scenario: Error — expired sign-in
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_sign_in() {
    let f = fixture().await;
    // Identity holds no such credential: its answer is a permanent sign-in fault.
    f.grants.set(vec![rest(
        "tickets",
        person(CredentialKind::Microsoft, CredentialId::new()),
    )]);
    let e = f
        .call(
            RunId::new(),
            json!({"code": "print(1)", "requestedCapabilities": ["tickets"]}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_SIGN_IN_NEEDED");
    assert_eq!(
        e.message,
        "The person needs to sign in to tickets again before this can run."
    );
}

// Scenario: Error — mixed person credentials
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mixed_person_credentials() {
    let f = fixture().await;
    f.grants.set(vec![
        rest(
            "tickets",
            person(CredentialKind::Microsoft, CredentialId::new()),
        ),
        rest("repo", person(CredentialKind::Github, CredentialId::new())),
    ]);
    let e = f
        .call(
            RunId::new(),
            json!({"code": "print(1)", "requestedCapabilities": ["tickets", "repo"]}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_MIXED_CREDENTIALS");
}

// Scenario: Error — Rules Engine unavailable fails closed
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rules_engine_unavailable_fails_closed() {
    let f = fixture().await;
    f.grants.fail();
    let e = f
        .call(RunId::new(), json!({"code": "print(1)"}))
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_RULES_UNAVAILABLE");
    assert!(e.retryable);
    assert!(f.entries("sandbox_run").await.is_empty(), "no code runs");
}

// Scenario: Error — environment without the extra guests
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn environment_without_the_extra_guests() {
    let f = fixture().await;
    let e = f
        .call(
            RunId::new(),
            json!({"code": "print(1)", "environment": {"MODE": "fast"}}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_ENVIRONMENT_UNSUPPORTED");
}

// Scenario: Happy path — the program registers the provider and reads the status
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_program_registers_the_provider_and_reads_the_status() {
    let f = fixture().await;
    assert_eq!(
        f.sandbox.status(),
        SandboxStatus::Ready {
            guests: vec![GuestKind::Python]
        }
    );
    let provider = f.sandbox.tool_provider();
    let again = f.sandbox.tool_provider();
    assert!(std::sync::Arc::ptr_eq(&provider, &again));
    assert_eq!(provider.provider_id(), "sandbox");
    let servers = provider.servers();
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].name.as_str(), "sandbox");
    let tools: Vec<_> = servers[0].tools.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(tools, ["python_sandbox"]);
    let spec = &servers[0].tools[0];
    assert!(!spec.sandbox_callable);
    assert!(spec.description.len() <= 120 && !spec.description.contains('\n'));
    assert!(spec.argument_form.len() <= 300);
    assert!(spec.plain.len() <= 200);
}

// Scenario: Edge — a tool name with a hyphen is left out of ordin_tools
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tool_name_with_a_hyphen_is_left_out_of_ordin_tools() {
    let f = fixture().await;
    f.dispatch.offer(vec![
        tool("mail", "email_search"),
        tool("tickets", "get-ticket"),
    ]);
    let run = RunId::new();
    let result = f
        .call(
            run,
            json!({"code": "print(hasattr(ordin_tools, 'email_search'), hasattr(ordin_tools, 'get-ticket'))"}),
        )
        .await
        .expect("the run succeeds");
    assert_eq!(result["stdout"], "True False\n", "{result}");
    let lines = log_lines(&[
        "sandbox.module_tool_skipped",
        "get-ticket",
        &run.to_string(),
    ]);
    assert!(!lines.is_empty());
}

// The hosted agent is never offered the sandbox (B.2).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_hosted_agent_is_not_offered_the_sandbox() {
    let f = fixture().await;
    let provider = f.sandbox.tool_provider();
    let e = provider
        .call(
            f.ctx_as(RunId::new(), ToolCaller::HostedAgent),
            &python(),
            json!({"code": "print(1)"}),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "SBX_CALLER_NOT_SUPPORTED");
}
