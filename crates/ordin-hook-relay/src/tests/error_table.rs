//! One test per row of HookRelay.md's Error Table (X1), each provoking the condition and checking
//! the reply or the error and its code, plus the registry check (X4).

use std::time::Duration;

use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::wire::hook::{HarnessKind, HookReply};
use ordin_types::{ApprovalId, HookCallId};
use serde_json::json;

use super::*;
use crate::{HookRelayError, ALL_CODES};

fn nil(reply: &HookReply) -> bool {
    matches!(reply, HookReply::Deny { call_id, .. } if call_id.0 == uuid::Uuid::nil())
}

#[tokio::test]
async fn row_socket_peer_is_another_user() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let (_, bytes) = rig.line(&opened, pre("Read", None));
    let replies = rig.send_as(&bytes, false).await;
    assert!(nil(&replies[0]));
    assert_eq!(deny_reason(&replies[0]), "refused");
    // The Error Table's body, character for character.
    assert_eq!(
        serde_json::to_string(&replies[0]).unwrap(),
        r#"{"kind":"deny","call_id":"00000000-0000-0000-0000-000000000000","reason":"refused"}"#
    );
    assert_eq!(rig.refusals().await[0]["code"], "HOOK_PEER_REFUSED");
}

#[tokio::test]
async fn row_line_too_long_not_json_unknown_fields_read_timeout() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let expected = "ordin could not read this request";

    // Not JSON.
    let replies = rig.send(b"{{{\n").await;
    assert!(nil(&replies[0]));
    assert_eq!(deny_reason(&replies[0]), expected);
    // Invalid UTF-8.
    let replies = rig.send(b"\xff\xfe\n").await;
    assert_eq!(deny_reason(&replies[0]), expected);
    // Too long: 1 MiB of text and its newline is the most that is read.
    let mut long = vec![b'a'; 1_048_577];
    long.push(b'\n');
    let replies = rig.send(&long).await;
    assert!(nil(&replies[0]));
    assert_eq!(deny_reason(&replies[0]), expected);
    // Unknown top-level field.
    let request = signed(&opened, HookCallId::new(), pre("Read", None));
    let mut value = serde_json::to_value(&request).unwrap();
    value["extra"] = json!(1);
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    let replies = rig.send(&bytes).await;
    assert_eq!(deny_reason(&replies[0]), expected);
    // A wrong message kind.
    let mut value = serde_json::to_value(&request).unwrap();
    value["message"]["kind"] = json!("explode");
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    let replies = rig.send(&bytes).await;
    assert_eq!(deny_reason(&replies[0]), expected);
    // No newline before the stream ends.
    let replies = rig.send(b"{\"protocol\":1}").await;
    assert_eq!(deny_reason(&replies[0]), expected);
    // Read timeout: a connection that sends nothing and stays open.
    let (client, server) = tokio::io::duplex(1024);
    let relay = rig.relay.clone();
    let served = tokio::spawn(async move { relay.serve(server, true).await });
    let replies = tokio::time::timeout(Duration::from_secs(3), read_replies(client))
        .await
        .expect("the relay closes the idle connection");
    let _ = served.await;
    assert!(nil(&replies[0]));
    assert_eq!(deny_reason(&replies[0]), expected);
    assert!(rig.rules.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn row_protocol_other_than_one() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    for protocol in [0u16, 2, 65535] {
        let mut request = signed(&opened, HookCallId::new(), pre("Read", None));
        request.protocol = protocol;
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        let replies = rig.send(&bytes).await;
        assert_eq!(
            deny_reason(&replies[0]),
            "ordin needs updating",
            "protocol {protocol}"
        );
        assert!(
            matches!(&replies[0], HookReply::Deny { call_id, .. } if *call_id == request.call_id)
        );
    }
}

#[tokio::test]
async fn row_no_open_live_session() {
    let rig = Rig::new().await;
    // No session at all.
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.relay.close_session(opened.run_id);
    let replies = rig.call(&opened, pre("Read", None)).await;
    assert_eq!(
        deny_reason(&replies[0]),
        "this session is not known to ordin"
    );
    // A session the supervisor no longer calls live.
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.supervisor.live.lock().unwrap().remove(&opened.run_id);
    let replies = rig.call(&opened, pre("Read", None)).await;
    assert_eq!(
        deny_reason(&replies[0]),
        "this session is not known to ordin"
    );
}

#[tokio::test]
async fn row_mac_mismatch() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut request = signed(&opened, HookCallId::new(), pre("Read", None));
    // A MAC over another message, one that is not base64url, and one of the wrong length.
    for mac in [
        mac_of(&opened.secret, &stop()),
        "***".to_owned(),
        "AAAA".to_owned(),
    ] {
        request.mac = mac;
        request.call_id = HookCallId::new();
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        let replies = rig.send(&bytes).await;
        assert_eq!(deny_reason(&replies[0]), "refused");
    }
    let refusals = rig.refusals().await;
    assert_eq!(refusals.len(), 3);
    assert!(refusals
        .iter()
        .all(|r| r == &json!({"code": "HOOK_UNAUTHENTICATED"})));
    let entries = rig.entries("hook_refused").await;
    assert!(entries.iter().all(|e| e.run_id == Some(opened.run_id)));
}

#[tokio::test]
async fn row_agent_instance_does_not_match_the_session() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut request = signed(&opened, HookCallId::new(), pre("Read", None));
    request.agent_instance_id = ordin_types::AgentInstanceId::new();
    let mut bytes = serde_json::to_vec(&request).unwrap();
    bytes.push(b'\n');
    let replies = rig.send(&bytes).await;
    assert_eq!(deny_reason(&replies[0]), "refused");
    assert!(rig.rules.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn row_repeated_or_malformed_call_id() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    // Repeated.
    let (_, bytes) = rig.line(&opened, pre("Read", None));
    assert!(matches!(rig.send(&bytes).await[0], HookReply::Allow { .. }));
    assert_eq!(deny_reason(&rig.send(&bytes).await[0]), "refused");
    // Not a version-7 UUID (a version-4 one), and not a UUID at all.
    for bad in [uuid::Uuid::new_v4().to_string(), "not-a-uuid".to_owned()] {
        let request = signed(&opened, HookCallId::new(), pre("Read", None));
        let mut value = serde_json::to_value(&request).unwrap();
        value["call_id"] = json!(bad);
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        let replies = rig.send(&bytes).await;
        assert_eq!(deny_reason(&replies[0]), "refused", "{bad}");
    }
    assert_eq!(rig.rules.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn row_the_engine_does_not_decide_within_the_budget() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.rules.set_decide(Decide::Slow(Duration::from_secs(2)));
    let started = Instant::now();
    let replies = rig.call(&opened, pre("Read", None)).await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(deny_reason(&replies[0]), "ordin could not decide in time");
}

#[tokio::test]
async fn row_the_approval_service_fails_while_holding() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.rules.set_decide(Decide::Ask(Some(ApprovalId::new())));
    *rig.rules.waited.lock().unwrap() = Waited::Unavailable;
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert!(matches!(replies[0], HookReply::Holding { .. }));
    assert_eq!(deny_reason(&replies[1]), "ordin could not decide");
    assert_eq!(rig.late_rows().await, 0);
    // The stop-gate wait fails the same way.
    *rig.rules.end.lock().unwrap() = EndScript::Ask(ApprovalId::new());
    rig.approvals
        .unavailable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let replies = rig.call(&opened, stop()).await;
    assert_eq!(deny_reason(&replies[1]), "ordin could not decide");
}

#[tokio::test]
async fn row_the_person_did_not_decide_within_the_hold() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    rig.rules.set_decide(Decide::Ask(Some(approval)));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let started = Instant::now();
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert!(started.elapsed() >= Duration::from_millis(500));
    assert_eq!(
        deny_reason(&replies[1]),
        "waiting for the person's approval"
    );
    let stored: String = sqlx::query_scalar("SELECT approval_id FROM hook_late_decisions")
        .fetch_one(rig.store.sqlite().unwrap())
        .await
        .unwrap();
    assert_eq!(stored, approval.to_string());
}

#[tokio::test]
async fn row_any_other_internal_error() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    // An ask without an approval id is an engine defect (D.5.3).
    rig.rules.set_decide(Decide::Ask(None));
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert_eq!(deny_reason(&replies[0]), "ordin could not decide");
    // A panic inside the decision is caught and denied (C.9).
    rig.rules.set_decide(Decide::Panic);
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert_eq!(deny_reason(&replies[0]), "ordin could not decide");
    // So is any other error of the wait.
    rig.rules.set_decide(Decide::Ask(Some(ApprovalId::new())));
    *rig.rules.waited.lock().unwrap() = Waited::State;
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert_eq!(deny_reason(&replies[1]), "ordin could not decide");
    // In process, the same.
    rig.rules.set_decide(Decide::Panic);
    let reply = rig
        .relay
        .decide_in_process(opened.run_id, pre("Bash", None))
        .await;
    assert_eq!(deny_reason(&reply), "ordin could not decide");
}

// Unix only: the Windows endpoint is a pipe name, with no folder path to be too long.
#[cfg(unix)]
#[tokio::test]
async fn row_socket_path_too_long() {
    let dir = TempDir::new();
    let (store, journal) = open_store(&dir).await;
    let long_root = dir.0.join("x".repeat(120));
    let relay = HookRelay::build(
        store,
        journal,
        Ports {
            rules: super::ScriptedRules::new(),
            approvals: super::ScriptedApprovals::new(),
            supervisor: super::ScriptedSupervisor::new(),
            writes: Arc::new(crate::writes::NoPlaces),
        },
        HookRelayConfig {
            app_data_root: long_root.clone(),
            installation_id: "i".into(),
        },
        fast(),
    );
    let error = relay.start().await.expect_err("the path is too long");
    assert!(matches!(error, HookRelayError::EndpointTooLong));
    assert_eq!(error.code(), "HOOK_ENDPOINT_TOO_LONG");
    assert!(!error.retryable());
    assert!(!long_root.exists(), "nothing was created");
}

#[tokio::test]
async fn row_another_core_holds_the_socket_or_bind_fails() {
    let first = Rig::new().await;
    first.relay.start().await.expect("the first core starts");
    // A second core over the same data root finds a live socket.
    let second = Rig::over(
        TempDir(first.dir.0.clone()),
        first.store.clone(),
        first.journal.clone(),
        fast(),
        ScriptedApprovals::new(),
        ScriptedSupervisor::new(),
    );
    let error = second
        .relay
        .start()
        .await
        .expect_err("a live core holds the socket");
    assert!(matches!(error, HookRelayError::Listen(_)));
    assert_eq!(error.code(), "HOOK_LISTEN_FAILED");
    assert!(!error.retryable());
    // The first core's socket is untouched.
    assert!(std::path::Path::new(first.relay.endpoint()).exists());
    first.relay.shutdown().await;
    std::mem::forget(second.dir); // the folder is the first rig's
}

#[tokio::test]
async fn row_session_opened_twice() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let error = rig
        .relay
        .open_session(crate::HookSessionSpec {
            run_id: opened.run_id,
            agent_instance_id: opened.agent_instance_id,
            workspace_id: opened.workspace_id,
            harness: HarnessKind::ClaudeCode,
        })
        .expect_err("already open");
    assert!(matches!(error, HookRelayError::SessionExists));
    assert_eq!(error.code(), "HOOK_SESSION_EXISTS");
    assert!(!error.retryable());
}

#[test]
fn row_no_randomness_from_the_operating_system() {
    // The operating system's source cannot be made to fail from a test; the row's error is checked
    // for its code and its retryable flag.
    let error = HookRelayError::Random;
    assert_eq!(error.code(), "HOOK_RANDOM_UNAVAILABLE");
    assert!(error.retryable());
}

#[tokio::test]
async fn row_store_unavailable() {
    let rig = Rig::new().await;
    // At start (A.3).
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.rules.set_decide(Decide::Ask(Some(ApprovalId::new())));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    // In a call (D.5): the late row cannot be written, so the call is denied.
    rig.store.close().await;
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert_eq!(deny_reason(&replies[1]), "ordin could not decide");
    let error = rig.relay.start().await.expect_err("the store is closed");
    assert!(matches!(error, HookRelayError::Store(_)));
    assert_eq!(error.code(), "HOOK_STORE_UNAVAILABLE");
    assert!(error.retryable());
}

#[test]
fn every_code_is_registered_and_the_prefix_is_owned() {
    assert_eq!(ALL_CODES.len(), 16);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-hook-relay", "{code}");
        assert!(code.starts_with("HOOK_"));
    }
    assert!(owns_prefix("ordin-hook-relay", "HOOK"));
    // The returned errors carry registered codes with the registry's retryable flag.
    let errors = [
        HookRelayError::Listen(std::io::Error::other("x")),
        HookRelayError::EndpointTooLong,
        HookRelayError::SessionExists,
        HookRelayError::Random,
        HookRelayError::Store(sqlx::Error::PoolClosed),
    ];
    for error in errors {
        let entry = lookup(error.code()).expect("registered");
        assert_eq!(entry.retryable, error.retryable(), "{}", error.code());
        assert!(ALL_CODES.contains(&error.code()));
    }
    // The hook-side codes are not this crate's.
    assert!(!ALL_CODES.contains(&"HOOK_CORE_TIMEOUT"));
}
