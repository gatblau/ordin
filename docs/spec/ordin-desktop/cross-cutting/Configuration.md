# SPEC: Configuration

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** `ordin-organisation-profile` (`OrganisationProfileService`, `ProfileSnapshot`, `InstallerSettings`) on the desktop and the personal server; `ordin-agent-configuration` (`load_config`) and `apps/server/src/env.rs` (`ServerEnv::read`) on the agent profile; `ordin-identity` (`server_secret_backend`, `SecretBackendConfig`) for secrets in every program; clamping of the person's choices in the module that owns each setting (Usage Planner for `usage.*`, Journal for `history.*`, Localisation for the interface language).

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Applicability by program](#applicability-by-program)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Keys introduced in Phase 3](#keys-introduced-in-phase-3)
- [Defects found while compiling this spec](#defects-found-while-compiling-this-spec)
- [Gaps](#gaps)

## Purpose

States, once for the whole spec set, where every setting of the three ordin programs comes from, in which order the sources are read, what is checked before a program starts serving, and where secrets live. It carries the complete Phase 2D table of environment variables, organisation profile keys, installer-carried settings, person and server-owner settings and fixed constants, and adds the keys the Phase 3 component specs introduced after 2D was drafted, so an implementer of any module finds the type, default, whether it is required and its owner in one place (07 — *Configuration*, amended in revision 0.48.0; SA-XC-06).

## Approach

Configuration is not a module of its own: each source has exactly one reader, already specified in Phase 3, and this spec fixes the contract between them. The organisation profile is read and verified by the Organisation Profile crate and published as an immutable `ProfileSnapshot` behind a `tokio::sync::watch` channel; each module reads the keys it owns from the current snapshot at the moment of use and clamps the person's stored value to it. The agent profile reads only the agent configuration file and environment variables, with no key in both (SA-CFG-02, carried from wrightrun). Rejected: a central `Config` struct loaded once and passed to every module, because it would make every module depend on every section and break the 2B-3 dependency list; and environment variables overriding profile values, because a person could then exceed a company ceiling (SA-CFG-01 *Impact if wrong*).

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | Four sources, later wins where a key has more than one level: fixed constant (2D-4) → organisation profile value or ceiling (2D-2) → the person's choice in Settings (2D-3), clamped to the company maximum and then the application maximum. Installer-carried settings (2D-2b) only locate the profile and the update feed. No environment variable of its own. | SA-CFG-01, SA-XC-06; 2D *Configuration sources and precedence* |
| Personal server (P) | Yes | The desktop chain, plus the server owner's setup-command inputs (each stored by the module that owns it — the profile address by the Organisation Profile, the listening address by the Remote API, tokens and keys in the server's secret store through Identity and Credentials; OrdinServer.md Internal Logic 9) at level 2½ — below the profile's ceilings, above the person's settings — and the environment variables of 2D-1a that state the data root, the secrets key and the log level. These groups share no key. | SA-CFG-01, SA-XC-06; 2D-1a, 2D-3 |
| Agent profile (A) | Yes | Exactly two sources with no precedence between them: the agent configuration file inside the agent release (every per-agent choice) and environment variables (secrets as file paths, the release reference, the pod's facts). No organisation profile is read; `OrganisationProfileService` is started with `ProfileMode::NotUsed` (every key at its default). A `WRIGHTRUN_*` variable is ignored and named in the start-up report (SA-CFG-02 *Impact if wrong*). *Amended January 2026 (ADR-113): a knowledge agent is an agent-profile server whose agent configuration has the optional `knowledge-agent` section; its repositories, glossaries, audiences, front doors, proxies, service identity, reading budget, fetch interval, hot copy, MCP endpoint switch and sources record are keys of that section, read by the Agent Configuration component, never environment variables.* | SA-CFG-02, SA-XC-06; 2D-1a, 2D-1d |

## Shared Context

Types copied from the owning specs (do not redefine them; they are listed so this spec can be implemented from this section alone).

```rust
// ordin-organisation-profile (OrganisationProfile.md, Public Interface)
pub enum ProfileMode {
    /// Desktop and personal server.
    Read {
        /// The SQLite pool of the Journal's `Datastore`, handed over by the program (D4, R20).
        pool: sqlx::SqlitePool,
        data_root: std::path::PathBuf,
        http: reqwest::Client,
        /// Values a branded build carries (ADR-041); used when installer.json is absent.
        build_defaults: InstallerSettings,
    },
    /// Agent profile: no fetch, no tables, defaults only.
    NotUsed,
}
impl OrganisationProfileService {
    pub async fn start(mode: ProfileMode, options: ProfileOptions) -> Result<OrganisationProfileService, ProfileError>;
    pub fn current(&self) -> std::sync::Arc<ProfileSnapshot>;
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<ProfileSnapshot>>;
    pub async fn refresh(&self) -> Result<RefreshOutcome, ProfileError>;
    pub fn status(&self) -> ProfileStatus;
    pub fn installer_settings(&self) -> InstallerSettings;
    pub async fn set_profile_address(&self, url: &str, signing_keys: Vec<String>) -> Result<(), ProfileError>;
}
impl ProfileSnapshot { pub fn defaults() -> ProfileSnapshot; }

// ordin-agent-configuration (AgentConfiguration.md)
pub fn load_config(path: &std::path::Path) -> Result<AgentConfig, ConfigError>;
pub fn load_config_str(text: &str, label: &str) -> Result<AgentConfig, ConfigError>;

// apps/server/src/env.rs (OrdinServer.md)
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

// ordin-identity (IdentityAndCredentials.md)
pub enum KeySource { KeyFile(std::path::PathBuf), SystemdCredential(std::path::PathBuf) }
pub fn server_secret_backend(
    dbus_session_bus_address: Option<&str>,
    secrets_key_file: Option<&std::path::Path>,
    secrets_key_credential: Option<&std::path::Path>,
) -> Result<SecretBackendConfig, CredError>;
pub enum SecretBackendKind { OsKeychain, SecretService, EncryptedFile, None }
```

**Required column.** Every 2D table carries a *Required* column. 2D-2: only `format_version` and `profile_version` are required, and two keys are required when another key is set (`email.company_domains[]`, `server.client_certificate_name`); every other key takes its default, because a desktop or personal server may run with no profile. 2D-2b: the three installer keys and the compiled-in update key are required. 2D-3: the server profile, and in the agent profile the release and the model key, are required; the person's own choices each have a default. 2D-4: n/a, a constant is compiled in.

### Phase 2D, copied

The tables below are copied from `docs/spec/ordin-desktop/phase-2-architecture.md` §2D (from *Configuration sources and precedence* to the end of 2D-4) without change of content; the two copies are identical. Rows added to 2D since Phase 4 are marked *added in Phase 4* (the three `ORDIN_MCP_*` rows, finding C-5, and the check-commands row in 2D-3); the 2D-1c rows for `ANTHROPIC_BASE_URL`, `GH_TOKEN`, `GH_PROMPT_DISABLED`, `NO_COLOR`, `GITLAB_TOKEN` and `GITLAB_HOST`, the 2D-1g build-time table (`GITHUB_APP_CLIENT_ID`) and the *Required* columns of 2D-2 to 2D-4 were added in the Phase 6 Plan round; the 2D-1c rows for `CLAUDE_CONFIG_DIR` and the pass-through allow list were added in the Phase 6 re-audit (January 2026). The *Names chosen here*, *HLD inconsistencies* and *Former gaps, now settled* lists of 2D stay in 2D.

#### Configuration sources and precedence

**The rule that removes most precedence questions.** No setting exists in two sources *(chosen here; carried from wrightrun's EnvironmentConfiguration spec, `agent-wright-runtime/docs/spec/cross-cutting/EnvironmentConfiguration.md:11`)*. The organisation profile holds company policy; the installer holds the two addresses needed before the profile can be read; Settings and the setup command hold the person's or the server owner's choices; environment variables hold facts of the running program and its deployment (where data lives, where a secret file is, a port, a log level). Where a value has a default, a company default and a person's choice, the chain below applies; nothing else overrides anything.

**Desktop.**
1. Fixed constant in code (2D-4), including every *application maximum*.
2. Organisation profile value (2D-2): a company default, a company maximum, or a lock ("Set by your company", ADR-062). A company maximum can only lower an application maximum (07 — *Crew guardrail defaults*, `07-cross-cutting.md:44`).
3. The person's choice in Settings (2D-3), accepted only where `usage.person_may_change` (or the equivalent switch) lets the person change it, and clamped to the company maximum, then to the application maximum.
4. A profile that fails signature verification is ignored and the last verified profile is kept (`02-architecture.md:702`). A newer verified profile that lowers a maximum lowers the person's stored value at the next use, and a locked setting reverts to the company default (`04-domain.md:139`, for the GitHub approval ceiling).
5. The installer-carried values (2D-2b) are read only to find the profile and the update feed; once a verified profile names `updates.feed_url`, that address replaces the installer's.
6. The desktop reads **no environment variable of its own** *(chosen here)*. It reads standard platform state only through the operating system (proxy settings and trusted certificates, A-39), and it removes credential variables from the environment of each harness it starts (2D-1c).

**Personal-profile server.** The desktop chain applies to every setting it shares with the desktop, with two additions: the server owner's setup-command inputs (2D-3, stored under the application data root or in the server's secret store), and environment variables (2D-1a) that state where the data root and the secrets key are and the log level. These three groups have no key in common, so none overrides another. Where no organisation profile exists, A-40 lets the server owner choose the chat channels (`10-risks.md:51`); every other profile key then takes its default.

**Agent-profile server (single and scaled shapes).** Per-agent choices come only from the agent configuration file in the agent release (`08-interfaces.md:40`, ADR-098 item 2), validated by the Agent Configuration component; values above an application maximum are refused there, not clamped (`02-architecture.md:775`). Deployment facts and the paths of secret files come from environment variables (2D-1a), set by the systemd unit or the container in the single shape and by the Helm values in the scaled shape. Credentials named by the configuration are resolved from the server's secret store (single shape) or a mounted secrets folder (scaled shape), never from the configuration file and never from ordin's own environment *(chosen here)*. **The agent profile reads no organisation profile** *(chosen here)*: ADR-097 and ADR-098 give the hosted agent the agent configuration as its only policy source, and no HLD text names a profile key the agent profile consumes (see *Former gaps, now settled*, item 1).

#### 2D-1 Environment variables

#### 2D-1a Variables of ordin's own

All are read once at start, before the program serves anything; a change reaches a running instance only by a restart *(chosen here; wrightrun's rule, EnvironmentConfiguration step 10)*. A missing or malformed required variable stops the program with a named error, and the instance never reports ready. No variable holds a secret value: a secret is reached through a file path, a systemd credential or the secret store.

| Variable | Type | Default | Required | Program(s) | Owner Component | Description | HLD trace |
|---|---|---|---|---|---|---|---|
| `ORDIN_DATA_DIR` | path | `$XDG_DATA_HOME/ordin`, else `$HOME/.local/share/ordin`; the container image sets `/var/lib/ordin` *(chosen here)* | No | Server: personal profile, agent single shape | ordin server | The application data root: SQLite journal, memory store, encrypted secrets file, clones, logs. Must be owned by the running user; ordin refuses to start on a root it cannot write. | ADR-072, `02-architecture.md:570`, `05-tech-stack.md:35` |
| `ORDIN_SECRETS_KEY_FILE` | path | none | Yes, when no Secret Service runs in the user's session and `ORDIN_SECRETS_KEY_CREDENTIAL` is unset | Server: personal profile, agent single shape (container) | Identity and Credentials | Path of the container runtime's secret holding the key that decrypts the `0600` secrets file under `ORDIN_DATA_DIR`. The value never enters the environment. Setting both this and `ORDIN_SECRETS_KEY_CREDENTIAL` stops the start with a named error. | C-50 (`10-risks.md:184`), `02-architecture.md:570`, `05-tech-stack.md:23`, ADR-073 *Secrets on the server* |
| `ORDIN_SECRETS_KEY_CREDENTIAL` | path | none | Yes, when no Secret Service runs and `ORDIN_SECRETS_KEY_FILE` is unset | Server: personal profile, agent single shape (archive with systemd 256 or later) | Identity and Credentials | Path of the key encrypted with `systemd-creds encrypt --user`. ordin runs `systemd-creds decrypt --user` on it at start, because `LoadCredentialEncrypted=` fails in a user unit (lab result in C-50), and keeps the decrypted key in memory only. | C-50, OQ-44, ADR-073 *Status* (decision 53) |
| `ORDIN_LOG_LEVEL` | enum `error`, `warn`, `info`, `debug` | `info` | No | Server: every profile and shape | ordin server | Level of the JSON log lines. Any other value stops the start with a named error. Also given in-process to the embedded memory and sandbox components (2D-1e, 2D-1f). | `07-cross-cutting.md:14`, `05-tech-stack.md:46` |
| `ORDIN_RELEASE` | string `<registry>/<repository>@sha256:<hex>` | none | Yes, for the obtain-a-release sub-command | Server: agent profile | Agent Loader | The agent release a deployment names. The start command does not read it; it reads the reference and digest the obtain step wrote beside the release. | `08-interfaces.md:42`, `:47`, ADR-098 item 2; `WRIGHTRUN_RELEASE` |
| `ORDIN_REGISTRY_AUTH_FILE` | path | none | No; without it the registry is read with no credential | Server: agent profile (obtain, publish) | Agent Loader, Release Builder | Credentials file for the container registry. Never printed or logged. | `08-interfaces.md:87`; `WRIGHTRUN_REGISTRY_AUTH_FILE` |
| `ORDIN_RELEASE_DIR` | path | none | Yes, for the agent-profile start, obtain and run-one-job sub-commands | Server: agent profile | Agent Loader | Folder holding the obtained release; the bundle is read-only there. | ADR-098 item 3 *Work folder*; `WRIGHTRUN_RELEASE_DIR` |
| `ORDIN_DATABASE_URL_FILE` | path | none | Yes, in the scaled shape for start, the schema sub-command and run-one-job; absent means the SQLite datastore under `ORDIN_DATA_DIR` | Server: agent scaled shape | ordin server | Path of a file (a Kubernetes Secret mounted as a volume) holding the PostgreSQL connection string. The string is never printed, logged or placed in an error. | `05-tech-stack.md:35`, ADR-098 item 5, `08-interfaces.md:88`; `WRIGHTRUN_DATABASE_URL` (moved from a value to a file path) |
| `ORDIN_PORT` | integer 1024–65535 | `8080` | No | Server: agent profile | Caller API | The one port an instance listens on for the caller, intake, operator, work-folder, health and metrics routes. The personal profile does not read it: the Remote API's address and port are setup inputs (2D-3). | `07-cross-cutting.md:23`, `08-interfaces.md:43`–`44`; `WRIGHTRUN_PORT` |
| `ORDIN_RUN_CONCURRENCY` | integer 1–100 | `10` in an instance started in the runs role; `8` in the single shape *(chosen here)* | No | Server: agent profile | Session Supervisor | Most runs (harness programs) one instance executes at once. | A-48, A-49 (`10-risks.md:59`–`60`), `07-cross-cutting.md:43`; `WRIGHTRUN_RUN_CONCURRENCY` |
| `ORDIN_IMAGE` | string, at most 512 characters printed | none | No | Server: agent profile | Slack Channel Adapter | The server image reference, set by the Helm chart, printed by the chat's version command; absent, the command prints "not stated". | `02-architecture.md:610`; `WRIGHTRUN_IMAGE` |
| `ORDIN_OTLP_ENDPOINT` | URL (`https://`, or `http://` to loopback) | none | No | Server: agent profile | Telemetry | Address of the organisation's OpenTelemetry collector (OTLP over HTTP). Traces are sent only when the agent configuration switches tracing on **and** this is set; switched on with this unset, the start-up report says traces are not sent. ordin configures its exporter from this variable only and does not let the OpenTelemetry library read `OTEL_*` variables. | `07-cross-cutting.md:16`, `05-tech-stack.md:46`; wrightrun ADR-076 ("the address of the collector … is an environment variable") |
| `ORDIN_MODEL_KEY_FILE` | path | none | Yes, in the scaled shape for instances serving runs, the check and run-once; in the single shape only when the key is not in the secret store | Server: agent profile | Model Policy | Path of a file holding the organisation's model key. ordin reads it and gives the key to the Claude Code program alone (2D-1c); no hook, MCP server, sandbox or tool receives it. | ADR-097 item 2, C-74 (`10-risks.md:208`) *Amended January 2026 (ADR-107):* required as stated only when the configured provider is the default; for any other provider it is optional (1A SA-HOST-02). |
| `ORDIN_SECRETS_DIR` | path | none | Yes, in the scaled shape when the agent configuration names any credential variable | Server: agent scaled shape | Connection Binder | Folder where each file's name is a variable name the agent configuration lists and its content the value (the Kubernetes Secret mounted as a volume). A name the configuration lists with no file stops the instance with a named error naming the variable. The same Secret also holds two files that are not configuration variables: `ORDIN_DATABASE_URL` (the PostgreSQL connection string, read through `ORDIN_DATABASE_URL_FILE`) and `ORDIN_MODEL_KEY` (the organisation's model key, read through `ORDIN_MODEL_KEY_FILE`); the Helm chart mounts the Secret at `/var/run/ordin/secrets` and sets the three paths (OrdinServer.md *Helm values*, `secretName`). | `02-architecture.md:775` ("a credential value where a variable name is expected" is an error), `08-interfaces.md:40` |
| `ORDIN_SERVER_URL` | URL (`https://`, or `http://` for `localhost` and `127.0.0.1`) | none | Yes, for the operator sub-commands that call a running hosted agent | Server command (operator's machine) | ordin server | Base address of the hosted agent's routes. | `08-interfaces.md:47`; `WRIGHTRUN_SERVER_URL` |
| `ORDIN_TOKEN_FILE` | path | none | Yes, for the operator sub-commands that call a running hosted agent | Server command (operator's machine) | ordin server | Path of a file holding an identity-provider token with the operator role. Never printed or logged. | ADR-098 item 6; `WRIGHTRUN_TOKEN` (moved from a value to a file path) |

#### 2D-1b Standard variables ordin reads (names kept verbatim)

| Variable | Type | Default | Required | Program(s) | Owner Component | Description | HLD trace |
|---|---|---|---|---|---|---|---|
| `XDG_DATA_HOME`, `HOME` | path | platform | No | Server | ordin server | Used only to derive the default of `ORDIN_DATA_DIR`. ordin never reads Claude Code's `~/.claude/.credentials.json`. | ADR-073 *Signing in to the harness* |
| `DBUS_SESSION_BUS_ADDRESS` | string | none | No | Server: personal profile, agent single shape | Identity and Credentials | Presence of a session bus is how ordin finds a running Secret Service; absent, it uses the encrypted file and requires one of the two key variables *(chosen here)*. | `02-architecture.md:570`, C-50 |
| `NOTIFY_SOCKET`, `WATCHDOG_USEC` | string; integer microseconds | none | No | Server, archive under the systemd user unit | Always on | Set by systemd. When present, ordin reports ready and signals the watchdog every 30 seconds. The shipped unit sets `WatchdogSec=90s` *(chosen here)*, so one missed signal does not restart ordin. Whether the watchdog works in a user unit is C-57. | `02-architecture.md:626`, ADR-076, C-57 |
| `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` (either case) | URL; host list | none | No | Server | ordin server | The server's form of "the computer's own proxy settings" for every outbound HTTPS call. Not applied to NATS, which is reached directly. | A-39, A-41 (`10-risks.md:50`, `:52`), ADR-074 *Status* |
| `TZ` | IANA zone name | the system zone | No | Server | Trigger Service, Always on | The server's time zone, used for the daily triggered-run reset at midnight and the weekly journal compaction at 04:00. In a container without `TZ` this is UTC. | ADR-074 *Limits*, ADR-076 *Staying light* |
| `RUST_LOG` | string | none | — | — | — | **Not read.** The memory crate's `init_logging` reads it through `EnvFilter::try_from_default_env` (`memory/src/obs/log.rs:28`); ordin installs its own subscriber from `ORDIN_LOG_LEVEL` and never calls that function. | `07-cross-cutting.md:14` |
| `CREDENTIALS_DIRECTORY` | path | none | — | — | — | **Not read.** A user unit's `LoadCredentialEncrypted=` fails, so ordin decrypts the credential itself (`ORDIN_SECRETS_KEY_CREDENTIAL`). | C-50 lab result |

#### 2D-1c Variables ordin sets or removes in a harness's environment

ordin builds each harness child's environment explicitly; it never lets a child inherit ordin's own environment wholesale *(chosen here)*.

| Variable | Type | Default | Required | Program(s) | Owner Component | Description | HLD trace |
|---|---|---|---|---|---|---|---|
| `ANTHROPIC_API_KEY` | string (secret) | — | Agent profile: set for Claude Code only | Desktop, personal server: **removed**; agent profile: **set** | Claude Local Adapter | Desktop and personal server: removed from every Claude Code session, with "the other credential variables", because in `-p` mode a key in the environment wins over the person's sign-in. Agent profile: set from `ORDIN_MODEL_KEY_FILE` or the secret store in the Claude Code child only, unless C-74 finds that its hooks, stdio MCP servers or tools would inherit it, in which case the key goes through Claude Code's own key setting instead. Removed from every other child in every program. | `09-decisions.md:826` (ADR-044), ADR-097 item 2, C-74 *Amended January 2026 (ADR-107):* set only when the policy holds a key; for a provider other than the default the key is optional and, with none, the variable is not set (ModelPolicy.md MP-3.3; 1A SA-HOST-02). |
| `CLAUDE_CODE_OAUTH_TOKEN` | string (secret) | — | Never set | Desktop, server | Claude Local Adapter | Removed from every Claude Code session; `claude setup-token` is not used. | `09-decisions.md:1704` (ADR-073) |
| `CLAUDE_CONFIG_DIR` | path | — | Never set | Desktop, server | Claude Local Adapter | Removed from every Claude Code session, with `ANTHROPIC_API_KEY` (other than in the agent profile) and `CLAUDE_CODE_OAUTH_TOKEN`; the child environment is built from an empty map. | ClaudeLocalAdapter.md Internal Logic `start` step 8, `09-decisions.md:826` (ADR-044) |
| `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_ALL`, `TMPDIR`, `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY`; on Windows also `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `SystemRoot`, `TEMP` | inherited string (copied unchanged) | ordin's own value; absent when ordin's environment does not hold it | No: copied when present | Desktop, server | Claude Local Adapter; Copilot Local Adapter | The pass-through allow list: the only variables copied from ordin's own environment into a Claude Code or Copilot CLI child; every other variable of ordin's environment is left out. The Claude Local Adapter copies the three proxy variables in either case. | ClaudeLocalAdapter.md Internal Logic `start` step 8; CopilotLocalAdapter.md Internal Logic `start` step 4 |
| `COPILOT_GITHUB_TOKEN`, `GH_TOKEN`, `GITHUB_TOKEN` | string (secret) | — | Never set for the Copilot CLI | Desktop, server | Copilot Local Adapter | Removed from the Copilot CLI's environment; the CLI uses its own sign-in. | `09-decisions.md:1694` (ADR-073 *Status*) |
| `GITHUB_APM_PAT` | string (secret) | — | — | — | Package Catalogue | Superseded with `apm` (ADR-082); never set. | `08-interfaces.md:78`, `09-decisions.md:243` |
| `ORDIN_HOOK_ENDPOINT` | absolute path | `<app data root>/run/hook.sock`; Windows `\\.\pipe\ordin-hook-<installation id>` | Yes, for every harness session | Desktop, server | Hook Relay | Where the ordin hook (and its MCP-bridge mode, SA-MCP-01) reaches the core. Defined in 2C wire types §4. | ADR-045; C-22 |
| `ORDIN_HOOK_SECRET` | string (secret, 43 characters base64url) | regenerated per session | Yes | Desktop, server | Hook Relay | Per-session secret that signs each hook message. Set only in the harness child's environment; never journalled or logged. The harness's own shell tool is switched off (02 — Claude Local Adapter), so no model-chosen command can read it. | ADR-045 |
| `ORDIN_SESSION_ID` | UUIDv7 string | per session | Yes | Desktop, server | Hook Relay | Which session a hook call belongs to. | ADR-045 |
| `ORDIN_HOOK_PROTOCOL` | integer | `1` | Yes | Desktop, server | Hook Relay | Hook protocol version (2C wire types §4). | ADR-045 |
| `ORDIN_MCP_ENDPOINT` *(added in Phase 4, C-5)* | absolute path | `<app data root>/run/mcp.sock` (mode `0600`); Windows `\\.\pipe\ordin-mcp-<installation id>` | Yes, for `ordin-hook mcp-bridge` | Desktop, server | MCP Front | Set in each MCP-bridge server's `env`: where the bridge reaches the MCP Front. On macOS at most 104 bytes. | McpFront.md *Bridge protocol*; OrdinHook.md Internal Logic G.1 |
| `ORDIN_MCP_SERVER` *(added in Phase 4, C-5)* | string `^[a-z][a-z0-9_]{0,63}$` | set per bridge | Yes, for the bridge | Desktop, server | MCP Front | The MCP server name this bridge instance serves. | McpFront.md, OrdinHook.md |
| `ORDIN_MCP_TOKEN` *(added in Phase 4, C-5)* | string (secret), 32 random bytes, base64url without padding (43 characters) | regenerated per session | Yes, for the bridge | Desktop, server | MCP Front | Per-session token the bridge presents in its handshake; set only in the bridge's environment, never journalled or logged. | McpFront.md, OrdinHook.md, ClaudeLocalAdapter.md, CopilotLocalAdapter.md |
| `ANTHROPIC_BASE_URL` *(conditional on C-77)* | URL | none | Agent profile: set only when the agent configuration gives `models.gateway_url`; never set otherwise | Agent profile: **set** in the Claude Code child; desktop, personal server: never set (the child environment is built from an empty map) | Claude Local Adapter | The organisation's model gateway, from `HarnessModelSettings.gateway_url`, given to the Claude Code child only. If C-77 finds Claude Code does not honour it, the adapter refuses a configuration that has a `gateway_url` (`CLA_CONFIG_INVALID`), so no model call bypasses the configured gateway. | ClaudeLocalAdapter.md SA-CLOSE-01, ModelPolicy.md MP-2, C-77 |
| `GH_TOKEN` | string (secret) | — | Yes, in every `gh` child | Desktop, personal server: **set** in each `gh` child; **removed** from check commands and from the Copilot CLI | Project Service | The GitHub token from Identity and Credentials (`access_token(CredentialKind::Github, None)`), set for the `github.gh` tool and the `gh api` calls of `GithubAccess`. Never journalled or logged. | ProjectService.md Internal Logic (`github.gh`, `GithubAccess`), C-10 |
| `GH_PROMPT_DISABLED` | string, value `1` | `1` | Yes, in every `gh` child | Desktop, personal server | Project Service | Stops `gh` from asking a question on its terminal, which no one would answer. | ProjectService.md Internal Logic (`github.gh`) |
| `NO_COLOR` | string, value `1` | `1` | Yes, in every `gh` and `glab` child | Desktop, personal server | Project Service | Plain output without colour codes, so the tool result is plain text. | ProjectService.md Internal Logic (`github.gh`, `gitlab.glab`) |
| `GITLAB_TOKEN` | string (secret) | — | Yes, in every `glab` child | Desktop, personal server: **set** in each `glab` child; **removed** from check commands | Project Service | The GitLab token from Identity and Credentials (`CredentialKind::Gitlab`). Never journalled or logged. | ProjectService.md Internal Logic (`gitlab.glab`, `GithubAccess`), C-30 |
| `GITLAB_HOST` | host name | the profile's `gitlab.host`, else `gitlab.com` | Yes, in every `glab` child | Desktop, personal server | Project Service | The GitLab host `glab` talks to. | ProjectService.md SA-CLOSE-01 (`gitlab.host`) |

The `gh` and `glab` rows cover the git-host programs the Project Service starts; they follow the same rule as a harness child (an environment built explicitly, never inherited wholesale). Provider settings for the agent profile: a gateway address is given as `ANTHROPIC_BASE_URL` (row above; conditional on C-77, ClaudeLocalAdapter.md SA-CLOSE-01). Any other provider's settings are the names and values the agent configuration declares under `model.environment`, set in the Claude Code child's environment only; ordin names no provider and holds no provider-specific code (*amended January 2026, ADR-107*; AgentConfiguration.md, ModelPolicy.md MP-2, ClaudeLocalAdapter.md step 8).

#### 2D-1d Variables the agent configuration names (agent profile)

The agent configuration names a variable, never a value (`08-interfaces.md:40`). The keys that hold a variable name are wrightrun's, read in `agent-wright-runtime/docs/spec/cross-cutting/EnvironmentConfiguration.md:26` and `agent-wright-runtime/docs/spec/phase-2-architecture.md:970`–`991`. In ordin the **value** of each named variable is resolved from the server's secret store (single shape, set through the setup command) or from `ORDIN_SECRETS_DIR/<NAME>` (scaled shape), and never from ordin's own environment *(chosen here)*. Every name must match `^[A-Z_][A-Z0-9_]*$` (wrightrun's `ENV_NAME`).

| Variable | Type | Default | Required | Program(s) | Owner Component | Description | HLD trace |
|---|---|---|---|---|---|---|---|
| Each name under `tools.mcp[].env`; each value of `tools.mcp[].headers`, `tools.http[].headers`, `tools.publish[].headers` | string (secret) | none | Yes, in instances serving runs, the check and run-once, when the connection lists it | Server: agent profile | Connection Binder | Credentials of one connection. An MCP server started as a command receives only the values its own connection lists. | `02-architecture.md:804`, ADR-098 item 2 |
| The name given by `tools.publish[].url-env` | URL (secret) | none | Yes, when the connection gives it | Server: agent profile | Connection Binder (Outbound Publisher) | Destination of an outbound connection whose address is a secret; never in an error. | `02-architecture.md:804` |
| The name given by `intakes[].secret-env` | string (secret), at least 32 characters | none | Yes, in instances serving requests, when the intake gives it | Server: agent profile | Caller API | Shared secret of one intake's signature. | `02-architecture.md:788`, ADR-098 item 6 *Intakes* |
| The names given by `chat[].bot-token-env`, `chat[].app-token-env` | string (secret) | none | Bot token: instances serving chat and jobs, and runs that use the channel-document tool; app-level token: instances serving chat only | Server: agent profile | Slack Channel Adapter | The hosted agent's Slack bot token and Socket Mode app-level token. | ADR-097 item 7, `02-architecture.md:610` |
| The names given by `knowledge-agent.repositories[].credential-env`, `knowledge-agent.glossaries[].credential-env` *(amended January 2026, ADR-113)* | string (secret) | none | Yes, in instances that run the Knowledge Reader (role `runs`), the check and run-once, for each repository and glossary listed | Server: agent profile | Knowledge Reader | Read-only Git credential for one repository or glossary; used for fetch only, never for a push; never in a log or error. *Amended January 2026 (ADR-115):* for a repository with `audience-may-read: false`, the same credential also asks GitHub or GitLab whether a person may read it (C-101, C-102). | ADR-113 |
| The name given by `knowledge-agent.service-identity.client-secret-env` *(amended January 2026, ADR-113)* | string (secret) | none | Yes, when `knowledge-agent.proxies[]` is non-empty, in instances that ask proxies | Server: agent profile | Knowledge Query Client | Client secret of the knowledge agent's own service identity, used to obtain the bearer token it presents to its proxies. | ADR-113 |
| The name given by `knowledge-agent.hot-copy.credential-env` *(amended January 2026, ADR-113)* | string (secret) | none | No; hot copies are off when the key is absent | Server: agent profile | Knowledge Reader | Read-only Git credential used for every hot copy a front door starts reading itself. | ADR-113 |
| The names given by `knowledge-agent.confluence.account-email-env`, `knowledge-agent.confluence.token-env` *(amended January 2026, ADR-115)* | string (secret) | none | Yes, when `knowledge-agent.confluence` is present, in every role | Server: agent profile | Source Access | The e-mail address and API token of the knowledge agent's own Atlassian account, for reading the configured spaces and, with `person-checks: true`, checking pages per person. | ADR-115 |
| The names given by `knowledge-agent.sharepoint.client-secret-env`, `knowledge-agent.sharepoint.read-token-env` *(amended January 2026, ADR-115)* | string (secret) | none | Yes, when `knowledge-agent.sharepoint` is present, in every role | Server: agent profile | Source Access | The secret of the agent's own Entra app (on-behalf-of exchange for per-person checks) and the refresh token of the agent's own Microsoft account (reading), made with `ordin-server knowledge sign-in sharepoint`. | ADR-115 |
| The names given by `knowledge-agent.notices.github-secret-env`, `knowledge-agent.notices.gitlab-token-env` *(amended January 2026, ADR-115)* | string (secret) | none | No; notices of that system are off when absent | Server: agent profile | Source Access | The secret GitHub signs webhook deliveries with, and the token GitLab sends in `X-Gitlab-Token`. | ADR-115 |
| Each name under `tools.connectors[].env` | — | — | — | — | Agent Configuration | wrightrun's Python connector plug-in contract is not kept (ADR-098 item 2); whether a configuration carrying this key is refused or reported as not carried is for the Agent Configuration component spec. | `08-interfaces.md:46` |

#### 2D-1e Embedded memory component (`memory/`) — variables it reads, and what ordin does

**Evidence.** `memory::config::Config::load()` reads every `MEMORY_*` variable through `std::env::var` with the order *variable > `MEMORY_CONFIG_FILE` file > default*, and fails on six required keys (`memory/src/config.rs:237`–`254`). Every field of `memory::config::Config` is `pub` (`memory/src/config.rs:97`–`171`), and `Store::connect(&Config)` takes the struct (`memory/src/store/mod.rs:54`). `memory::build_state` also builds the OIDC `Authenticator`, which "performs OIDC discovery + the first JWKS fetch and so fails fast if the IdP is unreachable" (`memory/src/lib.rs:40`–`43`).

**What ordin does, in every program** *(chosen here)*: ordin never calls `Config::load()`, `build_state` or `obs::log::init_logging`. The Memory Service builds a `memory::config::Config` value in Rust, so **no `MEMORY_*` variable is read or set**; a value present in the environment has no effect. That ordin can link the store, write pipeline and retrieval without the HTTP layer and the `Authenticator` is C-15 and stays unverified until its spike. The "In-process value" column below is what the Memory Service puts in the struct.

| Variable | Type | Default (crate) | Required (crate) | Program(s) | Owner Component | Description — ordin's treatment and in-process value | HLD trace |
|---|---|---|---|---|---|---|---|
| `MEMORY_CONFIG_FILE` | path | none | No | — | Memory Service | Not read (`load()` not called). | C-15 |
| `MEMORY_HTTP_ADDR` | socket address | `0.0.0.0:8080` | No | — | Memory Service | Unset. In-process `127.0.0.1:0`, never bound: the HTTP layer is not started. | C-15 |
| `MEMORY_MCP_HTTP_ADDR` | socket address | `0.0.0.0:8081` | No | — | Memory Service | Unset. In-process `127.0.0.1:0`, never bound: the MCP Front serves the memory tools (C-22). | C-15, C-22 |
| `MEMORY_MCP_PATH` | string | `/mcp` | No | — | Memory Service | Unset; crate default kept, unused. | C-22 |
| `MEMORY_STORE_PATH` | path | `./data/recall.db` | No | Desktop, personal server, agent single shape | Memory Service | Unset. *Amended January 2026 (decision 80; B8 Memory D-1, D-2):* in-process the memory tables live in the journal's datastore — its SQLite file on the desktop, the personal server and the single shape, PostgreSQL in the scaled shape (ADR-101); no separate memory store file. | ADR-030, ADR-101, `02-architecture.md:348` |
| `MEMORY_STORE_REMOTE_URL`, `MEMORY_STORE_REMOTE_USER`, `MEMORY_STORE_REMOTE_PASS` | URL; string; secret | none | No (user and password both or neither) | — | Memory Service | Unset; in-process `None`. The scaled shape's memory store is C-82, unresolved. | C-82 (`10-risks.md:216`) |
| `MEMORY_STORE_BACKEND` | enum `surrealkv`, `rocksdb` | `surrealkv` | No | — | Memory Service | Unset. *Amended January 2026 (decision 80; B8 Memory D-1):* no `surrealkv` or `rocksdb` store in-process; the memory tables live in the journal's SQLite file or PostgreSQL (ADR-101). | C-16, ADR-101 |
| `MEMORY_OIDC_ISSUER`, `MEMORY_OIDC_AUDIENCE` | string | none | **Yes** | — | Memory Service | Unset; in-process empty strings, never used because no `Authenticator` is built. | C-15 (`10-risks.md:149`) |
| `MEMORY_OIDC_SUBJECT_CLAIM`, `MEMORY_OIDC_TEAMS_CLAIM`, `MEMORY_OIDC_TENANT_CLAIM`, `MEMORY_OIDC_CLIENT_CLAIM`, `MEMORY_OIDC_AUTHORITY_CLAIM` | string | `sub`, `groups`, `tenant`, `azp`, `memory_fact_authority` | No | — | Memory Service | Unset; crate defaults kept, unused. The tenant, user and team come from the Memory Service's scope rule (one tenant, one user, one team per workspace; agent profile: one scope per channel, per direct chat and agent-wide). | `02-architecture.md:349`–`350` |
| `MEMORY_JWKS_REFRESH_SECS` | integer | `3600` | No | — | Memory Service | Unset; default kept, unused. | C-15 |
| `MEMORY_EMBED_URL`, `MEMORY_RERANK_URL` | URL | none | **Yes** | — | Memory Service | Unset. In-process: the loopback address of the core's model endpoint, or unused if C-18 settles on an in-process provider. | C-18 (`10-risks.md:151`), ADR-033 |
| `MEMORY_EMBED_API_KEY`, `MEMORY_RERANK_API_KEY` | secret | none | **Yes** | — | Memory Service | Unset. In-process: a random value made at each start and checked by the core's loopback endpoint, if C-18 settles on loopback *(chosen here)*. | C-18 |
| `MEMORY_EMBED_DIM` | integer > 0 | `1024` | No | — | Memory Service | Unset. In-process: the bundled embedding model's dimension (C-18). | C-18 |
| `MEMORY_EMBED_MODEL_VERSION` | string | `default` | No | — | Memory Service | Unset. In-process: the bundled model's identifier, so a model change is detectable. | ADR-033 |
| `MEMORY_PROFILE` | enum `collective`, `individual` | `collective` | No | — | Memory Service | Unset; in-process `Collective` *(chosen here)*: one user with shared team scopes, and the only fully implemented profile (`memory/src/types/placement.rs:30`–`35`). | `02-architecture.md:350` |
| `MEMORY_QUEUE_BACKEND` | enum `store`, `nats` | `store` | No | — | Memory Service | Unset; in-process `Store`. The company NATS connection (ADR-074) is never used as the memory queue. | ADR-074 |
| `MEMORY_QUEUE_NATS_URL` | URL | none | When backend is `nats` | — | Memory Service | Unset; in-process `None`. | — |
| `MEMORY_JOB_MAX_ATTEMPTS`, `MEMORY_JOB_BACKOFF_BASE_MS`, `MEMORY_QUEUE_REAPER_SECS`, `MEMORY_QUEUE_POLL_MS` | integer | `5`, `2000`, `30`, `500` | No | — | Memory Service | Unset; crate defaults kept in-process. | — |
| `MEMORY_RESULT_CAP_MAX` | integer 1–50 | `50` | No | — | Memory Service | Unset; default kept. | — |
| `MEMORY_STAGE1_K` | integer | `50` | No | — | Memory Service | Unset; default kept. | — |
| `MEMORY_ABSTAIN_THRESHOLD`, `MEMORY_RECENCY_WEIGHT` | number 0–1 | `0.2`, `0.15` | No | — | Memory Service | Unset; defaults kept. | — |
| `MEMORY_RECENCY_TAU_DAYS` | number | `30` | No | — | Memory Service | Unset; default kept. | — |
| `MEMORY_REFORMULATION_ENABLED` | boolean | `false` | No | — | Memory Service | Unset; in-process `false`. | — |
| `MEMORY_TRUST_ADMIT`, `MEMORY_TRUST_QUARANTINE`, `MEMORY_PII_REDACT_CONF`, `MEMORY_SOURCE_TRUST_DEFAULT` | number 0–1 | `0.7`, `0.4`, `0.9`, `0.5` | No | — | Memory Service | Unset; defaults kept; these set the write gate that content from documents passes through. | `02-architecture.md:351`, R-27 |
| `MEMORY_SALIENCE_FLOOR`, `MEMORY_PRUNE_RETRIEVABILITY` | number 0–1 | `0.3`, `0.05` | No | — | Memory Service | Unset; defaults kept. | — |
| `MEMORY_DECAY_K`, `MEMORY_REINFORCE_GAIN` | number | `10.0`, `0.5` | No | — | Memory Service | Unset; defaults kept. | — |
| `MEMORY_IDLE_QUIET_SECS`, `MEMORY_MAINT_MAX_INTERVAL_SECS`, `MEMORY_MAINT_BATCH_SIZE` | integer | `300`, `21600`, `500` | No | — | Memory Service | Unset; defaults kept. | — |
| `MEMORY_IDEMPOTENCY_TTL_SECS`, `MEMORY_MAX_BODY_BYTES` | integer | `86400`, `1048576` | No | — | Memory Service | Unset; defaults kept; HTTP edge settings, unused without the HTTP layer. | C-15 |
| `MEMORY_RATE_READ_PER_MIN`, `MEMORY_RATE_WRITE_PER_MIN` | integer | `120`, `30` | No | — | Memory Service | Unset. *Amended January 2026 (decision 80; B8 Memory D-2):* the rates are applied in-process by `Service::begin`, at 600 reads and 600 writes per minute. | C-15 |
| `MEMORY_OTLP_ENDPOINT` | URL | none | No | — | Memory Service | Unset; in-process `None` (A-10; in the agent profile the Telemetry component owns traces). | A-10, `07-cross-cutting.md:16` |
| `MEMORY_LOG_LEVEL` | enum | `info` | No | — | Memory Service | Unset; in-process from `ORDIN_LOG_LEVEL` (desktop: `info`). Unused for output, because ordin owns the subscriber. | `07-cross-cutting.md:14` |
| `MEMORY_ENV` | enum `production`, `development` | `production` | No | — | Memory Service | Unset; in-process `Production`. | — |
| `MEMORY_BENCH_*` (22 variables: `CONCURRENCY`, `CORESIDENTS`, `DIM`, `FRACTIONS`, `HOST_DISK`, `ISOLATION`, `POPULATION`, `QUERIES`, `REPORT_PATH`, `REPS`, `RETAIN`, `RIG_DIR`, `SIZES`, `STAGE1_K`, `STORE_URL`, `SWEEP_SAMPLES`, `TENANT_PREFIX`, `TEST_SWEEP_LIST`, `TOTAL`, `VISIBLE`, `WARMUP`, and `MEMORY_BENCH_DIM` in `store/mod.rs`) | various | various | No | — | Memory Service | Benchmark harness only (`memory/src/bench/mod.rs:208`–`231`); never read in an ordin build. | — |

#### 2D-1f Embedded sandbox (`sandbox/sandbox-host`, crate `sandboxd`) — variables it reads, and what ordin does

**Evidence.** `sandboxd::config::Config::load(get, resolver)` takes an injected getter and a `SecretResolver` (`sandbox/sandbox-host/src/config.rs:88`–`91`); `SecretResolver` is a public trait (`config.rs:9`). Only the daemon's `main.rs`, `mcp.rs` (`daemon_paths`, the stdio front door) and `keychain.rs::resolver_kind` read the process environment directly.

**What ordin does, in every program** *(chosen here)*: the Sandbox component calls `Config::load` with a getter over an in-process map (never `std::env::var`) and its own `SecretResolver` backed by Identity and Credentials. It never runs `sandboxd`'s `main`, its `mcp-stdio` front door or `resolver_kind`, so **no `PYS_*` variable is read or set**. Embedding the library without the daemon is C-17 and stays unverified until its spike.

| Variable | Type | Default (crate) | Required (crate) | Program(s) | Owner Component | Description — ordin's treatment and in-process value | HLD trace |
|---|---|---|---|---|---|---|---|
| `XDG_RUNTIME_DIR`, `LOCALAPPDATA` | path | temp dir | No | — | Sandbox | Read by the crate only to derive the socket and token defaults; not supplied in the map. | C-17 |
| `PYS_SOCKET_PATH`, `PYS_CONNECTION_TOKEN_PATH` | path | `$XDG_RUNTIME_DIR/sandboxd.sock`, `…/sandboxd.token` (Windows: named pipe, `%LOCALAPPDATA%`) | No | — | Sandbox | Daemon IPC; not supplied, no daemon runs. | C-17, ADR-072 |
| `PYS_REQUIRE_FIRST_CONNECT_CONSENT` | boolean | `true` | No | — | Sandbox | In-process `false`: there is no client connecting to a daemon; the Rules Engine decides every call. | ADR-037 |
| `PYS_CONSENT_UI_MODE` | enum `browser`, `dialog`, `auto` | `auto` | No | — | Sandbox | Not supplied; default kept, unused. | — |
| `PYS_OIDC_ISSUER`, `PYS_OIDC_CLIENT_ID`, `PYS_OIDC_SCOPES`, `PYS_OBO_ENDPOINT` | URL; string | none; `openid profile email`; none | No (required only when the manifest uses exchange or passthrough) | — | Sandbox | Not supplied. The broker uses the person's tokens from Identity and Credentials, never its own sign-in. | `02-architecture.md:379` |
| `PYS_POLICY_PATH` | path | none | **Yes** | — | Sandbox | In-process: a path under the application data root to the policy the core writes from the Rules Engine's grants *(chosen here)*; `PolicyEngine::load` itself takes JSON, so a file may prove unnecessary (Needs verification). | `02-architecture.md:379` |
| `PYS_ADMIN_SIGNING_KEY_REF` | secret reference | none | No | — | Sandbox | Not supplied; ordin does not use a signed policy override. | — |
| `PYS_MAX_CALLS_PER_RUN`, `PYS_MAX_CALLS_PER_SESSION` | integer (≥1; session ≥ run) | `50`, `500` | No | — | Sandbox | In-process `50`, `500` (fixed constants, 2D-4). | `02-architecture.md:378` |
| `PYS_RESPONSE_MAX_BYTES` | integer ≤ 1 MiB | `1048576` | No | — | Sandbox | In-process `1048576`. | — |
| `PYS_ALLOW_PLAINTEXT_LOOPBACK_EGRESS` | boolean | `false` | No | — | Sandbox | In-process `false`. | ADR-031 |
| `PYS_WASM_FUEL` | integer | none | No | — | Sandbox | Not supplied. | ADR-040 |
| `PYS_WASM_MAX_MEMORY_BYTES`, `PYS_WASM_DEADLINE_SECONDS` | integer | `536870912`, `30` | No | — | Sandbox | In-process `536870912`, `30` (fixed constants, 2D-4). | ADR-040 |
| `PYS_GUEST_ARTIFACT_DIGEST` | string | none | **Yes** | — | Sandbox | In-process: the guest's digest computed at build time and compiled in *(chosen here)*. | ADR-040 |
| `PYS_OTLP_ENDPOINT` | URL | none | No | — | Sandbox | Not supplied (A-10). The crate derives `credential_mode = Mock` when it is absent (`config.rs:181`–`185`); no other module of the crate reads `credential_mode` today (searched), so this has no effect now. See *Former gaps, now settled*, item 6. | A-10 |
| `PYS_AUDIT_HMAC_KEY_REF` | secret reference | none | **Yes** | — | Sandbox | In-process: a reference that ordin's resolver maps to a 32-byte key made at first start and kept in the keychain or the server's secret store *(chosen here)*. | `07-cross-cutting.md:61` |
| `PYS_LOG_LEVEL` | string | `info` | No | — | Sandbox | In-process from `ORDIN_LOG_LEVEL`. | `07-cross-cutting.md:14` |
| `PYS_SECRET_RESOLVER`, `PYS_KEYCHAIN_SERVICE` | enum `file`, `keychain`; string | `file`; `sandboxd` | No | — | Sandbox | Not read: ordin passes its own resolver and never calls `resolver_kind`. | `07-cross-cutting.md:61` |
| `SANDBOXD_CONSENT_PROMPT` | string | — | — | — | Sandbox | Set by `sandboxd`'s daemon for its Windows consent dialog only (`main.rs:183`); never set or run by ordin. | — |
| `HOME`, `USERPROFILE` | path | — | — | — | Sandbox | Read only by `sandboxd install-mcp-config`; never run by ordin. | — |

#### 2D-1g Build-time variables (read by the compiler, never at run time)

| Variable | Type | Default | Required | Program(s) | Owner Component | Description | HLD trace |
|---|---|---|---|---|---|---|---|
| `GITHUB_APP_CLIENT_ID` *(chosen here)* | string | absent | No | Desktop, server (every build of `ordin-identity`) | Identity and Credentials | Client identifier of ordin's GitHub App, compiled in by the branded build with `option_env!`; absent, GitHub sign-in is not offered. The running program never reads it from its environment. | OQ-6, ADR-041 build layer; IdentityAndCredentials.md *Configuration* |
| `ORDIN_SANDBOX_PY_DIGEST` | string, SHA-256 hex (64 lowercase characters) | none; computed at build by the `ordin-sandbox` crate's `build.rs` (with `sha2`) from the Python guest `pysandbox-guest.wasm` | Yes, at build (the crate reads it with `env!`, so a build without it fails) | Desktop, server: personal profile (every build of `ordin-sandbox`) | Sandbox | Digest of the Python guest compiled into `ordin-sandbox` with `env!("ORDIN_SANDBOX_PY_DIGEST")` and passed to `SandboxRuntime::new` at start, which refuses a guest that does not match it. Set by the crate's own build script, never by a person; the running program never reads it from its environment. | — (build detail); Sandbox.md *Internal Logic* A step 4 |

#### 2D-2 Organisation profile settings

Key names are *(chosen here)* throughout: `08-interfaces.md:77` states "Format and signing scheme fixed in `/spec`", and no HLD file fixes a field name. The sections follow the profile sections 08 names (agent catalogue, usage, email, voice defaults, ordin server listening, company workflow repository, brand pack). Keys use `snake_case`. "Maximum/Allowed" is the bound the profile value itself must respect; a profile value outside it fails profile validation and the last verified profile is kept. The desktop and the personal-profile server read the profile; the agent profile does not. *Required* says whether a verified profile must carry the key: a key marked No takes its *Default* when absent, because a desktop or personal server may run with no profile at all.

| Key (dotted path) | Type | Default | Required | Maximum/Allowed | Owner Component | Description | HLD trace |
|---|---|---|---|---|---|---|---|
| `format_version` | integer | — (required) | Yes | Versions this build reads | Organisation Profile | Format version; a newer one is refused and the last verified profile kept. | `02-architecture.md:702`, `04-domain.md:27` |
| `profile_version` *(amended January 2026, decision 80)* | string `MAJOR.MINOR.PATCH` | — (required) | Yes | strictly higher than the version in force | Organisation Profile | The profile's own version; a profile is used only when its version is higher than the one in force. | H08-9 (HLD 0.49.0), B2-F3 |
| `agent_catalogue.repositories[]` | list of {`key`, `display_name`, `host`, `repository`, `protected_branch`, `format` (`ordin`, `apm`), `signed_merges_required` (default `false`)} | empty | No | — | Catalogue Service | Agent repositories the company names; shown as *Set by your company*. `key` *(amended January 2026, decision 80)* is a stable key (lowercase letters, digits and hyphens) that workflow files format 3 cite as the repository's `profile_key`. | `08-interfaces.md:32`, ADR-086 |
| `agent_catalogue.person_may_add` | boolean | `true` | No | — | Catalogue Service | Whether the person may add agent repositories. | `07-cross-cutting.md:22`, `08-interfaces.md:32` |
| `agent_catalogue.trust_policy` | object: tool bindings and hooks the company trusts | empty: every executable part needs IT approval *(chosen here)* | No | — | Checker | The executable-trust policy over tool bindings and hooks. | `02-architecture.md:693`, `08-interfaces.md:32` |
| `agent_catalogue.it_approval_destination` | destination (as `help.destination`) | `help.destination` | No | — | Checker | Where IT approval requests go. | `08-interfaces.md:32` |
| `remote_agents[]` | list of {`name`, `route` (`hosted_agent`, `slack`), `address`, `identity` {`issuer`, `client_id`, `audience`}} | empty | No | — | wrightrun HTTP Adapter (Hosted Agent Adapter), Slack Thread Adapter | The remote agent catalogue; hosted ordin agents are registered here. *(amended January 2026, decision 80)*: for the `slack` route, `address` is the agent's Slack member identifier; the profile's `route` `hosted_agent` is stored as `http`. | `02-architecture.md:694`, ADR-098 item 8 |
| `slack.workspace`, `slack.dedicated_channel` | string | none | No | — | Slack Thread Adapter | Slack workspace and the dedicated channel for reaching agents as the person. *(amended January 2026, decision 80)*: `dedicated_channel` is the channel's Slack identifier, not its name. | `02-architecture.md:695`, ADR-008, B6 G-STA-1 |
| `slack.client_id` *(amended January 2026, decision 80)* | string | none | No | — | Identity and Credentials | Client identifier of the Slack app IT approves (PKCE-enabled). | H08-9 (HLD 0.49.0), B2-F24 |
| `confluence.site`, `confluence.oauth_client_id` | URL; string | none (Confluence off) | No | — | Confluence Adapter | Confluence Cloud site and the Atlassian OAuth app's client identifier. | `08-interfaces.md:77`, OQ-23 |
| `microsoft.tenant_id`, `microsoft.client_id` | string | none (SharePoint and Outlook off) | No | — | Identity and Credentials | Microsoft tenant and the Entra application identifier (public client). | `08-interfaces.md:77`, `:107`, C-14 |
| `google.client_id` | string | none (Gmail off) | No | — | Identity and Credentials | The internal Google application IT registers. | `08-interfaces.md:106`, C-44 |
| `google.client_secret` *(amended January 2026, decision 80)* | string | none | No | — | Identity and Credentials | The non-confidential secret of the Google Workspace Desktop OAuth client, sent in the token exchange. | H08-9 (HLD 0.49.0), B2-F24, C-44 |
| `confluence.token_exchange_url` *(amended January 2026, decision 80)* | URL | none (public client with PKCE) | No | `https` only | Identity and Credentials | A company-hosted token exchange; used only if C-12 shows Atlassian 3LO needs a client secret. | H08-9 (HLD 0.49.0), B2-F23, C-12 |
| `identity_provider.issuer`, `identity_provider.client_id`, `identity_provider.scopes[]` | URL; string; list | none | No | `https` issuer only | Identity and Credentials | The organisation's OIDC provider for hosted agents and wrightrun. | `02-architecture.md:697`, `08-interfaces.md:101` |
| `harnesses[]` | list of {`name` (`claude_code`, `copilot`), `tested_versions[]`} | `[{name: claude_code}]` *(chosen here)* | No | — | Setup Assistant | Harnesses the company uses, in order of preference, with tested versions. | `02-architecture.md:707`, `:829` |
| `usage.plan_mode` | enum `save`, `balanced`, `best` | `balanced` | No | — | Usage Planner | Company default of *How Ordin uses your plan*. | ADR-062, `08-interfaces.md:71` |
| `usage.levels_allowed.<kind>` (`kind`: `talking`, `coordinating`, `routine`, `documents`, `code`) | list of `automatic`, `smaller`, `standard`, `most_capable` | all four | No | — | Usage Planner | Levels allowed for each kind of work. | ADR-062, `08-interfaces.md:71` |
| `usage.level_models.<harness>.<level>` | string (harness model name) | Claude Code: Haiku, Sonnet, Opus for `smaller`, `standard`, `most_capable`; Copilot: `automatic` is Copilot's own selection | No | — | Usage Planner | Each harness's map from level to model. | ADR-062, `08-interfaces.md:71` |
| `usage.agents_at_once` | integer | `2` | No | 1–3 (the person's choices) | Usage Planner | Default agents at the same time. | ADR-062 |
| `usage.agents_at_once_max` | integer | `3` | No | 1–3 | Usage Planner | Company maximum of agents at once. | ADR-062, `08-interfaces.md:71` |
| `usage.defaults.<setting>` | per setting: `answer_without_model` (bool, `true`), `reuse_answers` (bool, `true`), `parts_only` (bool, `true`), `summarise` (`early`, `when_nearly_full`, `only_when_asked`; `when_nearly_full`), `continue_from_summary` (bool, `true`), `ask_before` (`heavy`, `medium_or_heavy`, `never`; `heavy`), `show_usage` (bool, `true` in workspaces with code), `daily_pause` (`none`, `100`, `250`, `500`; `none`) | as listed | No | as listed | Usage Planner | Company defaults of the remaining usage settings. | ADR-062 |
| `usage.person_may_change[]` | list of setting names | every usage setting | No | — | Usage Planner | Settings the person may change; others show "Set by your company". | ADR-062, `08-interfaces.md:71` |
| `voice.tone`, `voice.register`, `voice.terms_use[]`, `voice.terms_avoid[]` | string; string; list; list | none | No | — | Ordin Host | Default tone and register, and company terms. | `08-interfaces.md:73`, ADR-063 |
| `limits.crew.rounds` | integer | `10` | No | 1–30 | Crew Orchestrator | Company default and ceiling of rounds per crew. | `07-cross-cutting.md:45` |
| `limits.crew.wall_clock_minutes` | integer | `60` | No | 1–480 | Crew Orchestrator | Crew wall-clock limit. | `07-cross-cutting.md:46` |
| `limits.crew.members_at_once` | integer | `4` | No | 1–8 | Crew Orchestrator | Members running at once. | `07-cross-cutting.md:48` |
| `limits.crew.request_budget` | integer | none (no ceiling until OQ-3) | No | ≥1 | Crew Orchestrator | Company ceiling of a crew's request budget. | `07-cross-cutting.md:49`, OQ-3 |
| `limits.run.wall_clock_minutes` | integer | `60` *(chosen here)* | No | 1–480 *(chosen here)* | Session Supervisor | Wall-clock limit of one agent's run. | `02-architecture.md:486`, `07-cross-cutting.md:17` |
| `history.max_days` | integer | none (no company maximum) *(chosen here)* | No | ≥1 | Journal and Artefact Store | Upper bound on the person's history retention. | `07-cross-cutting.md:22`; see inconsistency 2 |
| `email.system` | enum `company_server`, `microsoft_365`, `google_workspace` | none (email off) | No | — | Mail Service | The company's mail system. | `08-interfaces.md:72`, ADR-071 |
| `email.enabled` | boolean | `true` when `email.system` is set | No | — | Mail Service | Email on or off. | `08-interfaces.md:72` |
| `email.imap.host`, `email.imap.port`, `email.imap.security` | string; integer; enum `tls`, `starttls` | none; `993`; `tls` | No | port 1–65535 | Mail Service | Company IMAP server. | `08-interfaces.md:72`, `:104` |
| `email.smtp.host`, `email.smtp.port`, `email.smtp.security` | string; integer; enum `tls`, `starttls` | none; `465`; `tls` | No | port 1–65535 | Mail Service | Company SMTP submission server. | `08-interfaces.md:72`, `:105` |
| `email.ca_certificate` | PEM | none | No | — | Mail Service | Company certificate when the mail server needs one. | `08-interfaces.md:72` |
| `email.sign_in_methods[]` | ordered list of `single_sign_on`, `app_password`, `mail_password` | all three, in that order | No | — | Identity and Credentials | Sign-in methods the company's server accepts. | `08-interfaces.md:72`, ADR-071 |
| `email.people_link_folders[]` | list | `[Inbox, Sent]` | No | — | Mail Service | Folders searched for people-and-domains links. | `08-interfaces.md:72` |
| `email.server_saves_sent` | boolean | `false` | No | — | Mail Service | Whether the server copies sent mail itself. | `02-architecture.md:392` |
| `email.company_domains[]` | list of domains | none | Yes, when `email.system` is `company_server` | Required when `email.system` is `company_server` | Rules Engine | The company's email domains. | `08-interfaces.md:72` |
| `email.internal_only` | boolean | `false` | No | — | Rules Engine | Recipients inside the company only. | `02-architecture.md:399` |
| `email.max_recipients` | integer | `20` | No | 1–500 *(chosen here)* | Rules Engine | Most recipients per email. | `02-architecture.md:399`, `08-interfaces.md:72` |
| `email.no_attach_places[]` | list of place patterns | empty | No | — | Rules Engine | Places whose files may not be attached. | `02-architecture.md:399` |
| `email.foot_line_required` | boolean | `false` | No | — | Rules Engine | Whether "Drafted with an agent in ordin" is required. | `08-interfaces.md:72` |
| `channels.allowed[]` | list of `telegram`, `slack` | `[slack]` | No | — | Channel Gateway | Chat channels the company allows; Telegram off unless listed. Also gates the Telegram tools on the desktop. | A-40, OQ-37, ADR-077 |
| `telegram.tools_enabled` | boolean | `true` (effective only when `telegram` is in `channels.allowed`) | No | — | Telegram Service | IT can turn the Telegram tools off and keep the chat channel. | ADR-075 *Rules from the organisation profile* |
| `telegram.allowed_chats[]` | list of chat identifiers | none (any chat a workspace links) | No | — | Telegram Service | Only these channels and groups may be linked. | ADR-075 |
| `telegram.foot_line_required` | boolean | `false` | No | — | Telegram Service | Foot line on agent-drafted posts. | ADR-075 |
| `slack.tools_enabled` | boolean | `true` | No | — | Slack Service | IT can turn the Slack tools, linked Slack channels, drafts and Ordin's reactions off; the person's own handover notices (direct messages and stage-channel posts) are not affected — `server.external_actions_off` naming `slack_post` stops those on the server. | ADR-102 |
| `slack.allowed_channels[]` | list of Slack channel identifiers (`^[CG][A-Z0-9]{8,20}$`) | none (any channel the person belongs to) | No | 200 | Slack Service | Only these channels may be linked to a workspace or named by a lifecycle stage. | ADR-102 |
| `slack.rules_allowed` | boolean | `true` | No | — | Slack Service | `false` turns every Slack rule off: every Slack post then waits for its card. | ADR-102 |
| `server.external_actions_off[]` | list of action kinds | empty | No | Kinds the Rules Engine names | Rules Engine | Kinds of external action switched off on the server. `slack_post` also stops every Slack Service post, rule-covered or not; cards then offer no Slack option (amended January 2026, ADR-102). | `07-cross-cutting.md:65`, ADR-073 *Approvals in the chat* |
| `server.listening_allowed` | boolean | `true` | No | — | Remote API | Whether a server may listen on the company network; `false` switches the Remote API off. | `08-interfaces.md:85`, ADR-096 item 4 |
| `server.client_certificate_required` | boolean | `false` | No | — | Remote API, Server Connector | Whether desktops must present a client certificate. | `08-interfaces.md:85` |
| `server.client_certificate_name` | string | none | Yes, when the previous key is `true` | Required when the previous key is `true` | Server Connector | Name under which the person imported the client certificate in Settings (Identity and Credentials keeps it in ordin's secret store; ordin reads no operating-system certificate store). *(amended January 2026, Phase 3 closing round, K11)* | `08-interfaces.md:85` |
| `server.triggers.daily_runs` | integer | `20` | No | 1–200 | Trigger Service | Triggered runs per installation per day. | `02-architecture.md:633`, ADR-074 |
| `nats.servers[]` | list of URLs (`tls://`, `wss://`) | empty (event triggers off) | No | — | NATS Adapter | Company NATS servers. | ADR-074 *Connection* |
| `nats.tls.ca_certificate` | PEM | none (system trust store) | No | — | NATS Adapter | Extra trust anchor for NATS. | ADR-074, C-53 |
| `nats.subjects_allowed[]` | list of subjects with `*`, `>` | empty | No | — | Trigger Service | Subjects ordin may subscribe to. | `02-architecture.md:641` |
| `craft.apps[]` | list of {`app_id` (string), `version` (string), `allowed_tools[]` (list of strings, optional), `platforms` (map platform → {`asset_url` (URL), `sha256` (64 hexadecimal characters), `file_name` (string)})}; platform ∈ `macos_arm64`, `macos_x86_64`, `windows_x86_64`, `linux_x86_64` *(amended January 2026, decision 80; shape fixed in the Phase 3 closing round, K9)* | empty | No | — | Craft Toolkit | Approved ArtCraft apps, pinned per platform by download address, file name and SHA-256; `allowed_tools` is the app's allowed list (ADR-017), and an absent `allowed_tools` allows every tool the app's MCP mode lists. | `02-architecture.md:696`, `:747` |
| `craft.mirror_url` | URL | none (GitHub releases) | No | `https` only | Craft Toolkit | Company mirror for downloads. | `02-architecture.md:747` |
| `craft.file_types` | map extension → app | empty | No | — | Craft Toolkit | The file-type map. | `02-architecture.md:696` |
| `language_servers[]` *(amended January 2026, Phase 3 closing round, K12)* | list of {`language` (`go`, `rust`, `java`, `typescript`), `name`, `version`, `platforms` (map platform → {`asset_url` (URL), `sha256` (64 hexadecimal characters), `file_name`, `binary_path` (path inside the unpacked archive)})}; platform as `craft.apps[]` | none: the table compiled into the build (`language-servers.json`, same shape, pinned at release) | No | at most one entry per `language` | Setup Assistant (install), Project Service (which server a language uses) | The approved language server per language and where to download it. Install = download, verify SHA-256, unpack into `<data root>/language-servers/<name>/<version>/`. | `09-decisions.md:1080` (ADR-055) |
| `gitlab.host` *(Phase 3 closing round, K10)* | string (host name) | `gitlab.com` | No | a host name, no scheme or path | Identity and Credentials | The company's GitLab host. Read by the Project Service (`ProjectSettings.gitlab_host`) to recognise a GitLab remote; Identity and Credentials owns it (C-1). | decision D9 |
| `gitlab.client_id` *(Phase 3 closing round, K10)* | string | none | No | 1–256 characters | Identity and Credentials | Client identifier of the company's GitLab OAuth application; present → sign-in by OAuth device flow on `gitlab.host`; absent → the person pastes a personal access token (scopes `read_repository`, `write_repository`, `api`). | decision D9 |
| `engineering.github.always_approved_kinds[]` | list of GitHub operation kinds | empty | No | Write kinds only | Project Service | Kinds the engineer cannot switch approval off for (the ceiling). | `02-architecture.md:766`, `04-domain.md:139` |
| `engineering.coding_tasks.max_attempts` *(chosen here; amended January 2026, decision 80)* | integer | `3` | No | ≥ 1 | Project Service | Attempts per coding-task request (the Project Service's `max_attempts`). | ADR-053 |
| `workflows.company_repository` | {`host`, `repository`, `folder`} | none | No | — | Ordin Host | The company workflow repository. | `08-interfaces.md:20` |
| `brand.*` (`strings.<locale>`, `logos`, `icon_set`, `design_tokens`, `fonts`, `links`, `legal_text`) | brand pack | the build's own brand | No | WCAG 2.2 AA contrast | Brand | Run layer of the brand pack. | `08-interfaces.md:33`, ADR-041 |
| `locales.offered[]`, `locales.fallback` | list of BCP 47 tags; tag | every bundled locale; `en-GB` *(chosen here)* | No | ⊆ bundled locales | Localisation | Offered locales and fallback. | `02-architecture.md:527`, `08-interfaces.md:34` |
| `help.destination` | {`kind` (`ticket_url`, `email`, `slack_channel`), `address`} | none ("Send to IT" disabled) | No | — | Setup Assistant | Where help requests and repeating-work requests go. | `02-architecture.md:700`, `08-interfaces.md:14`–`15`, OQ-14 |
| `updates.feed_url` | URL | the installer's feed | No | `https` only | Setup Assistant | Replaces the installer-carried feed once verified. | `02-architecture.md:699`; see inconsistency 1 |
| `sandbox.session_idle_minutes` | integer | `30` *(chosen here)* | No | 1–240 *(chosen here)* | Sandbox | Idle limit after which a session's sandbox working folder is deleted. | `09-decisions.md:737`, `04-domain.md:403` |
| `team_knowledge.read_folders[]` *(amended January 2026, decision 90)* | list of {`key`, `display_name`, `host`, `repository`, `branch`, `folder` (default `""`), `applies_to` (`team_knowledge`, `code`; default `team_knowledge`), `review_required` (default `false`), `person_may_stop` (default `false`)} | empty | No | ≤ 20 entries; `key` `^[a-z0-9-]{1,64}$` unique; `display_name` 1–120 chars unique; `host` `github.com` or `gitlab.host`; (`host`, `repository`, `branch`, `folder`) unique | Team Knowledge Service (validated by Organisation Profile) | Team knowledge folders every applicable workspace reads, shown as *Set by your company*; read with the person's own sign-in, never granting access. Absent or empty: no workspace gains a read folder. | ADR-110, `08-interfaces.md` (organisation profile, team knowledge) |
| `knowledge_agents.front_doors[]` *(amended January 2026, ADR-113)* | list of {`key` (`^[a-z0-9-]{1,64}$`, unique), `display_name` (1–120 characters, unique), `url` (`https` only, at most 2,048 bytes, no user information, query, fragment or trailing `/`, unique), `applies_to` (`team_knowledge`, `code`; default `team_knowledge`)} | empty | No | at most 8 entries | Knowledge Query Client (reader, through the program); Organisation Profile (validation) | Knowledge agents every applicable workspace connects to by default, each shown *Set by your company*; the person may remove any of them from a workspace and the removal is kept. Grants no access: each front door admits the person with their own company sign-in and only when they are in its audience. | ADR-113 |

#### 2D-2b Installer-carried settings

Written by the company's (branded) installer, read by the Organisation Profile and Setup Assistant before any profile exists. Device management may write the same file instead (OQ-14, non-blocking).

| Setting | Type | Default | Required | Owner Component | HLD trace |
|---|---|---|---|---|---|
| `profile_url` *(chosen here)* | URL, `https` | — (required; without it setup cannot finish, R-11) | Yes | Organisation Profile | `02-architecture.md:691`, ADR-013, OQ-14 |
| `profile_signing_keys[]` *(chosen here)* | public keys | — (required) | Yes | Organisation Profile | `02-architecture.md:691` ("verifies its signature"); scheme fixed in `/spec` (`08-interfaces.md:77`) |
| `update_feed_url` *(chosen here)* | URL, `https` | — (required); *amended January 2026 (ADR-114):* the unbranded open-source build carries the project's release feed address | Yes | Setup Assistant | `09-decisions.md:761` (ADR-041 build layer), A-02, ADR-114 item 3 |
| Update signing public key | Tauri updater key | fixed at build | Yes (compiled in) | Setup Assistant | `05-tech-stack.md:47`, A-02 |

#### 2D-3 Person and server-owner settings

| Setting | Type | Default | Required | Where set | Owner Component | HLD trace |
|---|---|---|---|---|---|---|
| Interface language | locale tag | the operating system's, if offered | No | Desktop Settings | Localisation | `07-cross-cutting.md:27` |
| Notifications | per event kind: on, off | on for approvals, finished work, crew questions, own agent draft reviews | No | Desktop Settings | ordin desktop program, in its `preferences.json` *(amended January 2026, decision 80; the shell reads and writes through `settings.get` and `settings.change`)* | UX-A4 (`11-ux-guidelines.md:1226`), `07-cross-cutting.md:22` |
| Display (theme, density) | enum | system theme | No | Desktop Settings | ordin desktop program, in its `preferences.json` *(amended January 2026, decision 80)* | `07-cross-cutting.md:22` |
| History retention | whole days ≥ 1 | `90` | No | Desktop Settings › History; server: setup command | Journal and Artefact Store | A-08 (`10-risks.md:19`), `04-domain.md:386` |
| *How Ordin uses your plan* and the other usage settings of ADR-062 | as `usage.*` in 2D-2 | the profile's `usage.*` | No | Desktop Settings › Using your plan | Usage Planner | ADR-062 |
| How Ordin talks to you (tone, register, length, notes) | enums and text | the profile's `voice.*` | No | Desktop Settings | Ordin Host | ADR-063 |
| Added agent repositories, each with its display name | list | empty | No | Desktop Settings › Agent repositories (when `agent_catalogue.person_may_add`) | Catalogue Service | `07-cross-cutting.md:22`, ADR-086 |
| GitHub operation approval per kind, per project | per kind: on, off | on for every write kind | No | Desktop project settings, within `engineering.github.always_approved_kinds` | Project Service | `04-domain.md:139` |
| Language server per language and its JSON settings | name; JSON | the profile's default | No | Desktop Settings | Project Service | ADR-055 |
| Check commands per project *(added in Phase 4)* | list of strings, at most 20, each at most 1,000 characters after trimming; empty entries dropped, duplicates removed | none (no check runs; the screens show "No checks set") | No | Desktop project settings screen, sent as `projects.set_check_commands { project_id, commands }`; stored in the Project Service's `projects.check_commands` (JSON array); no organisation profile key and no program sets it | Project Service | ProjectService.md SA-CLOSE-03, Internal Logic 6 |
| IDE to open | one of the IDEs found | none | No | Desktop Settings | Project Service | ADR-052, `02-architecture.md:762` |
| Paired servers (address, desktop key) | list | empty | No | Desktop Settings › Servers › Connect to a server… | Server Connector | `02-architecture.md:585`, ADR-096 item 5 |
| Desktop Telegram bot | bot key (keychain) | none | No | Desktop guided screen | Telegram Service | `02-architecture.md:623`, ADR-077 |
| The person's own Slack app | client identifier (`^[0-9]+\.[0-9]+$`), not a secret | none (the profile's `slack.client_id` is used when present) | No | Desktop Settings › Slack › Connect your own Slack app | Slack Service | ADR-102 |
| Slack rules | list of `SlackRule` (kind, channel, scope, daily cap 1–20, on or off) | none | No | Settings › Slack and the stage's screen | Slack Service | ADR-102 |
| Let Ordin look at new messages | boolean per Slack link | `false` | No | The linked channel's screen | Slack Service | ADR-102 |
| Server profile | `personal`, `agent` | — (required; fixed for the installation's life) | Yes | Server setup command | ordin server | ADR-097 item 1 |
| Organisation profile address | URL | none (A-40: the server owner chooses channels) | No | Server setup command | Organisation Profile | `02-architecture.md:570` |
| Remote API listening address and port | private or loopback IP; port 1025–65535 | none (Remote API off) | No | Server setup command | Remote API | `02-architecture.md:579`, ADR-096 item 4 |
| Remote API TLS certificate and key | file paths read once; key stored in the secret store | none | No | Server setup command | Remote API | ADR-096 item 4 |
| Pair a desktop, end a pairing | one-time code; pairing line | — | No | Server setup command | Remote API | ADR-096 item 5 |
| Telegram bot token; Telegram pairing | secret; code | none | No | Server setup command | Telegram Adapter | ADR-073 |
| Slack bot token and app-level token; Slack pairing | secrets; code | none | No | Server setup command | Slack Channel Adapter | ADR-076 |
| *Needs you* to both paired channels | boolean | `false` (the channel last written on) | No | Server setup command | Channel Gateway | ADR-076 *Telegram and Slack at once* |
| NATS credentials (credentials file, user and password, or token) | secret | none | No | Server setup command | NATS Adapter | ADR-074 *Connection* |
| Mail password or app password | secret | none | No | Server setup command | Identity and Credentials | ADR-071, ADR-073 |
| Triggers (on, off, filter, inputs) | trigger records | none | No | The person, by chat (trigger card, **/triggers**) | Trigger Service | ADR-074 |
| Agent release or package with its configuration | release reference; files | — (required, agent profile) | Yes, agent profile | Server setup command (single shape); Helm values `release` (scaled) | Agent Loader, Release Builder | ADR-097 *Consequences*, `08-interfaces.md:48` |
| Organisation model key; connection credentials by configured name | secrets | — | Yes, agent profile: the model key in instances serving runs, the check and run-once; each credential the configuration names (2D-1d) | Server setup command (single shape); Kubernetes Secret behind `ORDIN_MODEL_KEY_FILE` and `ORDIN_SECRETS_DIR` (scaled) | Model Policy, Connection Binder | ADR-097 item 2 |
| Instance role | `requests`, `runs`, `chat`, `jobs`, `chat-jobs` *(chosen here)* | all roles in the single shape | No | Start sub-command flag (name fixed in `/spec`, `08-interfaces.md:47`); Helm values per role | ordin server | `05-tech-stack.md:41` |
| Helm values (*chosen here*): `image`, `release`, `secretName`, `roles.<role>.replicas`, `roles.<role>.resources`, `roles.runs.concurrency`, `roles.runs.stopPeriodSeconds`, `otlpEndpoint`, `logLevel` | values schema | chart defaults | No | Helm values file | ordin server | `05-tech-stack.md:41`, `08-interfaces.md:48`, ADR-098 item 5 |

Per-agent choices of a hosted agent (models, connections and their `timeout-seconds`, intakes, schedules, retention periods, approvers, identity-provider issuer, audience and role claim) are keys of the agent configuration file (`08-interfaces.md:40`), not of this phase; they belong to the Agent Configuration component spec.

#### 2D-4 Fixed constants

*(chosen here)* marks a value the HLD does not state. *Required* is n/a for every row: a constant is compiled in and no one sets it.

| Constant | Value | Required | Owner Component | HLD trace |
|---|---|---|---|---|
| Concurrent local sessions | 8 | n/a (fixed in code) | Session Supervisor | A-04, `07-cross-cutting.md:39` |
| Crew members | 8 plus the coordinator | n/a (fixed in code) | Crew Orchestrator | `07-cross-cutting.md:41` |
| Crew rounds, application maximum | 30 | n/a (fixed in code) | Crew Orchestrator | `07-cross-cutting.md:45` |
| Crew wall-clock, application maximum | 8 hours | n/a (fixed in code) | Crew Orchestrator | `07-cross-cutting.md:46` |
| Crew members at once, application maximum | 8 | n/a (fixed in code) | Crew Orchestrator | `07-cross-cutting.md:48` |
| Same member failing in a row before escalation | 2 | n/a (fixed in code) | Crew Orchestrator | `07-cross-cutting.md:47` |
| Run idle timeout (no harness event) | 10 minutes *(chosen here)* | n/a (fixed in code) | Session Supervisor | `02-architecture.md:486` |
| Run wall-clock, application maximum | 8 hours *(chosen here)* | n/a (fixed in code) | Session Supervisor | `02-architecture.md:486` |
| Journal size before pruning | 100,000 runs | n/a (fixed in code) | Journal and Artefact Store | `07-cross-cutting.md:42` |
| Log retention | 14 days | n/a (fixed in code) | ordin server; desktop core logging | A-09 |
| Workflow file size; nesting depth; steps and blocks; call depth | 256 KB; 4; 50; 3 | n/a (fixed in code) | Ordin Host | `02-architecture.md:343`, `08-interfaces.md:18` |
| Report limits | 12 parts, 2 MB; table 10,000 rows × 30 columns; chart 8 series, 5,000 points; 8 key figures | n/a (fixed in code) | Block Renderer | A-34 |
| Reused answer lifetime | 7 days | n/a (fixed in code) | Usage Planner | ADR-062, `04-domain.md:188` |
| Break after which a conversation continues from a summary | 1 hour | n/a (fixed in code) | Ordin Host | ADR-062 |
| Task size that triggers an estimate | more than 20 documents | n/a (fixed in code) | Usage Planner | ADR-062 |
| Organisation profile refresh | at start and every 60 minutes *(chosen here)* | n/a (fixed in code) | Organisation Profile | `02-architecture.md:702` ("periodically") |
| New-email check; handover check; repository fetch for originals | at start, on workspace open, every 15 minutes | n/a (fixed in code) | Mail Service; Handover Service; Project Service | A-37, `02-architecture.md:735`, A-36 |
| Team knowledge folder check | when the workspace opens, at start, and every 15 minutes while ordin runs (*amended January 2026, ADR-103*) | n/a (fixed in code) | Team Knowledge Service | ADR-103 item 5; A-52 |
| Team knowledge fetch | 120 s timeout per fetch; 3 attempts on a network error, 2 s then 8 s apart (*chosen here*; ADR-103) | n/a (fixed in code) | Team Knowledge Service | ADR-103 |
| Team share rebuilds when the branch moves | at most 3 per share (*chosen here*; ADR-103) | n/a (fixed in code) | Team Knowledge Service | ADR-103 |
| Team knowledge folder, soft size | 20,000 fact files; above it a get runs and the status warns once (*chosen here*; ADR-103, SA-TEAM-07) | n/a (fixed in code) | Team Knowledge Service | A-52; SA-TEAM-07 |
| Team knowledge clone | bare clone at `<data root>/team-knowledge/<WorkspaceId>.git` (*chosen here*; ADR-103) | n/a (fixed in code) | Team Knowledge Service | ADR-103 |
| Slack Thread Adapter minimum poll interval | 10 seconds (SA-RATE-01; amended January 2026, decision 80) | n/a (fixed in code) | Slack Thread Adapter | `07-cross-cutting.md:17` |
| Linked Slack channel reading interval | every 60 seconds, never under 10 seconds per conversation; `Retry-After` honoured (30 s when absent) | n/a (fixed in code) | Slack Service | SA-RATE-02, A-51 |
| Linked Slack channel catch-up at start | at most 1,000 messages per channel | n/a (fixed in code) | Slack Service | SA-RATE-02 |
| Slack post retries | one, only after HTTP 429; none after no answer | n/a (fixed in code) | Slack Service | SA-RATE-02, 04:200 |
| Hook deadline | 50 seconds (wire types, LW-22); ordin writes the harness hook timeout as 60 seconds in the settings it passes with `--settings`, so the hook always answers first | n/a (fixed in code) | Hook Relay | `02-architecture.md:522`, `09-decisions.md:849` |
| Craft CLI time limit per call | 120 seconds *(chosen here)* | n/a (fixed in code) | Craft Toolkit | `07-cross-cutting.md:57` |
| Sandbox calls per run; per session | 50; 500 | n/a (fixed in code) | Sandbox | `sandbox/sandbox-host/src/config.rs:148`–`149` |
| Sandbox response size; guest memory; guest deadline | 1 MiB; 512 MiB; 30 seconds | n/a (fixed in code) | Sandbox | `sandbox/sandbox-host/src/config.rs:157`, `:172`–`173` |
| Remote API heartbeat | an SSE comment every 20 seconds | n/a (fixed in code) | Remote API | `02-architecture.md:578`, ADR-096 item 3 |
| Server marked **Not connected** | 45 seconds without an event or heartbeat | n/a (fixed in code) | Server Connector | `02-architecture.md:588` |
| Server Connector reconnect backoff | 1 second to 60 seconds, never giving up while open | n/a (fixed in code) | Server Connector | `02-architecture.md:587` |
| Desktop pairing code | valid 10 minutes; 5 wrong codes in 10 minutes stops pairing | n/a (fixed in code) | Remote API | ADR-096 item 5 |
| Failed desktop-key checks | 10 a minute from one address, then that address refused for 10 minutes | n/a (fixed in code) | Remote API | `02-architecture.md:580` |
| Desktop key | 32 random bytes, kept on the server as a hash | n/a (fixed in code) | Remote API | ADR-096 item 5 |
| Remote API transport | HTTP/2, TLS 1.3, path prefix `/v1/` | n/a (fixed in code) | Remote API | `08-interfaces.md:39` |
| Service API compatibility | same major, same or previous minor | n/a (fixed in code) | Server Connector | `08-interfaces.md:38` |
| Telegram pairing code | valid 10 minutes; 5 wrong codes in 10 minutes stops pairing | n/a (fixed in code) | Telegram Adapter | `02-architecture.md:606`, ADR-073 |
| Slack pairing code | valid 10 minutes | n/a (fixed in code) | Slack Channel Adapter | ADR-076 *Pairing* |
| Telegram limits | send files ≤ 50 MB; receive ≤ 20 MB; button data ≤ 64 bytes; about 1 message a second per chat; text ≤ 4,096 characters; caption ≤ 1,024 | n/a (fixed in code) | Telegram Adapter | `02-architecture.md:605`, C-52, ADR-073 *Status* |
| Slack limits | text ≤ 4,000 characters; ≤ 50 blocks; button value ≤ 2,000 characters | n/a (fixed in code) | Slack Channel Adapter | ADR-076 *Status* |
| Telegram reconnect backoff | 1 second to 5 minutes, with jitter, never giving up | n/a (fixed in code) | Telegram Adapter | `03-sequences.md:965` |
| Slack reconnect backoff | 1 second to 5 minutes, never giving up | n/a (fixed in code) | Slack Channel Adapter | `02-architecture.md:611` |
| NATS reconnect backoff | 1 to 60 seconds, exponential with jitter, never giving up; retry on first connect | n/a (fixed in code) | NATS Adapter | `02-architecture.md:639`, C-53 |
| Channel outage reported on the other channel | after 10 minutes, once | n/a (fixed in code) | Always on | ADR-076, `03-sequences.md:1084` |
| Hosted agent's Slack channel list check | once an hour | n/a (fixed in code) | Slack Channel Adapter | `02-architecture.md:610` |
| Trigger queue | 1 run at a time per trigger; up to 10 waiting | n/a (fixed in code) | Trigger Service | `02-architecture.md:633` |
| Daily triggered-run reset | midnight, server time zone | n/a (fixed in code) | Trigger Service | ADR-074 *Limits* |
| Ordin asked about new Slack messages | at most once per link every 5 minutes; at most 20 times a day per installation | n/a (fixed in code) | Slack Service | SA-RATE-02, R-98 |
| Slack rule daily cap | 1–20 posts a day per rule, default 20; reset at midnight, local time | n/a (bounds fixed in code) | Slack Service | SA-RATE-02, ADR-102 |
| Event payload size | the smaller of 1 MB and the NATS server's maximum | n/a (fixed in code) | NATS Adapter | ADR-074 *Status*, `02-architecture.md:641` |
| Workflow input from an event | text ≤ 4,000 characters | n/a (fixed in code) | Trigger Service | `02-architecture.md:631` |
| Event identifiers kept | 7 days | n/a (fixed in code) | Trigger Service | `02-architecture.md:635` |
| systemd unit restart | 5 seconds, doubling to 5 minutes (`RestartSteps=`, `RestartMaxDelaySec=`, systemd 254+) | n/a (fixed in code) | Always on | `02-architecture.md:626`, ADR-073 *Status* |
| Watchdog signal | every 30 seconds | n/a (fixed in code) | Always on | `02-architecture.md:626` |
| Idle harness session stopped | 30 minutes without a turn | n/a (fixed in code) | Always on | `02-architecture.md:626` |
| Harness sign-in check | every 6 hours and before each harness session | n/a (fixed in code) | Always on | `02-architecture.md:626`, ADR-076 |
| Journal compaction | weekly, 04:00 server time, when no run is going | n/a (fixed in code) | Always on | ADR-076 |
| Run lease | 30 seconds, renewed | n/a (fixed in code) | Run Queue | wrightrun `RunQueue.md:92` (SA-77), `02-architecture.md:796` |
| Missed schedule firing | older than 1 minute is not made up | n/a (fixed in code) | Duty Timer | `02-architecture.md:800` |
| Intake shared secret | at least 32 characters | n/a (fixed in code) | Caller API | wrightrun SA-58 (EnvironmentConfiguration row `intakes[].secret-env`) |
| Desktop update download *(amended January 2026, ADR-114; chosen here)* | at most 512 MiB and 30 minutes | n/a (fixed in code) | Ordin Desktop | OrdinDesktop.md DSK-UP-11 |
| Server update download *(amended January 2026, ADR-114; chosen here)* | at most 512 MiB and 10 minutes | n/a (fixed in code) | Setup Assistant | ADR-114 item 5; SetupAssistant.md SET-UP-17 |
| Server update taken as abandoned *(amended January 2026, ADR-114; chosen here)* | a `running` update older than 30 minutes | n/a (fixed in code) | Setup Assistant | SetupAssistant.md SET-UP-15 |
| Kept release notes *(amended January 2026, ADR-114; chosen here)* | newest 5 versions | n/a (fixed in code) | Setup Assistant | ADR-114 item 4; SetupAssistant.md SET-UP-4 |

## Public Interface

This concern adds no function. Its interface is the set of reads each program performs at start and the rule each owning module applies at use:

| Entry | Program | Called by | Contract |
|---|---|---|---|
| `OrganisationProfileService::start(ProfileMode::Read { … }, ProfileOptions { refresh_interval: 60 min, … })` | D, P | ordin desktop program, ordin server program (`start`) | Loads `installer.json` (or the build defaults), the last verified profile from the datastore, then refreshes in the background; never fails for network reasons. |
| `OrganisationProfileService::start(ProfileMode::NotUsed, …)` | A | ordin server program (`agent start`) | Every profile key at its 2D-2 default; no fetch, no tables. |
| `OrganisationProfileService::current()` / `subscribe()` | D, P, A | every module that owns a 2D-2 key | The module reads its own keys from the snapshot at the moment of use; a new snapshot is applied at the next use (2D *Configuration sources*, desktop item 4). |
| `ServerEnv::read()` | P, A | ordin server program, before anything else | Reads every 2D-1a variable once; a malformed value stops the program with `SERVER_ENV_INVALID` naming the variable. |
| `load_config(path)` | A | Agent Loader at `agent start`, Release Builder at publish | Reads and checks the agent configuration file; any refusal stops the start with a `CFG_*` code. |
| `server_secret_backend(dbus, key_file, key_credential)` | P, A | ordin server program | Chooses the secret back end: a key variable → encrypted file; otherwise the session's Secret Service; neither → refusal. |
| `IdentityService` with `SecretBackendConfig::OsKeychain` | D | ordin desktop program | Secrets in the OS keychain under the branded build's bundle identifier. |

### Example

A personal server started under systemd with:

```
ORDIN_DATA_DIR=/home/asha/.local/share/ordin
ORDIN_SECRETS_KEY_CREDENTIAL=/home/asha/.config/ordin/secrets.cred
ORDIN_LOG_LEVEL=info
```

and a verified profile holding `limits.crew.rounds: 20` while the person's stored crew-round choice is 25 resolves, at the next crew start, to 20: the application maximum is 30 (2D-4), the company maximum 20 (2D-2), and the person's 25 is clamped to 20 and lowered in storage. The key file's path may be logged; its content never is. A second program started with both `ORDIN_SECRETS_KEY_FILE` and `ORDIN_SECRETS_KEY_CREDENTIAL` set stops with exit status 1 and `error[CRED_SECRET_KEY_CONFLICT]: Set ORDIN_SECRETS_KEY_FILE or ORDIN_SECRETS_KEY_CREDENTIAL, not both.` — Identity and Credentials' code, passed through by the server program.

## Internal Logic

**A. Desktop start (ordin desktop program).**
1. Construct `IdentityService` with `SecretBackendConfig::OsKeychain`; a locked or refused keychain → `CRED_KEYCHAIN_UNAVAILABLE` (retryable; the Setup Assistant shows it).
2. Start `OrganisationProfileService` in `Read` mode. `installer.json` unreadable or invalid → logged `PROF_INSTALLER_INVALID` and the build defaults are used. No profile address at all → `PROF_NO_ADDRESS` (setup cannot finish, R-11).
3. The cached verified profile (if any) becomes the first snapshot at once; the fetch runs in the background every 60 minutes and on demand. A fetched profile that fails size, signature, key, format, content or version checks is refused (`PROF_TOO_LARGE`, `PROF_SIGNATURE_MISSING`, `PROF_SIGNATURE_INVALID`, `PROF_KEY_UNKNOWN`, `PROF_FORMAT_UNSUPPORTED`, `PROF_CONTENT_INVALID`, `PROF_VERSION_NOT_NEWER`) and the last verified profile stays active.
4. Once a verified profile names `updates.feed_url`, that address replaces the installer's `update_feed_url`.
5. Each module owning a person-adjustable setting reads its stored value, accepts a change only where the profile lets the person change it (`usage.person_may_change`, or the equivalent switch), and clamps it to the company maximum, then the application maximum. A refused change returns the owner's code (for example `USAGE_FIXED_BY_COMPANY`, `USAGE_ABOVE_PROFILE_LIMIT`) and the setting is not saved.
6. The desktop reads no environment variable of its own. It removes credential variables from each harness's environment (2D-1c).

**B. Personal server start (`ordin-server start`).**
1. `ServerEnv::read()`; malformed → `SERVER_ENV_INVALID`; both key variables are kept as read (`secrets_key_file`, `secrets_key_credential`) and judged in step 2; data root not owned or not writable → `SERVER_DATA_ROOT_UNWRITABLE`. Each is exit status 1 before any module is constructed.
2. `server_secret_backend(DBUS_SESSION_BUS_ADDRESS, ORDIN_SECRETS_KEY_FILE, ORDIN_SECRETS_KEY_CREDENTIAL)` chooses the back end: the encrypted file when either key variable is set, otherwise the Secret Service when a session bus is set. Its refusal stops the start with exit status 1 and is passed through unchanged: both key variables → `CRED_SECRET_KEY_CONFLICT`; neither and no session bus → `CRED_SECRET_KEY_MISSING` (OrdinServer.md Internal Logic 4). The encrypted file is `<ORDIN_DATA_DIR>/secrets/secrets.enc`, mode `0600`, XChaCha20-Poly1305 (05 — *Server secrets file*).
3. Read the `server_profiles` row; none → `SERVER_NOT_SET_UP`. Setup-command values are read by the module that stored each of them (Applicability, personal server).
4. Steps A.2–A.5 apply. With no organisation profile, A-40 lets the server owner choose the chat channels and every other profile key takes its default.

**C. Agent profile start (`ordin-server agent start`).**
1. `ServerEnv::read()` as B.1; `ORDIN_RELEASE_DIR` missing for `agent start` → `SERVER_ENV_INVALID`.
2. The Agent Loader reads the release and calls `load_config` on the configuration file it carries; any refusal (`CFG_FILE_NOT_FOUND`, `CFG_NOT_YAML`, `CFG_UNKNOWN_KEY`, `CFG_MISSING_KEY`, `CFG_INVALID_VALUE` and the other `CFG_*` rows of the Agent Configuration Error Table) stops the start; the instance never reports ready.
3. A credential the configuration names is read only from `ORDIN_SECRETS_DIR/<NAME>` (scaled shape) or the secret store (single shape), never from ordin's own environment (2D-1d).
4. `OrganisationProfileService` starts with `ProfileMode::NotUsed`.
5. The program's `server_settings` table holds one key, `agent_instance_id` (a UUIDv7 minted on the first `agent start` or `agent db apply`, written once); a stored value that is not a UUID → `SERVER_START_FAILED` naming `SERVER_SETTINGS_INVALID` (OrdinServer.md SA-CLOSE-04, Error Table).

**D. Validation at start, every program.** Every source is checked in full before the program serves its first request or starts its first run; there is no partly-applied configuration. A refused organisation profile is not a start failure (the last verified one, or the defaults, apply); every other refusal above is.

**E. Secrets.**
1. No setting in any source holds a secret value. Secrets live in: the OS keychain (desktop), the Secret Service or the encrypted file (personal server, single shape), files under `ORDIN_SECRETS_DIR` or the file named by an `ORDIN_*_FILE` variable (scaled shape).
2. Environment variables carry the *path* of a secret, never its value (`ORDIN_DATABASE_URL_FILE`, `ORDIN_MODEL_KEY_FILE`, `ORDIN_TOKEN_FILE`, `ORDIN_SECRETS_DIR`).
3. The start-up log line and the start-up report name each variable that was read and its non-secret value; for a secret path they log the path only, and for a secret value nothing (Logging spec: tokens and secrets are never logged). `ordin-server setup secret` reads the value with echo off (`rpassword`) and never accepts it on the command line.
4. Key material is wiped from memory after use with `zeroize` in Identity and Credentials; the server's control socket zeroes a secret request line after parsing (OrdinServer.md).

## Data Model

N/A for this concern — it owns no table. The tables that hold configuration belong to their modules: `organisation_profiles`, `remote_agents` (Organisation Profile), `server_profiles`, `server_settings` (ordin server program; one key, `agent_instance_id`), `projects.check_commands` (Project Service), `usage_settings` (Usage Planner), `locale_preferences` (Localisation), `setting_values` (Settings Store, agent profile), `credentials`, `credential_meta` (Identity and Credentials). Their DDL is in those specs; the migration rules are in `DatabaseMigrations.md`.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A variable of ordin's own is malformed, or `ORDIN_RELEASE_DIR` missing for `agent start` | exit 1, `Err(permanent)` | `SERVER_ENV_INVALID` | `error[SERVER_ENV_INVALID]: ORDIN_LOG_LEVEL must be error, warn, info or debug.` |
| Both secrets key variables set (Identity and Credentials' check, passed through by the server program) | exit 1, `Err(permanent)` | `CRED_SECRET_KEY_CONFLICT` | `error[CRED_SECRET_KEY_CONFLICT]: Set ORDIN_SECRETS_KEY_FILE or ORDIN_SECRETS_KEY_CREDENTIAL, not both.` |
| No Secret Service and no key variable (Identity and Credentials' check, passed through) | exit 1, `Err(permanent)` | `CRED_SECRET_KEY_MISSING` | `error[CRED_SECRET_KEY_MISSING]: No secret store: run a Secret Service or set ORDIN_SECRETS_KEY_CREDENTIAL or ORDIN_SECRETS_KEY_FILE.` |
| Data root not owned by the user or not writable | exit 1, `Err(permanent)` | `SERVER_DATA_ROOT_UNWRITABLE` | `error[SERVER_DATA_ROOT_UNWRITABLE]: …` |
| No server profile | exit 1, `Err(permanent)` | `SERVER_NOT_SET_UP` | `error[SERVER_NOT_SET_UP]: Run ordin-server setup first.` |
| `server_settings.agent_instance_id` holds a value that is not a UUID | exit 1, `Err(permanent)` | `SERVER_START_FAILED` (`SERVER_SETTINGS_INVALID`) | `error[SERVER_START_FAILED]: server could not start (SERVER_SETTINGS_INVALID).` |
| Check commands over 20, or one over 1,000 characters | `Err(permanent)`; Service reply `error` | `PROJ_CHECKS_INVALID` | `{"request_id":…,"kind":"error","error":{"code":"PROJ_CHECKS_INVALID","message":"…"}}` |
| Fetched profile's signature fails or its key is unknown | `Err(permanent)` → `Refused`; last verified profile kept | `PROF_SIGNATURE_INVALID` / `PROF_KEY_UNKNOWN` | Diagnostics line; no reply to the person unless they asked for the refresh |
| Fetched profile has an unknown key, a wrong type or a broken bound | `Err(permanent)` → `Refused`; last verified profile kept | `PROF_CONTENT_INVALID` | as above |
| No profile address known (desktop) | `Err(permanent)` | `PROF_NO_ADDRESS` | `{"request_id":…,"kind":"error","error":{"code":"PROF_NO_ADDRESS","message":"…","action":"send_to_it"}}` |
| `installer.json` unreadable or invalid | logged; build defaults used | `PROF_INSTALLER_INVALID` | — |
| Agent configuration file has an unknown key | `Err(permanent)`; start stops | `CFG_UNKNOWN_KEY` | `error[CFG_UNKNOWN_KEY]: …` |
| Agent configuration file lacks a required key | `Err(permanent)`; start stops | `CFG_MISSING_KEY` | `error[CFG_MISSING_KEY]: …` |
| The person changes a setting the company fixed | `Err(permanent)`; Service reply `error` | `USAGE_FIXED_BY_COMPANY` | `{"request_id":…,"kind":"error","error":{"code":"USAGE_FIXED_BY_COMPANY","message":"…"}}` |
| A value above the company's frame | `Err(permanent)`; Service reply `error` | `USAGE_ABOVE_PROFILE_LIMIT` | as above with `USAGE_ABOVE_PROFILE_LIMIT` |
| Keychain or Secret Service locked, refused or unreachable | `Err(retryable)` | `CRED_KEYCHAIN_UNAVAILABLE` | as above, action `try_again` |
| Encrypted secrets file cannot be decrypted or has an unknown format | `Err(permanent)` | `CRED_SECRET_STORE_CORRUPT` | start stops; `error[…]` line |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Configuration

  Scenario: Happy path — a company maximum clamps the person's stored choice
    Given the application maximum for crew rounds is 30
    And the active verified profile sets limits.crew.rounds to 20
    And the person's stored crew-round choice is 25
    When a crew run starts
    Then the crew run uses 20 rounds
    And the person's stored value is lowered to 20

  Scenario: Edge case — a refused profile keeps the last verified one
    Given the active profile has profile_version 7
    When a fetched profile with profile_version 8 fails its signature check
    Then the refresh outcome is refused with code PROF_SIGNATURE_INVALID
    And the snapshot still holds profile_version 7

  Scenario: Edge case — the agent profile reads no organisation profile
    Given the server program is started with "agent start"
    When the Organisation Profile is constructed
    Then it is started with ProfileMode::NotUsed
    And every 2D-2 key holds its default

  Scenario: Error path — both secrets key variables are set
    Given ORDIN_SECRETS_KEY_FILE and ORDIN_SECRETS_KEY_CREDENTIAL are both set
    When "ordin-server start" runs
    Then the program exits with status 1 and code CRED_SECRET_KEY_CONFLICT
    And the journal is not opened

  Scenario: Error path — an unknown key in the agent configuration file
    Given the agent configuration file holds the key "modle"
    When "ordin-server agent start" loads the release
    Then the start stops with code CFG_UNKNOWN_KEY
    And the instance never reports ready

  Scenario: Security — a secret value never appears in the start-up log
    Given ORDIN_MODEL_KEY_FILE names a file holding a model key
    When the agent profile starts
    Then the start-up log line holds the path of ORDIN_MODEL_KEY_FILE
    And no log line holds the file's content
```

## Performance, Security, Observability

- **Performance targets:** `ServerEnv::read()` and the secret back-end choice finish within 50 ms (no network). Reading a snapshot is a lock-free `Arc` clone (`watch::Receiver::borrow`), so a module's per-use read adds no measurable latency to A-03's 150 ms. The profile fetch runs in the background and never delays start to an interactive Start view (A-03: under 3 s).
- **Security:** no setting holds a secret; secret values reach a module only through Identity and Credentials or a secret file path; the profile is accepted only with a valid Ed25519 detached JWS from a key the installer carries (at most two, for rotation; SA-SIGN-01); the person's values are clamped to company and application maxima in fixed code; harness environments are stripped of ordin's credential variables (2D-1c).
- **Observability:** a malformed variable is named in its `SERVER_ENV_INVALID` line; profile refresh outcomes and refusals logged by the Organisation Profile with their `PROF_*` code; in the agent profile the start-up report lists every configuration refusal (Agent Loader).

## Keys introduced in Phase 3

Enumerated by comparing every `ORDIN_*` variable and every dotted profile-key path in `components/*.md` with the 2D tables above. Most were added to 2D during the Phase 3 corrections (decision 80) and the Phase 3 closing round, so they already appear in the copied tables; the `ORDIN_MCP_*` variables and the per-project check commands were added to the copied tables in Phase 4. Each is listed here with the spec that introduced it.

| Key or variable | Kind | Type | Default | Required | Owner | Introduced by |
|---|---|---|---|---|---|---|
| `gitlab.host` | profile key | string (host name, no scheme or path) | `gitlab.com` | No | Identity and Credentials (owner); Project Service reads it (C-1 resolved) | IdentityAndCredentials.md, ProjectService.md (K10, decision D9) |
| `gitlab.client_id` | profile key | string, 1–256 characters | none (absent → personal access token pasted) | No | Identity and Credentials | IdentityAndCredentials.md (K10) |
| `craft.apps[]` (`app_id`, `version`, `allowed_tools[]`, `platforms.<platform>.{asset_url, sha256, file_name}`) | profile key | list of objects; platform ∈ `macos_arm64`, `macos_x86_64`, `windows_x86_64`, `linux_x86_64` | empty | No | Craft Toolkit | CraftToolkit.md, SetupAssistant.md (K9, decision 80) |
| `language_servers[]` (`language`, `name`, `version`, `platforms.<platform>.{asset_url, sha256, file_name, binary_path}`) | profile key | list, at most one entry per `go`, `rust`, `java`, `typescript` | none: the build's `language-servers.json` | No | Organisation Profile (shape), Setup Assistant (install), Project Service (use) | OrganisationProfile.md SA-CLOSE-03 (K12); replaces `language_servers.<language>.approved[]` / `.default` |
| `engineering.coding_tasks.max_attempts` | profile key | integer ≥ 1 | `3` | No | Project Service | ProjectService.md (decision 80) |
| `slack.client_id` | profile key | string | none | No | Identity and Credentials | IdentityAndCredentials.md (decision 80, H08-9) |
| `google.client_secret` | profile key | string (the non-confidential secret of a Google Desktop OAuth client) | none | No | Identity and Credentials | IdentityAndCredentials.md (decision 80, C-44) |
| `confluence.token_exchange_url` | profile key | URL, `https` only | none (public client with PKCE) | No | Identity and Credentials | OrganisationProfile.md (decision 80, C-12) |
| `team_knowledge.read_folders[]` | profile key | list of `CompanyReadFolder` (OrganisationProfile.md amendment January 2026) | empty | No | Team Knowledge Service (reader); Organisation Profile (validation) | OrganisationProfile.md, TeamKnowledgeService.md (decision 90, ADR-110) |
| `knowledge_agents.front_doors[]` | profile key | list of `CompanyFrontDoor` {`key`, `display_name`, `url`, `applies_to`} (OrganisationProfile.md amendment January 2026) | empty | No | Knowledge Query Client (reader, through the program); Organisation Profile (validation) | OrganisationProfile.md, KnowledgeQueryClient.md (decision 92, ADR-113) |
| `knowledge-agent` (`name`; `repositories[]` and `glossaries[]` each {`url`, `path`, `branch`, `credential-env`, `audience-may-read`}; `audiences[]`; `group-claim`; `front-doors[]`; `proxies[]` {`name`, `url`, `audience`}; `max-proxies` (default 8); `service-identity` {`token-url`, `client-id`, `client-secret-env`, `scope`}; `reading.daily-documents` (default 1,000); `fetch-interval-minutes` (default 15); `hot-copy.asks-per-day` (default 200), `hot-copy.credential-env`; `mcp.enabled` (default `true`); `sources-record.enabled` (default `false`), `sources-record.retention-days` (default 365)) | agent configuration key (optional section) | object; bounds in AgentConfiguration.md amendment January 2026 | absent: the agent is not a knowledge agent | No; when present, `auth` is required | Agent Configuration (owner); Knowledge Reader, Knowledge Query Server, Knowledge Query Client, Caller Authentication read it | AgentConfiguration.md (decision 92, ADR-113) |
| `server_settings.knowledge_scope_id` | server program table row | text, a UUID, written once | minted on the first start of an agent whose configuration has `knowledge-agent` | Yes in a knowledge agent | ordin server program | OrdinServer.md amendment January 2026 (ADR-113) |
| `profile_version` | profile key | string `MAJOR.MINOR.PATCH`, strictly higher than the version in force | — | Yes in every profile document | Organisation Profile | OrganisationProfile.md (decision 80) |
| `agent_catalogue.repositories[].key` | profile key | string | none | No | Catalogue Service | CatalogueService.md (decision 80) |
| `ORDIN_MCP_ENDPOINT` | variable ordin sets in each MCP-bridge server's environment | path: `<app data root>/run/mcp.sock` (mode `0600`); Windows `\\.\pipe\ordin-mcp-<installation id>` | set per session | Yes, for `ordin-hook mcp-bridge` | MCP Front | McpFront.md, OrdinHook.md — added to 2D-1c above (C-5) |
| `ORDIN_MCP_SERVER` | as above | string `^[a-z][a-z0-9_]{0,63}$`, the MCP server name the bridge serves | set per bridge | Yes, for the bridge | MCP Front | McpFront.md, OrdinHook.md — added to 2D-1c above (C-5) |
| `ORDIN_MCP_TOKEN` | as above (secret) | 32 random bytes, base64url without padding (43 characters), one per session | regenerated per session | Yes, for the bridge | MCP Front | McpFront.md, OrdinHook.md, ClaudeLocalAdapter.md, CopilotLocalAdapter.md — added to 2D-1c above (C-5) |
| `ORDIN_SANDBOX_PY_DIGEST` | build-time variable (`env!` in `ordin-sandbox`, set by its `build.rs`) | SHA-256 hex of the Python guest | computed at build | Yes, at build | Sandbox | Sandbox.md — not a runtime setting; its full row is in 2D-1g *Build-time variables* above |
| `limits.max_receive_file_bytes` | channel adapter constant, not a profile key | `u64`: Telegram 20,971,520 bytes, Slack 104,857,600 bytes | fixed per adapter | — | Telegram Adapter, Slack Channel Adapter (`limits()`) | ChannelGateway.md, TelegramAdapter.md, SlackChannelAdapter.md |
| `limits.max_waiting` | trigger field, not a profile key | integer 0–10 | `10` | No | Trigger Service | TriggerService.md |
| `projects.check_commands` | per-project setting set by `projects.set_check_commands` | JSON array of strings, at most 20, each at most 1,000 characters | `[]` (no checks) | No | Project Service | ProjectService.md SA-CLOSE-03; OrdinDesktop.md SA-CLOSE-10, OrdinServer.md SA-CLOSE-08 (no program sets it); added to 2D-3 above |
| `server_settings.agent_instance_id` | server program table row | text, a UUIDv7, 1–256 characters (`CHECK`), written once | minted on the first `agent start` or `agent db apply` | Yes in the agent profile | ordin server program | OrdinServer.md SA-CLOSE-04, migration `0002_server_settings` |
| `ORDIN_SECRETS_KEY_FILE`, `ORDIN_SECRETS_KEY_CREDENTIAL` as `ServerEnv.secrets_key_file`, `ServerEnv.secrets_key_credential` | variables (2D-1a) | path each; both kept as read | none | see 2D-1a | ordin server program reads; Identity and Credentials judges | OrdinServer.md Public Interface (replaces `ServerEnv.secrets_key: Option<SecretsKeySource>`) |

Paths such as `workflows.run`, `telegram.link`, `usage.today` or `nats.connected` that the grep also returns are Service API command names or log event names, not configuration keys, and are not listed.

## Defects found while compiling this spec

Re-checked on January 2026 against the current component specs. All five are resolved; C-1, C-3 and C-5 were closed in the correction round of January 2026 in the specs named.

| ID | Finding | Status | Evidence | Recommended owner of the fix |
|---|---|---|---|---|
| C-1 | `gitlab.host` has two owners in 2D-2 (Identity and Credentials, Project Service); the self-audit rule is one owner per key. | Resolved | phase-2-architecture.md 2D-2 row `gitlab.host` and the copy above name Identity and Credentials the owner and the Project Service a reader; ProjectService.md SA-CLOSE-01 already reads only the host | — |
| C-2 | The Copilot harness key under `usage.level_models` was spelt `copilot_cli` in one place. | Resolved | CopilotLocalAdapter.md *Configuration* row "Model per level" reads `usage.level_models.copilot.<level>`, matching `HarnessName::Copilot` (OrganisationProfile.md) | — |
| C-3 | Model-name length differs: the profile accepts 1–128 characters, the adapters' `accepts_model` 1–100 characters. A 101–128-character name passed the profile and was refused at use. | Resolved | OrganisationProfile.md validation row `usage.level_models` now reads 1–100 characters, matching ClaudeLocalAdapter.md and CopilotLocalAdapter.md `HarnessModels::accepts_model` | — |
| C-4 | One condition had two codes (`SERVER_SECRETS_KEY_BOTH` / `CRED_SECRET_KEY_CONFLICT`; `SERVER_SECRETS_KEY_MISSING` / `CRED_SECRET_KEY_MISSING`). | Resolved (EH-D15) | OrdinServer.md Internal Logic 4 and Error Table pass Identity and Credentials' `CRED_SECRET_KEY_CONFLICT` and `CRED_SECRET_KEY_MISSING` through unchanged; the `SERVER_SECRETS_KEY_*` codes and `SecretsKeySource` are gone; `ServerEnv` carries `secrets_key_file` and `secrets_key_credential` | — |
| C-5 | `ORDIN_MCP_ENDPOINT`, `ORDIN_MCP_SERVER`, `ORDIN_MCP_TOKEN` are set by ordin in a harness-started program's environment but are absent from 2D-1c. | Resolved | McpFront.md *Bridge protocol*; the three rows (owner MCP Front) are in phase-2-architecture.md 2D-1c and in the copy above | — |

## Gaps

None.
