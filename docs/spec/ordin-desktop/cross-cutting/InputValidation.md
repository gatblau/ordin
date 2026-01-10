# SPEC: InputValidation

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** `ordin-types` (`text.rs`, `paging.rs`, `error.rs`, `yaml.rs`: `read_restricted`, `read_restricted_node`, `normalise_text`, `check_len`, `check_display_name`, `check_abs_path`, `check_https_url`, `Cursor::decode`, `TypesError`, `CodedError`); `ordin-service-api` (`accept`, `Input::check`, `check_block`); each entry-point crate for its own surface (`ordin-command-bridge`, `ordin-remote-api`, `ordin-caller-api`, `ordin-hook-relay`, `apps/hook`, `ordin-mcp-front`, and the file-format readers listed in *Internal Logic* step 5).

## Purpose

Fixes one rule set for every value that enters the core from outside — a command from the WebView or a paired desktop, a Caller API request, a hook request, an MCP tool call, a file ordin reads (workflow, charter, agent part, profile, archive, release), and an answer from an external system — so that each is refused at the boundary with a stable code before any module acts on it. Text that arrives from packages, remote agents, documents and adapters is data and never an instruction (HLD 07 — Input validation; SA-XC-12).

## Approach

Typed deserialisation at the boundary, then one `check()` per type, then module rules. Every type that enters from outside derives `serde::Deserialize` with `deny_unknown_fields`, is parsed with `serde_path_to_error` so the refusal names the JSON Pointer of the field (SA-API-02), and implements a `check()` that applies 2C's limits (LD-01..LD-56, LW-1..LW-47) through the shared helpers in `ordin-types`. Size is checked before parsing, from the declared length where the transport gives one, so an oversized body is never buffered. Rejected: a schema validator over `serde_json::Value` (a second definition of every type that can drift from the Rust one); a general validation-attribute library (`validator`), because the 2C rules carry per-field codes that the helpers already take as arguments (`AreaCodes`) and a derive library would add a second code scheme.

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | Command Bridge (every IPC command), hook requests, MCP tool calls, file formats at load, adapter answers. | SA-XC-12; CommandBridge.md, HookRelay.md, McpFront.md |
| Personal server (P) | Yes | As D, with the Remote API in place of the Command Bridge, plus chat updates (Channel Gateway) and NATS events (Trigger Service). | SA-XC-12; RemoteApi.md, ChannelGateway.md, TriggerService.md |
| Agent profile (A) | Yes | Caller API (wrightrun's bodies and limits, SA-ENV-02), intake signatures, agent configuration and release at load, hook and MCP as D. | SA-XC-12; CallerApi.md, AgentConfiguration.md, AgentLoader.md |

## Shared Context

**Conventions (2C wire §0, copied).**

| Convention | Rule |
|---|---|
| Unknown fields | Rejected (`#[serde(deny_unknown_fields)]`) on every type that enters the core from outside (Remote API bodies, hook requests, blocks, MCP tool arguments, workflow files). Types the core emits accept unknown fields on read. |
| Text | UTF-8, NFC-normalised at the boundary; control characters other than `\n` and `\t` are rejected. Text is always drawn as text (ADR-028, ADR-067). |
| Identifiers | UUIDv7, 36-character lowercase hyphenated form (SA-ID-01). |
| Timestamps | RFC 3339, UTC, millisecond precision, `Z` (SA-TIME-01); the Caller API keeps wrightrun's form. |

**`ordin-types` helpers (SharedTypes.md, copied).**

```rust
pub struct AreaCodes { pub name_empty: &'static str, pub name_too_long: &'static str,
                       pub path_invalid: &'static str, pub url_invalid: &'static str }
pub fn normalise_text(raw: &str) -> Result<String, TypesError>;
pub fn check_len(value: &str, min: usize, max: usize, code: &'static str) -> Result<(), TypesError>;
pub fn check_display_name(raw: &str, max: usize, codes: AreaCodes) -> Result<String, TypesError>;
pub fn check_abs_path(raw: &str, codes: AreaCodes) -> Result<(), TypesError>;
pub fn check_https_url(raw: &str, allow_loopback_http: bool, codes: AreaCodes) -> Result<url::Url, TypesError>;

pub enum TypesError { IdNotUuidV7, TimestampNotRfc3339MsUtc, HashNotSha256Hex, NumNotDecimal,
                      TextControlChar, BadCursor, Rule { code: &'static str } }
pub trait CodedError: std::error::Error + Send + Sync + 'static {
    fn code(&self) -> &'static str;
    fn retryable(&self) -> bool;
    fn repair(&self) -> Option<RepairAction> { None }
}

// module ordin_types::yaml (SharedTypes.md, P4-1): the one restricted YAML reader, over `saphyr`
pub fn read_restricted<T: serde::de::DeserializeOwned>(text: &str, max_bytes: usize) -> Result<T, YamlError>;
pub fn read_restricted_node(text: &str, max_bytes: usize) -> Result<YamlNode, YamlError>;   // scalars keep their source text
pub enum YamlError { FeatureRefused { feature: YamlFeature, line: u32 }, Malformed { line: u32 }, TooLarge { max_bytes: usize } }
pub enum YamlFeature { Anchor, Alias, MergeKey, CustomTag }
impl YamlError { pub fn code(&self) -> &'static str; }   // YAML_FEATURE_REFUSED | YAML_MALFORMED | YAML_TOO_LARGE
```

**Size limits per surface (copied from the owning specs).**

| Surface / input | Limit | Refusal | Owner spec |
|---|---|---|---|
| Service API command body (IPC and Remote API) | `MAX_BODY_BYTES` = 1 MiB; `MAX_TEXT_SAVE_BODY_BYTES` = 9 MiB for `files.save_edit`, `knowledge.save_edit`, `projects.save_file`, `localisation.import_language` and `files.save_export` (*amended January 2026, ADR-109*; the language file itself at most 2 MiB, `LOC_FILE_TOO_LARGE`) | `SVC_BODY_TOO_LARGE` (Remote API 422) | ServiceApi.md, CommandBridge.md, RemoteApi.md |
| Remote API pairing body | 4 KiB | `SVC_BODY_TOO_LARGE` | RemoteApi.md |
| Caller API start body | `MAX_START_BODY_BYTES` = 1,048,576 | 413 `SRV_BODY_TOO_LARGE` | CallerApi.md |
| Caller API decision body | `MAX_DECISION_BODY_BYTES` = 65,536; approver decision 8 KiB | 413 `SRV_BODY_TOO_LARGE` | CallerApi.md |
| Caller API operator body | `MAX_OPERATOR_BODY_BYTES` = 65,536 | 413 `OPS_BODY_TOO_LARGE` | CallerApi.md |
| Caller API intake body | the intake's limit | 413 `INTAKE_TOO_LARGE` | CallerApi.md |
| Caller API text | `MAX_TEXT_CHARS` = 262,144 | 400 `SRV_BAD_REQUEST` | CallerApi.md |
| Knowledge account links (route 41) *(added January 2026, ADR-115)* | 1–3 entries; `system` ∈ `github`, `gitlab`, `confluence`, each at most once; `token` 1–8,192 bytes | 400 `KQ_BAD_ACCOUNTS` | KnowledgeQueryServer.md |
| Knowledge sharing notices (routes 43, 44) *(added January 2026, ADR-115)* | body ≤ 1,048,576 bytes, JSON; signature or token compared in constant time before parsing | 401 `KQ_NOTICE_REJECTED` | SourceAccess.md; KnowledgeQueryServer.md |
| Asker statement *(added January 2026, ADR-115)* | `subject` 1–255 characters; ≤ 3 accounts, keys `github`, `gitlab`, `confluence`, values 1–256 characters; `issued_at` within 30 s past and 5 s ahead | 400 `KQ_ASKER_STATEMENT_INVALID` | 2C wire §12 |
| SharePoint delegation *(added January 2026, ADR-115)* | 1–16,384 bytes; from a member only | 400 `KQ_BAD_QUERY` / `KQ_DELEGATION_NOT_ALLOWED` | 2C wire §12 |
| Whoami answers (linking) *(added January 2026, ADR-115)* | account id 1–256 characters | the link is refused (`KQ_ACCOUNT_LINK_REFUSED` entry) | SourceAccess.md |
| Work-folder write | 10 MiB (10,485,760 bytes) | 413 `WF_TOO_LARGE` | FileService.md, CallerApi.md |
| Hook request line | one JSON object ≤ 1 MiB ending `\n` | `HOOK_REQUEST_TOO_LARGE` (hook side), `HOOK_MALFORMED` (relay side), both deny | OrdinHook.md, HookRelay.md |
| MCP bridge handshake | one line ≤ 1 KiB | `MCP_BRIDGE_MALFORMED` | McpFront.md |
| MCP tool arguments | ≤ 1 MiB serialised (LW-18) | `MCP_ARGUMENTS_TOO_LARGE` (`isError`); `RULE_ARGUMENTS_TOO_LARGE` (deny) | McpFront.md, RulesEngine.md |
| Block from a tool | ≤ 2 MiB | `BLOCK_TOO_LARGE` | ServiceApi.md |
| Workflow file (import, export) | ≤ 256 KB (`read_restricted`, `max_bytes` 262,144), YAML 1.2, no anchors, aliases or custom tags | `WFF_FILE_TOO_LARGE`, `WFF_FILE_MALFORMED` (mapped from `YAML_*`) | OrdinHost.md |
| Agent part file | ≤ 1 MiB (`read_restricted_node`, `max_bytes` 1,048,576) | `AM_FILE_TOO_LARGE`; YAML features `AM_YAML_FEATURE_REFUSED` (mapped from `YAML_FEATURE_REFUSED`) | AgentModel.md |
| Package declaration | ≤ 1 MiB (`read_restricted`, `max_bytes` 1,048,576; the one limit, wrightrun's, in PackageInspector.md and ReleaseBuilder.md), no anchors, aliases or custom tags | `PKI_DECLARATION_TOO_LARGE` | PackageInspector.md |
| Agent archive import (`.zip`) | no absolute path, no `..` segment, ≤ 2,000 entries, ≤ 50 MB unpacked | `AUTHR_ARCHIVE_INVALID` | AuthoringService.md |
| Agent release | part, manifest, entry count and unpacked size limits | `REL_TOO_LARGE` | AgentLoader.md |
| Format plug-in tree | ≤ 20,000 files or 256 MiB | `FMT_TREE_TOO_LARGE` | FormatPlugins.md |
| Organisation profile | document ≤ 1 MiB, signature ≤ 8 KiB; detached JWS Ed25519 | `PROF_TOO_LARGE`, `PROF_SIGNATURE_INVALID` | OrganisationProfile.md |
| File Service read or write | ≤ 64 MiB | `FILE_TOO_LARGE` | FileService.md |
| Paging cursor | ≤ 1,024 characters, base64url of the key | `SRV_BAD_CURSOR` | SharedTypes.md |

## Public Interface

The concern adds no function beyond those listed in *Shared Context*. Each entry point follows this contract:

| Entry point | Parse | Error shape returned |
|---|---|---|
| Command Bridge (Tauri IPC) and Remote API commands | `ordin_service_api::accept` → `serde_path_to_error` into the command's `Input` → `Input::check()` | `CommandReply` with `kind: "error"`, `error.code` (`SVC_INPUT_INVALID`, `SVC_BODY_TOO_LARGE`, `SVC_REQUEST_ID_MALFORMED`, `SVC_VERSION_MALFORMED`, `SRV_BAD_CURSOR`) (SA-ENV-01) |
| Caller API | size from `Content-Length`, then chunked read; body as wrightrun's types | `{"error": "<sentence>", "code": "<CODE>"}` with the registry status (SA-ENV-02) |
| Hook Relay | one line, `HookRequest` with `deny_unknown_fields`, MAC check | a `deny` reply carrying the code (2C hook protocol) |
| MCP Front | tool name, size, then the provider's typed arguments | an MCP result with `isError: true` and a fixed English text |
| File formats | size, then parser, then the format's checks | the module's `CodedError` (for example `WFF_*`, `AM_*`, `PROF_*`) |
| Adapter answers | provider body into a typed shape | the adapter's own code (for example `CONF_RESPONSE_INVALID`, `SP_RESPONSE_INVALID`, `HAA_EVENT_MALFORMED`, `CLA_STREAM_MALFORMED`) |

### Example

Input — a Remote API command whose input carries an unknown field:

```json
{"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","api_version":{"major":1,"minor":0},
 "backend":"0192a1c0-0d1e-7f20-8a31-4253647586aa","command":"runs.list",
 "input":{"workspace_id":"0192a1c0-0d1e-7f20-8a31-4253647586bb","colour":"red"}}
```

Output — status 422:

```json
{"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","kind":"error",
 "error":{"code":"SVC_INPUT_INVALID","message":"ordin couldn't read that request. Try again.","action":"try_again"}}
```

The pointer `/input/colour` is logged with the code (`svc.command`, `outcome` = the code); it is not put in the person's sentence.

## Internal Logic

1. **Size first.** Every surface compares the declared or read length with its limit (*Shared Context* table) before parsing. HTTP surfaces refuse on `Content-Length` without reading the body, otherwise read in chunks and stop as soon as the sum exceeds the limit. Line protocols (hook, MCP bridge) read at most limit + 1 byte.
2. **Typed parse.** Deserialise with `serde_path_to_error` into the boundary type (`deny_unknown_fields`). A failure carries the JSON Pointer of the field; the pointer is logged, never shown to the person.
3. **Normalise and check.** `Input::check()` (Service API), the Caller API's wrightrun checks, and each file format's checks call the `ordin-types` helpers: `normalise_text` (NFC; control characters other than `\n` and `\t` → `TEXT_CONTROL_CHAR`), `check_len` and `check_display_name` with the entity's `AreaCodes`, `check_abs_path` (absolute, 1–4096 bytes, no NUL), `check_https_url` (`https`, ≤ 2048 bytes; `http` to loopback only where 2D allows), identifier and timestamp parsers (`ID_NOT_UUIDV7`, `TS_NOT_RFC3339_MS_UTC`), `Cursor::decode` (`SRV_BAD_CURSOR`). The first failing rule is returned, except `check_block`, which collects up to 20 rejections with pointers so the model can correct them in one turn.
4. **References.** Identifiers in the input are resolved by the owning module (for example `BLOCK_REF_UNRESOLVED`, `APPR_CARD_REF_UNKNOWN`); an identifier the caller may not see answers as not found, never as forbidden, on the Caller API (`SRV_RUN_NOT_FOUND`: "Run absent or another caller's").
5. **File formats at load.** Charters (Ordin Host, `CHRT_*`), workflow files (Ordin Host, `WFF_*`, format 3, every version read from a repository as on import), package declarations (Package Inspector, `PKI_*`), agent parts in either format (Agent Model and Format Plug-ins, then the Checker on every part before it is offered, `AM_*`, `FMT_*`, `CHK_*`), the organisation profile (`PROF_*`, signature checked before any value is read), the agent configuration (`CFG_*`), agent releases (`REL_*`, digest checked before unpacking), archives (`AUTHR_ARCHIVE_INVALID`). Every YAML file is read with the one restricted reader `ordin_types::yaml::read_restricted` (typed) or `read_restricted_node` (untyped tree, Agent Model) over `saphyr`: the size is checked before parsing (`YAML_TOO_LARGE`), and anchors, aliases, merge keys and tags outside the YAML 1.2 core schema are refused (`YAML_FEATURE_REFUSED`; LW-46), as are several documents, duplicate keys and nesting over 64 (`YAML_MALFORMED`) (SharedTypes.md Internal Logic 10, P4-1). Each reading crate maps the three `YAML_*` codes to a code of its own — Agent Model `AM_YAML_FEATURE_REFUSED`, Ordin Host `WFF_FILE_TOO_LARGE` and `WFF_FILE_MALFORMED` — as SharedTypes.md requires (Agent Model, Ordin Host, Catalogue Service through the Agent Model's `parse_withdrawn_list`, Handover Service, Package Inspector, Connection Binder).
6. **Adapter answers.** Every answer from Slack, Graph, Confluence, Telegram, NATS, a hosted agent or a harness is parsed into a typed shape before use; a shape failure is the adapter's own code (*Public Interface*). A size above the adapter's limit is refused (`CONF_TOO_LARGE`, `SP_TOO_LARGE`, `NATS_PAYLOAD_TOO_LARGE`, `TGA_FILE_TOO_LARGE`, `SCA_FILE_TOO_LARGE`).
7. **Data, never instructions.** Text from packages, documents, chat, events and remote agents reaches a model only as content of a named, size-limited input; it never selects a tool, a rule or an approval (fixed-code decisions, ADR-085). Event content is at most 4,000 characters per input (HLD 07 threat model; Trigger Service).
8. **No model in a validation decision.** Every check above is Rust code whose result for a given input is the same every time (HLD 07 — Fixed-code decisions); model-scored checks are opinions and never gate.
9. **Logging.** A refusal is logged with the code and, for parse failures, the pointer; the refused value itself is never logged (Logging concern: no message content, file content or tool arguments).

## Data Model

N/A — validation holds no state.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Service API body not an envelope, unknown field, or input rule fails | 422 / `Err(permanent)` | `SVC_INPUT_INVALID` | `{"request_id":…,"kind":"error","error":{"code":"SVC_INPUT_INVALID","message":"…","action":"try_again"}}` |
| Service API body over its limit | 422 / `Err(permanent)` | `SVC_BODY_TOO_LARGE` | as above with `SVC_BODY_TOO_LARGE` |
| Caller API body not a JSON object or breaking *StartRun*/*Resume* | 400 | `SRV_BAD_REQUEST` | `{"error": "…", "code": "SRV_BAD_REQUEST"}` |
| Caller API body above the route's ceiling | 413 | `SRV_BODY_TOO_LARGE` | `{"error": "the body exceeds 1048576 bytes", "code": "SRV_BODY_TOO_LARGE"}` |
| Hook request line over 1 MiB, not JSON, or unknown fields | `deny` | `HOOK_REQUEST_TOO_LARGE` / `HOOK_MALFORMED` | hook reply `deny` with reason "ordin could not read this request" |
| MCP tool arguments above 1 MiB | `isError` | `MCP_ARGUMENTS_TOO_LARGE` | text "The arguments are larger than 1 MiB; pass a reference to the document instead." |
| Text with a control character other than `\n`, `\t` | `Err(permanent)` | `TEXT_CONTROL_CHAR` | `TypesError::TextControlChar` |
| Workflow file over 256 KB / malformed | Service reply `error` | `WFF_FILE_TOO_LARGE` / `WFF_FILE_MALFORMED` | `CommandReply` error |
| YAML anchor, alias, merge key or custom tag (restricted reader) | `Err(permanent)` | `YAML_FEATURE_REFUSED` | `YamlError::FeatureRefused { feature, line }`, mapped by the reading crate |
| YAML not well-formed, several documents, duplicate key, nesting over 64, wrong shape | `Err(permanent)` | `YAML_MALFORMED` | `YamlError::Malformed { line }`, mapped by the reading crate |
| YAML text longer than `max_bytes` | `Err(permanent)` | `YAML_TOO_LARGE` | `YamlError::TooLarge { max_bytes }`, mapped by the reading crate |
| YAML anchor, alias or custom tag in an agent part | `Err(permanent)` | `AM_YAML_FEATURE_REFUSED` | `AmError::YamlFeatureRefused { path, feature }` |
| Unsafe or oversized archive | `Err(permanent)` | `AUTHR_ARCHIVE_INVALID` | `{"error":"ordin couldn't open this archive safely.","code":"AUTHR_ARCHIVE_INVALID"}` |
| Provider answer does not parse | `Err(permanent)` | `CONF_RESPONSE_INVALID` / `SP_RESPONSE_INVALID` | adapter error |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: InputValidation

  Scenario: Happy path — a well-formed command is normalised and accepted
    Given a runs.list command whose input text field holds "Café"
    When the Service API accepts it
    Then the module receives the NFC form "Café"
    And the reply kind is "ok"

  Scenario: Edge case — an oversized body is refused before it is read
    Given the Caller API start route with a ceiling of 1,048,576 bytes
    When a request declares Content-Length 2,000,000
    Then the answer is 413 with code SRV_BODY_TOO_LARGE
    And no body bytes are read

  Scenario: Error path — an unknown field is refused with its pointer logged
    Given a Remote API command whose input carries the field "colour"
    When the Remote API receives it
    Then the answer is 422 with code SVC_INPUT_INVALID
    And the log line svc.command records outcome SVC_INPUT_INVALID
    And the person's sentence names no field, path or value

  Scenario: Error path — a YAML alias is refused by the restricted reader
    Given a workflow file whose YAML holds an anchor "&a" and an alias "*a"
    When the Ordin Host imports it with read_restricted
    Then read_restricted fails with YAML_FEATURE_REFUSED
    And the import is refused with WFF_FILE_MALFORMED
```

## Performance, Security, Observability

- **Performance targets:** boundary checks are linear in input size and run before any module call; a 1 MiB command body is parsed and checked within 10 ms on a supported machine, inside the 150 ms p95 interface target (A-03). Size refusals cost no parse.
- **Security:** unknown fields refused at every entry; size limits on every surface; every file format checked before use; adapter answers typed before use; fetched and remote text is data (ADR-085); YAML features that expand (anchors, aliases) refused, so a small file cannot become a large tree; archive paths checked against absolute paths and `..` before extraction.
- **Observability:** each surface logs its refusal with the code — `svc.command` (Service API, `outcome`), `request_refused` (Caller API, INFO, `code`, `method`), hook relay and MCP Front log lines per their specs; on A, `TelemetryEvent::HttpRefusal { code }` counts Caller API refusals.

*Closed gaps and defects.* The earlier gap (no shared YAML reader in SharedTypes.md) is closed: SharedTypes.md defines `ordin_types::yaml::read_restricted` and `read_restricted_node` with the codes `YAML_FEATURE_REFUSED`, `YAML_MALFORMED`, `YAML_TOO_LARGE` (decision P4-1), cited in *Shared Context* and *Internal Logic* step 5.

## Gaps

None.
