//! `mcp-launch`: starts a package's MCP server with only what it declares (OrdinHook.md I).
//!
//! Claude Code gives a stdio server its whole environment, which holds the session's hook secret and
//! may hold the model key, and it expands `${NAME}` in the `args` and `env` of `mcp.json` before the
//! server starts. So `mcp.json` names only this mode and a file, which the adapter writes: the
//! server's program, arguments and variables never pass through Claude Code. This mode clears the
//! environment and adds a short fixed list and the file's variables.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::api_key::check_mode;
use crate::error::{HookFailure, LAUNCH_FAILED};

/// The longest launch file.
const SPEC_MAX: u64 = 1024 * 1024;

/// What a program needs to start at all. Every other name must be declared.
#[cfg(not(windows))]
const PASSED: [&str; 7] = [
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TMPDIR",
];
#[cfg(windows)]
const PASSED: [&str; 8] = [
    "PATH",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
    "PATHEXT",
];

/// Never passed: ordin's own session variables.
const RESERVED_PREFIX: &str = "ORDIN_";

/// One server, as the launch file states it.
#[derive(Debug, PartialEq, Eq)]
pub struct Spec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

fn name_ok(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn failed() -> HookFailure {
    HookFailure::new(LAUNCH_FAILED, "the package's server could not start")
}

/// Reads the launch file, which must be this user's alone.
pub fn read_spec(path: &Path) -> Result<Spec, HookFailure> {
    let file = std::fs::File::open(path).map_err(|_| failed())?;
    let meta = file.metadata().map_err(|_| failed())?;
    if !check_mode(&meta) {
        return Err(failed());
    }
    let mut bytes = Vec::new();
    file.take(SPEC_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failed())?;
    if bytes.len() as u64 > SPEC_MAX {
        return Err(failed());
    }
    parse_spec(&bytes).ok_or_else(failed)
}

fn parse_spec(bytes: &[u8]) -> Option<Spec> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let program = value.get("program")?.as_str()?.to_owned();
    if program.is_empty() {
        return None;
    }
    let args = value
        .get("args")?
        .as_array()?
        .iter()
        .map(|arg| arg.as_str().map(str::to_owned))
        .collect::<Option<Vec<String>>>()?;
    let env = value
        .get("env")?
        .as_object()?
        .iter()
        .map(|(name, text)| Some((name.clone(), text.as_str()?.to_owned())))
        .collect::<Option<Vec<(String, String)>>>()?;
    env.iter()
        .all(|(name, _)| name_ok(name))
        .then_some(Spec { program, args, env })
}

fn same_name(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn reserved(name: &str) -> bool {
    name.to_ascii_uppercase().starts_with(RESERVED_PREFIX)
}

/// What this process passes on of its own environment.
// spec: ordin-desktop/components/OrdinHook#SA-RECON-04
pub fn inherited(parent: impl Iterator<Item = (OsString, OsString)>) -> Vec<(OsString, OsString)> {
    parent
        .filter(|(name, _)| {
            name.to_str().is_some_and(|name| {
                !reserved(name) && PASSED.iter().any(|passed| same_name(passed, name))
            })
        })
        .collect()
}

/// Becomes the server. Reached past the start only on a failure, which is status 1 with the code
/// on standard error, so the harness reports the server as failed to start.
pub fn run(spec: &Path, stderr: &mut dyn Write) -> i32 {
    let started = read_spec(spec).and_then(|spec| {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .env_clear()
            .envs(inherited(std::env::vars_os()))
            .envs(spec.env.iter().filter(|(name, _)| !reserved(name)).cloned());
        become_server(command).map_err(|_| failed())
    });
    match started {
        Ok(status) => status,
        Err(failure) => {
            let _ = writeln!(stderr, "{}: {}", failure.code, failure.reason);
            1
        }
    }
}

#[cfg(unix)]
fn become_server(mut command: Command) -> std::io::Result<i32> {
    use std::os::unix::process::CommandExt;
    Err(command.exec())
}

/// Windows has no `exec`: the server inherits the three streams and its status is passed on.
#[cfg(not(unix))]
fn become_server(mut command: Command) -> std::io::Result<i32> {
    Ok(command.status()?.code().unwrap_or(1))
}
