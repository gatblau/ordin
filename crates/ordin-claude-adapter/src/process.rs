//! One running Claude Code process: its pipes, its output tasks and how it is stopped
//! (ClaudeLocalAdapter.md `start` step 10, `cancel` step 2).

use std::collections::VecDeque;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::BoxStream;
use ordin_hook_relay::HookSessionEvent;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::mpsc;

use crate::command::user_line;
use crate::run::{lock, run_log, Run};
use crate::stream::MAX_LINE_BYTES;

/// Size of the stderr ring buffer.
const STDERR_RING_BYTES: usize = 64 * 1024;
/// What is kept of it after a failure.
const STDERR_TAIL_BYTES: usize = 2 * 1024;
/// Lines waiting for the run's task.
const LINE_QUEUE: usize = 64;

/// One line of the child's stdout.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    Text(Vec<u8>),
    TooLong,
}

pub(crate) struct Process {
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    pub lines: mpsc::Receiver<Line>,
    /// `None` once the Hook Relay has ended the stream.
    pub hook_events: Option<BoxStream<'static, HookSessionEvent>>,
    pub stderr: Arc<Mutex<VecDeque<u8>>>,
    pub pid: Option<u32>,
    pub init_seen: bool,
    /// The session's whole process tree on Windows; `None` where the operating system refused a job.
    #[cfg(windows)]
    job: Option<crate::job::Job>,
}

/// Reads stdout into lines of at most [`MAX_LINE_BYTES`]; a longer one is dropped whole.
pub(crate) async fn read_lines<R: tokio::io::AsyncRead + Unpin>(
    stdout: R,
    lines: mpsc::Sender<Line>,
) {
    let mut reader = BufReader::with_capacity(64 * 1024, stdout);
    let mut line: Vec<u8> = Vec::new();
    let mut too_long = false;
    loop {
        let chunk = match reader.fill_buf().await {
            Ok(chunk) if !chunk.is_empty() => chunk,
            _ => break,
        };
        let (part, ended) = match chunk.iter().position(|b| *b == b'\n') {
            Some(at) => (&chunk[..at], true),
            None => (chunk, false),
        };
        if line.len() + part.len() > MAX_LINE_BYTES {
            too_long = true;
            line.clear();
        } else if !too_long {
            line.extend_from_slice(part);
        }
        let used = part.len() + usize::from(ended);
        reader.consume(used);
        if ended {
            let item = if too_long {
                Line::TooLong
            } else {
                Line::Text(std::mem::take(&mut line))
            };
            too_long = false;
            if lines.send(item).await.is_err() {
                return;
            }
        }
    }
    if too_long {
        let _ = lines.send(Line::TooLong).await;
    } else if !line.is_empty() {
        let _ = lines.send(Line::Text(line)).await;
    }
}

/// Keeps the last [`STDERR_RING_BYTES`] of stderr. Never logged.
async fn read_stderr(mut stderr: ChildStderr, ring: Arc<Mutex<VecDeque<u8>>>) {
    let mut chunk = [0u8; 4096];
    while let Ok(read) = stderr.read(&mut chunk).await {
        if read == 0 {
            return;
        }
        let mut ring = lock(&ring);
        ring.extend(&chunk[..read]);
        let over = ring.len().saturating_sub(STDERR_RING_BYTES);
        ring.drain(..over);
    }
}

impl Process {
    pub(crate) fn new(
        child: Child,
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: ChildStderr,
        hook_events: BoxStream<'static, HookSessionEvent>,
    ) -> Process {
        let pid = child.id();
        #[cfg(windows)]
        let job = crate::job::Job::holding(&child);
        let (sender, lines) = mpsc::channel(LINE_QUEUE);
        tokio::spawn(read_lines(stdout, sender));
        let ring = Arc::new(Mutex::new(VecDeque::new()));
        tokio::spawn(read_stderr(stderr, ring.clone()));
        Process {
            child,
            stdin: Some(stdin),
            lines,
            hook_events: Some(hook_events),
            stderr: ring,
            pid,
            init_seen: false,
            #[cfg(windows)]
            job,
        }
    }

    /// Writes one stream-json `user` line and flushes. A failed write is not reported here: the
    /// child has gone, and the run's task sees its stdout close ("CLI exits").
    pub(crate) async fn write_turn(&mut self, text: &str) {
        if let Some(stdin) = self.stdin.as_mut() {
            let line = user_line(text);
            if stdin.write_all(&line).await.is_ok() {
                let _ = stdin.flush().await;
            }
        }
    }

    #[cfg(unix)]
    fn signal(&self, signal: nix::sys::signal::Signal) {
        if let Some(pid) = self.pid {
            let group = nix::unistd::Pid::from_raw(pid as i32);
            let _ = nix::sys::signal::killpg(group, signal);
        }
    }

    /// `cancel` step 2: close stdin; on Unix SIGTERM to the process group, on Windows end the session's
    /// job (every process of the session); wait up to `deadline`, then kill (`CLA_STOP_DEADLINE`).
    // spec: ordin-desktop/components/ClaudeLocalAdapter#SA-STOP-01
    // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-JOB-1
    pub(crate) async fn stop(&mut self, run: &Run, deadline: Duration) -> Option<ExitStatus> {
        self.stdin = None;
        #[cfg(unix)]
        self.signal(nix::sys::signal::Signal::SIGTERM);
        #[cfg(not(unix))]
        self.end_tree();
        self.wait_or_kill(run, deadline).await
    }

    /// On Windows: ends the session's job, or only the started process where there is no job.
    #[cfg(not(unix))]
    fn end_tree(&mut self) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
            return;
        }
        let _ = self.child.start_kill();
    }

    /// The pause path: close stdin and wait up to `deadline` for the process to end by itself.
    pub(crate) async fn close_and_wait(
        &mut self,
        run: &Run,
        deadline: Duration,
    ) -> Option<ExitStatus> {
        self.stdin = None;
        self.wait_or_kill(run, deadline).await
    }

    async fn wait_or_kill(&mut self, run: &Run, deadline: Duration) -> Option<ExitStatus> {
        if let Ok(status) = tokio::time::timeout(deadline, self.child.wait()).await {
            return status.ok();
        }
        run_log!(
            warn,
            run,
            "claude.stop.deadline",
            code = "CLA_STOP_DEADLINE"
        );
        #[cfg(unix)]
        self.signal(nix::sys::signal::Signal::SIGKILL);
        #[cfg(not(unix))]
        self.end_tree();
        let _ = self.child.start_kill();
        self.child.wait().await.ok()
    }

    /// Keeps the last 2 KiB of stderr with the run, after a failure.
    pub(crate) fn keep_stderr_tail(&self, run: &Run) {
        let ring = lock(&self.stderr);
        let skip = ring.len().saturating_sub(STDERR_TAIL_BYTES);
        *lock(&run.stderr_tail) = ring.iter().skip(skip).copied().collect();
    }
}
