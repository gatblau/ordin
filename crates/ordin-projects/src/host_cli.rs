//! Starting `gh` and `glab` (Internal Logic 8.3, 14.1). Each child's environment is built
//! explicitly, never inherited wholesale (2D-1c); the token travels in the environment, never in an
//! argument. `--include` puts the status line and the headers on standard output, so the HTTP
//! status of a reply is read directly, whatever the tool's exit status.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use ordin_identity::CredError;
use ordin_types::domain::CredentialKind;
use ordin_types::CodedError;
use tokio::process::Command;

use crate::error::GithubError;
use crate::service::{lock, ProjectService};

/// Inherited unchanged by a `gh` or `glab` child (2D-1c).
const INHERITED: [&str; 16] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "SystemRoot",
    "TEMP",
];

/// When no reply states one (seconds).
const DEFAULT_RETRY_AFTER_S: u32 = 60;

/// The two kinds of repository host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostKind {
    Github,
    Gitlab,
}

impl HostKind {
    pub(crate) fn program(self) -> ordin_rules::host_forms::HostProgram {
        match self {
            HostKind::Github => ordin_rules::host_forms::HostProgram::Gh,
            HostKind::Gitlab => ordin_rules::host_forms::HostProgram::Glab,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            HostKind::Github => "github",
            HostKind::Gitlab => "gitlab",
        }
    }

    pub(crate) fn git_username(self) -> &'static str {
        match self {
            HostKind::Github => "x-access-token",
            HostKind::Gitlab => "oauth2",
        }
    }

    pub(crate) fn credential(self) -> CredentialKind {
        match self {
            HostKind::Github => CredentialKind::Github,
            HostKind::Gitlab => CredentialKind::Gitlab,
        }
    }
}

/// One `-f key=value` field of a `gh api` call: always a string.
#[derive(Debug, Clone)]
pub(crate) struct Field(pub String, pub String);

/// One REST call through `gh api` or `glab api`.
#[derive(Debug, Clone)]
pub(crate) struct ApiCall {
    pub operation: &'static str,
    pub method: &'static str,
    pub path: String,
    pub fields: Vec<Field>,
}

impl ApiCall {
    pub(crate) fn get(operation: &'static str, path: String) -> ApiCall {
        ApiCall {
            operation,
            method: "GET",
            path,
            fields: Vec::new(),
        }
    }

    pub(crate) fn with(
        operation: &'static str,
        method: &'static str,
        path: String,
        fields: Vec<Field>,
    ) -> ApiCall {
        ApiCall {
            operation,
            method,
            path,
            fields,
        }
    }

    fn arguments(&self) -> Vec<String> {
        let mut args = vec![
            "api".to_owned(),
            self.path.clone(),
            "--method".to_owned(),
            self.method.to_owned(),
            "--include".to_owned(),
        ];
        for Field(key, value) in &self.fields {
            args.push("-f".to_owned());
            args.push(format!("{key}={value}"));
        }
        args
    }
}

/// A reply with its HTTP status.
#[derive(Debug, Clone)]
pub(crate) struct HostReply {
    pub status: u16,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HostReply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub(crate) fn json(&self) -> Result<serde_json::Value, GithubError> {
        serde_json::from_slice(&self.body)
            .map_err(|_| GithubError::Other("The host's reply could not be read.".to_owned()))
    }

    /// `Some(seconds)` on a 429, or a 403 with a rate-limit header.
    fn rate_limit(&self) -> Option<u32> {
        let exhausted = ["x-ratelimit-remaining", "ratelimit-remaining"]
            .iter()
            .any(|name| self.header(name).is_some_and(|value| value.trim() == "0"));
        let retry = self.header("retry-after");
        let limited = self.status == 429 || (self.status == 403 && (exhausted || retry.is_some()));
        limited.then(|| {
            retry
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(DEFAULT_RETRY_AFTER_S)
        })
    }
}

/// Reads the output of `--include`: `HTTP/2.0 200 OK`, the headers, a blank line, the body.
pub(crate) fn parse_included(output: &[u8]) -> Option<HostReply> {
    if !output.starts_with(b"HTTP/") {
        return None;
    }
    let (head, body) = [&b"\r\n\r\n"[..], &b"\n\n"[..]]
        .iter()
        .filter_map(|mark| {
            output
                .windows(mark.len())
                .position(|window| window == *mark)
                .map(|at| (at, at + mark.len()))
        })
        .min()
        .map(|(end, start)| (&output[..end], &output[start..]))
        .unwrap_or((output, &output[output.len()..]));
    let head = String::from_utf8_lossy(head);
    let mut lines = head.lines();
    let status = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let headers = lines
        .filter_map(|line| {
            line.split_once(':')
                .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect();
    Some(HostReply {
        status,
        headers,
        body: body.to_vec(),
    })
}

/// A tool's first error line, without the token.
pub(crate) fn first_line(text: &[u8], token: &str) -> String {
    let text = String::from_utf8_lossy(text);
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("The tool failed without saying why.");
    if token.is_empty() {
        line.to_owned()
    } else {
        line.replace(token, "***")
    }
}

/// A failure with no HTTP reply: no connection, or the tool's own error.
fn without_reply(stderr: &[u8], token: &str) -> GithubError {
    let text = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    let offline = [
        "connect",
        "dial tcp",
        "timeout",
        "timed out",
        "no such host",
        "resolve",
        "network",
    ]
    .iter()
    .any(|word| text.contains(word));
    if offline {
        GithubError::Unreachable
    } else {
        GithubError::Other(first_line(stderr, token))
    }
}

/// What a finished tool left.
pub(crate) struct ToolRun {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn cred_failed(error: CredError) -> GithubError {
    if error.code() == "CRED_SIGN_IN_REQUIRED" {
        GithubError::SignInNeeded
    } else if error.retryable() {
        GithubError::Unreachable
    } else {
        GithubError::Other("The sign-in could not be read.".to_owned())
    }
}

impl ProjectService {
    /// Identity's token for one call (step 3.2).
    // spec: ordin-desktop/components/ProjectService#SA-RECON-05
    // spec: ordin-desktop/components/ProjectService#C-10
    // spec: ordin-desktop/components/ProjectService#C-30
    pub(crate) async fn host_token(&self, kind: HostKind) -> Result<String, GithubError> {
        let token = self
            .identity
            .access_token(kind.credential(), None)
            .await
            .map_err(cred_failed)?;
        Ok(token.token.expose().to_owned())
    }

    fn tool_program(&self, kind: HostKind) -> (PathBuf, Duration) {
        let tunables = lock(&self.tunables);
        let program = match kind {
            HostKind::Github => tunables.gh_program.clone(),
            HostKind::Gitlab => tunables.glab_program.clone(),
        };
        (program, tunables.gh_timeout)
    }

    /// Starts `gh` or `glab` with `arguments` as an argument list, within `GH_TIMEOUT`.
    // spec: ordin-desktop/components/ProjectService#SA-CLOSE-01
    pub(crate) async fn run_tool(
        &self,
        kind: HostKind,
        host: &str,
        token: &str,
        arguments: &[String],
        folder: Option<&Path>,
    ) -> Result<ToolRun, GithubError> {
        let (program, limit) = self.tool_program(kind);
        let mut command = Command::new(&program);
        command
            .args(arguments)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for name in INHERITED {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.env("NO_COLOR", "1");
        match kind {
            HostKind::Github => {
                command
                    .env("GH_TOKEN", token)
                    .env("GH_PROMPT_DISABLED", "1");
            }
            HostKind::Gitlab => {
                command.env("GITLAB_TOKEN", token).env("GITLAB_HOST", host);
            }
        }
        if let Some(folder) = folder {
            command.current_dir(folder);
        }
        let child = command.spawn().map_err(|_| {
            GithubError::Other(format!("{} could not be started.", kind.tool_name()))
        })?;
        let output = tokio::time::timeout(limit, child.wait_with_output())
            .await
            .map_err(|_| GithubError::Unreachable)?
            .map_err(|_| GithubError::Other(format!("{} failed.", kind.tool_name())))?;
        Ok(ToolRun {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    /// One API call. The token and the network are step 1's; a rate limit is always an error; every
    /// other status is the caller's to read.
    pub(crate) async fn api(
        &self,
        kind: HostKind,
        host: &str,
        call: ApiCall,
    ) -> Result<HostReply, GithubError> {
        let token = self.host_token(kind).await?;
        let started = Instant::now();
        let run = self
            .run_tool(kind, host, &token, &call.arguments(), None)
            .await?;
        let reply = parse_included(&run.stdout);
        crate::log::git_host_api(
            call.operation,
            kind.name(),
            reply.as_ref().map_or(0, |reply| reply.status),
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        );
        let reply = reply.ok_or_else(|| without_reply(&run.stderr, &token))?;
        if let Some(retry_after_s) = reply.rate_limit() {
            return Err(GithubError::RateLimited { retry_after_s });
        }
        Ok(reply)
    }

    /// One API call that must succeed: a 404 or a 403 is `GH_NOT_FOUND`, any other failure
    /// `GH_FAILED` with the host's message.
    pub(crate) async fn api_ok(
        &self,
        kind: HostKind,
        host: &str,
        call: ApiCall,
    ) -> Result<HostReply, GithubError> {
        let reply = self.api(kind, host, call).await?;
        if reply.success() {
            return Ok(reply);
        }
        Err(failure_of(&reply))
    }
}

impl HostKind {
    pub(crate) fn tool_name(self) -> &'static str {
        match self {
            HostKind::Github => "gh",
            HostKind::Gitlab => "glab",
        }
    }
}

/// The error of a failed reply (Internal Logic 14.1).
pub(crate) fn failure_of(reply: &HostReply) -> GithubError {
    match reply.status {
        403 | 404 => GithubError::NotFound,
        status => {
            let message = reply
                .json()
                .ok()
                .and_then(|body| body["message"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("The host answered {status}."));
            GithubError::Other(message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_included_reply_is_split_into_status_headers_and_body() {
        let reply =
            parse_included(b"HTTP/2.0 200 OK\r\nX-Thing: 1\r\nContent-Type: a\r\n\r\n{\"a\":1}")
                .unwrap();
        assert_eq!(reply.status, 200);
        assert_eq!(reply.header("x-thing"), Some("1"));
        assert_eq!(reply.json().unwrap()["a"], 1);
        let plain = parse_included(b"HTTP/1.1 404 Not Found\nA: b\n\n{}").unwrap();
        assert_eq!(plain.status, 404);
        assert!(parse_included(b"gh: something else").is_none());
        assert!(parse_included(b"HTTP/2.0 204 No Content\r\n")
            .unwrap()
            .body
            .is_empty());
    }

    #[test]
    fn a_rate_limit_needs_its_header_on_a_403() {
        let exhausted = parse_included(
            b"HTTP/1.1 403 Forbidden\nX-RateLimit-Remaining: 0\nRetry-After: 30\n\n",
        )
        .unwrap();
        assert_eq!(exhausted.rate_limit(), Some(30));
        let plain = parse_included(b"HTTP/1.1 403 Forbidden\n\n").unwrap();
        assert_eq!(plain.rate_limit(), None);
        let many = parse_included(b"HTTP/1.1 429 Too Many\n\n").unwrap();
        assert_eq!(many.rate_limit(), Some(DEFAULT_RETRY_AFTER_S));
    }

    #[test]
    fn the_token_never_reaches_an_error_line() {
        assert_eq!(first_line(b"\nbad secret here\n", "secret"), "bad *** here");
        assert!(matches!(
            without_reply(b"dial tcp: refused", ""),
            GithubError::Unreachable
        ));
        assert!(matches!(
            without_reply(b"some other complaint", ""),
            GithubError::Other(_)
        ));
    }
}
