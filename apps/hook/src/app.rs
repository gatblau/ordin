//! The program body: dispatch on the command line, run one mode, return the exit status.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use ordin_types::wire::hook::{HarnessKind, HookReply, HookRequest};
use ordin_types::{AgentInstanceId, HookCallId};

use crate::channel::{connect, exchange};
use crate::cli::{self, Command};
use crate::env::{self, Lookup};
use crate::error::HookFailure;
use crate::event::{Event, Outcome};
use crate::{api_key, bridge, claude, copilot, launch, mac};

/// Protocol version this program speaks.
pub const PROTOCOL: u16 = 1;
/// The standard input cap (Ordin Hook D, chosen there).
pub const STDIN_MAX: u64 = 8 * 1024 * 1024;
/// The longest request line, newline included (2C).
pub const REQUEST_MAX: usize = 1024 * 1024;

/// The waits of Ordin Hook A and F.
pub struct Timings {
    pub overall: Duration,
    pub connect: Duration,
    pub first_reply: Duration,
}

impl Timings {
    pub const STANDARD: Timings = Timings {
        overall: Duration::from_secs(50),
        connect: Duration::from_secs(1),
        first_reply: Duration::from_secs(2),
    };
}

/// The three standard streams.
pub struct Io {
    pub stdin: Box<dyn Read + Send>,
    pub stdout: Box<dyn Write + Send>,
    pub stderr: Box<dyn Write + Send>,
}

pub fn version_line() -> String {
    format!(
        "ordin-hook {} protocol {PROTOCOL}",
        env!("CARGO_PKG_VERSION")
    )
}

/// Runs the program and returns its exit status.
pub fn run(args: &[String], get: Lookup<'_>, mut io: Io, timings: &Timings) -> i32 {
    let started = Instant::now();
    let command = match cli::parse(args) {
        Ok(command) => command,
        Err(failure) => {
            let _ = writeln!(io.stderr, "{}", failure.reason);
            return 64;
        }
    };
    match command {
        Command::Version => {
            let _ = writeln!(io.stdout, "{}", version_line());
            0
        }
        Command::McpBridge => run_bridge(get, io),
        Command::McpLaunch { spec } => launch::run(&spec, &mut io.stderr),
        Command::ApiKey { run, data_root } => {
            let endpoint = get("ORDIN_HOOK_ENDPOINT");
            match api_key::run(
                run,
                data_root.as_deref(),
                endpoint.as_deref(),
                &mut io.stdout,
            ) {
                Ok(()) => 0,
                Err(_) => 1,
            }
        }
        Command::Hook {
            harness,
            event,
            agent_instance,
        } => run_hook(harness, event, agent_instance, get, io, timings, started),
    }
}

fn run_bridge(get: Lookup<'_>, mut io: Io) -> i32 {
    let env = match bridge::read_env(get) {
        Ok(env) => env,
        Err(failure) => {
            let _ = writeln!(io.stderr, "{}", failure.reason);
            return 1;
        }
    };
    match bridge::run(&env, io.stdin, io.stdout, &mut *io.stderr) {
        Ok(()) => 0,
        Err(failure) => {
            if failure.code != crate::error::BRIDGE_REFUSED {
                let _ = writeln!(io.stderr, "{}", failure.code);
            }
            1
        }
    }
}

fn run_hook(
    harness: HarnessKind,
    event: Event,
    agent_instance: AgentInstanceId,
    get: Lookup<'_>,
    mut io: Io,
    timings: &Timings,
    started: Instant,
) -> i32 {
    let deadline = started + timings.overall;
    // C. Every failure becomes the harness's deny form for the event.
    let outcome = decide(
        harness,
        event,
        agent_instance,
        get,
        &mut io.stdin,
        timings,
        deadline,
    )
    .unwrap_or_else(|failure| match event {
        Event::PreToolUse => Outcome::Deny(failure.reason.to_string()),
        // An ending that cannot be checked may end (SA-HOOK-03); a failed after-tool event prints nothing.
        Event::PostToolUse | Event::Stop => Outcome::Ack,
    });
    let written = match harness {
        HarnessKind::ClaudeCode => claude::write_decision(event, &outcome, &mut io.stdout),
        HarnessKind::CopilotCli => copilot::write_decision(event, &outcome, &mut io.stdout),
    };
    match written {
        Ok(()) => 0,
        Err(_) => {
            let _ = writeln!(io.stderr, "{}", HookFailure::output_failed().reason);
            2
        }
    }
}

fn read_stdin(stdin: &mut dyn Read) -> Result<Vec<u8>, HookFailure> {
    let mut bytes = Vec::new();
    stdin
        .take(STDIN_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HookFailure::payload_invalid())?;
    if bytes.len() as u64 > STDIN_MAX {
        return Err(HookFailure::payload_invalid());
    }
    Ok(bytes)
}

fn decide(
    harness: HarnessKind,
    event: Event,
    agent_instance: AgentInstanceId,
    get: Lookup<'_>,
    stdin: &mut dyn Read,
    timings: &Timings,
    deadline: Instant,
) -> Result<Outcome, HookFailure> {
    let hook_env = env::read(get)?;
    let payload = read_stdin(stdin)?;
    let message = match harness {
        HarnessKind::ClaudeCode => claude::read_event(&payload, event)?,
        HarnessKind::CopilotCli => copilot::read_event(&payload, event)?,
    };
    let call_id = HookCallId::new();
    let request = HookRequest {
        protocol: hook_env.protocol,
        call_id,
        session: hook_env.session,
        agent_instance_id: agent_instance,
        mac: mac::sign(&hook_env.secret, &message),
        message,
    };
    let line = serde_json::to_vec(&request).map_err(|_| HookFailure::payload_invalid())?;
    if line.len() + 1 > REQUEST_MAX {
        return Err(HookFailure::request_too_large());
    }
    let conn = connect(&hook_env.endpoint, timings.connect)?;
    let reply = exchange(conn, &line, call_id, deadline, timings.first_reply)?;
    map_reply(reply, event)
}

/// F.6. `Allow` is valid for `PreToolUse` only, `Ack` for `PostToolUse` and `Stop`.
pub fn map_reply(reply: HookReply, event: Event) -> Result<Outcome, HookFailure> {
    match (reply, event) {
        (HookReply::Allow { .. }, Event::PreToolUse) => Ok(Outcome::Allow),
        (HookReply::Deny { reason, .. }, _) => Ok(Outcome::Deny(reason)),
        (HookReply::Ack { .. }, Event::PostToolUse | Event::Stop) => Ok(Outcome::Ack),
        _ => Err(HookFailure::reply_malformed()),
    }
}
