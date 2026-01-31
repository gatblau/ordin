//! A. The command line (`std::env::args`, no parsing library).

use std::path::PathBuf;

use ordin_types::wire::hook::HarnessKind;
use ordin_types::{AgentInstanceId, RunId};

use crate::error::{HookFailure, USAGE};
use crate::event::Event;

pub const USAGE_TEXT: &str = "usage:\n  ordin-hook --harness <claude-code|copilot-cli> --event <pre-tool-use|post-tool-use|stop> --agent-instance <uuid>\n  ordin-hook mcp-bridge\n  ordin-hook mcp-launch --spec <path>\n  ordin-hook api-key --run <run_id> [--data-root <path>]\n  ordin-hook --version";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Hook {
        harness: HarnessKind,
        event: Event,
        agent_instance: AgentInstanceId,
    },
    McpBridge,
    McpLaunch {
        spec: PathBuf,
    },
    ApiKey {
        run: RunId,
        data_root: Option<PathBuf>,
    },
    Version,
}

fn usage() -> HookFailure {
    HookFailure::new(USAGE, USAGE_TEXT)
}

/// `args` excludes the program name.
pub fn parse(args: &[String]) -> Result<Command, HookFailure> {
    match args.first().map(String::as_str) {
        Some("mcp-bridge") if bridge_args_ok(&args[1..]) => Ok(Command::McpBridge),
        Some("mcp-launch") => parse_launch(&args[1..]),
        Some("--version") if args.len() == 1 => Ok(Command::Version),
        Some("api-key") => parse_api_key(&args[1..]),
        _ => parse_hook(args),
    }
}

/// `mcp-bridge` takes no arguments, or the rest of the line the MCP Front launches it with
/// (OrdinHook.md, McpFront.md SA-RECON-03): `<server> --agent-instance <uuid>`. The bridge is
/// configured by its environment alone, so these values are checked and not used.
fn bridge_args_ok(rest: &[String]) -> bool {
    match rest {
        [] => true,
        [server, flag, agent] => {
            !server.starts_with('-')
                && flag == "--agent-instance"
                && AgentInstanceId::parse(agent).is_ok()
        }
        _ => false,
    }
}

/// `--spec <path>`, absolute.
fn parse_launch(args: &[String]) -> Result<Command, HookFailure> {
    match args {
        [flag, path] if flag == "--spec" && std::path::Path::new(path).is_absolute() => {
            Ok(Command::McpLaunch {
                spec: PathBuf::from(path),
            })
        }
        _ => Err(usage()),
    }
}

/// Pairs of `--flag value`; each flag at most once.
fn pairs<'a>(args: &'a [String], allowed: &[&str]) -> Result<Vec<(&'a str, &'a str)>, HookFailure> {
    let mut out: Vec<(&str, &str)> = Vec::new();
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(usage)?;
        if !allowed.contains(&flag.as_str()) || out.iter().any(|(f, _)| f == flag) {
            return Err(usage());
        }
        out.push((flag.as_str(), value.as_str()));
    }
    Ok(out)
}

fn find<'a>(pairs: &[(&str, &'a str)], flag: &str) -> Option<&'a str> {
    pairs.iter().find(|(f, _)| *f == flag).map(|(_, v)| *v)
}

fn parse_hook(args: &[String]) -> Result<Command, HookFailure> {
    let given = pairs(args, &["--harness", "--event", "--agent-instance"])?;
    let harness = match find(&given, "--harness").ok_or_else(usage)? {
        "claude-code" => HarnessKind::ClaudeCode,
        "copilot-cli" => HarnessKind::CopilotCli,
        _ => return Err(usage()),
    };
    let event = match find(&given, "--event").ok_or_else(usage)? {
        "pre-tool-use" => Event::PreToolUse,
        "post-tool-use" => Event::PostToolUse,
        "stop" => Event::Stop,
        _ => return Err(usage()),
    };
    let agent_instance =
        AgentInstanceId::parse(find(&given, "--agent-instance").ok_or_else(usage)?)
            .map_err(|_| usage())?;
    Ok(Command::Hook {
        harness,
        event,
        agent_instance,
    })
}

fn parse_api_key(args: &[String]) -> Result<Command, HookFailure> {
    let given = pairs(args, &["--run", "--data-root"])?;
    let run = RunId::parse(find(&given, "--run").ok_or_else(usage)?).map_err(|_| usage())?;
    let data_root = match find(&given, "--data-root") {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(usage());
            }
            Some(path)
        }
        None => None,
    };
    Ok(Command::ApiKey { run, data_root })
}
