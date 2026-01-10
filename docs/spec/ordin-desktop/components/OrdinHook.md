# SPEC: ordin hook

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `apps/hook` (binary `ordin-hook`) · **Build phase:** 2 · **Programs:** D P A
> **Dependencies:** Shared types

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)

## Purpose

`ordin-hook` (`ordin-hook.exe` on Windows) is ordin's small signed Rust program that Claude Code runs before every tool call (02:836, ADR-045; the Copilot CLI's calls are decided in process, X3b). It reads the harness's own hook payload, turns it into one harness-neutral `HookRequest`, sends it to the Hook Relay over the authenticated local channel named in its environment, waits for the answer — holding while a person decides, up to its own deadline — and prints the decision in the harness's own format. It fails closed: if the core does not answer, the payload cannot be read, or anything else goes wrong, it denies. The same program relays the harness's after-tool and stop events; in its `mcp-bridge` mode it carries a stdio MCP server connection from the harness to the MCP Front's own local socket, authenticated by the session's MCP token (SA-MCP-01 as amended; R8, X2), so ordin opens no TCP port for MCP; and in its `api-key` mode it hands Claude Code the organisation key when the key cannot travel in the environment (C-74 fallback).

## Approach

A synchronous, single-purpose program with no async runtime: start, read standard input, open one connection, write one line, read one or two lines, print, exit — so it starts in milliseconds under every tool call. Harness differences live in one mapping module per harness (Claude Code; Copilot CLI); the protocol module is shared. It depends on `ordin-types` only (2B-1 rule 3). Error prefix: `HOOK_` (2C). Rejected: a resident hook daemon — it would outlive sessions and hold secrets longer than a call needs. Rejected: answering the harness with *ask* — sessions run with `--permission-prompts none` and the person decides on ordin's approval card, never in the harness (2C: the hook never returns `ask` or `defer`).

## Shared Context

### From `ordin-types` (2C wire §3, §4; copied)

```rust
// Environment (2C HookEnvironment; 2D-1c), set by the adapters in the harness's environment:
// ORDIN_HOOK_ENDPOINT  Unix: <app data root>/run/hook.sock (≤ 104 bytes on macOS); Windows: \\.\pipe\ordin-hook-<installation id>
// ORDIN_HOOK_SECRET    32 bytes base64url without padding (43 characters), per session
// ORDIN_SESSION_ID     the RunId (UUIDv7)
// ORDIN_HOOK_PROTOCOL  "1"

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HookRequest { pub protocol: u16, pub call_id: HookCallId, pub session: RunId,
    pub agent_instance_id: AgentInstanceId, pub mac: String, pub message: HookMessage }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookMessage {
    PreToolUse { harness: HarnessKind, tool: String, origin: ToolOrigin, arguments: serde_json::Value,
                 #[serde(skip_serializing_if = "Option::is_none")] harness_tool_use_id: Option<String>,
                 /// The sub-agent that asked (2C C-20). Conditional on C-78: filled only once the spike
                 /// confirms the harness payload names it; absent until then.
                 #[serde(skip_serializing_if = "Option::is_none")] subagent: Option<String> },
    PostToolUse { harness: HarnessKind, tool: String,
                  #[serde(skip_serializing_if = "Option::is_none")] harness_tool_use_id: Option<String>, succeeded: bool },
    Stop { harness: HarnessKind },
}
pub enum HarnessKind { ClaudeCode, CopilotCli }                       // snake_case
pub enum ToolOrigin { BuiltIn, OrdinMcpFront { server: String }, PackageMcpServer { server: String } } // tag "kind"
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookReply {
    Holding { call_id: HookCallId, approval_id: ApprovalId, deadline: Timestamp },
    Allow { call_id: HookCallId },
    Deny { call_id: HookCallId, reason: String },
    Ack { call_id: HookCallId },
}
```

Rules copied from 2C: one JSON request line ≤ 1 MiB ending `\n`; `mac` = HMAC-SHA256 over the canonical JSON of `message` keyed with the secret's decoded 32 bytes, base64url without padding; `call_id` a fresh UUIDv7; allow or deny without *ask* within 2 s, else deny (`HOOK_CORE_TIMEOUT`); the hook's own deadline 50 s, always shorter than the 60 s harness hook timeout ordin registers; any parse failure, closed connection or unknown `kind` → deny (`HOOK_REPLY_MALFORMED`); the hook never returns `ask` or `defer`. Canonical JSON (Hook Relay spec H): object keys sorted by their UTF-8 bytes, no white space, `serde_json`'s string and number forms (the workspace does not enable `serde_json`'s `preserve_order`).

MCP-bridge environment (MCP Front spec, `SessionMcpConfig`; set by the MCP Front in each bridge server's `env`, never by the hook adapters):

```
// ORDIN_MCP_ENDPOINT  Unix: <app data root>/run/mcp.sock (mode 0600); Windows: \\.\pipe\ordin-mcp-<installation id>
// ORDIN_SESSION_ID    the RunId (UUIDv7)
// ORDIN_MCP_SERVER    the MCP server name this bridge serves (SA-NAME-01..04)
// ORDIN_MCP_TOKEN     32 random bytes, base64url without padding (43 characters), per session; a secret
```

Bridge handshake (MCP Front spec, *Bridge protocol*): one line of at most 1 KiB, `{"protocol":1,"session":"<run_id>","server":"<server>","token":"<ORDIN_MCP_TOKEN>"}`; the core answers `{"kind":"ready"}`, or `{"kind":"refused","code":"<MCP_…>"}` and closes; then MCP JSON-RPC lines are relayed byte for byte.

`ToolOrigin.server`: 1–64 characters, `^[a-zA-Z0-9_.-]+$` (2C); the relay's Rules Engine reclassifies a server the MCP Front serves, so the hook's claim is not trusted for that.

### Harness payloads (08:94; ADR-045; read from the harnesses' hook documentation as the HLD cites it)

- **Claude Code** (`PreToolUse`, `PostToolUse`, `Stop` command hooks registered through `--settings` with `"onFailure": "block"`): standard input is one JSON object with `hook_event_name`, `tool_name`, `tool_input` (object), `tool_use_id`, and for an MCP tool `mcp_server` = `{name, source}` (C-26: `source` is `dynamic` for a server from `--mcp-config`); `PostToolUse` adds `tool_response`; `Stop` carries `stop_hook_active`. Decision output for `PreToolUse`: `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"|"deny","permissionDecisionReason":"<reason>"}}` on standard output, exit status 0. A stop that must not end: `{"decision":"block","reason":"<reason>"}`. Exit status 2 blocks a tool call with standard error as the reason (used only when standard output cannot be written).
- **Copilot CLI** (`preToolUse`, `postToolUse` command hooks): standard input with `toolName` and `toolArgs` (a JSON object, or a string holding one); decision output `{"permissionDecision":"allow"|"deny","permissionDecisionReason":"<reason>"}`. The Copilot CLI documents a Claude-compatible payload too (C-27); the Copilot mapping accepts either form. A timed-out Copilot hook is treated as *allow* by the CLI, which is why the hook's own 50 s deadline is shorter than the 60 s registered (ADR-045).

## Public Interface

### Command line (*chosen here*; the program name is fixed by 2C and SA-NAME-05)

```
ordin-hook --harness <claude-code|copilot-cli> --event <pre-tool-use|post-tool-use|stop> --agent-instance <uuid>
ordin-hook mcp-bridge
ordin-hook api-key --run <run_id> [--data-root <path>]
ordin-hook --version
```

- **Hook mode.** `--harness`, `--event` and `--agent-instance` are written by the Claude Local Adapter into the command it registers in Claude Code's `--settings`, one registration per event, for example `ordin-hook --harness claude-code --event pre-tool-use --agent-instance <id>` (values in kebab case, X3; JSON keeps `HarnessKind`'s snake case). `--agent-instance` supplies `HookRequest.agent_instance_id`, which the 2C environment does not carry. `copilot-cli` is accepted for the conformance tests of C-27, but no adapter registers it in the first release: the Copilot Local Adapter writes no hook file and decides in process through the Hook Relay (X3b). Invoked by: Claude Code, before and after each tool call and at a stop.
- **`mcp-bridge`** (a sub-command with no flags; X2). The MCP Front's `SessionMcpConfig` registers it for each ordin MCP server of a session (`ordin`, `work`, `sandbox`, `memory`, `code`, `mail`, `telegram`, `knowledge`, `craft_<app>`; SA-NAME-01..04) as a stdio MCP server with `args: ["mcp-bridge"]`; everything it needs is in the `ORDIN_MCP_*` and `ORDIN_SESSION_ID` environment. Invoked by: the harness, when it starts that MCP server.
- **`api-key --run <run_id>`** (X2). Prints the organisation key of that run, read from `<data root>/run/sessions/<run_id>/key` (mode `0600`, written by the Claude Local Adapter), followed by a newline, and exits 0. `<data root>` is `--data-root` when given; otherwise, on macOS and Linux, the folder two levels above `ORDIN_HOOK_ENDPOINT` (`<app data root>/run/hook.sock`). On Windows the endpoint is a pipe name, so the adapter always writes `--data-root`. Invoked by: Claude Code, as the `apiKeyHelper` the Claude Local Adapter registers when C-74 fails (`KeyRoute::SettingsApiKeyHelper`).
- **`--version`** prints `ordin-hook <semver> protocol 1` and exits 0. Invoked by: the Setup Assistant's "the ordin hook answers" check (it expects `protocol 1`).

### Exit statuses and output

| Case | Standard output | Exit |
|---|---|---|
| Claude Code `PreToolUse` allow | `hookSpecificOutput` with `permissionDecision: "allow"` | 0 |
| Claude Code `PreToolUse` deny (any cause) | `hookSpecificOutput` with `permissionDecision: "deny"` and the reason | 0 |
| Claude Code `Stop`, ending allowed | nothing | 0 |
| Claude Code `Stop`, ending refused | `{"decision":"block","reason":"<reason>"}` | 0 |
| Claude Code `PostToolUse` | nothing | 0 |
| Copilot CLI `preToolUse` allow or deny | `{"permissionDecision":…,"permissionDecisionReason":…}` | 0 |
| Copilot CLI `postToolUse` | nothing | 0 |
| Standard output cannot be written | reason on standard error | 2 |
| Bad command line | usage on standard error | 64 |
| `mcp-bridge` ended by either side | nothing further | 0 (normal end) or 1 (refused or broken) |
| `api-key`, key read | the key and a newline | 0 |
| `api-key`, key missing or unreadable | nothing | 1 |

### Rust modules inside `apps/hook` (*chosen here*; private to the binary)

```rust
mod env      { pub struct HookEnv { pub endpoint: String, pub secret: [u8; 32], pub session: RunId, pub protocol: u16 }
               pub fn read() -> Result<HookEnv, HookFailure>; }
mod channel  { pub fn connect(endpoint: &str, timeout: Duration) -> Result<Box<dyn ReadWrite>, HookFailure>;
               pub fn exchange(conn, request_line: &[u8], deadline: Instant) -> Result<HookReply, HookFailure>; }
mod mac      { pub use ordin_types::encoding::canonical_json;   // the workspace's one definition (Logging R-3)
               pub fn sign(secret: &[u8; 32], message: &HookMessage) -> String; }
mod claude   { pub fn read_event(stdin: &[u8], event: Event) -> Result<HookMessage, HookFailure>;
               pub fn write_decision(event: Event, outcome: &Outcome, out: &mut dyn Write) -> io::Result<()>; }
mod copilot  { /* same two functions for the Copilot CLI */ }
mod bridge   { pub struct BridgeEnv { pub endpoint: String, pub session: RunId, pub server: String, pub token: String }
               pub fn read_env() -> Result<BridgeEnv, HookFailure>;
               pub fn run(env: &BridgeEnv) -> Result<(), HookFailure>; }
mod api_key  { pub fn run(run_id: RunId, data_root: Option<&std::path::Path>) -> Result<(), HookFailure>; }

pub enum Event { PreToolUse, PostToolUse, Stop }
pub enum Outcome { Allow, Deny(String), Ack }
#[derive(Debug)]
pub struct HookFailure { pub code: &'static str, pub reason: &'static str }
```

### Example

Claude Code runs `ordin-hook --harness claude-code --event pre-tool-use --agent-instance 0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff` with standard input:

```json
{"session_id":"7f1c…","hook_event_name":"PreToolUse","tool_name":"mcp__supplier-db__update_rate","tool_input":{"supplier":"Kestrel","rate":412.5},"tool_use_id":"toolu_01AbCdEf","mcp_server":{"name":"supplier-db","source":"dynamic"}}
```

The hook sends the `HookRequest` of the Hook Relay spec's example, receives `holding` and then `deny` with reason "waiting for the person's approval" after 50 s, and prints:

```json
{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"waiting for the person's approval"}}
```

## Internal Logic

**A. Start.** Parse the command line (`std::env::args`, no parsing library): a first argument `mcp-bridge` selects G, `api-key` selects H, `--version` prints the version; otherwise the hook-mode flags apply; an unknown flag, a missing value or an `--agent-instance` that is not a UUID → usage on standard error, exit 64 (the harness then blocks the call, `onFailure: block`; Copilot treats a failed hook as a failure, not an allow, per C-27's check). Record `started = Instant::now()`; `deadline = started + 50 s`.

**B. Read the environment.** `ORDIN_HOOK_ENDPOINT` (absolute; on macOS ≤ 104 bytes), `ORDIN_HOOK_SECRET` (base64url decoding to exactly 32 bytes), `ORDIN_SESSION_ID` (UUID), `ORDIN_HOOK_PROTOCOL` (must be `1`). Any missing or malformed → deny with `HOOK_ENV_INVALID`, reason "ordin is not set up for this session" (C).

**C. Deny path.** Every failure after A ends here: write the harness's deny form for the event (`PreToolUse`: deny with the reason; `Stop`: `{"decision":"block","reason":…}` is **not** used on failure — an ending that cannot be checked is allowed to end, because blocking an ending on a broken channel would loop the agent; *assumed here*, SA-HOOK-03; `PostToolUse`: nothing), exit 0. If standard output cannot be written, write the reason to standard error and exit 2.

**D. Read the harness payload.** Read standard input to the end, at most 8 MiB (*chosen here*); more, invalid UTF-8 or invalid JSON → `HOOK_PAYLOAD_INVALID`, reason "ordin could not read this tool call".
1. Claude Code: `tool = tool_name` (1–128 characters); `arguments = tool_input` (must be an object); `harness_tool_use_id = tool_use_id`; `subagent` absent (conditional on C-78: once the spike confirms which payload field names the sub-agent, it is read from that field); origin: `mcp_server.name` present → `PackageMcpServer { server: name }`; else a `tool_name` of the form `mcp__<server>__<tool>` → `PackageMcpServer { server }`; else `BuiltIn`. `PostToolUse`: `succeeded = !(tool_response.is_error == true)` (*assumed here*: Claude Code marks failed tool results with `is_error`). `Stop`: no fields read.
2. Copilot CLI: `subagent` absent (as for Claude Code, conditional on C-78); `tool = toolName` (or `tool_name`); `arguments = toolArgs` parsed from a string when it is a string, else the object (or `tool_input`); origin: a `tool_name` of the form `mcp__<server>__<tool>` → `PackageMcpServer`; a name in the Copilot built-in list `view`, `create`, `edit`, `bash`, `powershell`, `report_intent` → `BuiltIn`; any other name → `PackageMcpServer { server: "unknown" }` (*assumed here*, SA-HOOK-04: an unrecognised tool is never claimed as built-in, so the Rules Engine asks for it). `postToolUse`: `succeeded = (toolResult.resultType != "failure")` (*assumed here*). The Copilot CLI offers no stop event to this mapping (ADR-045: "on harnesses that offer these events").
3. A tool name over 128 characters → `HOOK_PAYLOAD_INVALID`.

**E. Build and sign.** `message` from D; `call_id = Uuid::now_v7()`; `mac = base64url_nopad(HMAC-SHA256(secret, canonical_json(serde_json::to_value(&message))))`; `HookRequest { protocol: 1, call_id, session, agent_instance_id, mac, message }`; serialise to one line. A line over 1 MiB → deny `HOOK_REQUEST_TOO_LARGE`, reason "this tool call is too large for ordin to check" (the relay would refuse it, 2C).

**F. Exchange.**
1. Connect within 1 s (*chosen here*): Unix `std::os::unix::net::UnixStream::connect`; Windows open the pipe with `std::fs::OpenOptions` (read, write), retrying every 50 ms while the pipe is busy. Failure → `HOOK_CORE_UNREACHABLE`, reason "ordin is not running".
2. Write the line and `\n`; flush.
3. Read the first reply line with a read timeout of `min(2 s from sending, deadline − now)`. Timeout → `HOOK_CORE_TIMEOUT`, reason "ordin did not answer in time".
4. Parse `HookReply`; any failure, a `call_id` different from the request's, or a closed connection → `HOOK_REPLY_MALFORMED`, reason "ordin's answer could not be read".
5. `Holding { deadline: d }` → read the next line with timeout `min(d, deadline) − now` (the hook never waits past its own 50 s, whatever `d` says). Timeout → deny with reason "waiting for the person's approval" (`HOOK_APPROVAL_PENDING`). A second `Holding` → `HOOK_REPLY_MALFORMED`.
6. Map the final reply: `Allow` → `Outcome::Allow` (only valid for `PreToolUse`); `Deny { reason }` → `Outcome::Deny(reason)`; `Ack` → `Outcome::Ack` (only valid for `PostToolUse` and `Stop`). A reply kind not valid for the event → `HOOK_REPLY_MALFORMED`.
7. Print per the exit-status table: for `Stop`, `Deny(reason)` prints the block form with `reason` (the relay's way of saying "do not end yet"), `Ack` prints nothing.

**G. `mcp-bridge` mode.**
1. Read `ORDIN_MCP_ENDPOINT` (absolute; on macOS ≤ 104 bytes), `ORDIN_SESSION_ID` (UUID), `ORDIN_MCP_SERVER` (`^[a-z][a-z0-9_]{0,63}$`) and `ORDIN_MCP_TOKEN` (43 base64url characters). Any missing or malformed → exit 1 with standard error "ordin is not set up for this session" (`HOOK_BRIDGE_ENV_INVALID`); nothing on standard output, so the harness reports the MCP server as failed to start, which the start-up report shows.
2. Connect to `ORDIN_MCP_ENDPOINT` as F.1 does; failure → exit 1 (`HOOK_CORE_UNREACHABLE`).
3. Send `{"protocol":1,"session":…,"server":…,"token":…}` and `\n` (keys in that order; at most 1 KiB); read one line within 5 s: `{"kind":"ready"}` → continue; `{"kind":"refused","code":…}`, anything else, a closed connection or a timeout → exit 1 with the code on standard error (`HOOK_BRIDGE_REFUSED`).
4. Copy standard input to the connection and the connection to standard output on two threads (`std::io::copy`), each flushing per write; when either side reaches end of file or fails, shut down the other direction and exit 0 (end of file) or 1 (error). No message is parsed or changed: MCP framing is the MCP Front's. The token is never printed.

**H. `api-key` mode.**
1. `--run` must be a UUID → else exit 64 (usage). Data root: `--data-root` when given (absolute, else exit 64); otherwise the parent of the parent of `ORDIN_HOOK_ENDPOINT` on macOS and Linux; on Windows without `--data-root`, or with the variable missing → exit 1 (`HOOK_KEY_UNREADABLE`).
2. Read `<data root>/run/sessions/<run_id>/key`, at most 4 KiB; on macOS and Linux the file must be a regular file owned by the current user with no group or other permission bits (`0600` or stricter), else exit 1 (`HOOK_KEY_UNREADABLE`). Trim one trailing newline; empty → exit 1.
3. Write the key and `\n` to standard output, exit 0. The key is never written to standard error or anywhere else.

## Data Model

N/A — the program keeps no state between invocations and writes no file; its only inputs are the environment, the command line and standard input.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Bad command line (A) | exit 64 | `HOOK_USAGE` | usage text on standard error |
| Environment missing or malformed (B) | exit 0, harness deny | `HOOK_ENV_INVALID` | reason "ordin is not set up for this session" |
| Payload unreadable or tool name too long (D) | exit 0, deny | `HOOK_PAYLOAD_INVALID` | reason "ordin could not read this tool call" |
| Request line over 1 MiB (E) | exit 0, deny | `HOOK_REQUEST_TOO_LARGE` | reason "this tool call is too large for ordin to check" |
| Core not reachable within 1 s (F.1) | exit 0, deny | `HOOK_CORE_UNREACHABLE` | reason "ordin is not running" |
| No first reply within 2 s (F.3) | exit 0, deny | `HOOK_CORE_TIMEOUT` | reason "ordin did not answer in time" |
| Reply unreadable, wrong call id, wrong kind, second holding, closed (F.4–F.6) | exit 0, deny | `HOOK_REPLY_MALFORMED` | reason "ordin's answer could not be read" |
| Person did not decide by the hook's deadline (F.5) | exit 0, deny | `HOOK_APPROVAL_PENDING` | reason "waiting for the person's approval" |
| Relay denies (any relay code) | exit 0, deny | the relay's code (not visible to the hook) | the relay's reason, unchanged |
| Standard output cannot be written (C) | exit 2 | `HOOK_OUTPUT_FAILED` | reason on standard error |
| Bridge environment missing or malformed (G.1) | exit 1 | `HOOK_BRIDGE_ENV_INVALID` | "ordin is not set up for this session" on standard error |
| Bridge refused, broken or no `ready` within 5 s (G.2–G.4) | exit 1 | `HOOK_BRIDGE_REFUSED` | the core's `MCP_…` code on standard error; nothing on standard output |
| Key file missing, unreadable, too open or empty (H) | exit 1 | `HOOK_KEY_UNREADABLE` | nothing on standard output; Claude Code reports the key helper as failed |

For a `Stop` event the failure cases above end the turn normally (C, SA-HOOK-03).

`HOOK_` is the 2C hook-protocol prefix, raised at both ends of the protocol: by this program and by the Hook Relay (HookRelay.md). `HOOK_APPROVAL_PENDING` is the one code both raise, with the same meaning — the person has not decided yet: the relay raises it at its 50 s hold, this program at its own deadline (Error Handling, EH-D10). Every other `HOOK_` code has one raiser.

Reconciliation assumptions (January 2026):
- **SA-RECON-01** — R8, X2: the bridge mode is the sub-command `mcp-bridge`, configured only by the `ORDIN_MCP_*` environment the MCP Front writes, and it connects to the MCP Front's own socket with the session's MCP token; the earlier `--mcp-bridge <server>` flag and the HMAC hello on the hook socket are removed.
- **SA-RECON-02** — X2, X3: modes are sub-commands (`mcp-bridge`, `api-key`); the hook mode keeps the flags `--harness`, `--event`, `--agent-instance` with kebab-case values `claude-code`, `copilot-cli`.
- **SA-RECON-03** — X3b: no adapter writes a Copilot hook file in the first release; the Copilot mapping (D.2) stays for the C-27 conformance tests. The person never sees a code; the harness shows the reason to the model.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: ordin hook

  Scenario: Happy path — Claude Code tool call allowed
    Given the environment holds a valid endpoint, secret, session and protocol 1
    And the relay answers allow
    When Claude Code runs the hook for PreToolUse of Read
    Then standard output holds permissionDecision "allow" and the exit status is 0
    And the request's MAC verifies with the session secret over the canonical JSON of the message

  Scenario: Held, then approved
    Given the relay answers holding and, 12 seconds later, allow
    When the hook runs for PreToolUse
    Then it prints allow after about 12 seconds

  Scenario: Held past the hook's deadline
    Given the relay answers holding with a deadline 120 seconds away and nothing more
    When the hook runs
    Then after 50 seconds it prints deny with reason "waiting for the person's approval"

  Scenario: Core not running
    Given nothing listens on the endpoint
    When the hook runs for PreToolUse
    Then it prints deny with reason "ordin is not running" within about 1 second

  Scenario: Core slow
    Given the relay accepts the connection but writes nothing
    When the hook runs
    Then it prints deny "ordin did not answer in time" after 2 seconds

  Scenario: Missing secret
    Given ORDIN_HOOK_SECRET is not set
    When the hook runs for PreToolUse
    Then it prints deny "ordin is not set up for this session"

  Scenario: Reply for another call
    Given the relay answers allow with a different call id
    Then the hook prints deny "ordin's answer could not be read"

  Scenario: Copilot payload with string arguments
    Given a Copilot preToolUse payload with toolName "bash" and toolArgs "{\"command\":\"ls\"}"
    When the hook runs with --harness copilot-cli
    Then the request's arguments are the object {"command":"ls"} with origin built_in
    And the output is {"permissionDecision":…}

  Scenario: Unknown Copilot tool is not claimed as built-in
    Given a Copilot payload with toolName "supplier_update"
    Then the request's origin is package_mcp_server with server "unknown"

  Scenario: Stop refused
    Given the relay answers deny with reason "The run cannot end yet. The control judged-loop:draft is not met."
    When Claude Code runs the hook for Stop
    Then standard output is {"decision":"block","reason":"The run cannot end yet. The control judged-loop:draft is not met."}

  Scenario: Stop with the core unreachable
    Given nothing listens on the endpoint
    When Claude Code runs the hook for Stop
    Then nothing is printed and the exit status is 0

  Scenario: Bridge relays bytes unchanged
    Given the environment holds ORDIN_MCP_ENDPOINT, ORDIN_SESSION_ID, ORDIN_MCP_SERVER "work" and ORDIN_MCP_TOKEN
    And the MCP Front answers {"kind":"ready"} to the handshake
    When the harness runs "ordin-hook mcp-bridge" and writes an MCP initialise message on standard input
    Then the handshake line carried protocol 1, the session, server "work" and the token
    And the same bytes arrive on the connection, and the connection's bytes appear on standard output

  Scenario: Bridge refused by the MCP Front
    Given the MCP Front answers {"kind":"refused","code":"MCP_BRIDGE_UNAUTHENTICATED"}
    When the harness runs "ordin-hook mcp-bridge"
    Then it exits 1 with nothing on standard output

  Scenario: API key helper prints the run's key
    Given the file "<data root>/run/sessions/<run_id>/key" holds the key with mode 0600
    When Claude Code runs "ordin-hook api-key --run <run_id> --data-root <data root>"
    Then standard output is the key followed by a newline and the exit status is 0

  Scenario: API key file readable by others
    Given the key file has mode 0644
    When the api-key mode runs
    Then it exits 1 and prints nothing

  Scenario: Bad command line
    When the hook runs with --harness vim
    Then it exits 64 with usage on standard error
```

## Performance / Security / Observability

- **Performance targets:** from start to the request written, p95 ≤ 15 ms on a supported laptop (static binary, no async runtime, no file read); an allowed call completes within relay time + 20 ms. Peak memory ≤ 2 × the payload size + 4 MiB (the 8 MiB input cap bounds it at 20 MiB). Binary size ≤ 3 MiB (assumed target, stripped release build).
- **Security:** The secret is read from the environment, decoded once, used for the MAC and never printed, logged or written. The program trusts nothing on standard input beyond its shape: payload text is data. It fails closed on every error for tool calls. It is signed and pinned with the installer (ADR-045); on the server image it is installed beside `ordin` with mode `0755`, owned by root (*assumed here*). It opens no listening socket and makes no network call: only the local endpoint in its environment.
- **Observability:** none of its own: no log file and no output beyond the decision (anything extra on standard output would be read by the harness as the decision). The Hook Relay logs every call on the core's side; `--version` serves the health check.

## Gaps

None.
