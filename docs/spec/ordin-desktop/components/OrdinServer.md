# SPEC: ordin server

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-103) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `apps/server` (package `ordin-server`, binary `ordin-server`) · **Build phase:** 12 · **Programs:** P A
> **Dependencies:** every module listed for P or A

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Gaps](#gaps)
- [Amendment January 2026 — Slack for working together (ADR-102)](#amendment-january-2026--slack-for-working-together-adr-102)
- [Amendment January 2026 — Team knowledge (ADR-103)](#amendment-january-2026--team-knowledge-adr-103)
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)
- [Amendment January 2026 — Every run done by ordin (ADR-107)](#amendment-january-2026--every-run-done-by-ordin-adr-107)
- [Amendment January 2026 — Ordin's language and language files (ADR-109)](#amendment-january-2026--ordins-language-and-language-files-adr-109)
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)
- [Amendment January 2026 — Releases and updates (ADR-114)](#amendment-january-2026--releases-and-updates-adr-114)

## Purpose

`apps/server` is the ordin server program: the same Rust core built without the WebView, for Linux on x86-64 and ARM64, shipped as a signed archive and a container image (02:574; 09 ADR-073; 05 *Server deployment*). It is the second composition root of the workspace (2B-1 rule 3; ADR-100 item 2). It owns the server's start and its stop on `SIGTERM` (journal flushed, sessions given their stop deadline, recovered on next start), its setup command, and the server profile record the setup command writes (02:576–577). It runs in one of two profiles chosen at setup and fixed for the installation's life — **personal** (one person's ordin, reached through Telegram, Slack and paired desktops) or **agent** (one hosted agent, single or scaled shape, started in roles) (ADR-097 item 1; ADR-098 item 5) — and carries the **Always on** behaviour of ADR-076: supervision by a systemd user unit with watchdog or a container restart policy with a health command, idle-session stop, weekly journal compaction, harness sign-in checks, the restart message and the update announcement (02:630). It also carries the agent profile's operator commands, wrightrun's command list as sub-commands (ADR-098 item 2).

## Approach

One binary parses its command line with `clap` (derive API; HLD 05, row *Server command line*), reads its environment once, takes an exclusive lock on the data root, then either runs a long-lived core (`start`, `agent start`) or performs one short operation (`setup`, `health`, the other `agent` sub-commands) and exits. The long-lived core is built by `ServerCore::start` in the fixed construction order of *Internal Logic* step 5, with hub wiring exactly as on the desktop, and `ServerServiceApi` implementing `ServiceApi` for the Remote API and the Channel Gateway. A setup step that changes a running server (pairing, listening, secrets) is sent over a local control socket to that server; with no server running, the setup command constructs only the modules the step needs, in-process. Error-code prefix: `SERVER_` (*chosen here*); the agent sub-commands keep wrightrun's codes (for example `SRV_URL_MISSING`) and exit statuses unchanged (ADR-098 item 2, "What operators read"); this program's own checks of `--serve` use `SERVER_` (ErrorHandling EH-D2). Rejected: one binary per profile (ADR-098 item 1 requires one build and one image for all three uses). Rejected: the setup command writing the database while the server runs (two writers with no ordering; the running server would not see an ended pairing at once, which ADR-096 item 5 requires).

**Binary name.** The server binary is `ordin-server` and the desktop's is `ordin` (D6; SA-NAME-05; 2B-3 as amended January 2026).

## Shared Context

### Environment variables read (2D-1a, 2D-1b; read once at start, never re-read)

| Variable | Type / default | Used by | Rule |
|---|---|---|---|
| `ORDIN_DATA_DIR` | path; `$XDG_DATA_HOME/ordin`, else `$HOME/.local/share/ordin`; image sets `/var/lib/ordin` | every command | must be owned by the running user and writable, else `SERVER_DATA_ROOT_UNWRITABLE` |
| `ORDIN_SECRETS_KEY_FILE` | path | personal, agent single | key file of the encrypted `0600` secrets file (container secret); with `ORDIN_SECRETS_KEY_CREDENTIAL` also set → Identity and Credentials' `CRED_SECRET_KEY_CONFLICT`, passed through (step 4) |
| `ORDIN_SECRETS_KEY_CREDENTIAL` | path | personal, agent single (archive, systemd ≥ 256) | key encrypted with `systemd-creds encrypt --user`; decrypted by Identity and Credentials with `systemd-creds decrypt --user` (C-50) |
| `ORDIN_LOG_LEVEL` | `error`, `warn`, `info`, `debug`; default `info` | every command | other value → `SERVER_ENV_INVALID` |
| `ORDIN_RELEASE` | `<registry>/<repository>@sha256:<64 hex>` | `agent release obtain` | — |
| `ORDIN_REGISTRY_AUTH_FILE` | path | `agent release obtain`, `agent publish` | never printed or logged |
| `ORDIN_RELEASE_DIR` | path | `agent start`, `agent release obtain`, `agent job run-one` | required there |
| `ORDIN_DATABASE_URL_FILE` | path | agent scaled shape: `agent start`, `agent db apply`, `agent job run-one` | present → PostgreSQL; absent → SQLite under `ORDIN_DATA_DIR`; the URL is never printed, logged or placed in an error |
| `ORDIN_PORT` | 1024–65535; default `8080` | agent profile | the one port for caller, intake, operator, work-folder, health and metrics routes; not read by the personal profile |
| `ORDIN_RUN_CONCURRENCY` | 1–100; default `10` in the runs role, `8` in the single shape | agent profile | passed to the Session Supervisor |
| `ORDIN_IMAGE` | string ≤ 512 characters printed | agent profile | passed to the Slack Channel Adapter |
| `ORDIN_OTLP_ENDPOINT` | `https://` URL, or `http://` to loopback | agent profile | passed to Telemetry |
| `ORDIN_MODEL_KEY_FILE` | path | agent profile | passed to Model Policy |
| `ORDIN_SECRETS_DIR` | path | agent scaled shape | passed to the Connection Binder |
| `ORDIN_SERVER_URL`, `ORDIN_TOKEN_FILE` | URL; path | `agent` sub-commands of group S (operator's machine) | token never printed or logged |
| `NOTIFY_SOCKET`, `WATCHDOG_USEC` | set by systemd | `start`, `agent start` | present → `READY=1` and the watchdog signal every 30 s |
| `DBUS_SESSION_BUS_ADDRESS` | set by the session | personal, agent single | presence means a Secret Service may run |
| `TZ` | IANA zone; system zone | Always on | weekly compaction at 04:00 local |
| `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` | either case | outbound HTTPS | not applied to NATS |
| `XDG_DATA_HOME`, `HOME` | path | default of `ORDIN_DATA_DIR` | — |

`RUST_LOG` and `CREDENTIALS_DIRECTORY` are not read. No `MEMORY_*` or `PYS_*` variable is read or set (2D-1e, 2D-1f).

### Server profile record (2C domain, `ServerProfile`; owned by this program)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServerProfile {
    pub id: ServerProfileId,
    pub profile: ServerProfileKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<ServerShape>,   // present if and only if profile = agent (SPROF_SHAPE_MISMATCH)
    pub chosen_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerProfileKind { Personal, Agent }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerShape { Single, Scaled }
```

Written once; any update refused (`SPROF_IMMUTABLE`). Stored in table `server_profiles` (one row), SQLite, and PostgreSQL in the scaled shape.

### Reconciliation assumptions

- **SA-RECON-01** The setup command's secret names are the operator-facing names; each maps to one secret-store name (*Internal Logic* step 9.6 table). `model-key` maps to Model Policy's `agent.model_key` (reconciliation row 478); `env:<VARIABLE>` maps to `connection-env/<variable in lower case>`, which fits Identity's secret-name pattern and is the mapping the Connection Binder's single-shape `SecretSource` reads (row 479).
- **SA-RECON-02** The agent profile constructs neither the NATS Adapter nor the Trigger Service (row 480; D5): with no organisation profile the NATS Adapter has no server to reach, and agent intake belongs to the Caller API.
- **SA-RECON-03** `setup pair-desktop --end COMPUTER_NAME` resolves the name to a `DesktopPairingId` through `RemoteApi::list_pairings` and refuses a name two paired desktops share with `SERVER_PAIRING_NAME_AMBIGUOUS` (rows 161–163; the Remote API keeps `end_pairing(id)`, R6). The other Remote API calls take its own names: `start_pairing` → `PairingOffer`, `list_pairings` → `PairingSummary`, `configure_listening(Option<ListenSettings>)`, `serve` → `Serving`.
- **SA-RECON-04** The Journal has no server-profile, schema or flush calls (N4, N5, N135): the server profile is this program's own table (module `server`), the schema is applied or checked with `MigrationRegistry::apply(…, ApplyMode)`, and the stop is `Journal::shutdown` then `Datastore::close` (R6, R3).
- **SA-RECON-05** Always on uses the Channel Gateway's own notices — `send_restart_notice`, `send_sign_in_ended`, `send_update_available` — and the gateway reports channel outages itself; the gateway's shutdown token replaces `stop_accepting`, and the Trigger Service's shutdown token replaces a pause call (N119, N129–N131).
- **SA-RECON-06** Chat pairing starts at the adapter (`TelegramAdapter::begin_pairing`, `PersonalSlack::begin_pairing`, each taking `paired_by`) and ends at the gateway (`ChannelGateway::end_pairing(kind)`); the route of *Needs you* is `set_needs_you_both(bool)` (N132).
- **SA-RECON-07** After the Catalogue Service is built the program installs it as the `PartSource` of both the Resolver and Compiler and the Checker (R18); hub registration is `McpFront::register` until `McpFront::start`, which freezes the registry (N90). `StandardBlockCheck` is injected into the MCP Front and the Ordin Host.
- **SA-CLOSE-01** *(K4)* The MCP Front is built without a reference resolver; the program calls `mcp_front.bind_ref_resolver(<RefResolver>)` once, after every module the resolver reads exists and before `mcp_front.start`: personal profile after row 5.39 (over the File, Mail, Telegram and Knowledge Services and the Journal); agent profile, `runs` role, after the Knowledge Service (over the File Service, the Knowledge Service and the Journal; no Mail or Telegram Service in that profile, so a reference of those kinds resolves as not found). An error (`MCP_REF_RESOLVER_ALREADY_BOUND`) is `SERVER_START_FAILED`.
- **SA-CLOSE-02** *(O147, chosen here)* The Release Builder's `PackageReader` is the program's `TreePackageReader`: it reads `package_dir` into a `SourceTree` (every regular file under it, `/`-separated relative paths, symbolic links refused, ≤ 50 MiB in all), calls `FormatPlugins::read_any(&tree, &ReadRequest { publisher: "imported", vocabulary: VocabularySet::from_parts(&[])? (the empty set: a package brings its own Vocabulary parts), source_label: <package folder name> })` and keeps its `ReadOutcome`, mints a fresh `AgentRepositoryId` (UUIDv7), builds `TreeSource::new(&outcome, repository_id, "<package folder name>", <the folder's Git HEAD commit, or 40 zeros>)` and returns `PackageRead { source: Box::new(tree_source), root: tree_source.root()?, scope: SourceScope { home: RepositoryRef::Repository(repository_id), profile: vec![], include_bundled: false } }` (FormatPlugins.md SA-RECON-02). A `FormatError` becomes `PackageReadError { file: <the part's file, or package_dir>, message: <the error's code> }`.
- **SA-CLOSE-03** *(K7)* The Telegram Adapter and the Slack Channel Adapter (`PersonalSlack`) take the Journal and keep their tables in its `Datastore`; the program registers `TelegramAdapter::migrations()` and `PersonalSlack::migrations()` and hands neither a pool. Every other table owner except the two phase-1 owners (Organisation Profile, Identity and Credentials) registers its `migrations()` value too, Home and Search included (R3).
- **SA-CLOSE-04** *(K16, chosen here)* The hosted agent's `AgentInstanceId` is minted by this program as a UUIDv7 on the first `agent start` (or `agent db apply`) and kept in this program's `server_settings` table under the key `agent_instance_id`; every later start, and every instance of the scaled shape, reads the same value. It is passed to the Agent Loader in `LoadInput.agent_instance_id` and comes back in `LoadedAgent`.
- **SA-CLOSE-05** *(G-CAT-5)* In the agent profile the program opens the Catalogue Service's own SQLite datastore at `<ORDIN_DATA_DIR>/catalogue/index.sqlite` (folder created with mode `0700`, `backup_dir` = `<ORDIN_DATA_DIR>/catalogue/backups`), applies `ordin_catalogue::migrations()` to it with a separate `MigrationRegistry` and passes it as `CatalogueDeps.datastore`; a failure stops start with `SERVER_START_FAILED` naming `ordin-catalogue` and the module's `CAT_STORE_FAILED`. In the personal profile the Catalogue Service takes the Journal's `Datastore`.
- **SA-CLOSE-06** *(K5, K13)* In the personal profile the program fills what three commands do not carry, as the desktop does: `authoring.publish` reads `AuthoringService::lifecycle_of(&draft)` and, for `Some(lifecycle_id)`, fills `PublishContext` from `HandoverService::following(lifecycle_id)`; `checker.check_installed` gets `lock` = `resolver.open_bundle(installers.record(id).plan_hash).lock_file` and `scope` = `PackageCatalogue::source_scope(id)`; `checker.check_library_item` gets `scope` = `SourceScope { home: the repository (or Bundled), profile: the profile repositories in profile order, include_bundled: true }`; `identity.save_gitlab_token` gets the profile's `gitlab.host` (default `gitlab.com`). `identity.import_client_certificate` is scope `Local` and never reaches the server.
- **SA-CLOSE-07** At start, after the Rules Engine and the Package Inspector exist and before any session starts, the program calls `PackageInspector::reload_rule_sets()` (personal profile, and the `runs` role of the agent profile where the Package Inspector is built); each failure is logged `rule_set_reload_failed` (warn) with `agent_instance_id` and `code`, and start continues (the Rules Engine refuses that instance's sessions, fail-safe).
- **SA-CLOSE-08** *(chosen here)* The program never sets a project's check commands: the organisation profile has no key for them, and `ProjectSettings` carries none. In the personal profile the engineer sets them on the project settings screen, and `projects.set_check_commands` (scope `Any`) reaches `ProjectService::set_check_commands` through the Remote API like every other catalogue command (ProjectService.md SA-CLOSE-03).
- **SA-RECON-08** The program implements every trait that another spec says the server program implements (*Program-side implementations and calls*); the Telemetry counters of crates without a Telemetry edge reach Telemetry through the program's `MetricsSink` sink adapter, never from Journal events (Telemetry.md, R4).

### Fixed constants (2D-4, 1A)

| Constant | Value |
|---|---|
| systemd restart | 5 s, doubling to 5 minutes (`RestartSteps=`, `RestartMaxDelaySec=`, systemd 254+) |
| Watchdog signal | every 30 s; the shipped unit sets `WatchdogSec=90s` |
| Idle harness session stopped | 30 minutes without a turn |
| Harness sign-in check | every 6 hours, and before each harness session (the latter in the personal profile: `SetupAssistant::run_checks(CheckTrigger::BeforeSession)`, called by this program's `SessionSetupSource`) |
| Journal compaction | weekly, 04:00 server time, when no run is going |
| Channel outage reported on the other channel | after 10 minutes, once |
| Runs on `SIGTERM` (Phase 4 decision P4-2) | personal profile: running runs may finish until 30 s after the signal, then are cancelled and recorded `interrupted`; agent profile: never cut short by ordin, left to lease expiry |
| Stop step bound | 2 s per step; the Channel Gateway's outbox drain 10 s; the Remote API's in-flight wait 5 s (P4-2) |
| Stop period | `TimeoutStopSec=60s` (personal unit); container stop period 60 s (`--stop-timeout 60`) |
| Log retention (personal, single shape) | 14 days of daily files (A-09) |
| Remote API default port proposed by setup | `8443` (SA-NET-01); allowed 1025–65535 |
| Desktop pairing code | valid 10 minutes; 5 wrong codes in 10 minutes stops pairing |
| Telegram and Slack pairing codes | valid 10 minutes |
| Instance roles | `requests`, `runs`, `chat`, `jobs`, `chat-jobs`; all roles in the single shape |

### Where the Remote API may listen (09 ADR-096 item 4)

Loopback (`127.0.0.0/8`, `::1`) and private addresses only: RFC 1918 (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`) and RFC 4193 (`fc00::/7`). Any other address is refused by the setup command. The organisation profile key `server.listening_allowed = false` switches the Remote API off; `server.client_certificate_required` makes desktops present a client certificate (2D-2).

### What each profile runs (2B-3 *Programs* column; ADR-097 item 8)

- **Personal (P):** Journal, Organisation Profile, Identity and Credentials, Approval Service, Localisation, Agent Folder Manager, Format Plug-ins, Resolver and Compiler, SharePoint Adapter, Hosted Agent Adapter, Slack Thread Adapter, NATS Adapter, Usage Planner, Rules Engine, Checker, Installers, Session Supervisor, Remote API, MCP Front, Package Inspector, File Service, Hook Relay, Crew Orchestrator, Sandbox, Memory Service, Craft Toolkit, Project Service, Claude Local Adapter, Copilot Local Adapter, Workspace Service, Catalogue Service, Push Service, Knowledge Service, Mail Service, Package Catalogue, Ordin Host, Home and Search, Setup Assistant, Authoring Service, Channel Gateway, Handover Service, Telegram Adapter, Slack Channel Adapter, Trigger Service, Telegram Service. Not offered on the server in the first step: engineering mode's screens, ArtCraft windows for the person, microphone dictation, Confluence until C-49 confirms a sign-in route (02:574).
- **Agent (A):** Journal, Organisation Profile (no profile read, every key at its default — 2D *Agent-profile server*), Identity and Credentials, Agent Configuration, Telemetry, Approval Service, Localisation, Agent Folder Manager, Format Plug-ins, Resolver and Compiler, NATS Adapter, Usage Planner, Run Queue, Model Policy, Caller Authentication, Settings Store, Rules Engine, Checker, Installers, Session Supervisor, Duty Timer, MCP Front, Package Inspector, File Service, Hook Relay, Memory Service, Claude Local Adapter (organisation key), Connection Binder, Catalogue Service, Agent Loader, Knowledge Service, Release Builder, Retention and Erasure, Channel Gateway, Job Runner, Caller API, Slack Channel Adapter. The NATS Adapter is linked (2B-3: P A) but not constructed: with no organisation profile its `nats.servers[]` is empty and it would stay `Off` (NatsAdapter *Agent profile*). The Trigger Service is personal profile only (D5); the agent profile's intake routes belong to the Caller API. Ordin and its crews, workspaces linked with a person's sign-ins, engineering mode, ArtCraft windows and dictation are not in this profile; desktop pairing is refused (ADR-097 items 8 and 9).

## Public Interface

### Command line (program `ordin-server`, SA-NAME-05)

```text
ordin-server --version                         prints "ordin-server <version>"
ordin-server start                             personal profile: run until stopped
ordin-server health                            asks the running server over its control socket; exit 0 healthy, 1 not
ordin-server setup                             guided set-up (interactive terminal)
ordin-server setup profile [URL --signing-key KEY [--signing-key KEY]]   organisation profile address and its one or two signing keys (personal only)
ordin-server setup listen [--address IP --port N --certificate PEM_FILE --key PEM_FILE | --off]
ordin-server setup pair-desktop [--list | --end COMPUTER_NAME]
ordin-server setup pair-chat --channel telegram|slack [--end] [--needs-you both|last-used]
ordin-server setup secret NAME                 reads the value from standard input (echo off on a terminal)
ordin-server setup history --days N
ordin-server setup release REFERENCE           agent profile, single shape
ordin-server agent <command> ...               the agent profile's operator commands (table below)
```

`profile`, `pair-chat` and `pair-desktop` are SA-NAME-05's steps; `listen`, `secret`, `history` and `release` are *chosen here* (externally visible; in HLD 08 since revision 0.49.0). `NAME` of `setup secret` is one of: `telegram-bot-token`, `slack-bot-token`, `slack-app-token`, `nats-credentials` (the content of a NATS credentials file), `nats-user-password` (asks the user name, then the password), `nats-token`, `mail-password`, `model-key` (agent single shape), `env:<VARIABLE>` (agent single shape, a connection credential the agent configuration names; `<VARIABLE>` matches `^[A-Z_][A-Z0-9_]*$`) — names *chosen here*.

### Agent profile operator commands (`ordin-server agent …`; wrightrun's commands in kebab-case, ADR-098 item 2)

Arguments, output on success, confirmations and exit statuses are wrightrun's, as `agent-wright-runtime/docs/spec/components/OperatorCommands.md` fixes them, with `wrightrun` replaced by `ordin-server agent` and `WRIGHTRUN_` by `ORDIN_`. Group: L local, S calls a running hosted agent (`ORDIN_SERVER_URL`, `ORDIN_TOKEN_FILE`), D database, P instance.

| Command | Arguments | Group | Carried out by |
|---|---|---|---|
| `prepare` | `--config FILE --out DIR` | L | Release Builder |
| `check` | `--config FILE` | L | Release Builder (start-up report, no digest) |
| `publish` | `--config FILE --release-id ID --to REPOSITORY` | L | Release Builder |
| `release obtain` | none | P | Agent Loader (`obtain`) |
| `run-once` | `--config FILE` and `--command SLUG [--arguments TEXT]` or `--message TEXT` | L | Session Supervisor through the Claude Local Adapter |
| `expand` | `--bundle DIR SLUG [ARGUMENTS]` | L | Session Supervisor (command expansion) |
| `duty status` | `[DUTY ...]` | S | Caller API operator client |
| `runs list` / `runs show` / `runs resume` | as wrightrun | S | Caller API operator client |
| `files list` / `show` / `write` / `remove` | as wrightrun | S | Caller API operator client |
| `db apply` | none | D | Journal (`MigrationRegistry::apply(…, ApplyMode::ApplyPending)` over every module's registered migrations) |
| `settings list` / `show` / `set` | `--scope SCOPE …` | S | Caller API operator client |
| `jobs list` / `jobs start` | as wrightrun | S | Caller API operator client |
| `notes list` / `show` / `retire` | `--scope SCOPE …` | S | Caller API operator client |
| `entries add` / `list` / `verify` / `remove` | `--scope SCOPE …` | S | Caller API operator client |
| `erase person` / `erase scope` | as wrightrun | S | Caller API operator client |
| `hosts` | `--config FILE` | L | Agent Configuration (`connection_hosts`) |
| `start` | `[--serve ROLE ...]` | P | this program (*Internal Logic* step 6) |
| `job run-one` | `--kind KIND --key KEY` | P | Job Runner |

`--serve ROLE` takes `requests`, `runs`, `chat`, `jobs` or `chat-jobs` (same as `--serve chat --serve jobs`); given once per role; absent in the single shape means every role. The four confirmations (`files write`, `files remove`, `erase person`, `erase scope`) read one line from standard input; anything but `yes` exits with status 3 and changes nothing.

### Exit statuses (every command; wrightrun's)

`0` done; `1` a named error, printed on standard error as `error[<CODE>]: <message>` with the code and one sentence; `2` wrong usage (clap's usage text on standard error); `3` the person did not confirm; `4` `agent run-once` ended in a pause; `70` an unexpected failure that is no named error; `130` interrupted from the keyboard.

### Control socket (*chosen here*)

`<ORDIN_DATA_DIR>/run/control.sock`, Unix domain socket, mode `0600`, created by `start` and `agent start` in the personal profile and the single shape. Every connection is checked with `SO_PEERCRED`: a peer whose user id differs from the server's is closed with `SERVER_CONTROL_REFUSED`. One request per connection, one JSON line each way, read limit 64 KiB.

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlRequest {
    Health,
    Setup { step: SetupStep },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case", deny_unknown_fields)]
pub enum SetupStep {
    Profile { url: Option<String> },
    Listen { address: Option<std::net::IpAddr>, port: Option<u16>, certificate_pem: Option<String>, key_pem: Option<String>, off: bool },
    PairDesktop { action: PairDesktopAction },
    PairChat { channel: ChatChannel, end: bool, needs_you: Option<NeedsYouRoute> },
    Secret { name: String, value: String },
    History { days: u32 },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PairDesktopAction { Start, List, End { computer_name: String } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatChannel { Telegram, Slack }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedsYouRoute { Both, LastUsed }

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ControlReply {
    Ok { lines: Vec<String> },          // text the setup command prints, one line each
    Healthy { profile: ServerProfileKind, started_at: Timestamp },
    Error { code: String, message: String },
}
```

A `Secret` value travels only over this socket, is handed to Identity and Credentials and is never logged; the request line is zeroed in memory after parsing.

### Shipped files (*chosen here*; in HLD 08 since revision 0.49.0 — packaging names)

**Archive** `ordin-server-<version>-linux-<x86_64|aarch64>.tar.gz`: `bin/ordin-server`, `bin/ordin-hook`, `share/systemd/ordin-server.service`, `share/systemd/ordin-server-agent.service`, `LICENSE`, `NOTICE`. Installed by the server owner under `~/.local/bin` and `~/.config/systemd/user/`.

**`ordin-server.service`** (personal profile):

```ini
[Unit]
Description=ordin server
Wants=network-online.target
After=network-online.target

[Service]
Type=notify
NotifyAccess=main
ExecStart=%h/.local/bin/ordin-server start
Environment=ORDIN_SECRETS_KEY_CREDENTIAL=%h/.local/share/ordin/secrets-key.cred
Restart=on-failure
RestartSec=5s
RestartSteps=6
RestartMaxDelaySec=5min
WatchdogSec=90s
TimeoutStopSec=60s

[Install]
WantedBy=default.target
```

**`ordin-server-agent.service`** (agent profile, single shape): as above with `ExecStartPre=%h/.local/bin/ordin-server agent release obtain`, `ExecStart=%h/.local/bin/ordin-server agent start`, `Environment=ORDIN_RELEASE_DIR=%h/.local/share/ordin/release`, `EnvironmentFile=%h/.local/share/ordin/agent.env` (written by `setup release`), and `TimeoutStopSec=` the run limit plus 60 s, default `3660s` (wrightrun *GracefulShutdown*: a run instance's stop period is at least the run limit plus 60 s).

`Restart=on-failure` restarts after any exit other than a requested stop, a crash and a missed watchdog (ADR-076 *Supervision*); lingering after logout is the server owner's step (`loginctl enable-linger`, C-51), printed by `setup`, never run by it.

**Container image** (`deploy/container/Containerfile`):

```dockerfile
FROM debian:bookworm-slim
RUN useradd --uid 10001 --create-home --home-dir /home/ordin ordin \
 && mkdir -p /var/lib/ordin && chown 10001 /var/lib/ordin
COPY bin/ordin-server bin/ordin-hook /usr/local/bin/
ENV ORDIN_DATA_DIR=/var/lib/ordin
VOLUME ["/var/lib/ordin"]
USER 10001
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 CMD ["ordin-server", "health"]
ENTRYPOINT ["ordin-server"]
CMD ["start"]
```

The base image is *assumed here* (glibc, the archive's build target; 05 names none). Claude Code is not in the image: the deploying organisation or the server owner adds it in a layer of its own at the pinned version (ADR-098 item 7). The restart policy "unless stopped" is set by the run command the setup instructions give (`podman run --restart unless-stopped --stop-timeout 60 …`, ADR-076); `--stop-timeout 60` gives the orderly stop of step 11.1 the same 60-second stop period as the systemd unit (P4-2). For the agent profile the Helm chart overrides `CMD` per role and replaces `HEALTHCHECK` with HTTP probes.

**Helm chart** (`deploy/helm/ordin-server/`, one hosted agent; values *chosen* in 2D-3):

| Value | Default | Effect |
|---|---|---|
| `image` | — (required) | the ordin server image, by digest |
| `release` | — (required) | `ORDIN_RELEASE` of the init container |
| `secretName` | — (required) | Kubernetes Secret mounted at `/var/run/ordin/secrets`; keys `ORDIN_DATABASE_URL`, `ORDIN_MODEL_KEY` and each connection variable the configuration names. The chart sets `ORDIN_DATABASE_URL_FILE=/var/run/ordin/secrets/ORDIN_DATABASE_URL`, `ORDIN_MODEL_KEY_FILE=/var/run/ordin/secrets/ORDIN_MODEL_KEY`, `ORDIN_SECRETS_DIR=/var/run/ordin/secrets` *(assumed here: one Secret, files the Connection Binder does not list are ignored)* |
| `roles.<role>.replicas` | `requests` 2, `runs` 2, `chat-jobs` 1, `chat` 0, `jobs` 0 *(assumed here)* | one Deployment per role with replicas > 0, started `agent start --serve <role>` |
| `roles.<role>.resources` | chart defaults | container resources |
| `roles.runs.concurrency` | `10` | `ORDIN_RUN_CONCURRENCY` of run instances |
| `roles.runs.stopPeriodSeconds` | `3660` | `terminationGracePeriodSeconds` of run instances; requests 40, chat and jobs 60 (wrightrun's shipped values) |
| `otlpEndpoint` | empty | `ORDIN_OTLP_ENDPOINT` |
| `logLevel` | `info` | `ORDIN_LOG_LEVEL` |
| `podAnnotations` | `{}` | annotations written to the pod template of every role's Deployment and of the migration Job *(amended January 2026, ADR-107)* |
| `roles.<role>.podAnnotations` | `{}` | annotations written to that role's pod template only, after `podAnnotations`; on a key present in both, the role's value wins *(amended January 2026, ADR-107)* |

Each Deployment has an init container running `ordin-server agent release obtain` into an `emptyDir` at `ORDIN_RELEASE_DIR=/var/lib/ordin/release`, liveness `GET /healthz` and readiness `GET /readyz` on `ORDIN_PORT`, and runs as user 10001 with a read-only root file system. A Job annotated `helm.sh/hook: pre-install,pre-upgrade` runs `ordin-server agent db apply` before a release's instances start (07 *Migrations*). `deploy/argocd/application.yaml` is an example Argo CD Application pointing at the chart.

### Commands this program answers itself (Service API component spec, catalogue 1.0)

`ServerServiceApi` implements every catalogue method of scope `Any` by calling the module its *Implemented by* column names, with the `CallContext`, and maps a module's error to `ServiceError { code, message: sentences.sentence(code), action: sentences.action(code) }`. Methods of scope `Local` are never reached on the server: the Service API dispatcher refuses them with `SVC_COMMAND_LOCAL_ONLY` before dispatch. The one command the catalogue gives to the program is `service.hello` → `ServiceHello { api_version: CURRENT, backend: <the Journal's BackendId>, backend_name: <the server's host name, at most 63 characters> (*assumed here: 11 §5.31 shows the server by a display name the desktop keeps; the host name is the server's own default*), profile: PersonalServer, last_seq: journal.last_seq() }`. `ServerErrorSentences` implements the Service API's `ErrorSentences` over Localisation, as on the desktop. The agent profile serves no Service API (ADR-097 item 9: no desktop pairing; the Caller API is its way in).

### Program-side implementations and calls (*Interactions*, owner side)

Each item is named by its owning spec (ruling R6: the owner's names win; the program conforms). Profile: P personal, S agent single shape, C agent scaled shape. Step numbers refer to *Internal Logic*. Besides the rows below, every narrow port trait a module's `…Deps` struct declares (for example `JournalWriter`, `HandoverGithub`, `KnowledgeFiles`, `PublishedCopies`) is implemented by this program as a pass-through over the module that trait's own comment names.

| Item (owning spec) | What this program does | Profile | When |
|---|---|---|---|
| `Journal::recover_external_actions` (Journal and Artefact Store) | calls it once | P S C (`runs`) | before `session_supervisor.recover()` (step 5 tail, step 6.3) |
| `Journal::set_company_max_days` (Journal and Artefact Store) | passes the profile's `history.max_days` | P | at start and on each Organisation Profile change |
| `PruneParticipant` (Journal and Artefact Store) | `journal.register_prune_participant(…)` for each module that offers one: `prune_participant()` of the Approval Service, Session Supervisor, File Service and Usage Planner (P S C); of the Crew Orchestrator, Ordin Host, Mail Service and Authoring Service (P); and the Remote API itself (module `remote_api`, P) | P S C | after the module's construction |
| `EventSink` registrations | `journal.subscribe(module.event_sink())` for the Approval Service, Hook Relay and Session Supervisor (P S C) and the Push Service and Handover Service (P); `journal.subscribe(workspaces)` for the Workspace Service, which implements `EventSink` itself (P); nothing for the Mail Service, Telegram Service and Sandbox, which subscribe themselves | P S C | after the module's construction |
| `SupervisorHost` (Session Supervisor) | implements every method: `startup_items` over the Rules Engine, MCP Front `offered_tools` and the Installers; `remote_route` over the Organisation Profile's remote agents; `expand_command` over the Agent Loader's bundle (agent profile); `run_controls`, `run_resuming`, `run_ended` over the Rules Engine; `run_abandoned(run_id, adapter)` calls the Slack Thread Adapter's `abandon(run_id)` when `adapter` is `SlackThread` (personal profile; nothing for any other kind, and nothing in the agent profile, which has no Slack Thread Adapter); `reply_for` and `retry_after_block` over Model Policy's `reply_for` and `after_block` (agent profile; `None` and `false` in the personal profile); `check_content(run_id, point, text)` over `RulesEngine::check_content(<the loaded instance>, run_id, point, text)`, `ContentPoint::Input`/`Output` mapped to `CheckPoint::Input`/`Output` and the engine's `ContentCheckOutcome` mapped to the supervisor's `ContentCheck` (`Passed` → `Passed`; `Blocked { on_block, .. }` → `Blocked { retry_without_context: on_block == OnBlock::RetryWithoutContext }`) (agent profile; `Passed` in the personal profile) (*amended January 2026, ADR-107*) | P S C | passed to `SessionSupervisor::new` (row 5.14, step 6.3) |
| `RulesHost` (Rules Engine) | implements it and passes it to `RulesEngine::new`: the path facts `path_exists`, `path_contains`, `read_text`, `append_line` and `remove` over the agent folder (personal) or the agent's work folder (agent profile); `runs_in_progress`, `paused_run_with_command` and `run_ended` over the Session Supervisor's `runs_with_command`, `paused_with_command` and `has_ended` (personal) or the Run Queue's `in_progress` and `paused_with_command` (agent profile); `setting_value` and `setting_rules` over the Settings Store (agent profile); `word` and `plain_line` over Localisation; `content_checker()` returns the program's `ContentChecker` in the agent profile and `None` in the personal profile (*amended January 2026, ADR-107*) | P S C | row 5.12, step 6.1 |
| `SessionSetupSource` (Claude Local Adapter, Copilot Local Adapter) | implements its one method `session_setup` over the Session Supervisor, the Installers' `record(id).launch`, Model Policy, the Usage Planner, the Crew Orchestrator's `get(crew_run_id)` (a crew member run's `crew_charter_id`, Claude and Copilot, personal profile; `None` in the agent profile, which has no Crew Orchestrator), the Run Queue's run record (`get(run_id)` → `RunView.record.started_by` as `run_starter`, Claude only, agent profile; `None` in the personal profile) and the Agent Folder Manager (`lock(Session)` held for the run's life; `apply_grants` at session start and on resume and `session_access` when a resumed session's setup is rebuilt, Claude only — `CopilotSessionSetup` carries no granted folders); agent profile: the agent's set-up from `LoadedAgent.session` (`HostedSessionSetup`); personal profile: calls `SetupAssistant::run_checks(CheckTrigger::BeforeSession)` before each harness session and refuses the session with that report's failing step | P S C (Copilot: P) | rows 5.23, 5.24; step 6.3 |
| `SessionRecordStore` (Claude Local Adapter) | implements `save` over `SessionSupervisor::save_session_record` and `load` over `load_session_record` (the session record travels through the Run Queue in the agent profile) | P S C | rows 5.23; step 6.3 |
| `HarnessModels` (Runtime Adapter interface) | gives `harness_models()` of the Claude Local Adapter and, personal profile, the Copilot Local Adapter to the Usage Planner with `usage.set_harnesses(…)` (UsagePlanner late binding) | P S C (Copilot: P) | right after row 5.24 (personal) or after the Claude adapter is built (step 6.3) |
| `CopilotPort` (Copilot Local Adapter) | implements its one method `start_server` with `SdkCopilotPort` over the Copilot CLI at the profile's pinned path (row 5.24) | P | row 5.24 |
| `CrewHost` (Crew Orchestrator) | implements all seven methods, as the desktop does: `agent_status` over the Package Catalogue; `saved_agent_versions` and `workflow_steps` over `OrdinHost::{saved_agent_versions, workflow_steps}`; `document_changed` over the File Service; `word_step_approval` over Localisation (the card wording the `RulesHost::word` adapter uses); `startup_items` over the Rules Engine's `startup_items(agent_instance_id)` and the MCP Front's offered tools, for every member of the charter; `remote_route` over the Organisation Profile's remote agents, as `SupervisorHost::remote_route` | P | row 5.18; the Ordin Host (row 5.32) and the Package Catalogue (row 5.31) are constructed later, so the program's `CrewHost` holds handles set once after row 5.32; no crew run starts before the Ordin Host exists |
| `AuthoringHost`, `HandoverHost` (Ordin Host implements both) | passes the Ordin Host as `AuthoringDeps.host` and `HandoverDeps.host` | P | row 5.35 |
| `HandoverReach` (Authoring Service) | implements `reachable_places` over the Workspace Service's linked Git repositories and SharePoint folders (AuthoringService.md SA-CLOSE-05); passed as `AuthoringDeps.places` | P | row 5.35 |
| `PersonIdentity` (Handover Service) | implements `person` over `IdentityService::person()`; passed as `HandoverDeps.identity` | P | row 5.35 |
| `AgentNames` (Mail Service) | implements `display_name_of` over the Package Catalogue's `agents_in_workspace` (MailService.md); passed as `MailDeps.agent_names` (*late-bound*: the Package Catalogue is built at row 5.31) | P | row 5.30 |
| `ServerInstaller`, `StreamSink`, `watch::Receiver<ProjectSettings>` (Project Service) | `ServerInstaller::ensure` over `SetupAssistant::ensure_language_server` (*late-bound*: the Setup Assistant is built at row 5.34); `StreamSink::send` drops every payload, because the personal server offers no terminal or editor stream (ProjectService.md: `terminal_*` and `lsp_send` are desktop-only) *(chosen here)*; the settings channel built from the Organisation Profile's `language_servers[]` and `engineering.github` keys (no check commands: they come only from `projects.set_check_commands`, SA-CLOSE-08) | P | row 5.22 |
| `PublishContext`, `AuthoringService::lifecycle_of` (Authoring Service) | reads `lifecycle_of(&draft)`; `Some(lifecycle_id)` → fills `PublishContext` from `HandoverService::following(lifecycle_id)` (`following_workspaces`, `open_handovers`); `None` → no context (SA-CLOSE-06) | P | each `authoring.publish` |
| `PackageCatalogue::source_scope`, `ResolverAndCompiler::open_bundle`, `Installers::record` | fill the **Check it** commands' `lock` and `scope` (SA-CLOSE-06) | P | each `checker.check_installed` / `checker.check_library_item` |
| `PackageInspector::made_of`, `installed_files`, `part_page` (Package Inspector) | serve `agents.made_of`, `agents.installed_files`, `agents.part_page`, filling `folder_path` from `PackageCatalogue::get(id)` → `instance.agent_folder_path` and `bundle_dir` from `open_bundle(installers.record(id).plan_hash)` → `Bundle.dir` (ServiceApi.md SA-CLOSE-06) | P | each of the three commands |
| `PackageInspector::reload_rule_sets` (Package Inspector) | calls it once at start (SA-CLOSE-07) | P S C (`runs`) | step 5 tail; step 6.3 |
| `RefResolver` (Service API), `McpFront::bind_ref_resolver` (MCP Front) | implements all six `RefResolver` methods as the desktop does — `content` over the File Service; `email_message` over the Mail Service; `entity` over the Telegram Service's `resolve_list_block` and the Knowledge Service's `resolve_block`; `run_agent_name` over the Journal's run index; `plan_state` over the Journal's `current_plan`; `approval_verbs` from the Approval Service's approval verbs — and binds it once (SA-CLOSE-01); in the agent profile, which has no Mail or Telegram Service, `email_message` answers false and `entity` resolves through the Knowledge Service only | P S C (`runs`) | step 5 tail; step 6.3 |
| `CatalogueService` datastore (Catalogue Service) | opens `<ORDIN_DATA_DIR>/catalogue/index.sqlite` in the agent profile (SA-CLOSE-05) | S C (`runs`) | step 6.3 |
| `BlockCatalogueCheck` (Service API) | passes `StandardBlockCheck` to `McpFront::new` and as `OrdinHostDeps.blocks` | P S C (Ordin Host: P) | rows 5.15, 5.32; step 6.3 |
| `TriggerDesk` (Ordin Host) | implements it: `proposal_schema()` = `TriggerService::proposal_schema()`; `propose(workspace_id, json)` deserialises the JSON plus `workspace_id` into `TriggerProposal`, calls `TriggerService::propose` and returns the trigger id as text; a `TriggerError` becomes `TriggerDeskError { code, message, retryable }`. Registered with `ordin_host.set_trigger_desk` | P | after `TriggerService::start` (row 5.38) |
| `SlackReactor` (Slack Service) | as the desktop: `react(workspace_id, slack_link_id, channel_name, kept)` → `ordin_host.react_to_slack(workspace_id, slack_link_id, channel_name.to_owned(), kept)`; an `OhError` returned as `SlackReactError { code: e.code().to_owned(), retryable: e.retryable() }` | P | row 5.30a; Ordin Host *late-bound* after row 5.32 |
| `SlackRulesSource` (Slack Service) | built from the profile's `document.slack` (`tools_enabled`, `allowed_channels`, `rules_allowed`) and `document.server.external_actions_off`; `slack_post` in that list switches every Slack post off, cards included | P | row 5.30a |
| `RefResolver::entity` | gains `EntityRef::SlackLink` over the Slack Service's `resolve_list_block` | P | step 5 tail |
| `TeamMemory`, `TeamPages`, `TeamPush`, `TeamWorkspaces`, `TeamRepos` (Team Knowledge Service) | as the desktop (OrdinDesktop.md): one-for-one delegations to the Memory Service, Knowledge Service, Push Service, Workspace Service, Project Service as `GithubAccess`, and `IdentityService::person()` (amended January 2026, ADR-103) | P | row 5.32a |
| `PersonalCore` (Channel Gateway) | implements it: `ordin_run(workspace_id)` → `Ok(OrdinHost::current_run(workspace_id))`; `conversation_turn` → `OrdinHost::send_message`; `answer_question`, `submit_form` → `OrdinHost::answer_block`; `pause_all_runs`, `running`, `needs_you_line`, `step_texts` over the Session Supervisor and the Journal; `current_workspace`, `content_file`, `content_label` over the Workspace Service and File Service | P | passed in `PersonalDeps.core` (row 5.36) |
| `SandboxGrants`, `PlaceFiles` (Sandbox traits, Sandbox.md) | implements `SandboxGrants::sandbox_capabilities` from the Package Inspector's declared connections, the organisation profile and Identity and Credentials; `PlaceFiles`: `read_for_agent` over the File Service's grant-checked `read_for_agent`, `write_agent_output` into the Agent Folder Manager's `get(agent_instance_id)` folder | P | passed in `SandboxDeps.grants` and `SandboxDeps.place_files` (row 5.19) |
| `WorkingCopyStore` (Push Service) | implements all five methods, as the desktop does: `entry` over `WorkspaceService::working_copy_entry`; `set_state` over `WorkspaceService::set_working_copy_state`; `content` over the File Service's `read_file` (current bytes) or, with `local_version`, `versions` then `read_version` of that version; `restore_version` over the File Service's `versions` then `restore_version`; `local_changes` over `WorkspaceService::subscribe_working_copy_changes` | P | passed in `PushDeps.working_copy` (row 5.28) |
| `JournalReader` (Home and Search) | implements all four methods as HomeAndSearch.md documents them: `running` over the Journal's `running_runs`, `finished_since` over `runs_ended_since`, both titled from the Session Supervisor's and the Crew Orchestrator's `get`; `run_inputs` over `Run.inputs_used` (Session Supervisor `get`) or a crew run's inputs (Crew Orchestrator `get`); `waiting_items` maps each `OrdinHost::waiting_blocks()` entry to `WaitingItem { workspace_id, kind, title, since, opens: OpenTarget::Block { workspace_id, block_id } }` beside the Approval Service, Journal, Push Service and Handover Service rows (HomeAndSearch SA-CLOSE-01) | | P | row 5.33 |
| `InstanceActivity`, `HarnessSettings`, `ResolveContext.workspace_languages` (Package Catalogue); `SearchSource` registration (Home and Search `register_source`) | as on the desktop: `InstanceActivity::running` and `idle` over the Session Supervisor; `HarnessSettings::default_harness` over `ProfileSnapshot::default_harness()`; the agents source (`GoToKind::Agent`, `find` over the Package Catalogue's `list`) and the Ordin Host's workflows (`GoToKind::Workflow`, `workflows.list`) and conversations (`GoToKind::Conversation`, `ordin.list_conversations`) sources, each `find` returning at most `limit` results | P | rows 5.31, 5.33 |
| `RemoteAgentDirectory`, `CompanyTokenSource` (Hosted Agent Adapter), `SlackAgentDirectory`, `SlackUserTokenSource` (Slack Thread Adapter) | implements the directories (`resolve_hosted`, `resolve_slack`) over the Organisation Profile's `remote_agents`, building each entry's `OidcSettings` from `remote_agent_identity(id)`; calls `mark_remote_agent_enabled(id)` on the adapter's first sign-in and `mark_remote_agent_verified(id)` on its first successful read for that agent's route (OrganisationProfile.md step 7); implements `CompanyTokenSource` (`company_access_token`, `invalidate`) over Identity's `access_token_for_audience` and `invalidate_audience_token`, and `SlackUserTokenSource` (`slack_user_token`, `report_refused`) over `access_token(CredentialKind::Slack, None)` and `report_refused` — Identity offers no Slack sign-in flow on the server (`CRED_FLOW_NOT_OFFERED`), so `slack_user_token` answers `SignedOut` while no Slack token is stored | P | row 5.9 |
| `AgentFolderManager::recover`, `Checker::reevaluate_trust` | calls `recover` once at start; `reevaluate_trust` on each Organisation Profile change | P S C (`runs`) | step 5 tail, step 6.3 |
| `MemoryService::source_changed` | forwards File Service watcher events | P S C | after the Memory Service starts |
| `MemoryService::set_agent_rules`, `PackageInspector::memory_contract` | builds the hosted agent's `AgentMemoryRules` itself, as the Ordin Host does (OrdinHost.md SA-CLOSE-04): `package_inspector.memory_contract(<hosted agent instance id>)` → `Some(MemoryContract)` mapped to `AgentMemoryRules { retains, excludes, max_age_days: None when unbounded, store_location }` and passed to `memory.set_agent_rules(agent_instance_id, rules)`; `None` → no call (the Memory Service's defaults apply). The Agent Loader carries no memory rules | S C (`runs`) | after `ordin_agent_loader::load` (step 6.1) |
| `MemoryService::ready` | part of `ready()` in a role that starts the Memory Service | S C | step 6.6 |
| `LoaderStore` (Agent Loader) | implements it over the Journal: `schema_status` from `MigrationRegistry::apply(store, ApplyMode::CheckOnly)`; `record_loaded` with `store_startup_report(report, true)` and an `append_entry` naming the release; `hosted_workspace` reads, or on the first call creates, the one hosted-workspace identifier kept in the Journal's `Datastore` (AgentLoader.md SA-RECON-02) | S C | `LoadInput.store` (step 6.1) |
| `WorkFolderOpener` (Agent Loader) | implements its one method `open(config)` as `WorkFolder::new(<the datastore>, config)` (File Service) | S C (`runs`) | `LoadInput.work_folder` (step 6.1) |
| `Telemetry::set_release` (Telemetry) | calls it once with the loaded release's `record.release_id` and `digest` | S C (`runs`) | after `ordin_agent_loader::load` (step 6.1) |
| `RunQueue::mark_interrupted` (Run Queue) | calls it every 10 s in each run instance (records whose lease lapsed become `interrupted`; the Session Supervisor never calls it) | S C (`runs`) | from step 6.3 until the stop |
| `ChannelDocuments` (Connection Binder) | implements `read(connection, channel, max_bytes)` over the `HostedSlackLink` of `connection` (`HostedChatLink::channel_documents`), mapped as ConnectionBinder.md SA-RECON-02 states (`Piece` → `DocumentPiece`, `LeftOut` → `LeftOut`, a `LinkError` → one `LeftOut { reason: "read-failed" }`); passed in `Hosts.documents` | S C (`runs`) | `LoadInput.hosts` (step 6.3) |
| `RunHost`, `ScopeSettings` (Connection Binder) | `RunHost`: `run_view` over the Run Queue's `get(run_id)`, `record_agent_event` and `ask_reply_actions` over the Session Supervisor's run, recorded on the run's Run Queue record with `append_progress`; `ScopeSettings::settings_with_value` over `SettingsStore::all` (names whose `is_default` is false); passed in `Hosts` | S C (`runs`) | `BindInput.hosts` / `LoadInput.hosts` (step 6.3) |
| `SecretSource` (Connection Binder) | `get(name)` — single shape: over `IdentityService.secrets()` with the `env:` mapping of step 9.6; scaled shape: over `ORDIN_SECRETS_DIR` | S C | `LoadInput.secrets` (step 6.3) |
| `EnvironmentSecrets` (Model Policy) | `get(name)` over the same source as `SecretSource` and with the same mapping: single shape `IdentityService.secrets()` with the `env:` mapping of step 9.6; scaled shape a file of that name under `ORDIN_SECRETS_DIR` (*amended January 2026, ADR-107*; SA-HOST-03) | S C | `KeySource.environment_secrets` (step 6.1, L-14) |
| `ContentChecker` (Rules Engine) | `check(connection, operation, request, timeout)` over `BoundConnections::checker(connection, operation)` → `CheckerOperation::call` with the request as the JSON body, under `timeout`; no checker bound → `CheckerUnreachable { reason: "refused" }` (*amended January 2026, ADR-107*) | S C (`runs`) | `RulesHost::content_checker` (step 6.3) |
| `KnowledgeRunContext`, `KnowledgeScopeSettings`, `KnowledgeMetrics`, `ChatHistory` (Knowledge Service) | in `AgentKnowledgeDeps`: `runs` (`question`, the run's incoming message, and `item_opened`, recorded once per item) over the Run Queue's run record; `settings` (`number`, `flag`, `selection`) over the Settings Store's `get`; `metrics` (`query`, `note_written`, `note_refused`, `items_written`, `ingest_thread_failed`) over Telemetry, each to the counter family its comment names; `chat_history` with `chat_of(scope)` over Retention and Erasure's `ScopeRegistry` and `page(...)` over `HostedSlackLink::history` (ruling R14), `replies(...)` over `HostedSlackLink::replies`, and `ChatHistoryPage.threads` from the `ts` of each `HistoryMessage` with `reply_count > 0` (*amended January 2026, ADR-107*) | S C | Knowledge Service construction (steps 6.2, 6.3) |
| `SettingRuleCheck`, `SelectionLookup` (Settings Store) | `SettingRuleCheck` over the Rules Engine's message-rule check (`check_setting_rule`); `SelectionLookup` over the Knowledge Service's `senders` | S C | step 6.1 |
| `MetricsSink` (Run Queue, Settings Store, Channel Gateway, Retention and Erasure, and every other crate with its own sink) | the *sink adapter* implements `add(name, labels, by)` as `telemetry.record(event_from_named(name, labels, by)?)`, an `Err` logged once per name | S C | each sink-taking constructor |
| `HostedAgentCore` (Channel Gateway) | implements all fourteen methods (*amended January 2026, ADR-107*: `setting_entries`, `context_lookup` and `context_recent` added) over the Run Queue, Settings Store, Job Runner and Knowledge Service: `setting_entries` over the Settings Store's `get` of a `list` setting, each entry's fields as text; `enqueue` over the Run Queue's `enqueue`; `run_views` over its `get` for each id; `source_entries` over the Knowledge Service (the source entry of each opened item); `setting_text` over the Settings Store's `get`; `available_actions` over the loaded agent's configuration for `connection` (Agent Configuration) and the scope's settings — a `link` whose `url_setting` begins `https://` or whose `list_setting` holds an entry, a `hand-off` with a route by the Channel Gateway's rule K.8.1 (*amended January 2026, ADR-107*), and `share`; a `flag` setting's text for `setting_text` is `true` or `false`; `settings_form` over the Settings Store's `declarations` and `all`; `submit_settings` over its `check` then `set_many`; `entry_command` over the Settings Store (a `Text` reply or an `OpenForm`); `job_command` and `scope_enabled_first_time` over the Job Runner's `JobStore::request` (and `latest` for a status command); `version_lines` from the loaded release's `release_id` and `digest`; `context_lookup` over the Knowledge Service's `lookup(source, Some(scope), question, limit)` and `context_recent` over its `recent(source, Some(scope), limit)`, each result mapped to `ContextHit { item_id, origin_time, text }` (*amended January 2026, ADR-107*) | S C (`chat`) | `HostedDeps` (step 6.4) |
| `CHAN_NOT_SETUP_COMMAND` (Channel Gateway) | exposes `begin_pairing` and `end_pairing` only through the setup command | P | step 9.5 |
| `SlackTokens` (Slack Channel Adapter) | resolves `chat[].bot-token-env` and `app-token-env` into it | S C (`chat`) | step 6.4 |
| `JobOutcomeNotifier` (Job Runner) | `job_ended` over `ChannelGateway::notify_job_ended` (`None` in a `jobs` instance with no Channel Gateway) | S C (`jobs`) | step 6.5 |
| `JobDesk` (Caller API) | `request` and `list` over the Job Runner's `JobStore::request` and `list` | S C (`requests`) | step 6.2 |
| `RetainedContent` registrations (Retention and Erasure) | `retention.register(…)` for each provider, including `ChannelGateway::retained_content()` and the Rules Engine `ControlState` provider | S C | after Retention and Erasure is constructed (step 6.2) |
| `PackageReader` (over Format Plug-ins' `read_any` and `TreeSource`), `HarnessLayoutWriter` (over Installers) (Release Builder) | implements both (`PackageReader::read`, `HarnessLayoutWriter::write`) for `agent prepare`, `check` and `publish`; the reader as SA-CLOSE-02 states | L commands | step 10 |
| `LoadInput.agent_instance_id` (Agent Loader) | the value of `server_settings.agent_instance_id`, minted once (SA-CLOSE-04) | S C | step 6.1 |
| Help-request copies, setup reports, `profile_activated` (Journal `append_entry`); `locale_changed` (Localisation) | journals each | P | when produced |
| `Localisation::language_changes` (Localisation; amended January 2026, ADR-109) | appends each `LanguageChange` as `language_changed` with `Journal::append_event`, as `locale_changed`; a lagged receiver is logged `warn` `language_changes_lagged` with the count | P (the agent profile has no person layer, so its receiver yields nothing) | from after row 5.5 until shutdown |

Traits the desktop program implements that this program does not wire, each with the reason: `HarnessProbe` (the Setup Assistant skips `copilot` on the server, C-48, and its Claude Code probe `ClaudeCodeProbe` is built in, so no probe is registered); `WordToMarkdown` (no Confluence Adapter until C-49, row 5.27); `AppUpdater` (the server never replaces itself, ADR-076; row 5.34); `UrlOpener` and `BrowserOpener` (no system browser: `SetupConfig.url_opener` and `IdentityConfig.browser` are `None` on the server); `TelegramAppProbe` (`TelegramServiceDeps.app_probe` is `None` on the personal server); `DesktopKeyStore` (no Server Connector: the server does not pair with another server).

Called by (this program's own public items):

| Item | Caller |
|---|---|
| `service.hello` (through `ServerServiceApi`) | Server Connector on a paired desktop, through the Remote API |
| `ordin-server setup pair-desktop [--list \| --end COMPUTER_NAME]` | the server owner; the Remote API's pairing line and messages name it |
| `ordin-server setup pair-chat --channel telegram\|slack` | the server owner; Telegram Adapter and Slack Channel Adapter texts name it |
| `ordin-server agent release obtain`, `agent db apply`, `agent job run-one --kind KIND --key KEY`, `agent hosts --config FILE` | operators and the Helm chart; Agent Loader, Job Runner and Agent Configuration texts name them (reconciliation rows 472–475) |
| `ordin-server setup secret NAME` | the server owner; the adapters' and Model Policy's texts name it |

### Rust items of the program

```rust
// apps/server/src/main.rs
fn main() -> std::process::ExitCode;

// apps/server/src/cli.rs — clap derive
#[derive(clap::Parser)] #[command(name = "ordin-server", version)]
pub struct Cli { #[command(subcommand)] pub command: Command }
#[derive(clap::Subcommand)]
pub enum Command { Start, Health, Setup(SetupArgs), Agent(AgentArgs) }

// apps/server/src/env.rs
pub struct ServerEnv {
    pub data_dir: std::path::PathBuf,
    pub log_level: tracing::Level,
    /// Both kept as read; `server_secret_backend` refuses both set (CRED_SECRET_KEY_CONFLICT, step 4).
    pub secrets_key_file: Option<std::path::PathBuf>,
    pub secrets_key_credential: Option<std::path::PathBuf>,
    pub release: Option<String>,
    pub registry_auth_file: Option<std::path::PathBuf>,
    pub release_dir: Option<std::path::PathBuf>,
    pub database_url_file: Option<std::path::PathBuf>,
    pub port: u16,
    pub run_concurrency: Option<u16>,
    pub image: Option<String>,
    pub otlp_endpoint: Option<url::Url>,
    pub model_key_file: Option<std::path::PathBuf>,
    pub secrets_dir: Option<std::path::PathBuf>,
    pub server_url: Option<url::Url>,
    pub token_file: Option<std::path::PathBuf>,
    pub notify_socket: Option<std::ffi::OsString>,
    pub watchdog: Option<std::time::Duration>,
}
impl ServerEnv { pub fn read() -> Result<ServerEnv, ServerError>; }

// apps/server/src/core.rs
pub struct ServerCore { /* Arc of every constructed module; private */ }
#[derive(Debug, Clone)]
pub struct Roles { pub requests: bool, pub runs: bool, pub chat: bool, pub jobs: bool }
impl ServerCore {
    pub async fn start_personal(env: &ServerEnv) -> Result<Arc<ServerCore>, ServerError>;
    pub async fn start_agent(env: &ServerEnv, roles: Roles) -> Result<Arc<ServerCore>, ServerError>;
    pub async fn run_until_stopped(self: Arc<Self>) -> Result<(), ServerError>;   // returns after orderly stop
}

// apps/server/src/service_api.rs
pub struct ServerServiceApi { /* private */ }
#[async_trait::async_trait]
impl ordin_service_api::ServiceApi for ServerServiceApi { /* one method per catalogue command */ }

// apps/server/src/always_on.rs
pub struct AlwaysOn;
impl AlwaysOn { pub fn spawn(core: Arc<ServerCore>, env: &ServerEnv, stop: tokio_util::sync::CancellationToken) -> Vec<tokio::task::JoinHandle<()>>; }

// apps/server/src/sd_notify.rs — no library: one datagram to NOTIFY_SOCKET
pub fn notify(socket: &std::ffi::OsStr, state: &str) -> std::io::Result<()>;   // "READY=1", "WATCHDOG=1", "STOPPING=1"

#[derive(Debug, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ServerError { pub code: &'static str, pub message: String, pub exit: u8 }
```

### Boundary calls this program needs from other crates

Each call below is the owning spec's own name and signature (ruling R6); the program conforms. Module constructors not listed take the dependencies their spec's `new`, `start` or `…Deps` struct names, built from the 2B-3 edges of *Internal Logic* steps 5 and 6.

```rust
// ordin-journal (JournalAndArtefactStore.md)
pub async fn Datastore::open(config: DatastoreConfig) -> Result<Datastore, JournalError>;
//   DatastoreConfig::Sqlite { path, backup_dir } | DatastoreConfig::Postgres { url: DatabaseUrl, max_connections }
pub fn MigrationRegistry::new() -> MigrationRegistry;
pub fn MigrationRegistry::register(&mut self, migrations: ModuleMigrations) -> Result<(), JournalError>;
pub async fn MigrationRegistry::apply(&self, store: &Datastore, mode: ApplyMode) -> Result<MigrationReport, JournalError>;
//   ApplyMode::ApplyPending (personal, single shape, `agent db apply`) | ApplyMode::CheckOnly (scaled-shape instances)
pub async fn Journal::start(store: Datastore, options: JournalOptions) -> Result<Journal, JournalError>;
pub fn Journal::backend_uuid(&self) -> uuid::Uuid;              // wrapped as BackendId
pub fn Journal::last_seq(&self) -> u64;                          // `service.hello`
pub async fn Journal::ping(&self, within: Duration) -> bool;
pub async fn Journal::compact(&self) -> Result<(), JournalError>;
pub async fn Journal::recover_external_actions(&self) -> Result<u32, JournalError>;
pub async fn Journal::set_company_max_days(&self, max_days: Option<u32>) -> Result<RetentionSetting, JournalError>;
pub async fn Journal::set_retention_days(&self, days: u32) -> Result<RetentionSetting, JournalError>; // `setup history`
pub fn Journal::register_prune_participant(&self, participant: Arc<dyn PruneParticipant>);
pub fn Journal::subscribe(&self, sink: Arc<dyn EventSink>) -> SubscriptionId;
pub async fn Journal::shutdown(&self);
pub async fn Datastore::close(&self);
// The server profile record is this program's own table (module `server`, JournalAndArtefactStore.md *Called by*),
// read and written by this program over `Datastore::sqlite()` / `Datastore::postgres()`.

// ordin-identity (IdentityAndCredentials.md)
pub fn server_secret_backend(dbus_session_bus_address: Option<&str>, secrets_key_file: Option<&Path>,
    secrets_key_credential: Option<&Path>) -> Result<SecretBackendConfig, CredError>;
pub async fn IdentityService::new(config: IdentityConfig, pool: Option<sqlx::SqlitePool>) -> Result<IdentityService, CredError>;
pub async fn IdentityService::create_secrets_key_credential(credential_path: &Path) -> Result<(), CredError>;
pub async fn IdentityService::put_server_secret(&self, name: &str, value: &str) -> Result<(), CredError>;
impl ServerSecrets for IdentityService;                          // passed to the Remote API

// ordin-remote-api (RemoteApi.md)
pub fn RemoteApi::migrations() -> ModuleMigrations;
pub fn RemoteApi::new(cfg: RemoteApiConfig, api: Arc<dyn ServiceApi>, sentences: Arc<dyn ErrorSentences>,
    approvals: Arc<ApprovalService>, journal: Journal, secrets: Arc<dyn ServerSecrets>,
    profile: Arc<OrganisationProfileService>) -> RemoteApi;
pub async fn RemoteApi::serve(&self, cancel: CancellationToken) -> Result<Serving, RemoteApiError>;   // Serving::{Off, Stopped}
pub async fn RemoteApi::configure_listening(&self, s: Option<ListenSettings>) -> Result<(), RemoteApiError>;
pub async fn RemoteApi::listening(&self) -> Result<Option<ListenSummary>, RemoteApiError>;
pub async fn RemoteApi::start_pairing(&self) -> Result<PairingOffer, RemoteApiError>;
pub async fn RemoteApi::list_pairings(&self) -> Result<Vec<PairingSummary>, RemoteApiError>;
pub async fn RemoteApi::end_pairing(&self, id: DesktopPairingId) -> Result<(), RemoteApiError>;
// RemoteApi implements the Journal's PruneParticipant (module `remote_api`); registered in the personal profile.

// ordin-telegram-adapter (TelegramAdapter.md), ordin-slack-channel-adapter (SlackChannelAdapter.md)
pub async fn TelegramAdapter::begin_pairing(&self, paired_by: &str) -> Result<PairingCode, TelegramAdapterError>;
pub async fn PersonalSlack::begin_pairing(&self, paired_by: &str) -> Result<SlackPairingCode, SlackAdapterError>;
pub fn HostedSlackLink::new(connection: &ChatConnection, tokens: SlackTokens, gateway: Arc<ChannelGateway>,
    metrics: Arc<dyn ordin_channel_gateway::MetricsSink>) -> Result<Arc<HostedSlackLink>, SlackAdapterError>;
pub async fn HostedSlackLink::history(&self, channel: &str, oldest: Option<&str>, cursor: Option<&str>) -> Result<HistoryPage, LinkError>;
pub async fn HostedSlackLink::replies(&self, channel: &str, thread_ts: &str, cursor: Option<&str>) -> Result<HistoryPage, LinkError>; // ADR-107
pub fn HostedSlackLink::is_open(&self) -> bool;                  // chat role readiness

// ordin-channel-gateway (ChannelGateway.md)
pub async fn ChannelGateway::new(deps: GatewayDeps) -> Result<Arc<ChannelGateway>, GatewayError>;
//   GatewayMode::Personal(PersonalDeps) | Hosted(HostedDeps) | Setup(SetupDeps) (setup command, no server running)
pub async fn ChannelGateway::start(self: &Arc<Self>, shutdown: CancellationToken) -> Result<(), GatewayError>; // cancel = orderly stop
pub async fn ChannelGateway::end_pairing(&self, channel: ChannelKind) -> Result<(), GatewayError>;
pub async fn ChannelGateway::set_needs_you_both(&self, both: bool) -> Result<(), GatewayError>;
pub async fn ChannelGateway::send_restart_notice(&self, summary: RestartSummary) -> Result<(), GatewayError>;
pub async fn ChannelGateway::send_sign_in_ended(&self) -> Result<(), GatewayError>;
pub async fn ChannelGateway::send_update_available(&self, version: &str) -> Result<(), GatewayError>;
pub async fn ChannelGateway::notify_job_ended(&self, connection: &str, user: &str, text: &str) -> Result<(), GatewayError>;
pub fn ChannelGateway::retained_content(self: &Arc<Self>) -> Vec<Arc<dyn RetainedContent>>;
// Channel outages are reported by the gateway itself (ChannelGateway step J.6); this program does not watch them.

// ordin-claude-adapter (ClaudeLocalAdapter.md)
pub async fn ClaudeLocalAdapter::sign_in_status(&self) -> Result<SignInStatus, ClaudeAdapterError>; // SignedIn | SignedOut

// ordin-session-supervisor (SessionSupervisor.md)
pub fn SessionSupervisor::register_adapter(&self, adapter: Arc<dyn RuntimeAdapter>) -> Result<(), SupervisorError>;
pub async fn SessionSupervisor::recover(&self) -> Result<RecoveryReport, SupervisorError>;
//   RecoveryReport { resumed, interrupted, ended_remotely, awaiting_decision }
pub fn SessionSupervisor::start_queue_workers(self: &Arc<Self>);  // agent profile, runs role
pub async fn SessionSupervisor::stop_idle_sessions(&self, idle_for: Duration) -> u32;
pub fn SessionSupervisor::runs_going(&self) -> u32;
pub async fn SessionSupervisor::stop_taking_runs(&self);
pub async fn SessionSupervisor::wait_for_runs(&self, stop: CancellationToken);
pub async fn SessionSupervisor::shutdown(&self, reason: ShutdownReason) -> ShutdownReport; // ShutdownReason::Signal

// ordin-setup (SetupAssistant.md)
pub async fn SetupAssistant::check_update_feed(&self) -> Result<Option<String /* version */>, SetupError>;

// ordin-mcp-front (McpFront.md)
pub fn McpFront::new(config: McpFrontConfig, rules: Arc<RulesEngine>, journal: Journal,
    blocks: Arc<dyn BlockCatalogueCheck>, metrics: Option<Arc<dyn McpMetrics>>) -> McpFront;
/// Once; a second call → MCP_REF_RESOLVER_ALREADY_BOUND (permanent) (K4).
pub fn McpFront::bind_ref_resolver(&self, resolver: Arc<dyn RefResolver>) -> Result<(), McpError>;
pub fn McpFront::register(&self, provider: Arc<dyn ToolProvider>) -> Result<(), McpError>; // before `start` only
pub async fn McpFront::start(&self, shutdown: CancellationToken) -> Result<(), McpError>;   // freezes the registry

// ordin-resolver, ordin-checker (ResolverAndCompiler.md, Checker.md; ruling R18)
pub fn ResolverAndCompiler::install_part_source(&self, source: Arc<dyn PartSource>) -> Result<(), ResolveError>;
pub fn Checker::install_part_source(&self, source: Arc<dyn PartSource>) -> Result<(), CheckError>;

// ordin-hook-relay (HookRelay.md)
pub async fn HookRelay::start(self: &Arc<Self>) -> Result<(), HookRelayError>;   // binds the hook endpoint
pub async fn HookRelay::shutdown(&self);

// ordin-nats (NatsAdapter.md) — personal profile only
pub fn NatsSettings::from_profile(snapshot: &ordin_organisation_profile::ProfileSnapshot) -> Result<NatsSettings, NatsError>;
pub fn NatsAdapter::new(settings: NatsSettings, secrets: Arc<ordin_identity::SecretStore>, data_dir: PathBuf) -> NatsAdapter;
pub async fn NatsAdapter::start(&self, shutdown: CancellationToken) -> Result<(), NatsError>;
pub async fn NatsAdapter::reload(&self, settings: NatsSettings) -> Result<(), NatsError>;

// ordin-organisation-profile (OrganisationProfile.md)
pub async fn OrganisationProfileService::set_profile_address(&self, url: &str, signing_keys: Vec<String>) -> Result<(), ProfileError>;
pub fn OrganisationProfileService::remote_agent_identity(&self, id: RemoteAgentId) -> Option<RemoteIdentity>;
pub async fn OrganisationProfileService::mark_remote_agent_enabled(&self, id: RemoteAgentId) -> Result<RemoteAgent, ProfileError>;
pub async fn OrganisationProfileService::mark_remote_agent_verified(&self, id: RemoteAgentId) -> Result<RemoteAgent, ProfileError>;

// ordin-setup (SetupAssistant.md) — the personal profile's check before each harness session
pub async fn SetupAssistant::run_checks(&self, trigger: CheckTrigger) -> Result<SetupReport, SetupError>; // CheckTrigger::BeforeSession

// ordin-claude-adapter, ordin-copilot-adapter — given to the Usage Planner
pub fn ClaudeLocalAdapter::harness_models(&self) -> Arc<dyn HarnessModels>;
pub fn CopilotLocalAdapter::harness_models(&self) -> Arc<dyn HarnessModels>;

// ordin-triggers (TriggerService.md) — personal profile only
pub async fn TriggerService::start(deps: TriggerDeps, shutdown: CancellationToken) -> Result<Arc<TriggerService>, TriggerError>;
//   cancelling `shutdown` pauses it for the orderly stop (no separate pause)
pub fn TriggerService::proposal_schema() -> serde_json::Value;
pub async fn TriggerService::propose(&self, proposal: TriggerProposal) -> Result<Trigger, TriggerError>;

// ordin-host (OrdinHost.md)
pub fn OrdinHost::set_trigger_desk(&self, desk: Arc<dyn TriggerDesk>);   // personal profile, after TriggerService::start
pub fn OrdinHost::set_policy(&self, policy: OrdinHostPolicy);            // each organisation-profile refresh

// ordin-agent-loader (AgentLoader.md), ordin-agent-configuration (AgentConfiguration.md)
pub async fn ordin_agent_loader::load(input: LoadInput<'_>) -> Result<LoadedAgent, LoadError>;
pub fn LoadedAgent::tool_provider(&self) -> Arc<dyn ToolProvider>;            // the `files` server; registered by this program
pub fn BoundConnections::tool_provider(&self) -> Arc<dyn ToolProvider>;        // the `agent` server; registered by this program
pub async fn ordin_agent_loader::release::obtain(reference: &str, dest: &Path, auth: &RegistryAuth, ordin_version: &str) -> Result<LoadedRelease, ReleaseError>;
pub fn ordin_agent_configuration::connection_hosts(config: &AgentConfig) -> Vec<ConnectionHost>; // `agent hosts`

// ordin-connection-binder (ConnectionBinder.md)
pub async fn ordin_connection_binder::bind(input: BindInput<'_>) -> Result<BoundConnections, BindError>;

// ordin-caller-api (CallerApi.md)
pub fn ordin_caller_api::build_router(settings: CallerApiSettings, deps: CallerApiDeps) -> Result<axum::Router, CallerApiError>;
pub fn ordin_caller_api::health_router(ready: ReadyCheck, telemetry: Arc<Telemetry>) -> axum::Router;
pub async fn ordin_caller_api::serve(router: axum::Router, port: u16, stop: CancellationToken) -> Result<(), CallerApiError>;
pub fn OperatorClient::from_env() -> Result<OperatorClient, OperatorClientError>;   // group S commands

// ordin-telemetry (Telemetry.md)
pub fn Telemetry::record(&self, event: TelemetryEvent);
pub fn Telemetry::set_release(&self, release_id: &str, digest: Option<&str>) -> Result<(), TelemetryError>; // after `load`
pub fn ordin_telemetry::event_from_named(name: &str, labels: &[(&str, &str)], by: u64) -> Result<TelemetryEvent, TelemetryError>;

// ordin-memory-service (MemoryService.md)
pub async fn MemoryService::ready(&self) -> Result<(), MemoryError>;
pub fn MemoryService::set_agent_rules(&self, agent_instance_id: AgentInstanceId, rules: AgentMemoryRules) -> Result<(), MemoryError>;
pub async fn PackageInspector::memory_contract(&self, id: AgentInstanceId) -> Result<Option<MemoryContract>, InspectError>;

// late binding and host calls (UsagePlanner.md, SlackThreadAdapter.md, CrewOrchestrator.md, RunQueue.md)
pub fn UsagePlanner::set_harnesses(&self, harnesses: Vec<Arc<dyn HarnessModels>>);
pub fn SlackThreadAdapter::abandon(&self, run: RunId);                       // SupervisorHost::run_abandoned
pub async fn CrewOrchestrator::get(&self, crew_run_id: CrewRunId) -> Result<(CrewRun, Vec<Round>), CrewError>;
async fn RunQueue::get(&self, run_id: &str) -> Result<Option<RunView>, RunQueueError>;   // run_starter
async fn RunQueue::mark_interrupted(&self) -> Result<Vec<String>, RunQueueError>;        // every 10 s, runs role

// ordin-retention (RetentionAndErasure.md)
pub fn Retention::register(&self, provider: Arc<dyn RetainedContent>) -> Result<(), RetentionError>;

// ordin-authoring (AuthoringService.md)
pub fn AuthoringService::prune_participant(&self) -> Arc<dyn PruneParticipant>;
pub fn AuthoringService::tool_provider(&self) -> Arc<dyn ToolProvider>;
pub async fn AuthoringService::run(&self, shutdown: CancellationToken) -> Result<(), AuthoringError>;
pub async fn AuthoringService::publish(&self, draft: DraftRef, context: Option<PublishContext>) -> Result<(), AuthoringError>;
pub fn AuthoringService::lifecycle_of(&self, draft: &DraftRef) -> Result<Option<LifecycleId>, AuthoringError>;   // K5

// ordin-package-inspector, ordin-package-catalogue (PackageInspector.md, PackageCatalogue.md)
pub async fn PackageInspector::reload_rule_sets(&self) -> Result<Vec<RuleSetReloadFailure>, InspectError>;
pub async fn PackageCatalogue::source_scope(&self, instance: AgentInstanceId) -> Result<SourceScope, PackageError>;   // K13

// ordin-format-plugins (FormatPlugins.md) — the Release Builder's PackageReader (SA-CLOSE-02)
pub fn FormatPlugins::read_any(&self, tree: &SourceTree, request: &ReadRequest) -> Result<(Detection, ReadOutcome), FormatError>;
pub fn TreeSource::new(outcome: &ReadOutcome, repository_id: AgentRepositoryId, repository_label: &str, commit: &str)
    -> Result<TreeSource, FormatError>;
pub fn TreeSource::root(&self) -> Result<PartRef, FormatError>;
```

### Example

```text
$ ordin-server setup pair-desktop
Pairing code: 482913 (valid until 10:52)
Or paste this line in ordin on the desktop:
<the pairing line the Remote API returns>

$ ordin-server health; echo $?
0

$ ordin-server setup listen --address 203.0.113.7 --port 8443 --certificate srv.pem --key srv.key
error[SERVER_LISTEN_ADDRESS_REFUSED]: Only a loopback or private address can be used.
$ echo $?
1

$ ordin-server agent start --serve chat --serve chat-jobs
error[SERVER_ROLE_COMBINATION]: each role is given once; chat-jobs already serves chat and jobs.
```

Restart message sent to the person's last-used chat after a crash at 03:12 (ADR-076): "ordin restarted at 03:12; 2 runs resumed, 1 approval still waiting."

## Internal Logic

1. **Parse and read the environment.** Parse the command line (usage error → exit 2). `ServerEnv::read()` reads every variable of the table once; a malformed value → `SERVER_ENV_INVALID` naming the variable; both key variables are kept as read and judged in step 4. `--version` prints and exits 0 before anything else.
2. **Data root and lock.** Create `ORDIN_DATA_DIR`, `run/` (mode `0700`) and `logs/`; refuse a root not owned by the running user or not writable (`SERVER_DATA_ROOT_UNWRITABLE`). `start` and `agent start` take an exclusive lock on `<data dir>/ordin.lock` with `std::fs::File::try_lock`; held → `SERVER_ALREADY_RUNNING`. `setup` tries the same lock: held → a server runs, so the step goes to the control socket (step 9); free → the setup command holds it while it acts in-process.
3. **Logging.** `tracing` JSON lines (SA-OBS-01) at `ORDIN_LOG_LEVEL`: personal profile and single shape to daily files `<data dir>/logs/ordin-server.log.<date>`, 14 kept (A-09); agent profile to standard output with wrightrun's field names and run summary (SA-OBS-01). Errors before logging is installed go to standard error only.
4. **Secret store and datastore.** `ordin_identity::server_secret_backend(DBUS_SESSION_BUS_ADDRESS, ORDIN_SECRETS_KEY_FILE, ORDIN_SECRETS_KEY_CREDENTIAL)` chooses the back end: `EncryptedFile` when either key variable is set *(assumed here: the owner's explicit choice wins; the shipped unit sets one because a lingering user unit usually has no unlocked Secret Service)*, otherwise `SecretService` when a session bus is set; its `CredError` stops start and is printed with its own code, passed through unchanged (`CRED_SECRET_KEY_CONFLICT` when both key variables are set, `CRED_SECRET_KEY_MISSING` when neither is set and no session bus exists; ErrorHandling EH-D15). The scaled shape uses `SecretBackendConfig::None`: secrets are files (`ORDIN_MODEL_KEY_FILE`, `ORDIN_SECRETS_DIR`). Open the datastore with `Datastore::open`: `DatastoreConfig::Postgres { url: DatabaseUrl::new(<file content, one trailing newline trimmed>), max_connections }` when `ORDIN_DATABASE_URL_FILE` is set (the string is dropped after connecting), else `DatastoreConfig::Sqlite { path: <data dir>/ordin.db, backup_dir: <data dir>/backups }`. Build one `MigrationRegistry`: the Journal's `MIGRATIONS` first, then this program's module `server` (*Data Model*), the `MODULE_NAME`/`SQLITE_MIGRATIONS` pairs of the Organisation Profile and Identity and Credentials (R20), and every constructed table-owning module's `migrations()` value (every table owner other than those two exports `pub fn migrations() -> ModuleMigrations`, R3). `registry.apply(&datastore, ApplyMode::ApplyPending)` in the personal profile, single shape and `agent db apply` (SQLite takes its backup first, SA-XC-07); `ApplyMode::CheckOnly` in scaled-shape instances. Then `Journal::start(datastore.clone(), options)`. The Organisation Profile and `IdentityService::new(config, datastore.sqlite().cloned())` receive the datastore directly (phase 1, R20); every other table owner reaches it through the Journal (D4). Read the `server_profiles` row: absent → `SERVER_NOT_SET_UP` ("Run ordin-server setup first."), except in the scaled shape, where `agent db apply` writes `{ profile: agent, shape: scaled }` when the table is empty *(assumed here: the scaled shape has no interactive setup)*. `start` on an agent profile, or `agent start` on a personal profile → `SERVER_PROFILE_MISMATCH`.
5. **Construct the personal core** (`start_personal`). Same rules as the desktop's step 3: each constructor error stops start with `SERVER_START_FAILED` naming the module; providers and adapters are registered right after construction.

    | # | Crate | Constructed with | Wiring done right after |
    |---|---|---|---|
    | 5.1 | `ordin-journal` | (step 4) | `journal.recover_external_actions()` and `journal.set_company_max_days(…)` after 5.2 |
    | 5.2 | `ordin-organisation-profile` | data dir, the `Datastore` (R20) | refresh at start and every 60 minutes; absent address → defaults, A-40; on each change: `set_company_max_days`, `checker.reevaluate_trust()`, `ordin_host.set_policy(…)` |
    | 5.3 | `ordin-identity` | (step 4) | — |
    | 5.4 | `ordin-approval` | Journal | `journal.subscribe(approval.event_sink())` |
    | 5.5 | `ordin-localisation` | Organisation Profile, Journal (D4); `LocalisationConfig.app_version` = the program's package version, `env!("CARGO_PKG_VERSION")` (amended January 2026, ADR-109) | personal profile: the `language_changes()` forwarding task (ADR-109) |
    | 5.6 | `ordin-agent-folders` | Journal, data dir | — |
    | 5.7 | `ordin-format-plugins`, `ordin-resolver` | Agent Model types | part source installed at 5.29 (R18: a call before then returns a permanent error) |
    | 5.8 | `ordin-sharepoint` | `SharePointAdapter::new(SharePointDeps { identity, journal, http })` — returns `Result`; `Err(e)` → `SERVER_START_FAILED` naming `ordin-sharepoint` and `e.code()` | — |
    | 5.9 | `ordin-hosted-agent-adapter`, `ordin-slack-thread-adapter` | Identity | registered at 5.26 |
    | 5.10 | `ordin-nats` | `NatsAdapter::new(NatsSettings::from_profile(&profile.current())?, identity.secrets(), <data dir>/nats)` (a `NatsError` → `SERVER_START_FAILED` naming `ordin-nats`) | `start(stop)` in the step 5 tail; `reload(NatsSettings::from_profile(&snapshot)?)` on each Organisation Profile change and after `setup secret nats-credentials`, `nats-user-password` or `nats-token` stores the secret on a running server (step 9.6) |
    | 5.11 | `ordin-usage-planner` | Datastore, Journal, Organisation Profile, an empty harness list | `usage.set_harnesses(vec![claude.harness_models(), copilot.harness_models()])` right after row 5.24 |
    | 5.12 | `ordin-rules` | Approval Service, Journal, Organisation Profile, the program's `RulesHost`, `RulesConfig { program: PersonalServer, ordin_servers: <the MCP server names this profile's providers claim, SA-NAME-01..04>, ordin_server_prefixes: <the claimed prefixes: craft_>, … }` (*chosen here*: the Rules Engine is built before the MCP Front, so the program passes the claimed lists that `McpFront::server_names()` reports after start; the agent profile passes `AgentSingle` or `AgentScaled` and its own providers' names) | — |
    | 5.13 | `ordin-checker`, `ordin-installers` | as 2B-3 (Installers: + Agent Model) | part source of the Checker installed at 5.29 |
    | 5.14 | `ordin-session-supervisor` | Journal, Usage Planner, no Run Queue, the program's `SupervisorHost`, `SupervisorConfig { program: PersonalServer, … }` | `journal.subscribe(session_supervisor.event_sink())` |
    | 5.15 | `ordin-mcp-front` | Rules Engine, Journal, `StandardBlockCheck`, Service API; no resolver at construction | providers registered with `register` until `start`; `bind_ref_resolver` in the step 5 tail (SA-CLOSE-01) |
    | 5.16 | `ordin-package-inspector`, `ordin-files` | as 2B-3 | — |
    | 5.17 | `ordin-hook-relay` | Rules Engine, Approval Service, Session Supervisor, Journal, hook endpoint `<data dir>/run/hook.sock` | `journal.subscribe(hook_relay.event_sink())` |
    | 5.18 | `ordin-crew-orchestrator` | as 2B-3, the program's `CrewHost` | — |
    | 5.19 | `ordin-sandbox` | `Sandbox::start(SandboxDeps { dispatch: MCP Front, grants: <program's SandboxGrants>, identity, journal, place_files: <program's PlaceFiles>, session_idle, data_root, log_level })` | `mcp_front.register(sandbox.tool_provider())` |
    | 5.20 | `ordin-memory-service` | MCP Front, Journal | `mcp_front.register(memory.tool_provider())` |
    | 5.21 | `ordin-craft-toolkit` | MCP Front, Rules Engine, File Service, Organisation Profile, Journal | `mcp_front.register(craft.tool_provider())` |
    | 5.22 | `ordin-projects` | `ProjectService::new(ProjectDeps { journal, tool_registry: MCP Front, files, rules, identity, installer: <program's ServerInstaller> (*late-bound*), streams: <program's StreamSink>, settings: <ProjectSettings channel>, data_root })` | `mcp_front.register(projects.tool_provider())` |
    | 5.23 | `ordin-claude-adapter` | MCP Front, Hook Relay, Rules Engine, hook program `<dir of current_exe>/ordin-hook`; person's own sign-in mode | `register_adapter` |
    | 5.24 | `ordin-copilot-adapter` | as desktop, Copilot CLI from the profile's pinned path | `register_adapter` |
    | 5.26 | — | — | `register_adapter(hosted_agent)`, `register_adapter(slack_thread)` |
    | 5.27 | `ordin-workspaces` | File Service, SharePoint Adapter, Rules Engine, Journal; no Confluence Adapter and no Jira Adapter (absent handles: `confluence: None`, `jira: None`; amended January 2026, ADR-106) | — |
    | 5.28 | `ordin-push` | `PushDeps { working_copy: <program's WorkingCopyStore>, confluence: None, … }` | — |
    | 5.29 | `ordin-catalogue` | Organisation Profile, Project Service, Format Plug-ins, Checker, Agent Model, Journal | `resolver.install_part_source(catalogue)` and `checker.install_part_source(catalogue)` (R18) |
    | 5.30 | `ordin-knowledge`, `ordin-mail` | as 2B-3 (Knowledge: `KnowledgeDeps` with `agent: None`; Mail: `MailDeps { workspaces, agent_names: <program's AgentNames>, identity, rules, approvals, files, profile, journal, tools: MCP Front }`) | `mcp_front.register(knowledge.tool_provider())`, `mcp_front.register(mail.tool_provider())` |
    | 5.30a | `ordin-slack-service` | `SlackService::start(SlackServiceDeps { identity, approvals, workspaces, rules: <program's SlackRulesSource>, journal, registry: MCP Front, reactor: None, http, api_base: https://slack.com/api/, program: PersonalServer })`, then `slack.set_reactor(<program's SlackReactor over OrdinHost::react_to_slack>)` once the Ordin Host exists; migrations registered at 5.1 | `mcp_front.register(slack.tool_provider())`; `start_reading(stop)` in the step 5 tail — always on, every linked channel every 60 s (SA-RATE-02) |
    | 5.31 | `ordin-package-catalogue` | as 2B-3, the program's `InstanceActivity` | — |
    | 5.32 | `ordin-host` | `OrdinHostDeps { blocks: StandardBlockCheck, localisation: the Localisation of row 5.5 (amended January 2026, ADR-109), … }` | `mcp_front.register(ordin_host.tool_provider())`; set the `CrewHost`'s Ordin Host and Package Catalogue handles |
    | 5.32a | `ordin-team-knowledge` | `TeamKnowledgeDeps { journal, memory: <TeamMemory>, pages: <TeamPages>, push: <TeamPush>, workspaces: <TeamWorkspaces>, repos: <TeamRepos>, clones_dir: <ORDIN_DATA_DIR>/team-knowledge }` (amended January 2026, ADR-103) | migrations registered at 5.1; `run(stop)` spawned in the step 5 tail — the 15-minute checks run always on |
    | 5.33 | `ordin-home-search` | as 2B-3, no Confluence Adapter | `register_source` for the Package Catalogue's search source |
    | 5.34 | `ordin-setup` | as 2B-3 (+ Journal, D4); no updater (the server never replaces itself, ADR-076) | — |
    | 5.35 | `ordin-authoring`, `ordin-handover`; `HandoverDeps.slack` = the Slack Service | as 2B-3 (Authoring + Journal); `AuthoringDeps.host` and `HandoverDeps.host` = the Ordin Host; `AuthoringDeps.places` = the program's `HandoverReach`; `HandoverDeps.identity` = the program's `PersonIdentity`; `HandoverDeps.jira` = `None` (amended January 2026, ADR-106) | `mcp_front.register(authoring.tool_provider())`, then `authoring.run(stop)` (spawned, ends at the stop token); `journal.register_prune_participant(authoring.prune_participant())`; `journal.subscribe(handover.event_sink())` |
    | 5.36 | `ordin-channel-gateway` | `GatewayDeps { journal, datastore, texts, mode: GatewayMode::Personal(PersonalDeps { core: <program's PersonalCore>, approvals, workspaces, profile, owner_channels }) }`; no Retention and Erasure in this profile (absent handle) | — |
    | 5.37 | `ordin-telegram-adapter`, `ordin-slack-channel-adapter` (`PersonalSlack`) | `TelegramDeps { mode: Server, secrets, journal, gateway }`, `PersonalSlackDeps { secrets, journal, gateway }` (Channel Gateway, Identity, Journal — K7; no pool; the Slack Channel Adapter has no Retention and Erasure edge, E4) | their `migrations()` registered at 5.1 (SA-CLOSE-03) |
    | 5.38 | `ordin-triggers` | Ordin Host, Channel Gateway, NATS Adapter, Slack Service, Organisation Profile, Journal (2B-3: workflow runs start through the Ordin Host); `TriggerDeps.slack` = the Slack Service | started in the step 5 tail; then `ordin_host.set_trigger_desk(<program's TriggerDesk>)` |
    | 5.39 | `ordin-telegram-service` | as 2B-3 (+ Journal) | `mcp_front.register(telegram.tool_provider())` |
    | 5.40 | `ServerServiceApi`, `ServerErrorSentences` | every module the Service API catalogue's *Implemented by* column names for a command of scope `Any`; Localisation | — |
    | 5.41 | `ordin-remote-api` | `RemoteApi::new(RemoteApiConfig { server_name, commands_per_second: 50, max_paired_desktops: 10 }, ServerServiceApi, ServerErrorSentences, Approval Service, Journal, IdentityService as `ServerSecrets`, Organisation Profile)` | `journal.register_prune_participant(remote_api)` |

    Row 5.25 is unused. Then, in order: `mcp_front.bind_ref_resolver(<RefResolver>)` (SA-CLOSE-01); `mcp_front.start(stop)` (freezes the registry; no `register` after it); `package_inspector.reload_rule_sets()` (SA-CLOSE-07); `hook_relay.start()` (a failure such as `HOOK_LISTEN_FAILED` → `SERVER_START_FAILED`); `agent_folders.recover()`; `session_supervisor.recover()` → `RecoveryReport { resumed, interrupted, ended_remotely, awaiting_decision }`; `channel_gateway.start(gateway_stop)` and the chat adapters' `start`; `nats.start(nats_stop)`; `TriggerService::start(deps, trigger_stop)` then `set_trigger_desk`; `remote_api.serve(remote_stop)` (returns `Serving::Off` when the profile forbids listening; with no listening settings it waits for `configure_listening`); start the control socket (step 9); `AlwaysOn::spawn` (step 7); send `READY=1` when `NOTIFY_SOCKET` is set; log `ready`.
6. **Construct the agent core** (`start_agent`). Roles: from `--serve`; absent → every role in the single shape; absent in the scaled shape → `SERVER_ROLE_COMBINATION`; a role given twice, or `chat-jobs` with `chat` or `jobs` → `SERVER_ROLE_COMBINATION`; `chat` with no chat connection in the configuration → `SERVER_NOTHING_TO_SERVE`. `ORDIN_RELEASE_DIR` absent → `SERVER_ENV_INVALID`.
    1. Every role: Journal (step 4; in the scaled shape `ApplyMode::CheckOnly`, pending migrations → stop with the Journal's code), Telemetry (`ORDIN_OTLP_ENDPOINT`), Agent Configuration, Organisation Profile with no profile, Localisation, Approval Service, Rules Engine, Settings Store (with the program's `SettingRuleCheck`, `SelectionLookup` and `MetricsSink`), Run Queue (`DatastoreRunQueue::new(&datastore, <MetricsSink>)`), Identity (single-shape secret store, or `SecretBackendConfig::None` in the scaled shape). The hosted agent's id: `INSERT INTO server_settings (key, value) VALUES ('agent_instance_id', <new UUIDv7>) ON CONFLICT (key) DO NOTHING`, then read it back (SA-CLOSE-04; a value that is not a UUID → `SERVER_START_FAILED` with `SERVER_SETTINGS_INVALID`). The Agent Loader's `ordin_agent_loader::load(LoadInput { mode, agent_instance_id: <that id>, release_dir, ordin_version, harness_version, store: Some(<program's LoaderStore>), keys, secrets, hosts, mcp_front, rules, work_folder: <program's WorkFolderOpener>, trace_status })` runs in the `runs` role once the MCP Front exists (step 6.3): digest, runtime and contract range; the start-up report logged once; then `telemetry.set_release(&loaded.release.record.release_id, loaded.release.digest.as_deref())` (a `TelemetryError` is logged and start continues), then the hosted agent's memory rules from `package_inspector.memory_contract(<that id>)` passed to `memory.set_agent_rules(…)` (program-side table; `None` → no call). `LoadedAgent.workspace_id` is the workspace of the hosted agent's MCP Front session grants and of its memory in the Memory Service.
    2. `requests`: Caller Authentication, File Service, Knowledge Service (`AgentKnowledgeDeps` as in the program-side table), Retention and Erasure (then `register` every `RetainedContent` provider), Caller API (`build_router`, every route, with the program's `JobDesk`).
    3. `runs`: Agent Folder Manager, Format Plug-ins, Resolver and Compiler, Checker, Installers, Usage Planner, Session Supervisor (`ORDIN_RUN_CONCURRENCY`, with the Run Queue), Model Policy (`ORDIN_MODEL_KEY_FILE` or the secret `model-key`), MCP Front, File Service, Hook Relay, Memory Service, Knowledge Service, Claude Local Adapter (organisation key mode), Connection Binder (`ORDIN_SECRETS_DIR` or the `env:` secrets), Catalogue Service (over `<ORDIN_DATA_DIR>/catalogue/index.sqlite`, SA-CLOSE-05); `package_inspector.reload_rule_sets()` (SA-CLOSE-07); register as providers `memory.tool_provider()`, `knowledge.tool_provider()` and, after `load`, the Connection Binder's `loaded.bound.tool_provider()` and the Agent Loader's `loaded.tool_provider()` (the `files` server) (E2: only this program calls `McpFront::register`; no module registers itself), `mcp_front.bind_ref_resolver(<RefResolver over the File Service, the Knowledge Service and the Journal>)` (SA-CLOSE-01), `mcp_front.start(stop)`, register the Claude adapter, `usage.set_harnesses(vec![claude.harness_models()])`, `session_supervisor.start_queue_workers()`, then every 10 s `run_queue.mark_interrupted()` (a failure is logged with the Run Queue's code and retried at the next tick); Duty Timer. No NATS Adapter and no Trigger Service (reconciliation row 480; D5).
    4. `chat`: Channel Gateway (`GatewayMode::Hosted`, with the program's `HostedAgentCore`; no Ordin Host in this profile), Retention and Erasure, one `HostedSlackLink` per chat connection (`SlackTokens` from the configuration's token variables; `ORDIN_IMAGE`).
    5. `jobs`: Job Runner (`JobRunner::new(&datastore, <KnowledgeIngest>, retention, telemetry, <JobOutcomeNotifier over ChannelGateway::notify_job_ended, or None>, timers, instance)`) with the ingest and retention kinds.
    6. Every role except `requests` serves `ordin_caller_api::health_router(ready, telemetry)` with `ordin_caller_api::serve` on `ORDIN_PORT`; `requests` serves the full `build_router` router, which includes it. `ready()` is false once stopping began; otherwise true when `journal.ping(2 s)`, `memory.ready()` succeeds in a role that started the Memory Service and, in a chat role, every `HostedSlackLink::is_open()`. Liveness `/healthz` answers 200 until the orderly stop ends. In the single shape the control socket is also served (step 9).
    7. Log `ready` with the roles and the release digest; send `READY=1` when `NOTIFY_SOCKET` is set. Always On (step 7) runs in the single shape only; in the scaled shape the cluster supervises.
7. **Always on** (`AlwaysOn::spawn`, personal profile and single shape). Each task stops at the stop token.
    1. *Watchdog.* When `WATCHDOG_USEC` is set: every 30 s, if `journal.ping(5 s)` is true, `notify(WATCHDOG=1)`; otherwise send nothing, so systemd restarts a hung core after `WatchdogSec`. A send failure is logged `SERVER_WATCHDOG_SEND_FAILED`.
    2. *Idle sessions.* Every 60 s, `session_supervisor.stop_idle_sessions(30 min)`; the conversation later resumes from ordin's summary (ADR-049). Personal profile only.
    3. *Harness sign-in.* At start and every 6 hours, `claude_adapter.sign_in_status()` (personal profile). On a change to `SignedOut`, call `channel_gateway.send_sign_in_ended()` once (the gateway words it, 11 §5.23) and log it. Called again only after a `SignedIn` was seen.
    4. *Weekly compaction.* Each Sunday at 04:00 in the `TZ` zone *(assumed here: the HLD says weekly, not which day)*: if `session_supervisor.runs_going() == 0`, `journal.compact()`; else retry every 15 minutes until 06:00, then skip that week and log `compaction_skipped`.
    5. *Restart message.* At start, when `<data dir>/run/started` exists and `<data dir>/run/clean-stop` does not (the last run did not stop in order), and no restart message was sent in the last hour (time kept in `<data dir>/run/last-restart-notice`), call `channel_gateway.send_restart_notice(RestartSummary { at: now, runs_resumed: report.resumed, approvals_waiting: report.awaiting_decision })` from the `RecoveryReport` (the gateway words and routes it, ADR-076). Then write `run/started` and delete `run/clean-stop`.
    6. *Channel outage.* Not a task of this program: the Channel Gateway reports a channel down for 10 minutes on the other paired channel itself (ChannelGateway step J.6).
    7. *Update announcement.* At start and every 24 hours *(assumed here)*, `setup_assistant.check_update_feed()`; a version newer than the running one and not recorded in `<data dir>/run/announced-version` is passed once to `channel_gateway.send_update_available(version)` and logged; the version is then recorded. ordin never replaces itself (ADR-076 *Updates*).
8. **Health command.** `ordin-server health` connects to `<data dir>/run/control.sock` within 5 s, sends `{"op":"health"}`, and exits 0 on `Healthy`; no socket or refused connection → `SERVER_NOT_RUNNING`, exit 1; no answer in 5 s → `SERVER_HEALTH_TIMEOUT`, exit 1. The server answers `Healthy` when `journal.ping(2 s)` is true.
9. **Setup steps.** With a running server, the setup command sends `ControlRequest::Setup` and prints the reply's lines; without one, it constructs Journal, Organisation Profile, Identity, Localisation and, as the step needs, Remote API, Telegram Adapter, Slack Channel Adapter, Channel Gateway (`GatewayMode::Setup(SetupDeps)`: pairing calls only) — and runs the same step function in-process. Every printed line is a catalogue message in en-GB (`server-setup-…` ids). Steps:
    1. **Guided `setup`** (terminal required; else `SERVER_SETUP_NEEDS_TERMINAL`). If no server profile: ask "Run this server for one person (personal) or as a hosted agent (agent)?" and insert the `server_profiles` row `{ personal | agent+single }` (a second write → `SPROF_IMMUTABLE`, shown as `SERVER_PROFILE_IMMUTABLE`). If the encrypted file store is in use and its key credential is missing: `IdentityService::create_secrets_key_credential(<data dir>/secrets-key.cred)` (`CRED_SECRET_KEY_EXISTS` when the file exists) (32 random bytes encrypted with `systemd-creds encrypt --user`). Personal: run `profile`, offer `listen`, then print the next steps — `pair-chat`, `pair-desktop`, signing in to Claude Code by running `claude` over SSH (ADR-073), and `systemctl --user enable --now ordin-server` and `loginctl enable-linger` (printed, never run). Agent single shape: run `release`, ask for `model-key`, then each connection variable the obtained configuration names, and the chat tokens.
    2. **`profile [URL]`** (personal only; agent → `SERVER_PROFILE_NOT_USED`): an `https` URL of at most 2,048 bytes, else `SERVER_PROFILE_URL_INVALID`, with one or two `--signing-key KEY` (asked for in the guided flow); stored with `organisation_profile.set_profile_address(url, signing_keys)` (`PROF_ADDRESS_INVALID` → `SERVER_PROFILE_URL_INVALID`; `PROF_INSTALLER_INVALID`, a missing or malformed key, is printed with the Organisation Profile's own sentence), fetched and verified at once; prints "Your company's settings were read and verified." or the Organisation Profile's own sentence. Without a URL in the guided flow: ask which chat channels to allow (A-40).
    3. **`listen`**: address (loopback or private, else `SERVER_LISTEN_ADDRESS_REFUSED`), port 1025–65535 proposing `8443` (else `SERVER_PORT_INVALID`), certificate and key read once from the given PEM files; `server.listening_allowed = false` in the profile → `SERVER_LISTENING_FORBIDDEN`; `--off` switches it off. The step calls `remote_api.configure_listening(Some(ListenSettings { address, port, certificate_chain_pem, key_pem }))`, or `configure_listening(None)` for `--off`; the Remote API stores the key through `ServerSecrets` and a running server re-binds within 1 s. Codes are mapped: `RAPI_ADDRESS_NOT_PRIVATE` → `SERVER_LISTEN_ADDRESS_REFUSED`, `RAPI_PORT_INVALID` → `SERVER_PORT_INVALID`, `RAPI_LISTENING_FORBIDDEN` → `SERVER_LISTENING_FORBIDDEN`; every other `RAPI_` code is printed as the Remote API gives it.
    4. **`pair-desktop`**: personal only (agent → `SERVER_PAIRING_REFUSED_AGENT`, ADR-097 item 9); `remote_api.listening()` returns `None` → `SERVER_LISTENING_NOT_SET`. `Start`: print `PairingOffer.code` with `expires_at` and `pairing_line` from `remote_api.start_pairing()`. `--list`: one line per `PairingSummary` from `list_pairings()` — computer name, paired at, last connected. `--end NAME`: take the `paired` rows of `list_pairings()` whose `computer_name` equals NAME; none → `DESK_UNKNOWN` (the Remote API's code); more than one → `SERVER_PAIRING_NAME_AMBIGUOUS` (names are not unique, SA-RECON-03); one → `remote_api.end_pairing(desktop_pairing_id)`.
    5. **`pair-chat --channel C`**: the channel must be allowed (`channels.allowed`, or the owner's A-40 choice) else `SERVER_CHANNEL_NOT_ALLOWED`; its tokens must be set (`telegram-bot-token`; `slack-bot-token` and `slack-app-token`) else `SERVER_CHANNEL_TOKEN_MISSING` naming the `setup secret` to run. Starts the pairing with `TelegramAdapter::begin_pairing("setup command")` → `PairingCode` or `PersonalSlack::begin_pairing("setup command")` → `SlackPairingCode` (`paired_by` text *chosen here*), and prints the code, its expiry, and "Send /start { $code } to your bot in a private chat." (Telegram) or "Send { $code } to the app in a direct message." (Slack). `--end` calls `channel_gateway.end_pairing(kind)` (the adapters define no end); `--needs-you both|last-used` calls `channel_gateway.set_needs_you_both(true | false)` (2D-3, default last used). These gateway pairing calls are reachable only here (`CHAN_NOT_SETUP_COMMAND` for any other path).
    6. **`secret NAME`**: unknown name → `SERVER_SECRET_NAME_UNKNOWN`; the value is read from standard input, with echo switched off when it is a terminal (`rpassword`, HLD 05, row *Server command line*); empty → `SERVER_SECRET_EMPTY`; stored by `identity.put_server_secret` under the store name the table below gives (SA-RECON-01); a NATS secret stored on a running server is followed by `nats.reload(…)` (row 5.10). `model-key` and `env:` names in a personal profile → `SERVER_SECRET_NAME_UNKNOWN`. Prints "Saved." and never echoes the value.

        | Setup `NAME` | Secret-store name (Identity pattern `^[a-z0-9][a-z0-9_./-]{0,199}$`) | Read by |
        |---|---|---|
        | `model-key` | `agent.model_key` (Model Policy's `MODEL_KEY_SECRET_NAME`) | Model Policy |
        | `env:<VARIABLE>` (`<VARIABLE>` matches `^[A-Z_][A-Z0-9_]{0,184}$`) | `connection-env/<variable in lower case>`, for example `env:CRM_API_TOKEN` → `connection-env/crm_api_token` | Connection Binder's single-shape `SecretSource`, which applies the same mapping |
        | `nats-credentials`, `nats-user-password`, `nats-token` | the NATS Adapter's own names (for example `nats/credentials`) | NATS Adapter |
        | `telegram-bot-token` | `telegram/bot-token`, stored by `TelegramAdapter::set_bot_token(token)` (checks it with `getMe`; the setup command then prints that a running server reads it at its next start) instead of `put_server_secret` | Telegram Adapter |
        | `slack-bot-token`, `slack-app-token` | `slack/bot-token`, `slack/app-token` | Slack Channel Adapter |
        | `mail-password` | asks the mail account label, then stores with `IdentityService::save_mail_password(account_label, MailPasswordKind::MailPassword, password)` instead of `put_server_secret` | Identity and Credentials, Mail Service |

        Upper-case `<VARIABLE>` maps to lower case one to one, so two variables never share a store name; the 185-character limit keeps the store name within 200 characters.
    7. **`history --days N`**: N ≥ 1 and ≤ the profile's `history.max_days` when set, else `SERVER_HISTORY_DAYS_INVALID`; stored with `journal.set_retention_days(N)`.
    8. **`release REFERENCE`** (agent single shape): must match `^[^@\s]+/[^@\s]+@sha256:[0-9a-f]{64}$` else `SERVER_RELEASE_REFERENCE_INVALID`; writes `<data dir>/agent.env` (mode `0600`) holding `ORDIN_RELEASE=<REFERENCE>`, then runs `agent release obtain` (`ordin_agent_loader::release::obtain`) into `<data dir>/release`.
10. **Agent operator commands.** Each delegates to the module in the command table; group S commands build `OperatorClient::from_env()` from `ORDIN_SERVER_URL` and `ORDIN_TOKEN_FILE` (missing → wrightrun's `SRV_URL_MISSING`, `SRV_TOKEN_MISSING`); group D and P commands read the environment of step 1. Output, confirmations and codes are the delegated module's, printed as wrightrun prints them.
11. **Orderly stop** (`SIGTERM` or `SIGINT`, caught with `tokio::signal::unix`):
    1. Personal profile (Phase 4 decision P4-2: the same modules as the desktop stops, in the desktop's order — OrdinDesktop.md step 10.3 — preceded by the server's own inputs; no asking on the server, SA-XC-13). Send `STOPPING=1`. Then, each step bounded: (a) cancel the Channel Gateway's shutdown token (`receive` refuses with `CHAN_STOPPING`; outboxes drain, bound 10 s); (b) cancel the Remote API's `serve` token (open streams closed; in-flight commands answered, bound 5 s; `Serving::Stopped`); (c) cancel the Trigger Service's shutdown token (it pauses: no new run, no acknowledgement; 2 s); (c′) `slack.shutdown()` — reading stops, no new post starts, a post in `posting` is left to the start-up reconciliation (never resent); bound 2 s, taken from the 6 s margin of (d) (*amended January 2026, ADR-102*); (d) runs: while `session_supervisor.runs_going() > 0`, check once a second until 30 s after the signal (the 60 s stop period less the 24 s steps (d)–(i) may take after it, less 6 s margin), so running runs may finish; then `session_supervisor.shutdown(ShutdownReason::Signal)` cancels every run still going and records it `interrupted` (2 s); (e) `setup_assistant.close()`; (f) `shutdown()` on the Telegram Service, Workspace Service, Project Service, Craft Toolkit, Sandbox, File Service and Organisation Profile, one step each; (g) cancel the core's stop token (Mail Service, Catalogue Service, Memory Service, Telegram Adapter, Slack Channel Adapter, NATS Adapter, MCP Front, Ordin Host, Authoring Service, Team Knowledge Service — amended January 2026, ADR-103); (h) `hook_relay.shutdown()`; (i) `journal.shutdown()`, then `datastore.close()`; write `run/clean-stop`; exit 0. Every step other than (a) and (b) is bounded by 2 s; an overrun is logged `SERVER_SHUTDOWN_STEP_TIMEOUT` and the next step runs. The stop therefore ends at most 54 s after the signal, inside `TimeoutStopSec=60s` and the container's 60 s stop period.
    2. Agent profile, single and scaled shapes (wrightrun *GracefulShutdown* order; P4-2: ordin never cuts a run short): readiness answers 503 at once; `requests` stops accepting new connections and finishes requests in flight; `runs` calls `session_supervisor.stop_taking_runs()` then `wait_for_runs()` — runs finish within the stop period and are never cut short by ordin; one still running when the supervisor (cluster, or systemd after the agent unit's `TimeoutStopSec`) ends the process is left to lease expiry and recorded `interrupted` by another instance's `mark_interrupted` when its lease lapses; `chat` closes its links; `jobs` records each job's checkpoint; then the health server stops and the process exits 0. The single shape also sends `STOPPING=1` first and writes `run/clean-stop` last.
    3. A second signal during the stop exits at once with status 130 (`SIGINT`) or 143 (`SIGTERM`).

## Data Model

Table owned by this program, registered with the Journal's migration registry as module `server` (JournalAndArtefactStore.md *Called by*; the pattern `[a-z][a-z0-9_]{1,40}` admits no hyphen):

```sql
-- SQLite (personal profile, agent single shape)
CREATE TABLE server_profiles (
    id          TEXT PRIMARY KEY,                         -- ServerProfileId, UUIDv7
    profile     TEXT NOT NULL CHECK (profile IN ('personal', 'agent')),
    shape       TEXT CHECK (shape IN ('single', 'scaled')),
    chosen_at   TEXT NOT NULL,                            -- RFC 3339, ms, Z
    singleton   INTEGER NOT NULL DEFAULT 1 CHECK (singleton = 1) UNIQUE,
    CHECK ((profile = 'agent') = (shape IS NOT NULL))
);
CREATE TRIGGER server_profiles_immutable BEFORE UPDATE ON server_profiles
BEGIN SELECT RAISE(ABORT, 'SPROF_IMMUTABLE'); END;
```

```sql
-- PostgreSQL (agent scaled shape)
CREATE TABLE server_profiles (
    id          UUID PRIMARY KEY,
    profile     TEXT NOT NULL CHECK (profile IN ('personal', 'agent')),
    shape       TEXT CHECK (shape IN ('single', 'scaled')),
    chosen_at   TIMESTAMPTZ NOT NULL,
    singleton   SMALLINT NOT NULL DEFAULT 1 CHECK (singleton = 1) UNIQUE,
    CHECK ((profile = 'agent') = (shape IS NOT NULL))
);
CREATE FUNCTION server_profiles_immutable() RETURNS trigger LANGUAGE plpgsql AS
$$ BEGIN RAISE EXCEPTION 'SPROF_IMMUTABLE'; END $$;
CREATE TRIGGER server_profiles_immutable BEFORE UPDATE ON server_profiles
FOR EACH ROW EXECUTE FUNCTION server_profiles_immutable();
```

```sql
-- SQLite and PostgreSQL alike (migration 0002_server_settings, SA-CLOSE-04)
CREATE TABLE server_settings (
    key    TEXT PRIMARY KEY CHECK (key IN ('agent_instance_id')),
    value  TEXT NOT NULL CHECK (length(value) BETWEEN 1 AND 256)
);
```

`agent_instance_id` holds a UUIDv7 in its hyphenated text form; it is written once (`ON CONFLICT DO NOTHING`) and never changed or deleted by the program.

Migration `0001_server_profiles` is additive; `0002_server_settings` is additive too (rollback: drop the table; the next start mints a new id, which makes the agent's earlier rule-set and memory records unreachable, so rollback is done only before the first `agent start`); rollback is the SQLite backup copy taken before migrating (SA-XC-07), or, in the scaled shape, dropping the table before any instance started (no data loss: the row is rewritten by `agent db apply`). Deleting the row is never done by the program: the profile is fixed for the installation's life.

Files under `<data dir>` owned by this program: `ordin.lock`; `run/control.sock`; `run/started`, `run/clean-stop`, `run/last-restart-notice` (RFC 3339 time), `run/announced-version` (one version string); `agent.env` (mode `0600`); `secrets-key.cred` (written by Identity and Credentials at the setup command's request); `logs/`.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A variable of ordin's own is malformed, or `ORDIN_RELEASE_DIR` missing for `agent start` | exit 1, `Err(permanent)` | `SERVER_ENV_INVALID` | `error[SERVER_ENV_INVALID]: ORDIN_LOG_LEVEL must be error, warn, info or debug.` |
| Both secrets key variables set (Identity and Credentials' check, passed through) | exit 1, `Err(permanent)` | `CRED_SECRET_KEY_CONFLICT` | `error[CRED_SECRET_KEY_CONFLICT]: Set ORDIN_SECRETS_KEY_FILE or ORDIN_SECRETS_KEY_CREDENTIAL, not both.` |
| No Secret Service and no key variable (Identity and Credentials' check, passed through) | exit 1, `Err(permanent)` | `CRED_SECRET_KEY_MISSING` | `error[CRED_SECRET_KEY_MISSING]: No secret store: run a Secret Service or set ORDIN_SECRETS_KEY_CREDENTIAL or ORDIN_SECRETS_KEY_FILE.` |
| Data dir not owned by the user or not writable | exit 1, `Err(permanent)` | `SERVER_DATA_ROOT_UNWRITABLE` | `error[SERVER_DATA_ROOT_UNWRITABLE]: /var/lib/ordin cannot be written by this user.` |
| Lock held at `start` | exit 1, `Err(permanent)` | `SERVER_ALREADY_RUNNING` | `error[SERVER_ALREADY_RUNNING]: An ordin server already runs on this data directory.` |
| No server profile | exit 1, `Err(permanent)` | `SERVER_NOT_SET_UP` | `error[SERVER_NOT_SET_UP]: Run ordin-server setup first.` |
| `start` on an agent profile, or `agent start` on a personal one | exit 1, `Err(permanent)` | `SERVER_PROFILE_MISMATCH` | `error[SERVER_PROFILE_MISMATCH]: This server is set up for the agent profile; use ordin-server agent start.` |
| A second profile choice | exit 1, `Err(permanent)` | `SERVER_PROFILE_IMMUTABLE` | `error[SERVER_PROFILE_IMMUTABLE]: The profile is fixed for this installation.` |
| A module fails to construct or start | exit 1, `Err(permanent)`; systemd restarts it | `SERVER_START_FAILED` | `error[SERVER_START_FAILED]: ordin-hook-relay could not start (HOOK_LISTEN_FAILED).` |
| `server_settings.agent_instance_id` holds a value that is not a UUID | exit 1, `Err(permanent)` | `SERVER_START_FAILED` (`SERVER_SETTINGS_INVALID`) | `error[SERVER_START_FAILED]: server could not start (SERVER_SETTINGS_INVALID).` |
| The Release Builder's package folder cannot be read as one package (no or several WorkspaceProfile parts, an unreadable file) | exit 1, `Err(permanent)` | the Release Builder's code, with `PackageReadError { file, message: <FMT_ code> }` | as the Release Builder prints it |
| Roles repeated, overlapping, or absent in the scaled shape | exit 1, `Err(permanent)` | `SERVER_ROLE_COMBINATION` | `error[SERVER_ROLE_COMBINATION]: each role is given once; chat-jobs already serves chat and jobs.` |
| `chat` role and no chat connection in the configuration | exit 1, `Err(permanent)` | `SERVER_NOTHING_TO_SERVE` | `error[SERVER_NOTHING_TO_SERVE]: the instance was started to serve chat and the configuration declares no chat connection.` |
| Guided `setup` without a terminal | exit 1, `Err(permanent)` | `SERVER_SETUP_NEEDS_TERMINAL` | `error[SERVER_SETUP_NEEDS_TERMINAL]: Run setup in a terminal, or use one of its steps.` |
| `profile` URL not `https` or too long | exit 1, `Err(permanent)` | `SERVER_PROFILE_URL_INVALID` | `error[SERVER_PROFILE_URL_INVALID]: The address must start with https://.` |
| `profile` on an agent profile | exit 1, `Err(permanent)` | `SERVER_PROFILE_NOT_USED` | `error[SERVER_PROFILE_NOT_USED]: A hosted agent reads no organisation profile.` |
| `listen` with a public address | exit 1, `Err(permanent)` | `SERVER_LISTEN_ADDRESS_REFUSED` | `error[SERVER_LISTEN_ADDRESS_REFUSED]: Only a loopback or private address can be used.` |
| `listen` port outside 1025–65535 | exit 1, `Err(permanent)` | `SERVER_PORT_INVALID` | `error[SERVER_PORT_INVALID]: Choose a port from 1025 to 65535.` |
| The profile forbids listening | exit 1, `Err(permanent)` | `SERVER_LISTENING_FORBIDDEN` | `error[SERVER_LISTENING_FORBIDDEN]: Your company's settings do not allow this server to listen on the network.` |
| `pair-desktop` before `listen` | exit 1, `Err(permanent)` | `SERVER_LISTENING_NOT_SET` | `error[SERVER_LISTENING_NOT_SET]: Run ordin-server setup listen first.` |
| `pair-desktop --end NAME` matches more than one paired desktop | exit 1, `Err(permanent)` | `SERVER_PAIRING_NAME_AMBIGUOUS` | `error[SERVER_PAIRING_NAME_AMBIGUOUS]: More than one computer is called "Asha's laptop". End the one you mean from that computer: Settings › Servers › Disconnect this computer.` |
| `pair-desktop` on an agent profile | exit 1, `Err(permanent)` | `SERVER_PAIRING_REFUSED_AGENT` | `error[SERVER_PAIRING_REFUSED_AGENT]: A hosted agent is reached by company sign-in, not by pairing a desktop.` |
| `pair-chat` for a channel the profile does not allow | exit 1, `Err(permanent)` | `SERVER_CHANNEL_NOT_ALLOWED` | `error[SERVER_CHANNEL_NOT_ALLOWED]: Your company's settings do not allow Telegram.` |
| `pair-chat` before the channel's tokens | exit 1, `Err(permanent)` | `SERVER_CHANNEL_TOKEN_MISSING` | `error[SERVER_CHANNEL_TOKEN_MISSING]: Run ordin-server setup secret slack-app-token first.` |
| `secret` with an unknown name | exit 1, `Err(permanent)` | `SERVER_SECRET_NAME_UNKNOWN` | `error[SERVER_SECRET_NAME_UNKNOWN]: Unknown secret name "slack-token".` |
| `secret` with an empty value | exit 1, `Err(permanent)` | `SERVER_SECRET_EMPTY` | `error[SERVER_SECRET_EMPTY]: Nothing was entered.` |
| `history` days out of range | exit 1, `Err(permanent)` | `SERVER_HISTORY_DAYS_INVALID` | `error[SERVER_HISTORY_DAYS_INVALID]: Enter a whole number of days, 1 or more, within your company's limit.` |
| `release` reference malformed | exit 1, `Err(permanent)` | `SERVER_RELEASE_REFERENCE_INVALID` | `error[SERVER_RELEASE_REFERENCE_INVALID]: Give the release as <registry>/<repository>@sha256:<digest>.` |
| `health` finds no server | exit 1, `Err(retryable)` | `SERVER_NOT_RUNNING` | `error[SERVER_NOT_RUNNING]: No ordin server answers on this data directory.` |
| `health` gets no answer in 5 s | exit 1, `Err(retryable)` | `SERVER_HEALTH_TIMEOUT` | `error[SERVER_HEALTH_TIMEOUT]: The ordin server did not answer within 5 seconds.` |
| Control socket peer of another user | connection closed | `SERVER_CONTROL_REFUSED` | `{"result":"error","code":"SERVER_CONTROL_REFUSED","message":"Refused."}` |
| A watchdog datagram cannot be sent | logged; retried at the next tick | `SERVER_WATCHDOG_SEND_FAILED` | — |
| A stop step overruns its bound | logged; next step runs | `SERVER_SHUTDOWN_STEP_TIMEOUT` | — |

`SPROF_IMMUTABLE`, Identity and Credentials' `CRED_SECRET_KEY_CONFLICT`, `CRED_SECRET_KEY_MISSING`, `CRED_SECRET_KEY_INVALID`, `CRED_SYSTEMD_CREDS_FAILED`, wrightrun's `SRV_URL_MISSING`, `SRV_TOKEN_MISSING` and every delegated module's codes pass through unchanged. The person in the chat never sees a code: chat words are the `server-…` messages of step 7.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: ordin server

  Scenario: Happy path — personal start under systemd
    Given a personal server profile and NOTIFY_SOCKET and WATCHDOG_USEC set by systemd
    When ordin-server start runs
    Then the modules are constructed in the order of Internal Logic step 5
    And READY=1 is sent after the Remote API, chat adapters and control socket are started
    And WATCHDOG=1 is sent every 30 seconds while the journal answers within 5 seconds

  Scenario: Happy path — pairing a desktop over SSH while the server runs
    Given a running personal server with listening configured
    When the server owner runs ordin-server setup pair-desktop
    Then the request goes over the control socket
    And the code and the pairing line are printed with the expiry time 10 minutes ahead

  Scenario: Happy path — agent instance in the runs role
    Given ORDIN_DATABASE_URL_FILE and ORDIN_RELEASE_DIR are set and the release digest matches
    When ordin-server agent start --serve runs starts
    Then the Session Supervisor takes runs from the Run Queue with concurrency 10
    And GET /readyz answers 200 once the release is loaded and the datastore answers

  Scenario: Happy path — the hosted agent's id is minted once and shared
    Given an empty server_settings table
    When two instances run ordin-server agent start --serve runs at the same time
    Then exactly one row server_settings(agent_instance_id) exists holding a UUIDv7
    And both instances pass that id in LoadInput.agent_instance_id

  Scenario: Happy path — the reference resolver is bound before the MCP Front starts
    When ordin-server start runs in the personal profile
    Then bind_ref_resolver is called once after the Telegram Service is built
    And McpFront.start is called after it, and reload_rule_sets before session_supervisor.recover

  Scenario: Happy path — the chat adapters keep their tables in the Journal
    When ordin-server start runs in the personal profile
    Then TelegramAdapter::migrations() and PersonalSlack::migrations() are registered with the Journal
    And neither adapter is handed a SQLite pool

  Scenario: Happy path — the agent profile opens its own catalogue index
    When ordin-server agent start --serve runs starts
    Then <ORDIN_DATA_DIR>/catalogue/index.sqlite exists and the Catalogue Service's migrations are applied to it

  Scenario: Error path — a package with two workspace profiles
    Given a package folder whose read gives two WorkspaceProfile parts
    When ordin-server agent check runs on it
    Then the PackageReader returns PackageReadError with message FMT_ROOT_NOT_SINGLE

  Scenario: Edge case — restart after a crash tells the person once
    Given the last run of the server did not stop in order
    And recovery resumed 2 runs and 1 approval waits
    When the server starts
    Then "ordin restarted at 03:12; 2 runs resumed, 1 approval still waiting." is sent to the last-used channel
    When it crashes and restarts again 10 minutes later
    Then no second restart message is sent

  Scenario: Edge case — hung core is restarted by the watchdog
    Given the journal does not answer within 5 seconds
    Then no WATCHDOG=1 is sent
    And systemd restarts the service after WatchdogSec of 90 seconds

  Scenario: Edge case — compaction waits for running work
    Given it is Sunday 04:00 in the server's time zone and one run is going
    Then compaction is retried every 15 minutes
    And it runs at the first retry when no run is going, before 06:00

  Scenario: Edge case — idle harness session
    Given a harness session had no turn for 30 minutes
    When the next minute's check runs
    Then the session is stopped

  Scenario: Edge case — harness sign-in ends
    Given Claude Code reports signed out at the 6-hourly check
    Then "Ask whoever runs your ordin server to sign in again; I can't take sign-ins in the chat." is sent once

  Scenario: Edge case — a channel down for 10 minutes
    Given Telegram and Slack are paired and Telegram has been unreachable for 10 minutes
    Then one message is sent on Slack saying Telegram can't be reached
    And no second message is sent while Telegram stays down

  Scenario: Edge case — container health command
    Given a server running in the container image
    When the container runtime runs ordin-server health
    Then it exits 0 within 5 seconds

  Scenario: Error path — setup listen on a public address
    When the server owner runs ordin-server setup listen --address 203.0.113.7 --port 8443
    Then the command exits 1 with SERVER_LISTEN_ADDRESS_REFUSED

  Scenario: Error path — the profile forbids listening
    Given the organisation profile sets server.listening_allowed to false
    When the server owner runs ordin-server setup listen with a private address
    Then the command exits 1 with SERVER_LISTENING_FORBIDDEN

  Scenario: Error path — ending a desktop pairing by an ambiguous name
    Given two paired desktops are both called "Asha's laptop"
    When the server owner runs ordin-server setup pair-desktop --end "Asha's laptop"
    Then the command exits 1 with SERVER_PAIRING_NAME_AMBIGUOUS
    And no pairing is ended

  Scenario: Happy path — ending a desktop pairing by name
    Given one paired desktop called "Asha's laptop" with id 0192a1d1-0000-7000-8000-000000000042
    When the server owner runs ordin-server setup pair-desktop --end "Asha's laptop"
    Then RemoteApi.end_pairing is called with 0192a1d1-0000-7000-8000-000000000042

  Scenario: Happy path — a proposed trigger reaches the Trigger Service through the Ordin Host
    Given a running personal server whose Trigger Service has started
    When Ordin's trigger tool calls TriggerDesk.propose for workspace 0192a1e0-0000-7000-8000-000000000001
    Then TriggerService.propose receives a TriggerProposal carrying that workspace_id
    And the trigger id is returned as text

  Scenario: Error path — pairing a desktop with an agent profile
    Given an agent server profile
    When the server owner runs ordin-server setup pair-desktop
    Then the command exits 1 with SERVER_PAIRING_REFUSED_AGENT

  Scenario: Error path — pairing a chat before its tokens
    When the server owner runs ordin-server setup pair-chat --channel slack with no Slack tokens stored
    Then the command exits 1 with SERVER_CHANNEL_TOKEN_MISSING naming setup secret slack-bot-token

  Scenario: Error path — both secrets key variables
    Given ORDIN_SECRETS_KEY_FILE and ORDIN_SECRETS_KEY_CREDENTIAL are both set
    When ordin-server start runs
    Then it exits 1 with CRED_SECRET_KEY_CONFLICT before opening the journal

  Scenario: Error path — no secret store
    Given no Secret Service answers and neither key variable is set
    When ordin-server start runs
    Then it exits 1 with CRED_SECRET_KEY_MISSING

  Scenario: Error path — not set up
    Given an empty data directory
    When ordin-server start runs
    Then it exits 1 with SERVER_NOT_SET_UP

  Scenario: Error path — second instance
    Given an ordin server runs on the data directory
    When a second ordin-server start runs
    Then it exits 1 with SERVER_ALREADY_RUNNING

  Scenario: Error path — role combination
    When ordin-server agent start --serve chat --serve chat-jobs runs
    Then it exits 1 with SERVER_ROLE_COMBINATION

  Scenario: Error path — chat role with nothing to serve
    Given the agent configuration declares no chat connection
    When ordin-server agent start --serve chat runs
    Then it exits 1 with SERVER_NOTHING_TO_SERVE

  Scenario: Error path — health with no server
    Given no ordin server runs
    When ordin-server health runs
    Then it exits 1 with SERVER_NOT_RUNNING

  Scenario: Error path — orderly stop on SIGTERM, personal profile
    Given two harness sessions are running
    When the service receives SIGTERM
    Then STOPPING=1 is sent and the runs may finish until 30 seconds after the signal
    And a run still going then is cancelled and recorded interrupted
    And the journal is shut down, the datastore closed and run/clean-stop is written
    And the process exits 0 within 60 seconds of the signal

  Scenario: Error path — orderly stop of a run instance
    Given an agent run instance executing one run
    When it receives SIGTERM
    Then /readyz answers 503 at once
    And no new run is taken
    And the run is not cancelled by ordin
    And the process exits 0 after the run ends
```

## Performance, Security, Observability

- **Performance targets:** personal start to `READY=1` ≤ 5 s p95 on a 2-vCPU server with an SQLite journal of 100,000 runs, excluding the harness's own start; agent instance start to readiness ≤ 15 s p95 excluding the release download; `ordin-server health` ≤ 1 s p95; control-socket setup steps ≤ 2 s p95 excluding network fetches; Remote API commands that wait for no model ≤ 150 ms p95 and events to the desktop ≤ 500 ms (A-46, served by the Remote API). Resident memory of the personal core idle, excluding child processes and the memory component's models: ≤ 300 MB *(assumed here)*.
- **Security:**
  - Runs as an ordinary user; no root, no administrator rights; everything under `ORDIN_DATA_DIR` (ADR-072, ADR-073). The container runs as user 10001 with a read-only root file system in the chart.
  - The only listening sockets: the Remote API on the one private address chosen at setup (personal), the agent profile's one port, and the `0600` control socket checked by `SO_PEERCRED`. NATS, Telegram and Slack connections are outbound.
  - Secrets: never in an environment variable value (only paths), never on the command line (`setup secret` reads standard input with echo off), never in a log line, error or control reply; the database URL is read from a file and dropped after connecting.
  - The setup command is the only way to pair, end a pairing, change listening or store a secret; the chat never asks for or accepts a secret (ADR-073).
  - Input validation: every command-line argument per *Internal Logic* step 9; control requests with `deny_unknown_fields` and a 64 KiB limit.
  - Never logged: secret values, the database URL, pairing codes and pairing lines (printed to the owner's terminal only), chat text of the notices, tokens of `ORDIN_TOKEN_FILE`.
- **Observability:** JSON log lines (SA-OBS-01) with `component` = `server`: `start` (`profile`, `shape`, `roles`, `version`), `ready` (`start_ms`, `release_digest` in the agent profile), `recovered` (`runs_resumed`, `approvals_waiting`), `watchdog_missed`, `idle_sessions_stopped` (`count`), `harness_signed_out`, `compaction` (`duration_ms`) and `compaction_skipped`, `restart_notice_sent`, `channel_down_notice_sent` (`channel`), `update_announced` (`version`), `setup_step` (`step`, `outcome`, never the values), `stop` (`signal`, `duration_ms`, `sessions_stopped`, `sessions_killed`), and one line per error code. Personal profile: no metrics, no traces (SA-XC-04, SA-XC-05). Agent profile: the metrics and traces belong to Telemetry and are served on `/metrics` by the health router; this program adds none.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

The Slack Service runs in the personal profile; the sections above carry it in place:

- **Shared Context — program-side implementations:** `SlackReactor` over `OrdinHost::react_to_slack` (late-bound after row 5.32), `SlackRulesSource` from the profile's `document.slack` and `document.server.external_actions_off` (`slack_post` there switches every Slack post off, cards included), the Slack token source over `IdentityService::access_token_for` and `report_refused` (the server's Slack credential comes from the relayed sign-in, decision 82), and `RefResolver::entity` for `EntityRef::SlackLink`.
- **Step 5 construction:** row 5.30a builds the Slack Service (phase 7) and starts reading in the step 5 tail, every linked channel every 60 s (SA-RATE-02); rows 5.35 and 5.38 pass it as `HandoverDeps.slack` and `TriggerDeps.slack`; the `ServerServiceApi` of row 5.40 serves the `slack.*` commands and `identity.complete_relayed_sign_in`.
- **Step 7.1:** stop step (c′) stops the Slack Service within 2 s.
- **No deep link on the server:** the `ordin://handover/open` link is opened by a desktop; a handover the person receives on the server is found by the 15-minute check (HandoverService D); `handover.open_link` is scope Local, answered by a desktop only.
- **Error Table:** no new row; start failures of row 5.30a use `SERVER_START_FAILED` naming `ordin-slack-service`.

### Acceptance Criteria (Gherkin) — added scenarios


```gherkin
  Scenario: Wiring — the Slack Service on the personal server
    Given the personal profile
    When the server starts
    Then row 5.30a builds the Slack Service before the Handover Service and the Trigger Service
    And the MCP Front holds the provider for server "slack" before it starts
    And TriggerDeps.slack is the Slack Service

  Scenario: Edge case — the agent profile builds no Slack Service
    Given the agent profile, single shape
    When the server starts
    Then no ordin-slack-service is constructed and no "slack" MCP server is offered

  Scenario: Error path — the company switches Slack posts off on the server
    Given the profile's server.external_actions_off contains "slack_post"
    When the Handover Service prepares a stage-channel post
    Then the Slack Service refuses it with SLK_SERVER_SLACK_OFF
    And the handover card shows no Slack line and email is used
```

### Gaps

None.

## Amendment January 2026 — Team knowledge (ADR-103)

> Source: decision 83 (the user's decisions D1–D10 of January 2026); HLD revision 0.51.0, ADR-103. The module: `components/TeamKnowledgeService.md`.

The Team Knowledge Service runs in the personal profile only; the agent profile builds none (its memory scopes are channels, not workspaces; ADR-103 item 1). The sections above carry it in place:

- **Shared Context — program-side implementations:** `TeamMemory`, `TeamPages`, `TeamPush`, `TeamWorkspaces` and `TeamRepos`, as the desktop.
- **Step 5 construction:** row 5.32a builds the module (phase 8) with `clones_dir = <ORDIN_DATA_DIR>/team-knowledge`; its migrations are registered at 5.1; `run(stop)` is spawned in the step 5 tail; the `ServerServiceApi` of row 5.40 serves the `team_knowledge.*` commands and `memory.keep_checked`.
- **Step 7.1:** stop step (g) cancels it with the core's stop token.
- **Error Table:** no new row; start failures of row 5.32a use `SERVER_START_FAILED` naming `ordin-team-knowledge`.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — the Team Knowledge Service on the personal server
    Given the personal profile
    When the server starts
    Then row 5.32a builds the Team Knowledge Service and its run task checks every bound folder every 15 minutes

  Scenario: Edge case — the agent profile builds no Team Knowledge Service
    Given the agent profile, single shape
    When the server starts
    Then no ordin-team-knowledge is constructed and no team_knowledge.* command is served in that profile
```

### Gaps

None.

## Amendment January 2026 — Jira issues and handovers (ADR-106)

> Source: decision 86 (the user's decisions of January 2026); HLD revision 0.54.0, ADR-106. The module: `components/JiraAdapter.md` (programme D only).

The server builds no Jira Adapter; the sections above carry this in place:

- **Step 5 construction:** row 5.27 passes `WorkspaceDeps.jira = None` and row 5.35 passes `HandoverDeps.jira = None`. A Jira issue link is then refused with `WS_JIRA_UNAVAILABLE` and a Jira handover place with `HAND_PLACE_TYPE_UNAVAILABLE`; a handover the person receives on a Jira place is read by their desktop.
- **Commands:** the `jira.*` commands are scope Local, so the Remote API refuses them with `SVC_COMMAND_LOCAL_ONLY` before this program sees them; `identity.sign_in` with `kind: jira` returns `CRED_FLOW_NOT_OFFERED` (IdentityAndCredentials.md amendment January 2026 — Jira issues and handovers).
- **Error Table:** no new row.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — no Jira on the personal server
    Given the personal profile
    When the server starts
    Then no ordin-jira is constructed and WorkspaceDeps.jira and HandoverDeps.jira are None

  Scenario: Error path — a Jira link asked for on the server
    When workspaces.add_link is sent with a jira_issue location to the server
    Then the reply is an error with code WS_JIRA_UNAVAILABLE
```

### Gaps

None.

## Amendment January 2026 — Every run done by ordin (ADR-107)

> Source: decision 87 (the user's decisions of January 2026); HLD revision 0.55.0, ADR-107.

The Helm chart carries two more values, in place in the *Helm chart* table above: `podAnnotations` and `roles.<role>.podAnnotations`. They exist so that a deployment can attach a controller of its own choosing that restarts pods when the mounted Secret changes (a rotated model key or connection secret is then picked up without a manual restart); the chart names no such controller and installs none. The chart adds no sidecar and no extra init container. Keys and values are written as given; Helm's own template rendering refuses a value that is not a map of strings, so no error row is added.

The program also wires, in place in the *Program-side implementations and calls* table above: `EnvironmentSecrets` for Model Policy over the same secret source as the Connection Binder's `SecretSource`; the Rules Engine's `ContentChecker` over the Connection Binder's bound checker operation, returned by `RulesHost::content_checker` in the agent profile; `SupervisorHost::check_content` over `RulesEngine::check_content`; and `HostedAgentCore::context_lookup` and `context_recent` over the Knowledge Service. In the personal profile none of these has an effect: there are no content checks and no context steps.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Pod annotations reach every role
    Given the values podAnnotations {"a": "b"} and roles.runs.replicas 2 and roles.requests.replicas 2
    When the chart is rendered
    Then the pod template of the runs and requests Deployments and of the migration Job each carry the annotation a: b

  Scenario: A role's annotation wins over the shared one
    Given podAnnotations {"a": "b"} and roles.runs.podAnnotations {"a": "c", "d": "e"}
    When the chart is rendered
    Then the runs pod template carries a: c and d: e
    And the requests pod template carries a: b only

  Scenario: No annotations by default
    Given no podAnnotations value
    When the chart is rendered
    Then no pod template carries an annotation added by these values
```

### Gaps

None.

## Amendment January 2026 — Ordin's language and language files (ADR-109)

> Source: decision 89 (the user's decisions of January 2026); HLD revision 0.58.0, ADR-109 (amends ADR-042 and ADR-063), 11 §4, §5.19a and UX-A77. The modules: `components/Localisation.md` and `components/OrdinHost.md` (each amendment January 2026 — Ordin's language and language files).

The sections above carry the change in place, each marked where amended:

- **Row 5.5:** `LocalisationConfig.app_version` is the program's package version, in every profile; Localisation uses it to re-check a person's languages after an update.
- **Row 5.32 (personal profile):** `OrdinHostDeps.localisation` is the Localisation of row 5.5, so Ordin on the server states the server's own active locale and can change it when asked in words.
- **Events (personal profile):** each item of `Localisation::language_changes()` is journalled as `language_changed`, as `locale_changed` is.
- **Commands:** `ServerServiceApi` (row 5.40) answers the ten new `localisation.*` commands (scope Any) over Localisation and, in the personal profile, the three `ordin.*` drafting commands over the Ordin Host. A paired desktop lists, edits, imports and exports the server's languages through the Remote API (Settings › Servers › a server › **Languages…**). A language made on the desktop is not copied to the server: its messages show in en-GB there until the person imports the exported file on the server. In the agent profile Localisation runs with `PreferenceStore::None` and refuses each language command with `LOC_LANGUAGES_UNAVAILABLE`; the Ordin Host is not built, so the `ordin.*` drafting commands are refused as every Ordin Host command is there.
- **Error Table:** no new row.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — the personal server's Ordin Host receives Localisation
    Given the server runs in the personal profile
    When it starts
    Then row 5.32 builds the Ordin Host with OrdinHostDeps.localisation set to the Localisation of row 5.5

  Scenario: Edge case — a desktop-made language is not on the server
    Given the person's desktop has a confirmed made language "ja-JP" and the server has none
    When the paired desktop sends localisation.list_languages to the server's backend
    Then the reply lists no "ja-JP"

  Scenario: Error path — agent profile refuses language commands
    Given the server runs in the agent profile
    When localisation.import_language is sent with a valid language file
    Then the reply is an error with code LOC_LANGUAGES_UNAVAILABLE
    And no table of the journal database is written
```

### Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111 (amends ADR-030; open question 6). The modules: `components/MemoryService.md`, `components/KnowledgeService.md`, `components/SessionSupervisor.md`, `components/ChannelGateway.md` and `components/CallerApi.md` (each amendment January 2026 — Agentic memory as a core capability).

The sections above carry the change in place, each marked where amended:

- **Personal profile (rows added):** `MemoryPages`, the `KnowledgeMemory` `graph` argument, `SupervisorHost::run_starting` and `set_recall_log_days` exactly as the desktop (OrdinDesktop.md amendment January 2026), with log names `server.page_explaining_failed` and `server.memory_rules_failed`. `PersonalCore::memory_used(workspace_id, recall_ids)` → `MemoryService::memory_used(MemoryPlace::Workspace(workspace_id), recall_ids)`, rendered as `MemoryUsedText`: one line per fact `"<statement> (kept <date>)"` and per link `"<from> <kind> <to> (since <date>)"`, in the view's order, at most 20, `gone` copied.
- **Agent profile (rows added):** `SupervisorHost::run_starting` does nothing (hosted runs are `ToolCaller::HostedAgent`, whose rules are set after the Agent Loader loads, as before). `HostedAgentCore::memory_used(scope, run_id)` → the run's recall ids from the Memory Service's recall log (`MemoryService::recalls_of_run(run_id)`), then `MemoryService::memory_used(MemoryPlace::Scope(scope), ids)`, rendered as above; no recall → empty `MemoryUsedText`. `MemoryDesk` (Caller API, group `requests`): `call(verb, input)` → the Memory Service method of `memory.<verb>` in its command table, with `MemoryPlace::Scope` from `input.scope_id`, the result serialised to JSON; a `MemoryError` → `MemoryDeskError { code, message: <the Memory Service's sentence>, kind: <its variant> }`. `CallerApiDeps.memory` is that implementation (step 6.2). `set_recall_log_days` is called with the configuration's history period (default 90).
- **Commands:** `ServerServiceApi` (row 5.40) answers the thirteen new `memory.*` commands in the personal profile, so a paired desktop reaches them through the Remote API.
- **Error Table:** no new row.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — owners reach a hosted agent's memory through the Caller API
    Given the server runs in the agent profile with channel scope C
    When an operator sends POST /operator/memory/things with {"scope_id": C}
    Then MemoryService::list_things is called with MemoryPlace::Scope(C)

  Scenario: Wiring — a hosted reply's used memory
    Given a hosted run R in scope C that recalled 2 facts
    When the Channel Gateway calls HostedAgentCore::memory_used(C, R)
    Then it returns 2 lines in the form "<statement> (kept <date>)"

  Scenario: Wiring — Telegram's What Ordin used on the personal server
    Given the server runs in the personal profile and Ordin's reply carries memory_used [R1]
    When the person presses "What Ordin used"
    Then MemoryService::memory_used is called with MemoryPlace::Workspace and [R1]

  Scenario: Error path — a memory error reaches the operator as the Memory Service's code
    Given MemoryService::get_thing fails with MEM_THING_NOT_FOUND
    When an operator sends POST /operator/memory/thing
    Then the answer is 404 with code MEM_THING_NOT_FOUND
```

### Gaps

None.

---

## Amendment January 2026 — Knowledge agents and the Open Knowledge Format (ADR-112, ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-112 (ordin's wiki is an Open Knowledge Format bundle) and ADR-113 (knowledge agents: ordin in the agent profile reads Git repositories into the one memory and answers retrieval; only agent-profile servers answer). The modules: `components/KnowledgeQueryClient.md`, `components/KnowledgeQueryServer.md` and `components/KnowledgeReader.md` (new), `components/MemoryService.md`, `components/DutyTimer.md`, `components/CallerApi.md` and `components/AgentConfiguration.md` (each amendment January 2026 — Knowledge agents and the Open Knowledge Format).

### Personal profile

The personal server asks and never answers, exactly as the desktop (OrdinDesktop.md amendment January 2026):

- Row **5.9a** `ordin-knowledge-query-client` — `KnowledgeQueryClient::new(KnowledgeClientDeps { journal, identity: Some(identity), http, mode: KnowledgeClientMode::Person { sign_in: CompanySignIn { issuer: <profile identity_provider.issuer>, client_id: <profile identity_provider.client_id> } } })`, the server owner's company sign-in held by Identity and Credentials; then `apply_company_front_doors(<profile knowledge_agents.front_doors[]>)`, and again on each verified profile change (the refresh of row 5.2).
- After row 5.20, `MemoryService::set_knowledge_client(<the client>)`.
- `ServerServiceApi` (row 5.40) answers the four `knowledge_agents.*` commands, so a paired desktop connects a server workspace through the Remote API.
- No Knowledge Query Server and no Knowledge Reader is constructed; the Remote API serves no knowledge route.
- **Knowledge Service (every profile).** `KnowledgeDeps` gains `identity: Arc<dyn KnowledgeIdentity>` and `app_version` (KnowledgeService.md amendment January 2026): in the personal profile `git_login` is the owner's Git host login from Identity and Credentials (`person()`'s `github_login`, else `None`); in the agent profile it is always `None` (no person; a knowledge agent writes no page, ADR-113); `app_version = env!("CARGO_PKG_VERSION")`, as row 5.5.

### Agent profile

A hosted agent is a **knowledge agent** when its agent configuration has a `knowledge-agent` section (`AgentConfig.knowledge_agent`, read with `knowledge_agent(&config)`; AgentConfiguration.md amendment January 2026). Without one, step 6 is unchanged: no knowledge route, no reader, no knowledge scope. With one (`ka` below), step 6 adds, in each role:

1. **Every role:** the knowledge scope: `INSERT INTO server_settings (key, value) VALUES ('knowledge_scope_id', <new ScopeId UUIDv7>) ON CONFLICT (key) DO NOTHING`, read back, and the scope registered with kind `knowledge` (2C `ScopeKind::Knowledge`; exactly one per agent) through the same scope registry the Channel Gateway's scopes use (*chosen here*, SA-SRV-KA-01). Then the Knowledge Query Client in agent mode — `KnowledgeQueryClient::new(KnowledgeClientDeps { journal, identity: None, http, mode: KnowledgeClientMode::Agent { proxies: ka.proxies, service_token } })`, where `service_token` is `Some(ServiceTokenSettings { identity: ka.service_identity, client_secret: <the value of ka.service_identity.client_secret_env, resolved as 2D-1d> })` when `ka.proxies` is not empty and `None` otherwise. The client makes the client-credentials token request itself (KnowledgeQueryClient.md SA-KQC-04); Identity and Credentials holds people's credentials only, and no person's credential is used (ADR-097 item 2). The server's and the reader's tables are registered with the migration registry at step 4 in every role, so each role reads the same `kq_*` and `kr_*` rows.
2. **`requests`:** the Memory Service (as in `runs`, step 6.3, with `MemoryServiceConfig.knowledge_scope = Some(<the scope>)`; no `set_knowledge_client`: an agent's own recall never asks wider), then the Knowledge Query Server — `KnowledgeQueryServer::new(KnowledgeServerDeps { config: ka, issuer: <auth.issuer>, token_audience: <auth.audience>, knowledge_scope, memory, client, journal, telemetry, hot_copy: <program's HotCopy>, serves_requests: true }, stop)` — and `CallerApiDeps.knowledge_query = Some(server)`, so `build_router` merges the server's router (routes 34–39, CallerApi.md amendment January 2026; the MCP route only when `ka.mcp.enabled`). The Caller Authentication `Verifier` is built `with_knowledge(KnowledgeAccess { group_claim: ka.group_claim, audiences: ka.audiences, front_door_subjects: ka.front_doors })` (CallerAuthentication.md amendment January 2026).
3. **`runs`:** the Knowledge Reader — `KnowledgeReader::new(ka, memory, <program's ReaderModel>, <program's ReaderOutlet>, <program's CredentialSource>, &datastore, journal, telemetry, knowledge_scope, <ORDIN_DATA_DIR>)` — and `duty_timer.register_built_in(reader.duty())` before the Duty Timer's `run_forever` starts (DutyTimer.md amendment January 2026). The reader's first cycle runs at the duty's offset within `ka.fetch_interval_minutes` of start (ADR-110 item 8's spread).
4. **`chat`, `jobs`:** nothing added.

In the single shape all four are one process, so steps 2 and 3 run in it and share one Memory Service and one Knowledge Query Server.

**Program-side implementations (rows added to the table above):**

| Interface (owner) | Implemented over | Constructed at |
|---|---|---|
| `ReaderModel` (Knowledge Reader) | one fixed harness turn: `session_supervisor` starts a run of the hosted agent's harness through the Claude Local Adapter in organisation-key mode with the reader's fixed instruction and the document text as the one input, **no tool offered** (the MCP Front opens the session with an empty tool list; the single-call answer form of ADR-098 item 3), and returns the turn's final text within the request's `deadline`; the reader parses and checks it in Rust. The turn's model use is counted against the configuration's reading budget by the reader, not here. A harness that cannot start → `ReaderModelError::Unavailable`; the deadline passed → `ReaderModelError::TimedOut`; a refusal by the Rules Engine or the content check → `ReaderModelError::Refused`. | step 6.3 |
| `ReaderOutlet` (Knowledge Reader) | `publish_contents(c)` → `KnowledgeQueryServer::publish_contents(c)`; `repository_moved(r, c)` → `KnowledgeQueryServer::repository_moved(r, c)`; `publish_status(s)` → `KnowledgeQueryServer::publish_status(s)`; an `Err` is logged (`knowledge_outlet_failed { code }`) and the reader carries on. In the scaled shape the `runs` role constructs a Knowledge Query Server with `serves_requests: false`: the three calls write the server's shared tables, and the `requests` instances learn of a move through `kq_changes`, which they poll every second (KnowledgeQueryServer.md; SA-SRV-KA-02). | step 6.3 |
| `HotCopy` (Knowledge Query Server) | `add(repository)` → the repository key (`<host>/<owner path>@<branch>[:<path>]`, KnowledgeReader.md SA-KRD-12) parsed into `https://<host>/<owner path>`, branch and path, then `KnowledgeReader::add_hot_repository(url, path, branch)`; `remove(repository)` → `KnowledgeReader::remove_hot_repository(repository)`. A `ReaderError` → `HotCopyError { code }`. Both only record the repository in the reader's table, which takes effect at the reader's next cycle; in the `requests` role of the scaled shape the program constructs the reader without registering its duty, for these two calls only (SA-SRV-KA-03). | step 6.2 |
| `CredentialSource` (Knowledge Reader) | `token(name)` → the value of the variable `name` from the server's secret store (single shape) or `ORDIN_SECRETS_DIR/<name>` (scaled shape), as 2D-1d resolves every variable the agent configuration names; `None` when absent | step 6.3 |
| `BuiltInDuty` (Duty Timer) | implemented by the Knowledge Reader itself (`knowledge_fetch`, `reader.duty()`); the program only registers it | step 6.3 |

**Readiness (step 6.6):** in a role that started the Knowledge Query Server, `ready()` also requires `knowledge_server.ready()` (its tables readable; it does not wait for the first fetch, so a new knowledge agent answers with what it holds and says "as of" no fetch yet).

**Orderly stop (step 11.2):** `requests` closes the server's open change streams and MCP sessions with the other connections; `runs` lets a reader cycle in progress finish its current document, then records the cycle's progress and stops (the next start continues from the last read commit of each repository); the client closes its open connections to proxies.

**Start-up report:** a knowledge agent's report adds, beside the connections, the repositories and glossaries it reads (address and path, never a credential), its audiences, the front doors allowed to ask it, its proxies, whether the MCP endpoint is on and whether the sources record is on with its retention days (ADR-097 item 3: who may reach the agent and through which way in).

### Commands

One operator sub-command: `ordin-server agent knowledge fetch` runs one reader cycle on demand (`KnowledgeReader::run_cycle`; KnowledgeReader.md). `ordin-server agent check` checks the `knowledge-agent` section through the Agent Configuration's rules (AgentConfiguration.md amendment January 2026).

### Error Table (rows added)

| Code | When | Action |
|---|---|---|
| `SERVER_KNOWLEDGE_SCOPE_INVALID` | the `knowledge_scope_id` setting read back is not a UUID | start stops with `SERVER_START_FAILED` naming `server_settings`; the operator restores the database or removes the row |

The Knowledge Query Server's, Reader's and Client's own start errors stop start with `SERVER_START_FAILED` naming the crate and the module's code, as every module's do (step 6).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Wiring — a knowledge agent serves the knowledge routes
    Given the agent configuration has a knowledge-agent section with audiences ["supply-chain"]
    When the server starts in the agent profile, single shape
    Then the Knowledge Query Server is constructed and the Caller API serves POST /v1/knowledge/query
    And the Knowledge Reader is constructed and its knowledge_fetch duty is registered with the Duty Timer

  Scenario: Edge — a hosted agent without a knowledge-agent section is unchanged
    Given the agent configuration has no knowledge-agent section
    When the server starts in the agent profile
    Then no Knowledge Query Server and no Knowledge Reader is constructed
    And POST /v1/knowledge/query answers 404

  Scenario: Edge — the personal profile asks and never answers
    When the server starts in the personal profile
    Then the Knowledge Query Client is constructed and no knowledge route is served

  Scenario: Wiring — reading a document is one turn with no tools
    Given the reader asks ReaderModel to read "suppliers/borealis.md"
    When the program runs the turn
    Then the harness session is opened with no tool and the reader receives the turn's final text

  Scenario: Wiring — a moved repository reaches the query server
    Given the reader finished a fetch that moved repository R to commit C
    When it calls ReaderOutlet::repository_moved(R, C)
    Then KnowledgeQueryServer::repository_moved(R, C) is called

  Scenario: Error path — the knowledge scope setting is damaged
    Given server_settings holds knowledge_scope_id "not-a-uuid"
    When the server starts
    Then start stops with SERVER_START_FAILED naming server_settings and SERVER_KNOWLEDGE_SCOPE_INVALID
```

### Assumptions

- **SA-SRV-KA-01** The knowledge scope is kept like the hosted agent's id: one row in `server_settings`, created on the first start with a `knowledge-agent` section, registered with kind `knowledge`.
- **SA-SRV-KA-02** In the scaled shape the reader (`runs`) and the query server (`requests`) run in different processes; they meet only through the query server's tables in the shared datastore. The `runs` role therefore constructs the query server without routes, for publishing.
- **SA-SRV-KA-03** In the scaled shape the `requests` role constructs the Memory Service for recall and the Knowledge Reader without its duty, for `HotCopy`. The `requests` and `runs` roles therefore each construct a Memory Service over the one shared PostgreSQL store. Two or more memory instances over one store is the condition the memory component's RISK-014 covers (`memory/docs/wip/backlog.md`); a knowledge agent in the scaled shape depends on that risk's confirmation, as every scaled hosted agent with several `runs` instances already does, and recall at a knowledge agent's size is measured under C-95.
- **SA-SRV-KA-04** The scaled shape of a knowledge agent is built and run only after the memory component confirms that more than one instance may share one store (its RISK-014; HLD R-123; settled by the user, January 2026). Until then a knowledge agent is deployed in the single shape, and the rows of this amendment that name the `requests` and `runs` roles describe the target, not the first delivery.

### Gaps

None.

---

## Amendment January 2026 — Releases and updates (ADR-114)

> Source: decision 93 (the user's decisions of January 2026); HLD revision 0.62.0, ADR-114 (each build answers its own module groups; a personal server updates itself when the person asks; the agent profile never replaces itself) and its amendment of ADR-076 (*Updates*: "never unasked" replaces "never"). The modules: `components/ServiceApi.md` (the groups a backend answers, `SVC_NOT_IN_THIS_VERSION`, `setup.server_update`, `setup.update_server`), `components/SetupAssistant.md` (the server's self-update and the trait it is carried out through), `components/ArchitectureTest.md` (metadata read with `--all-features`), each amendment January 2026 — Releases and updates. Where this amendment and the sections above differ, this amendment wins; in particular the *Traits the desktop program implements that this program does not wire* sentence no longer lists `AppUpdater` as the reason the server never replaces itself, and row 5.34 changes as below.
> *Decoupled January 2026: build switches replace release-numbered features; releases are planned in the maintainers' release roadmap, kept outside the repository.*

### Public Interface — package and build

**SRV-UP-1** Every crate of step 70's *Needs done* not named below stays a normal dependency and is in every build. The crates below are `optional = true` and enabled by the build switch (Cargo feature) named. This spec turns no switch on by default; the manifest of a shipped version lists the switches that version turns on, and which version turns on which switch is planned outside this spec, in the maintainers' release roadmap. The module switches carry the desktop's names (OrdinDesktop.md amendment January 2026) for the modules the personal profile wires; they take effect only with a profile switch.

```toml
[features]
default          = []
personal-profile = ["slack", "dep:ordin-remote-api", "dep:ordin-triggers",
                    "dep:ordin-agent-configuration", "dep:ordin-settings-store", "dep:ordin-retention",
                    "dep:ordin-nats", "dep:ordin-channel-gateway", "dep:ordin-slack-channel-adapter"]   # Steps 35, 68, 9, 24, 60, 19, 62, 67
agent-profile    = ["dep:ordin-agent-configuration", "dep:ordin-settings-store", "dep:ordin-retention",
                    "dep:ordin-nats", "dep:ordin-channel-gateway", "dep:ordin-slack-channel-adapter",
                    "dep:ordin-telemetry", "dep:ordin-model-policy", "dep:ordin-caller-auth",
                    "dep:ordin-duty-timer", "dep:ordin-connection-binder", "dep:ordin-agent-loader",
                    "dep:ordin-release-builder", "dep:ordin-job-runner", "dep:ordin-caller-api",
                    "dep:ordin-knowledge-query-server"]                                               # Steps 9, 24, 60, 19, 62, 67, 10, 22, 23, 33, 48, 52, 56, 64, 65, 77
sandbox          = ["dep:ordin-sandbox"]                                                              # Step 41
copilot          = ["dep:ordin-copilot-adapter"]                                                      # Step 47
slack            = ["dep:ordin-slack-service", "dep:ordin-slack-thread-adapter"]                      # Steps 73, 18
mail             = ["dep:ordin-mail"]                                                                 # Step 54
team-knowledge   = ["dep:ordin-team-knowledge"]                                                       # Step 74
authoring        = ["dep:ordin-authoring"]                                                            # Step 61
handover         = ["mail", "slack", "dep:ordin-handover"]                                            # Step 63
telegram         = ["personal-profile", "dep:ordin-telegram-adapter", "dep:ordin-telegram-service"]  # Steps 66, 69
hosted-agents    = ["dep:ordin-hosted-agent-adapter"]                                                 # Step 17
knowledge-agents = ["dep:ordin-knowledge-reader"]                                                     # Step 78
```

The crates the two profile switches share are listed in both, so either switch alone builds them. `telegram` turns on `personal-profile` because the Telegram Adapter's crate depends on the Channel Gateway's (2B-3) and Telegram exists only in the personal profile. `personal-profile` turns on `slack` because the Trigger Service, which the personal profile constructs, takes a concrete Slack Service (TriggerService.md *Dependencies*). Three crates of the knowledge amendment are placed by the crate graph, not by the switch alone: `ordin-knowledge-query-client` is a normal dependency in every build, because the always-built Memory Service depends on it (ArchitectureTest.md); `ordin-knowledge-query-server` comes with `agent-profile`, because the Caller API depends on it (ArchitectureTest.md, `ordin-caller-api` row); only `ordin-knowledge-reader` is behind `knowledge-agents`, and it is constructed only when `agent-profile` is on as well.

| Switch | What the program offers with it |
|---|---|
| neither profile switch | `ordin-server --version` only *(chosen here)*: the crate builds in `cargo build --workspace` and `make check`, and no archive or image is produced |
| `personal-profile` | the personal profile: `start`, `health`, `update`, `setup` with every personal step; the guided `setup` offers the personal profile |
| `agent-profile` | the agent profile, single and scaled shapes: `agent …`, `setup release`; the guided `setup` offers the agent profile |
| `sandbox`, `copilot`, `slack`, `mail`, `team-knowledge`, `authoring`, `handover`, `telegram` | the personal-profile rows of those modules, as on the desktop |
| `hosted-agents` | `ordin-hosted-agent-adapter` in the personal profile (rows 5.9 and 5.26) |
| `knowledge-agents` | construction of the Knowledge Query Client in every profile built (row 5.9a; the crate itself is always built); with `agent-profile` also the knowledge-agent wiring of the agent profile (amendment January 2026 — Knowledge agents): the Knowledge Query Server's construction (its crate comes with `agent-profile`) and the Knowledge Reader, whose wiring code is compiled only with both switches |

- **SRV-UP-2** A server profile record of kind `agent` read by a build without `agent-profile` stops start with `SERVER_PROFILE_MISMATCH` (its existing sentence names `ordin-server agent start`, which this build does not have; the owner installs a build with the agent profile); a record of kind `personal` read by a build without `personal-profile` does the same. No new code.
- **SRV-UP-3** As on the desktop, if the C-17 spike fails the `sandbox` switch is not turned on, and row 5.19 is then absent from every profile.
- **SRV-UP-4** The code of a row behind a switch is compiled only with that switch (`#[cfg(feature = "…")]`); a build holds no stub of an absent module. For `knowledge-agents` that is the program's wiring only: the Knowledge Query Client's crate is in every build, and the Knowledge Query Server's in every build with `agent-profile`.

### Internal Logic — step 5 and step 6 by build switch

**SRV-UP-5** **Personal profile (step 5).** Exists only with `personal-profile`, which always brings `slack`: the Slack Service and the Trigger Service, which takes it, are constructed in every personal-profile build. Every row of step 5 is constructed except the rows of a module whose switch the build lacks: row 5.19 without `sandbox`; the Hosted Agent Adapter half of row 5.9 and its registration at 5.26 without `hosted-agents`; the Copilot Local Adapter without `copilot`; the Mail, Team Knowledge, Authoring, Handover and Telegram rows without their switches; row 5.9a without `knowledge-agents`. Usage Planner `set_harnesses` (row 5.11) takes the local adapters the build constructs. The MCP Front providers registered before `start` are those of step 5 whose modules the build constructs (OrdinDesktop.md DSK-UP-6 applied to step 5). Each seam of an absent module behaves as OrdinDesktop.md DSK-UP-5 states for the same module.

**SRV-UP-6** **Agent profile (step 6).** Exists only with `agent-profile`; built with it, step 6 is unchanged. The knowledge-agent rows of the amendment January 2026 — Knowledge agents exist only with `knowledge-agents` as well. A build with `agent-profile` and without `knowledge-agents` that reads a configuration with a `knowledge-agent` section constructs no knowledge row, logs `knowledge_agent_not_served` at `warn` once at start, and its start-up report names the section as not served by this version *(chosen here)*; the rest of the agent starts as without the section.

**SRV-UP-7** **Groups this build answers.** `ServerServiceApi` (row 5.40) implements `ServiceApi::groups()` (ServiceApi.md SVC-RL-1), a list fixed at build time, by the rule of OrdinDesktop.md DSK-UP-7 applied to the rows this build constructs, plus `service`. `ServiceHello.groups` is that list, sorted. A paired desktop therefore sees, per backend, only the groups that backend answers.

### Self-update (personal profile only)

**SRV-UP-8** **Row 5.34 (replaced).** `ordin-setup`, as 2B-3 (+ Journal, D4), and in the personal profile, right after `new`, `setup_assistant.register_server_platform(Arc::new(SystemdSelfUpdate))` *(`SystemdSelfUpdate` chosen here)*, which implements the Setup Assistant's `ServerPlatform` (SetupAssistant.md amendment January 2026 — Releases and updates). The agent profile constructs no Setup Assistant (step 6; SetupAssistant.md SET-UP-10): there `ServerServiceApi` answers `setup.server_update` and `setup.update_server`, and the control socket answers `update_check` and `update`, with the code `ordin_setup::SELF_UPDATE_UNAVAILABLE` (`SETUP_SELF_UPDATE_UNAVAILABLE`) and the Setup Assistant's sentence for it (ADR-114 item 6). The server still installs no `AppUpdater` (no Tauri on the server).

**SRV-UP-9** `SystemdSelfUpdate` implements `ServerPlatform`; the Setup Assistant downloads, verifies, unpacks and puts the programs in place itself (SET-UP-16 to SET-UP-22):

1. **`run_mode()`.** `ServerRunMode::UserUnit` when the first line of `/proc/self/cgroup` ends with `/ordin-server.service` *(chosen here: the process is the main process of the shipped user unit)* and the directory of `std::env::current_exe()` is writable by the running user and holds both `ordin-server` and `ordin-hook`; `ServerRunMode::Container` when `std::env::current_exe()` is `/usr/local/bin/ordin-server`, the container image's location (§*Container image*; that folder belongs to root) *(chosen here)*; `ServerRunMode::Other` otherwise. Read at each call, so a moved or re-permissioned folder is seen at the next request.
2. **`program_dir()`.** The directory of `std::env::current_exe()` (`~/.local/bin` for the shipped archive, §*Shipped files*).
3. **`feed_platform()`.** `"linux-x86_64"` or `"linux-aarch64"` from `std::env::consts::ARCH` *(chosen here; the key form is confirmed by C-99)*.
4. **`restart_unit()`.** Runs `systemctl --user --no-block restart ordin-server.service` with `std::process::Command`, waiting at most 10 s for that command (it queues the job and returns). A non-zero status or a timeout → `Err(<reason, at most 200 characters>)`; the Setup Assistant then restores both programs from `.previous` and returns `SETUP_UPDATE_REPLACE_FAILED`, so the running server keeps its own programs (SET-UP-21). `Ok` → systemd stops this process with `SIGTERM` (step 11.1: runs may finish for 30 s, then are recorded `interrupted`) and starts the new `ordin-server`, which applies its migrations after the Journal's backup copy (JournalAndArtefactStore, *Backup copy before migrating*). This relies on C-100 (a user unit's main process may ask systemd to restart its own unit, lingering on, with no administrator).
5. The files `share/systemd/*.service` are not replaced *(chosen here)*: a version whose unit file changes says so in its release notes, and its owner copies the new unit by hand.

**SRV-UP-10** **Commands over the Remote API.** `ServerServiceApi` answers `setup.server_update` and `setup.update_server` (scope `Any`) by calling the Setup Assistant (ServiceApi.md amendment). A paired desktop's **Update server…** reaches the server this way; the server's own stop then ends the desktop's connection, and the Server Connector reconnects to the new version as it reconnects after any restart.

### Command line (added)

```text
ordin-server update [--check]                  personal profile: update this server to the version the feed offers
```

**SRV-UP-11** `update` talks to the running server over the control socket, which carries out the update, so the steps always run in the process the user unit supervises:

1. No server answers on `<data dir>/run/control.sock` → `SERVER_NOT_RUNNING`, exit 1 (the owner starts it first: an update replaces programs the unit runs).
2. Send `{"op":"update_check"}`; the server calls `setup_assistant.server_update()`. The reply's lines are printed: `ordin-server <running> is up to date.` or `ordin-server <new> is available (running <current>).` With `--check`, exit 0 here.
3. When a newer version is available: print `Work running on this server stops when it restarts. Type yes to update:` and read one line from standard input; anything but `yes` → exit 3 (no confirmation, as the agent operator commands do).
4. Send `{"op":"update","version":"<new>"}` with the version printed in step 2. The server compares it with `setup_assistant.server_update()`'s version (different → `SETUP_UPDATE_VERSION_CHANGED`), then calls `setup_assistant.run_server_update()` and answers with its `ServerUpdateOutcome`, or with the first error:
   - `Restarting { version }` → prints `Updating to ordin-server <version>; the server restarts now.`, exit 0;
   - `UpToDate` → prints `ordin-server <running> is up to date.`, exit 0;
   - an error prints `error[<CODE>]: <message>` and exits 1.
5. On the agent profile (single shape) the reply is `SETUP_SELF_UPDATE_UNAVAILABLE`, exit 1; the scaled shape has no control socket, so step 1 applies.

```rust
// ControlRequest gains one variant (deny_unknown_fields kept):
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlRequest {
    Health,
    Setup { step: SetupStep },
    /// The version the feed offers, or none (`ordin-server update --check` and step 2 of SRV-UP-11).
    UpdateCheck,
    /// Update to the version the owner confirmed (step 4 of SRV-UP-11).
    Update { version: String },
}
```

The reply is `ControlReply::Ok { lines }` or `ControlReply::Error { code, message }`, as for setup steps. Exit statuses are the table above: 0, 1, 2, 3.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `update` or `setup.server_update` / `setup.update_server` where the server cannot replace itself (agent profile; not run by `ordin-server.service`; programs' folder not writable, as in the container image) | exit 1 / `Err(permanent)` | `SETUP_SELF_UPDATE_UNAVAILABLE` | `error[SETUP_SELF_UPDATE_UNAVAILABLE]: This server can't update itself. Whoever runs it installs new versions.` |
| The feed has no archive for this server's architecture | exit 1 / `Err(permanent)` | `SETUP_UPDATE_NO_ARCHIVE` | `error[SETUP_UPDATE_NO_ARCHIVE]: This version has no download for this server yet.` |
| The archive's signature does not verify with the build's update key | exit 1 / `Err(permanent)` | `SETUP_UPDATE_SIGNATURE_INVALID` | the Setup Assistant's sentence |
| The feed or the archive cannot be fetched | exit 1 / `Err(retryable)` | `SETUP_UPDATE_UNREACHABLE` | the Setup Assistant's sentence |
| The programs cannot be put in place; the previous programs are back (SET-UP-20) | exit 1 / `Err(retryable)` | `SETUP_UPDATE_REPLACE_FAILED` | `error[SETUP_UPDATE_REPLACE_FAILED]: The update couldn't be installed. The server is still on its current version.` |
| An update is already under way | exit 1 / `Err(permanent)` | `SETUP_UPDATE_IN_PROGRESS` | `error[SETUP_UPDATE_IN_PROGRESS]: An update is already under way.` |
| The confirmed version is no longer the one the feed offers | exit 1 / `Err(permanent)` | `SETUP_UPDATE_VERSION_CHANGED` | `error[SETUP_UPDATE_VERSION_CHANGED]: A newer version came out meanwhile. Check again.` |
| A command of a group this build does not answer, from a paired desktop | `Err(permanent)`, raised by the Service API; HTTP 404 | `SVC_NOT_IN_THIS_VERSION` | the Remote API's error body with "This needs a newer version of ordin." |

Every code above is the Setup Assistant's or the Service API's (ADR-114 contract); this program adds no `SERVER_` code.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — a personal-profile build without hosted agents or knowledge agents
    Given a build with the switches personal-profile and telegram and a personal server profile
    When ordin-server start runs
    Then the Hosted Agent Adapter and row 5.9a are not constructed
    And service.hello returns groups that include "telegram" and do not include "hosted" or "knowledge_agents"

  Scenario: Edge case — a personal-profile build always has the Slack Service for triggers
    Given a build with the switch personal-profile and no other switch
    When ordin-server start runs with a personal server profile
    Then the Slack Service and the Trigger Service are constructed
    And service.hello returns groups that include "slack"

  Scenario: Error path — the agent profile in a build without it
    Given a build with the switch personal-profile and without agent-profile, and a server profile of kind agent
    When ordin-server start runs
    Then it exits 1 with SERVER_PROFILE_MISMATCH

  Scenario: Happy path — the owner updates the server from the terminal
    Given a personal server run by ordin-server.service from ~/.local/bin at version 0.4.0
    And the feed offers 0.5.0 with an archive for this architecture signed with the update key
    When the owner runs ordin-server update and types yes
    Then ~/.local/bin/ordin-server.previous and ~/.local/bin/ordin-hook.previous hold the 0.4.0 programs
    And ~/.local/bin/ordin-server and ~/.local/bin/ordin-hook hold the 0.5.0 programs
    And "systemctl --user --no-block restart ordin-server.service" is run once
    And the command prints "Updating to ordin-server 0.5.0; the server restarts now." and exits 0

  Scenario: Edge case — the owner does not confirm
    Given the feed offers 0.5.0
    When the owner runs ordin-server update and types no
    Then nothing is downloaded and the command exits 3

  Scenario: Happy path — check only
    Given the feed offers 0.5.0 and the server runs 0.4.0
    When the owner runs ordin-server update --check
    Then it prints "ordin-server 0.5.0 is available (running 0.4.0)." and exits 0

  Scenario: Error path — a server in the container image
    Given a personal server running in the container image
    When the owner runs ordin-server update inside the container
    Then it exits 1 with SETUP_SELF_UPDATE_UNAVAILABLE

  Scenario: Edge case — the restart cannot be queued
    Given a personal server run by ordin-server.service
    And systemctl exits with status 1
    When the owner runs ordin-server update for 0.5.0 and types yes
    Then ordin-server and ordin-hook are the 0.5.0 programs and the .previous files hold 0.4.0
    And the command prints "Installed ordin-server 0.5.0; it runs from the next start." and exits 0

  Scenario: Error path — the agent profile never replaces itself
    Given an agent-profile server in the single shape
    When the operator runs ordin-server update
    Then it exits 1 with SETUP_SELF_UPDATE_UNAVAILABLE

  Scenario: Happy path — a paired desktop updates the server
    Given a paired desktop and a personal server at 0.4.0 that can replace itself
    When the desktop sends setup.update_server with version "0.5.0"
    Then the reply is accepted
    And the server restarts on 0.5.0 and the desktop reconnects to it
```

### Performance, Security, Observability (additions)

- **Performance:** the download and check run off the paths that answer runs and chats; the server answers commands until systemd stops it.
- **Security:** the server replaces only its own two programs, in its own folder, as the user it runs as; nothing runs with administrator rights. An archive is put in place only after its signature verifies with the update key built into this program (2D-2b). The update happens only on the owner's `yes` at the terminal or a paired desktop's `setup.update_server`, which the Remote API accepts only from a paired desktop (ADR-114 item 5: never unasked).
- **Observability:** `start` gains `features`; `update_requested` (`from`: `terminal` or `desktop`, `version`), `update_replaced` (`version`), `update_failed` (`code`) *(chosen here)*. No archive path, URL query or signature is logged.

### Assumptions

- **SA-SRV-UP-01** The first line of `/proc/self/cgroup` names the unit of the process on the cgroup v2 hierarchy that systemd 256 uses (C-59: company distributions ship systemd 256 or later).
- **SA-SRV-UP-02** `systemctl --user --no-block restart` called from the unit's own main process queues the restart and returns before the stop begins (C-100).

### Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (knowledge inherits its readers; per-person checks at question time against GitHub, GitLab, Confluence and SharePoint; no stored reader list; a person's own ordin links their accounts). The modules: `components/SourceAccess.md` (new), `components/KnowledgeQueryServer.md`, `components/KnowledgeReader.md`, `components/KnowledgeQueryClient.md`, `components/AgentConfiguration.md`, `components/CallerApi.md`, `components/CallerAuthentication.md` and `components/IdentityAndCredentials.md` (each amendment January 2026 — Per-document read access).

### Personal profile

The personal server asks and never answers (unchanged); it constructs no Source Access. Row 5.9a is unchanged: the client links the owner's accounts itself (KnowledgeQueryClient.md amendment January 2026, PD-A). One addition, as on the desktop (OrdinDesktop.md amendment January 2026): when the program builds the Microsoft scope set for `identity.sign_in` (row 5.40, Remote API path), it adds every scope `KnowledgeQueryClient::microsoft_check_scopes()` returns (SA-SRV-PD-02). Likewise, wherever the personal server builds a Confluence scope set for `identity.sign_in`, it adds `read:me` only while a live connection's `about.accounts` lists Confluence, as the desktop does (IdentityAndCredentials.md SA-ID-PD-02); today Confluence sign-in is not offered on the server (C-49, Atlassian has no device grant), so the rule takes effect only if that changes.

### Agent profile

With a `knowledge-agent` section (`ka`), step 6 of the January 2026 amendment gains:

1. **Every role, before the Knowledge Query Client:** Source Access — `SourceAccess::new(SourceAccessDeps { journal, telemetry, http, settings })` (SourceAccess.md), where the program builds `settings: SourceAccessSettings` from `ka`:
   - `git`: one `GitUnitSettings` per `ka.repositories[]` entry with `audience-may-read: false` — `unit` = its repository key without `@branch` and `:path`, `host_kind` from `host-kind`, `host` and `owner_path` from its URL, `token` = the value of its `credential-env`, `check_every_question` from the entry;
   - `confluence`: from `ka.confluence` (`site`, `person-checks`, `spaces[]`), with `account_email` and `api_token` the values of `account-email-env` and `token-env`;
   - `sharepoint`: from `ka.sharepoint` (`tenant-id`, `client-id`, `libraries[]`), with `client_secret` and `read_refresh_token` the values of `client-secret-env` and `read-token-env`;
   - `remember_seconds` from `ka.access-checks.remember-seconds` (default 60);
   - `github_notice_secret`, `gitlab_notice_token` from `ka.notices.github-secret-env` and `ka.notices.gitlab-token-env`, when set.
   Every value is resolved as 2D-1d resolves the variables the agent configuration names (single shape: the server's secret store under `connection-env/<name in lower case>`; scaled shape: `ORDIN_SECRETS_DIR/<name>`) — the same rule as the Knowledge Reader's `CredentialSource`. A variable named but not resolved stops start with `SERVER_START_FAILED` naming the variable, never its value. Source Access is constructed in every role: `requests` checks, links and receives notices; `runs` reads Confluence and SharePoint. `migrations()` (module `source_access`, table `src_notice_seen`) is registered at step 4 in every role, beside the `kq_*` and `kr_*` tables. A configuration with no per-person source still constructs it (SA-SRV-PD-01).
2. **`requests`:** `KnowledgeServerDeps` gains `source_access: <the Source Access of item 1>`. The Caller API's `build_router` merges the server's router with its added routes (`knowledge.link_accounts`, `knowledge.unlink_account`, `knowledge.notice_github`, `knowledge.notice_gitlab`, and route 45 `POST /operator/knowledge/forget-checks` used by `knowledge forget-checks`; route numbers and least roles as CallerAuthentication.md amendment January 2026 lists them).
3. **`runs`:** `KnowledgeReader::new(…)` gains `source_access` (KnowledgeReader.md amendment January 2026), so the reader lists and fetches Confluence spaces and SharePoint libraries through it.
4. **`chat`, `jobs`:** nothing added.

In the single shape the roles are one process and share one Source Access and its short memory. In the scaled shape each `requests` instance keeps its own short memory, at most `remember-seconds` old; a sharing-change notice reaches the instance that receives it, and the other instances' memories end at their own expiry (SA-SRV-PD-03).

The agent's SharePoint refresh token rotates: Source Access keeps the newest one in memory and uses the configured value only at start (SourceAccess.md, SharePoint reading step 1). The program does not write a rotated token back.

**Readiness (step 6.6):** unchanged; Source Access has no readiness of its own (an unreachable document system makes checks `Unchecked`, not the instance unready).

**Orderly stop (step 11.2):** the program calls `source_access.forget_all()` after the Caller API stops accepting requests; checks in flight end at their own deadline.

**Start-up report:** this program owns the knowledge-agent lines (as in the January 2026 amendment; the Agent Loader's L-22 report is not changed). A knowledge agent's report adds, after its repositories:

- "Per-person sources: <n> repositories, <n> Confluence spaces, <n> SharePoint libraries" — the counts of repositories and spaces with `audience-may-read: false` and of every library (AgentConfiguration.md amendment January 2026, *Observability*; `ReaderStatus.per_person_sources`);
- "Confluence pages left out: <n>" — `ReaderStatus.pages_left_out` from the reader's last status, or "not read yet" before its first cycle (KnowledgeReader.md amendment January 2026); also shown in the operator knowledge status;
- per repository with `audience-may-read: false`: "checked per person on <host-kind>", and "checked on every question" when `check-every-question` is set;
- `confluence`: the site, each space key with its `audience-may-read` value, and "person checks: on" or "person checks: off — restricted pages are not read";
- `sharepoint`: the site and library of each entry, "checked per person", and whether `read-token-env` resolved;
- `access-checks.remember-seconds`;
- "GitHub notices: on/off", "GitLab notices: on/off".

Never a token, a secret value, an account or a person.

### Commands

Two operator commands are added, owned by this program. They sit beside `ordin-server agent knowledge fetch`, under their own word `knowledge`:

| Command | Arguments | Group | Carried out by |
|---|---|---|---|
| `ordin-server knowledge sign-in sharepoint` | `--config FILE [--print]` | L | this program (device code flow below) |
| `ordin-server knowledge forget-checks` | none | S | Caller API operator client → route 45 `POST /operator/knowledge/forget-checks` (least role operator) on the running instance → `SourceAccess::forget_all` |

**`knowledge sign-in sharepoint`** signs the **agent's own** Microsoft account in with the device code flow: `POST https://login.microsoftonline.com/<ka.sharepoint.tenant-id>/oauth2/v2.0/devicecode` with `client_id = ka.sharepoint.client-id` and `scope = https://graph.microsoft.com/Files.Read.All offline_access` (delegated; SA-SRV-PD-04); it prints the verification address and the user code, polls the token endpoint at the interval Microsoft gives for at most 15 minutes, then:

- **single shape:** stores the refresh token with `identity.put_server_secret` under `connection-env/<ka.sharepoint.read-token-env in lower case>` — the name settings resolution reads — and prints "Saved. A running server reads it at its next start." It never prints the token.
- **scaled shape** (`ORDIN_SECRETS_DIR` set): refuses with `SERVER_SECRET_NOT_STORABLE` unless `--print` is given; with `--print` it writes the refresh token once to standard output, followed by "Place this value in the secret named <variable> for every instance. ordin has not kept it."

ordin never writes the refresh token to disk in plain text: in the single shape the server's secret store holds it (the session's Secret Service or the encrypted `0600` file, IdentityAndCredentials.md); in the scaled shape the operator places it in the instances' secret. It is never logged. Rejected: *a file beside the configuration* — a plain file is copied by every backup and outlives the agent's account.

Device-flow outcomes: `authorization_pending` → keep polling; `slow_down` → add 5 s to the interval; `expired_token` or 15 minutes passed → `SERVER_SIGN_IN_TIMED_OUT`; `access_denied` → `SERVER_SIGN_IN_REFUSED`; any other error or no `refresh_token` in the answer → `SERVER_SIGN_IN_REFUSED`.

**`knowledge forget-checks`** asks the running agent to drop every remembered check answer at once (for example after an owner removed a group's access and will not wait `remember-seconds`). It prints "Forgotten." on 200. In the scaled shape it reaches the instance the operator address leads to; the others' memories end within `remember-seconds` (printed as a second line, "Other instances forget within <n> seconds.").

`ordin-server agent check` also checks the new keys through the Agent Configuration's rules (AgentConfiguration.md amendment January 2026).

### Assertions

- **SA-SRV-PD-01** *(chosen here)* Source Access is constructed whenever `ka` is present, so the Knowledge Query Server always has one; a check of an audience-open document makes no outbound call.
- **SA-SRV-PD-02** *(chosen here)* The program, not the screens, decides the Microsoft scope set for a sign-in: the SharePoint Adapter's scopes plus every live connection's check scope, so the person consents once on Microsoft's page and no screen can ask for an arbitrary scope.
- **SA-SRV-PD-03** *(chosen here)* The short memory is per instance and in process. A notice received by one `requests` instance does not reach the others; their memories end within `remember-seconds` (default 60 s), which bounds the gap.
- **SA-SRV-PD-04** *(chosen here)* The agent's own SharePoint reading uses a delegated account that owners share libraries with, as with a colleague, rather than an application permission, so no administrator consent is needed; `Files.Read.All` delegated reads only what that account was given. Whether the tenant allows a user-consented app registration is C-104.

### Error Table (rows added)

| Code | When | Action |
|---|---|---|
| `SERVER_SECRET_NOT_STORABLE` | `knowledge sign-in sharepoint` in the scaled shape without `--print` | exit 2: "This server's secrets are placed by its operator. Run again with --print and place the value yourself." |
| `SERVER_KNOWLEDGE_NOT_CONFIGURED` | `knowledge sign-in sharepoint` with no `knowledge-agent.sharepoint` section or no `read-token-env`; `knowledge forget-checks` against an agent with no `knowledge-agent` section | exit 2 naming the missing key |
| `SERVER_SIGN_IN_TIMED_OUT` | the device code expired or 15 minutes passed | exit 1: "The sign-in was not completed in time. Run the command again." |
| `SERVER_SIGN_IN_REFUSED` | Microsoft refused the sign-in or returned no refresh token | exit 1: "Microsoft did not sign the knowledge agent in. Check the app registration and try again." |

Source Access's own start errors (`SACC_SYSTEM_NOT_CONFIGURED`) stop start with `SERVER_START_FAILED` naming `ordin-source-access` and the code, as every module's do (step 6).

*Drafter note:* none of the four codes appears in the current OrdinServer.md Error Table; they are new under the `SERVER_` prefix this program owns.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Wiring per-document read access (ADR-115)

  Scenario: Wiring — a knowledge agent constructs Source Access in every role
    Given the agent configuration has a knowledge-agent section with one repository with audience-may-read false and host-kind github
    When the server starts in the agent profile, scaled shape, in the role requests
    Then Source Access is constructed before the Knowledge Query Client with one GitUnitSettings
    And the Knowledge Query Server is given that Source Access
    And the Caller API serves POST /v1/knowledge/accounts

  Scenario: Error — a named secret is not resolved
    Given a confluence section whose token-env "CONF_TOKEN" has no value
    When the server starts
    Then start stops with SERVER_START_FAILED naming "CONF_TOKEN" and no value is printed

  Scenario: Happy path — the agent's SharePoint sign-in in the single shape
    Given a knowledge-agent section with sharepoint read-token-env "SP_READ_TOKEN"
    When the operator runs "ordin-server knowledge sign-in sharepoint --config agent.yaml" and completes the device sign-in
    Then the refresh token is stored with put_server_secret under "connection-env/sp_read_token"
    And standard output contains "Saved." and no token

  Scenario: Error — the scaled shape without --print
    Given ORDIN_SECRETS_DIR is set
    When the operator runs "ordin-server knowledge sign-in sharepoint --config agent.yaml"
    Then the command exits 2 with SERVER_SECRET_NOT_STORABLE and no device sign-in starts

  Scenario: Error — the device code expires
    Given the operator does not complete the device sign-in within 15 minutes
    Then the command exits 1 with SERVER_SIGN_IN_TIMED_OUT and nothing is stored

  Scenario: Happy path — forget-checks empties the short memory
    Given a running knowledge agent whose short memory holds 12 answers
    When the operator runs "ordin-server knowledge forget-checks"
    Then SourceAccess.forget_all is called on that instance and the command prints "Forgotten."

  Scenario: Security — the start-up report names no secret
    Given a knowledge-agent section with confluence person-checks false and two spaces
    When the server starts
    Then the start-up report lists the site, both space keys and "person checks: off — restricted pages are not read"
    And it contains no token, account or e-mail address
```
