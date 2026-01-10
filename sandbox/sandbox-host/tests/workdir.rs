//! C19 WorkDir with the real RustPython `wasm32-wasip1` guest (ADR-041): the guest reads
//! input files at `/work`, writes and deletes files there, and the change record reports
//! exactly that; the guest cannot reach outside `/work`; a guest that fills the folder past
//! its bound is stopped as RUNTIME_LIMIT; the folder is gone after the run.
#![cfg(feature = "integration")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use sandboxd::broker::{BrokerCall, BrokerError};
use sandboxd::runtime::{CapabilityBundle, Limits, SandboxRuntime, RUNTIME_LIMIT_EXIT};
use sandboxd::types::{Params, UntrustedMcpResult, UntrustedResponse};
use sandboxd::workdir::{ChangeKind, WorkDir, WorkFile, WorkLimits};

const GUEST: &[u8] = include_bytes!("../guest/pysandbox-guest.wasm");

/// No test here calls a capability; any call is a test failure.
pub struct NoBroker;
pub use NoBroker as NoBrokerPub;
impl BrokerCall for NoBroker {
    fn call_boxed<'a>(
        &'a self,
        _cap_id: &'a [u8; 16],
        _verb: &'a str,
        _path: &'a str,
        _params: &'a Params,
        _body: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<UntrustedResponse, BrokerError>> + Send + 'a>> {
        panic!("no broker call expected")
    }
    fn call_tool_boxed<'a>(
        &'a self,
        _cap_id: &'a [u8; 16],
        _tool: &'a str,
        _arguments: &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<UntrustedMcpResult, BrokerError>> + Send + 'a>> {
        panic!("no broker call expected")
    }
}

fn runtime() -> SandboxRuntime {
    SandboxRuntime::new(&SandboxRuntime::digest_of(GUEST), GUEST, Arc::new(NoBroker)).unwrap()
}

fn file(path: &str, content: &str) -> WorkFile {
    WorkFile {
        path: path.into(),
        content_base64: base64::engine::general_purpose::STANDARD.encode(content),
    }
}

fn decode(s: &Option<String>) -> String {
    String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(s.as_deref().unwrap())
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn guest_reads_inputs_and_its_writes_are_reported() {
    let rt = runtime();
    let work = WorkDir::create(
        &[
            file("rates.csv", "a,1\nb,2\n"),
            file("old.txt", "x"),
            file("keep.txt", "k"),
        ],
        WorkLimits::default(),
    )
    .unwrap();
    let code = r#"
import os
rows = open('/work/rates.csv').read().splitlines()
total = sum(int(r.split(',')[1]) for r in rows)
os.mkdir('/work/out')
open('/work/out/summary.txt', 'w').write('total=%d' % total)
open('/work/rates.csv', 'a').write('c,3\n')
os.remove('/work/old.txt')
print('ok', total)
"#;
    let r = rt
        .run_in(
            code,
            &CapabilityBundle::default(),
            &Limits::default(),
            Some(&work),
        )
        .await;
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert!(r.stdout.contains("ok 3"));
    let changes = work.changes().unwrap();
    let kinds: Vec<(&str, ChangeKind)> =
        changes.iter().map(|c| (c.path.as_str(), c.kind)).collect();
    assert_eq!(
        kinds,
        vec![
            ("old.txt", ChangeKind::Removed),
            ("out/summary.txt", ChangeKind::Added),
            ("rates.csv", ChangeKind::Changed),
        ]
    );
    assert_eq!(decode(&changes[1].content_base64), "total=3");
    assert_eq!(decode(&changes[2].content_base64), "a,1\nb,2\nc,3\n");
}

#[tokio::test]
async fn guest_cannot_reach_outside_the_folder() {
    let rt = runtime();
    let work = WorkDir::create(&[], WorkLimits::default()).unwrap();
    let code = r#"
results = []
for p in ['/etc/hosts', '/work/../etc/hosts', '/tmp', '/']:
    try:
        open(p).read()
        results.append('READ ' + p)
    except Exception:
        results.append('denied')
print(results)
"#;
    let r = rt
        .run_in(
            code,
            &CapabilityBundle::default(),
            &Limits::default(),
            Some(&work),
        )
        .await;
    assert!(!r.stdout.contains("READ"), "escaped: {}", r.stdout);
    assert!(work.changes().unwrap().is_empty());
}

#[tokio::test]
async fn without_a_folder_the_guest_has_no_filesystem() {
    let rt = runtime();
    let code = r#"
try:
    open('/work/x', 'w').write('x')
    print('WROTE')
except Exception:
    print('denied')
"#;
    let r = rt
        .run(code, &CapabilityBundle::default(), &Limits::default())
        .await;
    assert!(
        r.stdout.contains("denied"),
        "stdout: {} stderr: {}",
        r.stdout,
        r.stderr
    );
}

#[tokio::test]
async fn filling_the_folder_past_its_bound_stops_the_run() {
    let rt = runtime();
    let limits = WorkLimits {
        max_files: 1_000,
        max_total_bytes: 256 * 1024,
        max_file_bytes: 16 * 1024 * 1024,
    };
    let work = WorkDir::create(&[], limits).unwrap();
    let code = r#"
f = open('/work/big.bin', 'w')
chunk = 'x' * 65536
while True:
    f.write(chunk)
    f.flush()
"#;
    let run_limits = Limits {
        epoch_deadline: Duration::from_secs(20),
        ..Limits::default()
    };
    let started = std::time::Instant::now();
    let r = rt
        .run_in(code, &CapabilityBundle::default(), &run_limits, Some(&work))
        .await;
    assert_eq!(r.exit_code, RUNTIME_LIMIT_EXIT);
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "stopped by the watcher, not the deadline"
    );
    assert!(work.changes().is_err());
}

#[tokio::test]
async fn the_folder_is_removed_after_the_run() {
    let rt = runtime();
    let work = WorkDir::create(&[file("a.txt", "1")], WorkLimits::default()).unwrap();
    let path = work.host_path().to_path_buf();
    let _ = rt
        .run_in(
            "print(1)",
            &CapabilityBundle::default(),
            &Limits::default(),
            Some(&work),
        )
        .await;
    drop(work);
    assert!(!path.exists());
}
