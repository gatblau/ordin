//! The real local channel: a Unix socket in the application data root. The scenarios that reach the
//! relay through `serve` on an in-memory stream are in `acceptance`; these use the socket itself.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use ordin_types::wire::hook::{HarnessKind, HookReply};
use ordin_types::ApprovalId;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use super::*;

async fn over_socket(rig: &Rig, bytes: &[u8]) -> Vec<HookReply> {
    let mut stream = UnixStream::connect(rig.relay.endpoint())
        .await
        .expect("connect");
    stream.write_all(bytes).await.expect("write");
    read_replies(BufReader::new(stream)).await
}

#[tokio::test]
async fn socket_permissions() {
    let rig = Rig::new().await;
    // The directory exists wider than 0700; start narrows it.
    let run_dir = rig.dir.0.join("run");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::set_permissions(&run_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    rig.relay.start().await.expect("start");
    let dir_mode = std::fs::metadata(&run_dir).unwrap().permissions().mode() & 0o777;
    let sock_mode = std::fs::metadata(run_dir.join("hook.sock"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700);
    assert_eq!(sock_mode, 0o600);
    assert_eq!(
        rig.relay.endpoint(),
        run_dir.join("hook.sock").to_string_lossy()
    );
    rig.relay.shutdown().await;
    assert!(
        !run_dir.join("hook.sock").exists(),
        "shutdown removes the socket file"
    );
}

#[tokio::test]
async fn a_call_over_the_socket_is_answered() {
    let rig = Rig::new().await;
    rig.relay.start().await.expect("start");
    let opened = rig.open(HarnessKind::ClaudeCode);
    let (call_id, bytes) = rig.line(&opened, pre("Read", Some("t-1")));
    let replies = over_socket(&rig, &bytes).await;
    assert!(matches!(&replies[0], HookReply::Allow { call_id: c } if *c == call_id));
    // A second start on a started relay is refused.
    assert!(matches!(
        rig.relay.start().await,
        Err(crate::HookRelayError::Listen(_))
    ));
    rig.relay.shutdown().await;
}

#[tokio::test]
async fn a_stale_socket_is_replaced() {
    let rig = Rig::new().await;
    let run_dir = rig.dir.0.join("run");
    std::fs::create_dir_all(&run_dir).unwrap();
    // A socket file nobody accepts on: bind, then drop the listener without removing the file.
    drop(std::os::unix::net::UnixListener::bind(run_dir.join("hook.sock")).unwrap());
    assert!(run_dir.join("hook.sock").exists());
    rig.relay
        .start()
        .await
        .expect("a stale socket is removed and the channel opens");
    let opened = rig.open(HarnessKind::ClaudeCode);
    let replies = over_socket(&rig, &rig.line(&opened, pre("Read", None)).1).await;
    assert!(matches!(replies[0], HookReply::Allow { .. }));
    rig.relay.shutdown().await;
}

#[tokio::test]
async fn shutdown_denies_a_connection_still_open_after_the_grace() {
    let rig = Rig::with_timings(Timings {
        hold: Duration::from_secs(30),
        ..fast()
    })
    .await;
    rig.relay.start().await.expect("start");
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.rules.set_decide(Decide::Ask(Some(ApprovalId::new())));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let (call_id, bytes) = rig.line(&opened, pre("Bash", None));
    let mut stream = UnixStream::connect(rig.relay.endpoint()).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    let mut lines = tokio::io::AsyncBufReadExt::lines(BufReader::new(stream));
    let first = lines.next_line().await.unwrap().expect("holding");
    assert!(first.contains("\"kind\":\"holding\""), "{first}");
    let started = Instant::now();
    rig.relay.shutdown().await;
    assert!(started.elapsed() < Duration::from_secs(3));
    let second = lines
        .next_line()
        .await
        .unwrap()
        .expect("a deny at shutdown");
    let reply = reply_of(&second);
    assert!(matches!(&reply, HookReply::Deny { call_id: c, .. } if *c == call_id));
    assert_eq!(deny_reason(&reply), "ordin could not decide");
}

#[tokio::test]
async fn a_hook_that_goes_away_while_held_leaves_the_approval_open_for_the_late_path() {
    let rig = Rig::new().await;
    rig.relay.start().await.expect("start");
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    rig.rules.set_decide(Decide::Ask(Some(approval)));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let (_, bytes) = rig.line(&opened, pre("Bash", None));
    let mut stream = UnixStream::connect(rig.relay.endpoint()).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    let mut lines = tokio::io::AsyncBufReadExt::lines(BufReader::new(&mut stream));
    let _holding = lines.next_line().await.unwrap().expect("holding");
    drop(lines);
    drop(stream);
    // The hook is gone; the relay still records the late decision when the hold ends.
    wait_for_rows(&rig, 1).await;
    rig.approvals
        .decide(approval, ordin_types::domain::runs::ApprovalState::Approved);
    eventually("the run is resumed", || {
        !rig.supervisor.resumes.lock().unwrap().is_empty()
    })
    .await;
    rig.relay.shutdown().await;
}

#[tokio::test]
async fn sixty_four_connections_at_most_are_served_at_once() {
    assert_eq!(crate::consts::MAX_CONNECTIONS, 64);
    let rig = Rig::new().await;
    assert_eq!(rig.relay.slots.available_permits(), 64);
}
