# SPEC: Architecture test

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · amended January 2026 (`PERSON_ONLY_COMMANDS` copied with the four names of the ADR-113 and ADR-114 amendments of ServiceApi.md) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `crates/ordin-arch-test` (package `ordin-arch-test`) · **Build phase:** 12 · **Programs:** build only
> **Dependencies:** — (reads `cargo metadata`)

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
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)
- [Amendment January 2026 — Reading other teams' knowledge (ADR-110)](#amendment-january-2026--reading-other-teams-knowledge-adr-110)
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)
- [Amendment January 2026 — Knowledge agents (ADR-113)](#amendment-january-2026--knowledge-agents-adr-113)
- [Amendment January 2026 — Releases and updates (ADR-114)](#amendment-january-2026--releases-and-updates-adr-114)

## Purpose

The architecture test is the build check that ADR-096 item 1 requires and ADR-100 completes: it reads the crate graph of the Cargo workspace with `cargo metadata`, compares every dependency edge between workspace crates with the *Dependencies* column of Phase 2B-3, and fails the build on an edge the column does not list, on a workspace crate the column does not name, and on any cycle (02:194 "A build test reads the crate graph (`cargo metadata`) and fails on any other dependency and on any cycle"; 09 ADR-096 item 1; 09 ADR-100 *Consequences*; 2B-1 rule 8). It is what keeps "a module depends only on what its *Depends on* list names" true in code rather than only in the design. It holds no product behaviour and ships in no program.

## Approach

The test is a small library crate with one integration test: the library deserialises the subset of `cargo metadata --format-version 1` it needs, holds the allowed graph as a compiled-in table copied from 2B-3, and returns every violation in one report; the integration test runs `cargo metadata`, calls the library and fails with the whole report. The allowed graph is a Rust `const` table rather than a data file, so a change to 2B-3 is a reviewed source change to this crate and the table's own consistency (every edge points to a lower phase) is checked by a unit test. Error-code prefix: `ARCH_` (*chosen here*). Rejected: the `cargo_metadata` crate (not named in 1A or 05; the JSON subset needed is five fields, read with `serde_json`). Rejected: `cargo-deny`'s `[bans]` section (already in the build for licences, SA-LIC-01, but it cannot express "this crate may depend only on these crates" per crate, nor detect cycles between workspace members).

### Reconciliation assumptions

- **SA-RECON-01** The allowed table copies the 2B-3 row set amended under decision 80 (reconciliation rows 87–92, 94, 96). A code-level edge a module spec uses but 2B-3 does not list (rows 93 and 95) stays a violation: the module drops the edge, the table does not widen (ruling R6 applied to 2B-3 as the owner of the graph).

## Shared Context

### Rules this crate enforces (copied from Phase 2B-1)

1. One crate per module, named `ordin-<kebab-case component name>`, under `crates/`.
2. Three contract crates — `ordin-types`, `ordin-service-api`, `ordin-runtime-adapter` — depend on nothing inside the workspace except `ordin-types`.
3. Programs are composition roots: `apps/desktop` and `apps/server` are the only crates that depend on every module of their program; `apps/hook` depends on `ordin-types` only.
4. Hubs own traits their providers implement (ADR-100); the corrected dependency lists of 2B-2 are the ones checked.
5. The datastore belongs to `ordin-journal`; tables are not checked here.
6. The screens module (`apps/desktop/ui/`) is TypeScript and is not in the Cargo graph; this crate does not check it.
7. The embedded components `memory/` (package `memory`) and `sandbox/sandbox-host/` (package `sandboxd`, with its own workspace member `sandboxd-ipc`) are path dependencies of `ordin-memory-service` and `ordin-sandbox`, listed under `exclude` in the root workspace (SA-LAYOUT-01; `sandbox/sandbox-host/Cargo.toml:9`–`10` declares `members = ["ipc"]`).
8. The build test reads `cargo metadata`, compares every edge with the *Dependencies* column, and fails on an edge not listed or on any cycle.

### Package names of the programs (*chosen here*)

The 2B-3 rows name the programs by path. Their Cargo package names are fixed here so the table can name them:

| Path | Package name | Binary |
|---|---|---|
| `apps/desktop` | `ordin-desktop` *(chosen here)* | `ordin` (SA-NAME-05) |
| `apps/server` | `ordin-server` *(chosen here)* | `ordin-server` (SA-NAME-05; 08:47 revision 0.48.0) |
| `apps/hook` | `ordin-hook` *(chosen here)* | `ordin-hook` (2C wire, *Names fixed by this spec* item 1) |
| `crates/ordin-arch-test` | `ordin-arch-test` | none |

### The allowed graph (2B-3, verbatim, with component names resolved to crate names)

`ordin-types` is allowed as a direct dependency of every workspace crate (2B-3: Phase 1 is "depends on the shared types only", and every higher phase reaches it). Every other allowed edge is listed. "Programs" is the 2B-3 *Programs* column.

| Crate | Phase | Programs | Allowed workspace dependencies (besides `ordin-types`) |
|---|---|---|---|
| `ordin-types` | 0 | D P A | — (none, not even itself) |
| `ordin-service-api` | 1 | D P A | — |
| `ordin-runtime-adapter` | 1 | D P A | — |
| `ordin-journal` | 1 | D P A | — |
| `ordin-organisation-profile` | 1 | D P A | — |
| `ordin-identity` | 1 | D P A | — |
| `ordin-agent-model` | 1 | D P A | — |
| `ordin-agent-configuration` | 1 | A | — |
| `ordin-telemetry` | 1 | A | — |
| `ordin-approval` | 2 | D P A | `ordin-journal` |
| `ordin-localisation` | 2 | D P A | `ordin-organisation-profile`, `ordin-journal` |
| `ordin-agent-folders` | 2 | D P A | `ordin-journal` |
| `ordin-format-plugins` | 2 | D P A | `ordin-agent-model` |
| `ordin-resolver` | 2 | D P A | `ordin-agent-model` |
| `ordin-sharepoint` | 2 | D P | `ordin-identity`, `ordin-journal` |
| `ordin-jira` | 2 | D | `ordin-identity`, `ordin-journal` |
| `ordin-knowledge-query-client` | 2 | D P A | `ordin-identity`, `ordin-journal` |
| `ordin-hosted-agent-adapter` | 2 | D P | `ordin-identity`, `ordin-runtime-adapter` |
| `ordin-slack-thread-adapter` | 2 | D P | `ordin-identity`, `ordin-runtime-adapter` |
| `ordin-nats` | 2 | P A | `ordin-identity`, `ordin-organisation-profile` |
| `ordin-usage-planner` | 2 | D P A | `ordin-organisation-profile`, `ordin-journal`, `ordin-runtime-adapter` |
| `ordin-run-queue` | 2 | A | `ordin-journal` |
| `ordin-model-policy` | 2 | A | `ordin-agent-configuration`, `ordin-identity` |
| `ordin-caller-auth` | 2 | A | `ordin-agent-configuration` |
| `ordin-settings-store` | 2 | A | `ordin-agent-configuration`, `ordin-journal` |
| `ordin-server-connector` | 2 | D | `ordin-service-api`, `ordin-identity`, `ordin-journal` |
| `ordin-hook` (`apps/hook`) | 2 | D P A | — |
| `ordin-rules` | 3 | D P A | `ordin-approval`, `ordin-journal`, `ordin-organisation-profile` |
| `ordin-brand` | 3 | D | `ordin-organisation-profile`, `ordin-localisation`, `ordin-journal` |
| `ordin-dictation` | 3 | D | `ordin-localisation` |
| `ordin-checker` | 3 | D P A | `ordin-agent-model`, `ordin-resolver`, `ordin-organisation-profile`, `ordin-journal` |
| `ordin-installers` | 3 | D P A | `ordin-agent-folders`, `ordin-resolver`, `ordin-agent-model` |
| `ordin-session-supervisor` | 3 | D P A | `ordin-runtime-adapter`, `ordin-journal`, `ordin-usage-planner`, `ordin-run-queue` |
| `ordin-duty-timer` | 3 | A | `ordin-agent-configuration`, `ordin-run-queue`, `ordin-telemetry`, `ordin-journal` |
| `ordin-command-bridge` | 3 | D | `ordin-service-api`, `ordin-server-connector`, `ordin-journal` |
| `ordin-remote-api` | 3 | P | `ordin-service-api`, `ordin-approval`, `ordin-journal`, `ordin-identity`, `ordin-organisation-profile` |
| `ordin-mcp-front` | 4 | D P A | `ordin-rules`, `ordin-journal`, `ordin-service-api` |
| `ordin-package-inspector` | 4 | D P A | `ordin-rules`, `ordin-format-plugins`, `ordin-agent-model` |
| `ordin-files` | 4 | D P A | `ordin-agent-folders`, `ordin-rules`, `ordin-journal` |
| `ordin-hook-relay` | 4 | D P A | `ordin-rules`, `ordin-approval`, `ordin-session-supervisor`, `ordin-journal` |
| `ordin-crew-orchestrator` | 4 | D P | `ordin-session-supervisor`, `ordin-runtime-adapter`, `ordin-rules`, `ordin-approval`, `ordin-usage-planner`, `ordin-organisation-profile`, `ordin-journal` |
| `ordin-sandbox` | 5 | D P | `ordin-mcp-front`, `ordin-rules`, `ordin-identity`, `ordin-journal` |
| `ordin-memory-service` | 5 | D P A | `ordin-mcp-front`, `ordin-journal`, `ordin-knowledge-query-client` |
| `ordin-craft-toolkit` | 5 | D P | `ordin-mcp-front`, `ordin-rules`, `ordin-files`, `ordin-organisation-profile`, `ordin-journal` |
| `ordin-confluence` | 5 | D | `ordin-identity`, `ordin-approval`, `ordin-files`, `ordin-journal` |
| `ordin-projects` | 5 | D P | `ordin-mcp-front`, `ordin-files`, `ordin-rules`, `ordin-identity`, `ordin-journal` |
| `ordin-claude-adapter` | 5 | D P A | `ordin-runtime-adapter`, `ordin-mcp-front`, `ordin-hook-relay`, `ordin-rules` |
| `ordin-copilot-adapter` | 5 | D P | `ordin-runtime-adapter`, `ordin-mcp-front`, `ordin-hook-relay`, `ordin-rules`, `ordin-identity` |
| `ordin-connection-binder` | 5 | A | `ordin-agent-configuration`, `ordin-mcp-front`, `ordin-rules`, `ordin-telemetry` |
| `ordin-workspaces` | 6 | D P | `ordin-files`, `ordin-confluence`, `ordin-sharepoint`, `ordin-jira`, `ordin-rules`, `ordin-journal` |
| `ordin-catalogue` | 6 | D P A | `ordin-organisation-profile`, `ordin-projects`, `ordin-format-plugins`, `ordin-checker`, `ordin-agent-model`, `ordin-journal` |
| `ordin-push` | 6 | D P | `ordin-approval`, `ordin-sharepoint`, `ordin-confluence`, `ordin-projects`, `ordin-craft-toolkit`, `ordin-files`, `ordin-journal` |
| `ordin-agent-loader` | 6 | A | `ordin-agent-configuration`, `ordin-model-policy`, `ordin-connection-binder`, `ordin-mcp-front`, `ordin-rules`, `ordin-files` |
| `ordin-knowledge-query-server` | 6 | A | `ordin-knowledge-query-client`, `ordin-memory-service`, `ordin-caller-auth`, `ordin-agent-configuration`, `ordin-journal`, `ordin-telemetry` |
| `ordin-knowledge` | 7 | D P A | `ordin-mcp-front`, `ordin-files`, `ordin-workspaces`, `ordin-memory-service`, `ordin-rules`, `ordin-push`, `ordin-journal` |
| `ordin-mail` | 7 | D P | `ordin-mcp-front`, `ordin-identity`, `ordin-rules`, `ordin-approval`, `ordin-files`, `ordin-workspaces`, `ordin-organisation-profile`, `ordin-journal` |
| `ordin-slack-service` | 7 | D P | `ordin-identity`, `ordin-approval`, `ordin-mcp-front`, `ordin-workspaces`, `ordin-organisation-profile`, `ordin-journal` |
| `ordin-package-catalogue` | 7 | D P | `ordin-catalogue`, `ordin-resolver`, `ordin-installers`, `ordin-checker`, `ordin-organisation-profile`, `ordin-agent-folders`, `ordin-package-inspector`, `ordin-journal` |
| `ordin-release-builder` | 7 | A | `ordin-agent-configuration`, `ordin-checker`, `ordin-resolver`, `ordin-agent-loader` |
| `ordin-host` | 8 | D P | `ordin-runtime-adapter`, `ordin-service-api`, `ordin-mcp-front`, `ordin-checker`, `ordin-files`, `ordin-package-inspector`, `ordin-package-catalogue`, `ordin-crew-orchestrator`, `ordin-session-supervisor`, `ordin-usage-planner`, `ordin-memory-service`, `ordin-knowledge`, `ordin-push`, `ordin-projects`, `ordin-workspaces`, `ordin-journal` |
| `ordin-team-knowledge` | 8 | D P | `ordin-memory-service`, `ordin-knowledge`, `ordin-push`, `ordin-projects`, `ordin-workspaces`, `ordin-identity`, `ordin-journal` |
| `ordin-knowledge-reader` | 8 | A | `ordin-agent-configuration`, `ordin-memory-service`, `ordin-knowledge`, `ordin-duty-timer`, `ordin-journal`, `ordin-telemetry` |
| `ordin-home-search` | 8 | D P | `ordin-journal`, `ordin-knowledge`, `ordin-files`, `ordin-workspaces`, `ordin-sharepoint`, `ordin-confluence` |
| `ordin-setup` | 8 | D P | `ordin-organisation-profile`, `ordin-identity`, `ordin-package-catalogue`, `ordin-catalogue`, `ordin-craft-toolkit`, `ordin-journal` |
| `ordin-retention` | 8 | A | `ordin-knowledge`, `ordin-memory-service`, `ordin-files`, `ordin-run-queue`, `ordin-settings-store`, `ordin-journal` |
| `ordin-authoring` | 9 | D P | `ordin-agent-model`, `ordin-catalogue`, `ordin-checker`, `ordin-format-plugins`, `ordin-mcp-front`, `ordin-host`, `ordin-push`, `ordin-journal` |
| `ordin-channel-gateway` | 9 | P A | `ordin-host`, `ordin-approval`, `ordin-workspaces`, `ordin-organisation-profile`, `ordin-journal`, `ordin-retention` |
| `ordin-handover` | 9 | D P | `ordin-catalogue`, `ordin-checker`, `ordin-approval`, `ordin-push`, `ordin-projects`, `ordin-sharepoint`, `ordin-mail`, `ordin-workspaces`, `ordin-host`, `ordin-identity`, `ordin-slack-service`, `ordin-jira`, `ordin-journal` |
| `ordin-job-runner` | 9 | A | `ordin-knowledge`, `ordin-retention`, `ordin-journal`, `ordin-telemetry` |
| `ordin-caller-api` | 9 | A | `ordin-caller-auth`, `ordin-run-queue`, `ordin-files`, `ordin-settings-store`, `ordin-knowledge`, `ordin-retention`, `ordin-rules`, `ordin-telemetry`, `ordin-approval`, `ordin-knowledge-query-server` |
| `ordin-telegram-adapter` | 10 | D P | `ordin-channel-gateway`, `ordin-identity`, `ordin-journal` |
| `ordin-slack-channel-adapter` | 10 | P A | `ordin-channel-gateway`, `ordin-identity`, `ordin-journal` |
| `ordin-triggers` | 10 | P | `ordin-host`, `ordin-channel-gateway`, `ordin-nats`, `ordin-slack-service`, `ordin-organisation-profile`, `ordin-journal` |
| `ordin-telegram-service` | 11 | D P | `ordin-telegram-adapter`, `ordin-mcp-front`, `ordin-rules`, `ordin-approval`, `ordin-files`, `ordin-workspaces`, `ordin-organisation-profile`, `ordin-journal` |
| `ordin-server` (`apps/server`) | 12 | P A | every crate above whose Programs contain P or A |
| `ordin-desktop` (`apps/desktop`) | 12 | D | every crate above whose Programs contain D |
| `ordin-arch-test` | 12 | build only | — (no workspace dependency) |

The table follows the 2B-3 amendment of January 2026 (decision 80): Journal edges for the Server Connector, Duty Timer, Catalogue Service, Authoring Service, Project Service, Craft Toolkit, Telegram Service, Localisation, Brand and Setup Assistant; MCP Front → Service API; Installers, Package Inspector and Catalogue Service → Agent Model; Caller API → Approval Service; Channel Gateway → Retention and Erasure; Trigger Service → Session Supervisor removed and the Trigger Service in the personal profile only (D5). It also follows the closing-round amendment (K7, K19): Telegram Adapter and Slack Channel Adapter → Journal, Ordin Host → Checker and Ordin Host → File Service; and the re-audit amendment (E4), which removed Slack Channel Adapter → Retention and Erasure. Code-level edges a module spec uses but 2B-3 does not list (for example a Push Service use of a Service API type) stay violations; the module conforms, the table does not widen.

*Amended January 2026 (HLD 0.50.0, ADR-102):* new crate `ordin-slack-service` (phase 7); edges Handover Service → Slack Service and Trigger Service → Slack Service. The Ordin Host reaches no Slack crate: the Slack Service's `SlackReactor` is implemented by the programs.

*Amended January 2026 (HLD 0.54.0, ADR-106):* new crate `ordin-jira` (phase 2, D: its two dependencies are in phase 1, as the SharePoint Adapter); edges Workspace Service → Jira Adapter and Handover Service → Jira Adapter. Also added (amended January 2026, found by the fifteenth Phase 6 audit): the row `ordin-team-knowledge` (phase 8, D P), which the ADR-103 amendment of 2B-3 added and this table had not followed.

*Amended January 2026 (HLD 0.61.0, ADR-113):* new crates `ordin-knowledge-query-client` (phase 2, D P A: Identity and Journal, both phase 1), `ordin-knowledge-query-server` (phase 6, A: its dependencies sit in phases 1, 2 and 5) and `ordin-knowledge-reader` (phase 8, A: its dependencies sit in phases 1, 3, 5 and 7, the Duty Timer at 3 because the reader implements the Duty Timer's own trait `BuiltInDuty`); edges Memory Service → Knowledge Query Client (5 → 2) and Caller API → Knowledge Query Server (9 → 6). The Knowledge Reader reaches the Session Supervisor and the Query Server only through traits the programs implement (`ReaderModel`, `ReaderOutlet`), and its credentials through its own `CredentialSource`, so it has no edge to Identity and Credentials; the Query Server reaches the reader through its own `HotCopy`, so no other edge is added.

### Embedded path packages (not workspace members)

| Package | Only allowed dependent |
|---|---|
| `memory` | `ordin-memory-service` |
| `sandboxd` | `ordin-sandbox` |
| `sandboxd-ipc` | `sandboxd` (internal to the embedded workspace; never a direct dependency of an ordin crate) |

### Fields of `cargo metadata --format-version 1` that are read

`packages[].id`, `packages[].name`, `packages[].source` (`null` for a path package), `packages[].manifest_path`, `workspace_members[]`, `resolve.nodes[].id`, `resolve.nodes[].deps[].pkg`, `resolve.nodes[].deps[].dep_kinds[].kind` (`null` for a normal dependency, `"dev"`, `"build"`). Every other field is ignored, so newer Cargo versions that add fields do not break the test.

## Public Interface

```rust
// crates/ordin-arch-test/src/lib.rs
#![forbid(unsafe_code)]

/// One row of the allowed graph (2B-3).
#[derive(Debug, Clone, Copy)]
pub struct AllowedCrate {
    pub name: &'static str,
    pub phase: u8,
    pub programs: Programs,
    /// Direct workspace dependencies allowed besides `ordin-types`.
    pub deps: AllowedDeps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Programs { pub desktop: bool, pub personal: bool, pub agent: bool, pub build_only: bool }

#[derive(Debug, Clone, Copy)]
pub enum AllowedDeps {
    /// The listed crates.
    List(&'static [&'static str]),
    /// Every crate of the table with `programs.desktop` (the desktop program).
    EveryDesktopModule,
    /// Every crate of the table with `programs.personal || programs.agent` (the server program).
    EveryServerModule,
}

/// The table above, in the order shown. `ordin-types` is the first row.
pub const ALLOWED: &[AllowedCrate] = &[ /* one entry per row of the table */ ];

/// Embedded path packages and the one crate allowed to depend on each.
pub const EMBEDDED: &[(&str, &str)] = &[
    ("memory", "ordin-memory-service"),
    ("sandboxd", "ordin-sandbox"),
    ("sandboxd-ipc", "sandboxd"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind { Normal, Build, Dev }

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Violation {
    UnknownCrate { name: String },
    EdgeNotAllowed { from: String, to: String, kind: EdgeKind },
    DevEdgeNotAllowed { from: String, to: String },
    Cycle { members: Vec<String> },
    EmbeddedEdge { from: String, to: String },
    UnknownPathPackage { name: String },
}

impl Violation {
    /// `ARCH_UNKNOWN_CRATE`, `ARCH_EDGE_NOT_ALLOWED`, `ARCH_DEV_EDGE_NOT_ALLOWED`,
    /// `ARCH_CYCLE`, `ARCH_EMBEDDED_EDGE`, `ARCH_UNKNOWN_PATH_PACKAGE`.
    pub fn code(&self) -> &'static str;
}

impl std::fmt::Display for Violation { /* one line, see Example */ }

#[derive(Debug, Default)]
pub struct Report {
    /// Sorted, de-duplicated.
    pub violations: Vec<Violation>,
    /// Rows of `ALLOWED` with no workspace member yet (the Generation Playbook builds in phases).
    pub not_yet_built: Vec<&'static str>,
}

impl Report { pub fn is_ok(&self) -> bool { self.violations.is_empty() } }

#[derive(Debug, thiserror::Error)]
pub enum ArchTestError {
    #[error("ARCH_METADATA_FAILED: cargo metadata exited with {status}: {stderr_first_line}")]
    MetadataFailed { status: String, stderr_first_line: String },
    #[error("ARCH_METADATA_INVALID: {0}")]
    MetadataInvalid(String),
    #[error("ARCH_TABLE_INVALID: {0}")]
    TableInvalid(String),
}

/// Deserialised subset of `cargo metadata --format-version 1`.
#[derive(Debug, serde::Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub resolve: Option<Resolve>,
}
#[derive(Debug, serde::Deserialize)]
pub struct Package { pub id: String, pub name: String, pub source: Option<String>, pub manifest_path: String }
#[derive(Debug, serde::Deserialize)]
pub struct Resolve { pub nodes: Vec<Node> }
#[derive(Debug, serde::Deserialize)]
pub struct Node { pub id: String, pub deps: Vec<NodeDep> }
#[derive(Debug, serde::Deserialize)]
pub struct NodeDep { pub pkg: String, pub dep_kinds: Vec<DepKindInfo> }
#[derive(Debug, serde::Deserialize)]
pub struct DepKindInfo { pub kind: Option<String> }

/// Runs `$CARGO metadata --format-version 1 --locked --manifest-path <workspace>/Cargo.toml`
/// and parses its standard output.
pub fn read_metadata(workspace_manifest: &std::path::Path) -> Result<Metadata, ArchTestError>;

/// Checks the table itself: unique names, every listed dependency is a row of a strictly lower
/// phase, `ordin-types` is phase 0 with no dependency.
pub fn check_table(table: &[AllowedCrate]) -> Result<(), ArchTestError>;

/// Compares the metadata with the table and returns every violation.
pub fn check(metadata: &Metadata, table: &[AllowedCrate]) -> Report;
```

Cargo manifest (`crates/ordin-arch-test/Cargo.toml`): `name = "ordin-arch-test"`, `publish = false`, `edition = "2021"`, `license = "Apache-2.0"`; dependencies `serde` (feature `derive`), `serde_json`, `thiserror` — the same versions the workspace already uses; no workspace crate.

Integration test (`crates/ordin-arch-test/tests/architecture.rs`):

```rust
#[test]
fn crate_graph_matches_phase_2b3() {
    ordin_arch_test::check_table(ordin_arch_test::ALLOWED).expect("allowed table is consistent");
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let metadata = ordin_arch_test::read_metadata(&manifest).expect("cargo metadata");
    let report = ordin_arch_test::check(&metadata, ordin_arch_test::ALLOWED);
    for name in &report.not_yet_built { eprintln!("not yet built: {name}"); }
    assert!(report.is_ok(), "{}", report.violations.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n"));
}
```

It runs under `cargo test` (05 — *Build / test toolchain*) and therefore in `make check`.

### Example

A crate graph where `ordin-files` gained a dependency on `ordin-workspaces` (the D2 cycle of 2B-2) produces:

```text
not yet built: ordin-telegram-service
ARCH_CYCLE ordin-files -> ordin-workspaces -> ordin-files
ARCH_EDGE_NOT_ALLOWED ordin-files -> ordin-workspaces (normal)
test crate_graph_matches_phase_2b3 ... FAILED
```

A clean graph prints only the `not yet built:` lines (none once every crate exists) and passes.

## Internal Logic

`read_metadata`:

1. Read the `CARGO` environment variable that Cargo sets for every test; when it is absent, use `cargo` from the `PATH` (*assumed here: running the test outside Cargo is a developer convenience*).
2. Run `<cargo> metadata --format-version 1 --locked --manifest-path <workspace_manifest>` with `std::process::Command`, standard output and standard error captured. A non-zero exit returns `ArchTestError::MetadataFailed` with the status and the first line of standard error. `--all-features` is not passed: the default feature set is what the programs build (*assumed here*).
3. Parse standard output with `serde_json::from_slice::<Metadata>`. A parse failure, or `resolve` being `null`, returns `ArchTestError::MetadataInvalid` with the parser's message.

`check_table`:

4. Names are unique; `ordin-types` is the row of phase 0 with `List(&[])`.
5. For each `List` entry, each name is a row of the table and its phase is strictly lower than the row's phase; the first breach returns `ArchTestError::TableInvalid("<row> -> <dep>: phase <a> is not below <b>")`.
6. `EveryDesktopModule` appears on `ordin-desktop` only and `EveryServerModule` on `ordin-server` only.

`check`:

7. Build `id → name` from `packages`. Build the set `members` of names whose id is in `workspace_members`. Build the set `path_packages` of names with `source == None` that are not members.
8. **Unknown crates.** Each member that is not a row of the table → `UnknownCrate`. Each name in `path_packages` that is not in `EMBEDDED` → `UnknownPathPackage`.
9. **Not yet built.** Each row of the table that is not a member → `report.not_yet_built` (informational; does not fail).
10. **Edges.** For each `resolve.nodes[]` whose package is a member: for each `deps[]` whose `pkg` resolves to a member or an embedded path package, take the kinds from `dep_kinds[].kind` (`null` → `Normal`, `"build"` → `Build`, `"dev"` → `Dev`); one dependency may carry several kinds, and each is checked.
    - Target is `ordin-types`: allowed for every kind.
    - Target is an embedded path package: allowed only when the `EMBEDDED` pair names this source crate; otherwise `EmbeddedEdge`.
    - `Normal` or `Build`: allowed when the target is in the source row's allowed set — for `List`, the list; for `EveryDesktopModule`, every row with `programs.desktop` and phase < 12; for `EveryServerModule`, every row with `programs.personal || programs.agent` and phase < 12. Otherwise `EdgeNotAllowed`.
    - `Dev`: allowed when the target row's phase is strictly lower than the source row's phase (*assumed here: tests may use a lower-phase crate's real implementation or fakes; a lower phase cannot create a cycle*). Otherwise `DevEdgeNotAllowed`.
    - Edges from a member to registry crates are ignored.
11. **Cycles.** Build the directed graph of members with every `Normal` and `Build` edge found in step 10, allowed or not, and run Tarjan's strongly connected components algorithm. Each component of more than one member, and each member with an edge to itself, → `Cycle { members }`, the members listed starting from the alphabetically first and following edges, closed by repeating the first.
12. Sort and de-duplicate `violations` (by code, then the names), sort `not_yet_built`, and return the report. Nothing is logged; the integration test prints the report.

## Data Model

N/A — the crate owns no table and writes no file; it reads the output of `cargo metadata` and the compiled-in table.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `cargo metadata` exits non-zero (no lock file, a manifest error, `--locked` with an out-of-date `Cargo.lock`) | `Err(permanent)` — test fails | `ARCH_METADATA_FAILED` | `ARCH_METADATA_FAILED: cargo metadata exited with exit status: 101: error: the lock file needs to be updated but --locked was passed` |
| Standard output is not the expected JSON, or `resolve` is `null` | `Err(permanent)` — test fails | `ARCH_METADATA_INVALID` | `ARCH_METADATA_INVALID: missing field \`workspace_members\` at line 1 column 1834` |
| The compiled-in table lists an edge to an equal or higher phase, or a duplicate row | `Err(permanent)` — test fails | `ARCH_TABLE_INVALID` | `ARCH_TABLE_INVALID: ordin-rules -> ordin-files: phase 4 is not below 3` |
| A workspace member is not a row of the table | violation — test fails | `ARCH_UNKNOWN_CRATE` | `ARCH_UNKNOWN_CRATE ordin-reports` |
| A normal or build edge between members not listed | violation — test fails | `ARCH_EDGE_NOT_ALLOWED` | `ARCH_EDGE_NOT_ALLOWED ordin-approval -> ordin-command-bridge (normal)` |
| A dev edge to a member of an equal or higher phase | violation — test fails | `ARCH_DEV_EDGE_NOT_ALLOWED` | `ARCH_DEV_EDGE_NOT_ALLOWED ordin-journal -> ordin-rules` |
| A cycle among normal and build edges | violation — test fails | `ARCH_CYCLE` | `ARCH_CYCLE ordin-checker -> ordin-resolver -> ordin-catalogue -> ordin-checker` |
| `memory` or `sandboxd` used by a crate other than its one wrapper | violation — test fails | `ARCH_EMBEDDED_EDGE` | `ARCH_EMBEDDED_EDGE ordin-knowledge -> memory` |
| A path package that is neither a member nor embedded | violation — test fails | `ARCH_UNKNOWN_PATH_PACKAGE` | `ARCH_UNKNOWN_PATH_PACKAGE scratch-tool` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Architecture test

  Scenario: Happy path — the graph matches 2B-3
    Given a workspace whose members' normal edges are exactly the allowed edges of the table
    When the test crate_graph_matches_phase_2b3 runs
    Then the report has no violation
    And the test passes

  Scenario: Edge case — crates of later phases not yet built
    Given a workspace holding only the crates of phases 0 to 3
    When the test runs
    Then every crate of phase 4 and above is listed under "not yet built"
    And the test passes

  Scenario: Edge case — the shared types crate from a high phase
    Given ordin-telegram-service depends directly on ordin-types and on its eight listed crates
    When the test runs
    Then no violation is reported

  Scenario: Edge case — a dev-dependency on a lower phase
    Given ordin-rules has a dev-dependency on ordin-journal and on ordin-approval
    When the test runs
    Then no violation is reported

  Scenario: Error path — an edge not listed
    Given ordin-approval has a normal dependency on ordin-command-bridge
    When the test runs
    Then the report holds "ARCH_EDGE_NOT_ALLOWED ordin-approval -> ordin-command-bridge (normal)"
    And the test fails

  Scenario: Error path — a cycle
    Given ordin-files depends on ordin-workspaces and ordin-workspaces depends on ordin-files
    When the test runs
    Then the report holds "ARCH_CYCLE ordin-files -> ordin-workspaces -> ordin-files"
    And it also holds "ARCH_EDGE_NOT_ALLOWED ordin-files -> ordin-workspaces (normal)"

  Scenario: Error path — a dev-dependency on a higher phase
    Given ordin-journal has a dev-dependency on ordin-rules
    When the test runs
    Then the report holds "ARCH_DEV_EDGE_NOT_ALLOWED ordin-journal -> ordin-rules"

  Scenario: Error path — an unknown crate
    Given a workspace member named ordin-reports that the table does not name
    When the test runs
    Then the report holds "ARCH_UNKNOWN_CRATE ordin-reports"

  Scenario: Error path — the memory component used outside its wrapper
    Given ordin-knowledge has a path dependency on memory
    When the test runs
    Then the report holds "ARCH_EMBEDDED_EDGE ordin-knowledge -> memory"

  Scenario: Error path — a program depends on a module of another program
    Given ordin-desktop depends on ordin-run-queue, whose Programs column is A only
    When the test runs
    Then the report holds "ARCH_EDGE_NOT_ALLOWED ordin-desktop -> ordin-run-queue (normal)"

  Scenario: Error path — cargo metadata fails
    Given Cargo.lock is out of date with the manifests
    When the test runs
    Then read_metadata returns ARCH_METADATA_FAILED
    And the test fails with that message

  Scenario: Error path — the table itself breaks the phase order
    Given a table row ordin-rules of phase 3 that lists ordin-files of phase 4
    When check_table runs
    Then it returns ARCH_TABLE_INVALID naming "ordin-rules -> ordin-files"
```

Unit tests: one per error-table row, table-driven over hand-written `Metadata` values (no `cargo` call); `check_table(ALLOWED)` passes; a test that every row of the table has the phase 2B-3 gives it (the table is the oracle for the reviewer, so this test compares the number of rows per phase: 1, 8, 16, 9, 5, 8, 4, 4, 4, 5, 3, 1, 3 for phases 0 to 12, counting `ordin-hook` in phase 2 and `ordin-server`, `ordin-desktop` and `ordin-arch-test` in phase 12).

## Performance, Security, Observability

- **Performance targets:** `check` on 80 members and 1,500 resolved packages completes in under 50 ms; the whole test, dominated by `cargo metadata`, in under 10 s on a warm Cargo cache.
- **Security:** reads only the workspace's own manifests through Cargo; runs no other program; no network access beyond what `cargo metadata --locked` itself needs to read manifests of locked dependencies already in the local cache. No input from outside the repository.
- **Observability:** no `tracing` output. The integration test prints each `not yet built:` line to standard error and, on failure, every violation one per line in the assertion message, each led by its code.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

### Allowed-dependency table

The table above has the row `ordin-slack-service` (phase 7, D P: `ordin-identity`, `ordin-approval`, `ordin-mcp-front`, `ordin-workspaces`, `ordin-organisation-profile`, `ordin-journal`), and the `ordin-handover` and `ordin-triggers` rows allow `ordin-slack-service`; the paragraph under the table records the change. The `ordin-server` and `ordin-desktop` rows are unchanged in wording ("every crate above whose Programs contain …"); both include `ordin-slack-service`, and `ordin-server` never builds it in the agent profile (Programs D P). The test's fixtures and text count **73** component crates.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the Slack Service's edges are allowed
    Given ordin-slack-service depends on ordin-identity, ordin-approval, ordin-mcp-front, ordin-workspaces, ordin-organisation-profile and ordin-journal
    And ordin-handover and ordin-triggers depend on ordin-slack-service
    When the test crate_graph_matches_phase_2b3 runs
    Then no violation is reported

  Scenario: Error path — the Ordin Host may not depend on the Slack Service
    Given ordin-host has a normal dependency on ordin-slack-service
    When the test runs
    Then the report holds "ARCH_EDGE_NOT_ALLOWED ordin-host -> ordin-slack-service (normal)"
    And the test fails
```

### Gaps

None.

## Amendment January 2026 — Jira issues and handovers (ADR-106)

### Allowed-dependency table

The table above has the row `ordin-jira` (phase 2, D: `ordin-identity`, `ordin-journal`), and the `ordin-workspaces` and `ordin-handover` rows allow `ordin-jira`; the paragraph under the table records the change. The `ordin-desktop` row includes `ordin-jira` by its wording ("every crate above whose Programs contain D"); `ordin-server` never builds it. The test's fixtures and text count one more component crate than before this amendment.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the Jira Adapter's edges are allowed
    Given ordin-jira depends on ordin-identity and ordin-journal
    And ordin-workspaces and ordin-handover depend on ordin-jira
    When the test crate_graph_matches_phase_2b3 runs
    Then no violation is reported

  Scenario: Error path — the server reaching the Jira Adapter
    Given ordin-server lists ordin-jira as a dependency
    When the test runs
    Then it reports ordin-jira as outside the crates allowed for programmes P and A
```

### Gaps

None.

---

## Amendment January 2026 — Reading other teams' knowledge (ADR-110)

> Source: decision 90 (the user's decisions of January 2026); HLD revision 0.59.0, ADR-110 (amends ADR-103: one own folder, any number of read folders; adds Adopt and folders the company names). Counterpart: `components/ServiceApi.md` (amendment January 2026, *Adopting is the person's alone*, `PERSON_ONLY_COMMANDS`).
> Every rule below adds to the sections above. The allowed-dependency table does not change: the Team Knowledge Service reads the organisation profile, and the Memory and Knowledge Services read folder names, through traits the programs implement (MemoryService.md `MemoryFolders`, KnowledgeService.md `KnowledgeFolders`), so no crate edge is added.

### Purpose (addition)

The crate also proves that no agent-reachable code names the three commands only a person may send: `team_knowledge.adopt`, `team_knowledge.adopt_again` and `team_knowledge.keep_as_is` (ADR-110 item 3).

### Approach (addition)

A source scan, not a crate-graph rule: the programs depend on every crate, so the graph cannot tell a Service API dispatch from a tool handler. The test reads Rust source as text with a small tokeniser of its own (*chosen here*; no parser library is added, so HLD `05-tech-stack.md` is unchanged): it skips `//` and `/* … */` comments (doc comments included, block comments nested), reads string literals (`"…"` with escapes, and raw strings `r"…"`, `r#"…"#` up to 8 `#`) and records each identifier that follows a `.` and precedes `(`, so a comment or a doc line does not trip it. Rejected: *a runtime test enumerating every `ToolProvider`* — it would need every module built and started, and it would miss a workflow step kind or block action that names a command without being a tool.

Assumption (*chosen here*):

- **SA-ARCH-ADOPT-01** The command names are allowed as string literals only in `ordin-service-api` (the catalogue and `PERSON_ONLY_COMMANDS`) and in the program files that register the Service API dispatch (`ordin-desktop` and `ordin-server`, the module named `service_api` in each). The Team Knowledge Service's methods `adopt`, `adopt_again` and `keep_as_is` may be called only from `ordin-team-knowledge` itself and from those same dispatch modules.

### Shared Context (additions)

`PERSON_ONLY_COMMANDS: &[&str]` exported by `ordin-service-api` (ServiceApi.md amendment January 2026), copied: `["team_knowledge.adopt", "team_knowledge.adopt_again", "team_knowledge.keep_as_is"]`.

### Public Interface (additions)

```rust
/// Scans every `.rs` file of the workspace's member crates (from `cargo metadata`'s `manifest_path` directories,
/// excluding `target/` and `tests/fixtures/`) and returns one violation per offending site.
pub fn check_person_only(metadata: &Metadata, commands: &[&str]) -> Report;

#[test]
fn person_only_commands_not_reachable_by_agents() { /* check_person_only(read_metadata(..)?, PERSON_ONLY_COMMANDS).is_ok() */ }
```

### Internal Logic (additions)

1. For each member crate, tokenise each `.rs` file as *Approach* states; a file that is not UTF-8, or that ends inside a string literal or a block comment → `ARCH_SOURCE_UNREADABLE` with its path (the build would fail on it too).
2. For every string literal whose value equals a name in `commands`, outside the sites SA-ARCH-ADOPT-01 allows → violation `ARCH_PERSON_ONLY_NAMED { crate, file, line }`.
3. For every method-call name `adopt`, `adopt_again` or `keep_as_is` whose file is in a crate that depends on `ordin-team-knowledge`, outside the allowed sites → violation `ARCH_PERSON_ONLY_CALLED { crate, file, line }`.
4. The report joins these violations with the crate-graph ones; the test fails when any exists.

### Data Model

N/A — reads files only.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A command only a person may send is named outside the catalogue and the dispatch modules | test failure | `ARCH_PERSON_ONLY_NAMED` | `"[crate] [file]:[line] names [command], which only a person may send"` |
| An adopt method of the Team Knowledge Service is called outside the dispatch modules | test failure | `ARCH_PERSON_ONLY_CALLED` | `"[crate] [file]:[line] calls [method], which only a person may start"` |
| A source file is not UTF-8, or ends inside a string literal or a block comment | test failure | `ARCH_SOURCE_UNREADABLE` | `"[file] could not be read"` |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — only the catalogue and dispatch name the adopt commands
    Given "team_knowledge.adopt" appears in ordin-service-api and in ordin-desktop's service_api module only
    When person_only_commands_not_reachable_by_agents runs
    Then no violation is reported

  Scenario: Error path — a tool names an adopt command
    Given ordin-host defines a tool whose handler sends "team_knowledge.adopt"
    When the test runs
    Then it reports ARCH_PERSON_ONLY_NAMED for that file and line

  Scenario: Error path — a workflow step calls adopt directly
    Given ordin-run-queue calls TeamKnowledgeService::adopt_again
    When the test runs
    Then it reports ARCH_PERSON_ONLY_CALLED for that file and line

  Scenario: Edge — a comment is not a violation
    Given a doc comment in ordin-mcp-front that mentions team_knowledge.adopt
    When the test runs
    Then no violation is reported
```

### Performance, Security, Observability (additions)

- **Performance:** the scan parses every workspace source file once; ≤ 30 s on the CI runner for the whole workspace.
- **Security:** it is the build-time half of the runtime half is that no `CallOrigin` exists for a run (ServiceApi.md).
- **Observability:** none; the test's output lists violations.

### Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111 (amends ADR-030: memory is a core capability; the person sees, corrects and forgets things, links and facts). Counterpart: `components/ServiceApi.md` (amendment January 2026 — agentic memory), which extends `PERSON_ONLY_COMMANDS`. Every rule below adds to the sections above; no test, crate edge or library is added.

### Purpose (addition)

The person-only scan of the ADR-110 amendment also covers the memory actions that change or remove what ordin remembers, so no tool, workflow step or run can keep, correct, forget or merge on the person's behalf (ADR-111 item 6: the person sees the whole memory, and nothing changes without their choice).

### Shared Context (changes)

`PERSON_ONLY_COMMANDS` (exported by `ordin-service-api`, ServiceApi.md amendment January 2026 — agentic memory), copied in full: `["team_knowledge.adopt", "team_knowledge.adopt_again", "team_knowledge.keep_as_is", "memory.correct", "memory.forget", "memory.remember_everywhere", "memory.keep", "memory.correct_link", "memory.forget_link", "memory.forget_thing", "memory.same_as"]`.

### Internal Logic

Unchanged: `check_person_only` reads the constant, so the eight added names are checked by the existing steps 1 and 2 with the sites SA-ARCH-ADOPT-01 allows. The Memory Service names none of them as a string literal: its methods are reached by the program's `service_api` module, which is an allowed site.

### Error Table

No row added; the existing `ARCH_PERSON_ONLY_NAMED` and `ARCH_SOURCE_UNREADABLE` rows apply.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Architecture test — memory actions are the person's

  Scenario: Error — a tool names a person-only memory command
    Given crate "ordin-host" contains the string literal "memory.forget_thing" in a tool handler
    When the architecture test runs
    Then it fails with ARCH_PERSON_ONLY_NAMED naming ordin-host, the file and the line

  Scenario: Happy path — the dispatch module may name them
    Given "memory.same_as" appears only in ordin-service-api's catalogue and in ordin-desktop's service_api module
    When the architecture test runs
    Then person_only_commands_not_reachable_by_agents passes

  Scenario: Edge — a read command is not person-only
    Given crate "ordin-host" contains the string literal "memory.used"
    When the architecture test runs
    Then no violation is reported for it
```

### Performance, Security, Observability (additions)

- **Performance:** unchanged; eight more names in the same token pass.
- **Security:** keeping, correcting and forgetting memory stay reachable from screens only.
- **Observability:** none.

### Gaps

None.

---

## Amendment January 2026 — Knowledge agents (ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-113 (knowledge agents: query client and server and a background reader, all Rust modules of the one build under this test; the memory component stays a single-node store). Counterparts: phase 2B-3 (amended January 2026: three rows, two edges), `components/KnowledgeQueryClient.md`, `components/KnowledgeQueryServer.md`, `components/KnowledgeReader.md`. Every rule below adds to the sections above; no test is added.

### Purpose (addition)

The query client, the query server and the reader are crates of the one workspace, so the existing test `crate_graph_matches_phase_2b3` keeps knowledge agents inside ordin's modular monolith: no edge between them that 2B-3 does not list, and no cycle through the traits the programs implement.

### Allowed-dependency table

The table above has the rows `ordin-knowledge-query-client` (phase 2, D P A: `ordin-identity`, `ordin-journal`), `ordin-knowledge-query-server` (phase 6, A: `ordin-knowledge-query-client`, `ordin-memory-service`, `ordin-caller-auth`, `ordin-agent-configuration`, `ordin-journal`, `ordin-telemetry`) and `ordin-knowledge-reader` (phase 8, A: `ordin-agent-configuration`, `ordin-memory-service`, `ordin-knowledge`, `ordin-duty-timer`, `ordin-journal`, `ordin-telemetry`); the `ordin-memory-service` row allows `ordin-knowledge-query-client` and the `ordin-caller-api` row allows `ordin-knowledge-query-server`; the paragraph under the table records the change. The `ordin-desktop` row includes the query client by its wording ("every crate above whose Programs contain D") and never the server or the reader; `ordin-server` includes all three. The test's fixtures and text count three more component crates than before this amendment.

Assumption (*chosen here*):

- **SA-ARCH-KA-01** The `memory` path package stays a dependency of `ordin-memory-service` only (2B-1 rule 7): neither knowledge crate depends on it directly, so the memory component remains one store reached through the Memory Service. The existing embedded-package check covers this with the new crates in the table.

### Internal Logic, Data Model, Error Table

N/A — the existing steps read the table; an edge not listed is the existing `ARCH_EDGE_NOT_ALLOWED` row, a cycle the existing `ARCH_CYCLE` row, and a direct `memory` dependency outside `ordin-memory-service` the existing `ARCH_EMBEDDED_EDGE` row.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — the knowledge crates' edges are allowed
    Given ordin-memory-service depends on ordin-knowledge-query-client
    And ordin-caller-api depends on ordin-knowledge-query-server
    And ordin-knowledge-reader depends on ordin-knowledge and ordin-memory-service
    When the test crate_graph_matches_phase_2b3 runs
    Then no violation is reported

  Scenario: Error path — the query server reaching the reader
    Given ordin-knowledge-query-server lists ordin-knowledge-reader as a dependency
    When the test runs
    Then it fails with ARCH_EDGE_NOT_ALLOWED naming ordin-knowledge-query-server and ordin-knowledge-reader

  Scenario: Error path — the desktop building the query server
    Given ordin-desktop lists ordin-knowledge-query-server as a dependency
    When the test runs
    Then it reports ordin-knowledge-query-server as outside the crates allowed for programme D

  Scenario: Error path — a knowledge crate reaching the memory component directly
    Given ordin-knowledge-reader lists the path package memory as a dependency
    When the test runs
    Then it fails with ARCH_EMBEDDED_EDGE naming ordin-knowledge-reader and memory
```

### Performance, Security, Observability (additions)

- **Performance:** unchanged; three more rows in the same pass.
- **Security:** the query server and the reader are confined to the agent profile's program by the table.
- **Observability:** none.

### Gaps

None.

---

## Amendment January 2026 — Releases and updates (ADR-114)

> Source: decision 93 (the user's decisions of January 2026); HLD revision 0.62.0, ADR-114 (each build answers its own module groups). The two programs (`apps/desktop`, `apps/server`) now declare some module crates as `optional = true`, each enabled by a build switch of the program (for example `mail`, `sandbox`, `agent-profile`; OrdinDesktop.md and OrdinServer.md amendments January 2026 — Releases and updates).
> *Decoupled January 2026: build switches replace release-numbered features; releases are planned in the maintainers' release roadmap, kept outside the repository.*

### Why the test changes

`cargo metadata` resolves an optional dependency only when a feature that enables it is active. Step 2 of *Internal Logic* runs without `--all-features`, so with the default switches (none) the edges `ordin-desktop → ordin-mail`, `ordin-server → ordin-caller-api` and every other edge behind a switch would be absent from `resolve.nodes[].deps[]` and would not be checked. Turning a switch on would then add an edge the test never saw.

### Internal Logic (step 2 replaced)

2. Run `<cargo> metadata --format-version 1 --locked --all-features --manifest-path <workspace_manifest>` with `std::process::Command`, standard output and standard error captured. A non-zero exit returns `ArchTestError::MetadataFailed` with the status and the first line of standard error. **`--all-features` is passed** *(chosen here; replaces the assumption that the default feature set is what the programs build)*: every edge behind every switch is checked in every build, the same graph `deny.toml`'s `[graph] all-features = true` gives the licence check. `cargo metadata` resolves and never compiles, so a feature whose code cannot yet compile (the Sandbox's `extra-guests` before Step E-2, Playbook §5) does not fail this step.

Steps 3 to 12 are unchanged. The allowed sets of `EveryDesktopModule` and `EveryServerModule` stay the 2B-3 rows of their program; an optional edge is checked exactly as a normal one, because `dep_kinds[].kind` carries no feature information and none is needed.

### Error Table

No new row. An edge behind a switch that 2B-3 does not allow is the existing `ARCH_EDGE_NOT_ALLOWED` row.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Happy path — an optional program dependency is still checked
    Given ordin-desktop lists ordin-mail as optional, enabled by its switch mail
    And ordin-desktop's default features are empty
    When the test crate_graph_matches_phase_2b3 runs
    Then the edge ordin-desktop -> ordin-mail is read from the metadata and allowed

  Scenario: Error path — an optional edge outside the program's set
    Given ordin-desktop lists ordin-run-queue as optional, enabled by its switch hosted-agents
    When the test runs
    Then it fails with ARCH_EDGE_NOT_ALLOWED naming ordin-desktop and ordin-run-queue
```

### Performance, Security, Observability (additions)

- **Performance:** unchanged; the resolved graph is the full graph the test assumed before build switches existed.
- **Security:** no build switch can bring in an edge the test has not checked.
- **Observability:** none.

### Gaps

None.

## Amendment January 2026 — The person-only list copied in full

> Source: `components/ServiceApi.md`, whose ADR-113 amendment (SA-SVC-KA-01) adds `"knowledge_agents.add"` and `"knowledge_agents.remove"` to `PERSON_ONLY_COMMANDS`, and whose ADR-114 amendment (SVC-RL-4) adds `"setup.update_server"` and `"setup.install_update"`. Both say the Architecture Test's existing assertion covers the new names; the copy in *Shared Context* stopped at eleven, so the four were not scanned. Every rule below replaces the copy of the ADR-111 amendment; no test, crate edge or error row is added.

### Shared Context (changes)

`PERSON_ONLY_COMMANDS` (exported by `ordin-service-api`), copied in full, 15 names: `["team_knowledge.adopt", "team_knowledge.adopt_again", "team_knowledge.keep_as_is", "memory.correct", "memory.forget", "memory.remember_everywhere", "memory.keep", "memory.correct_link", "memory.forget_link", "memory.forget_thing", "memory.same_as", "knowledge_agents.add", "knowledge_agents.remove", "setup.update_server", "setup.install_update"]`.

### Internal Logic

Unchanged: `check_person_only` reads the constant, so the four added names are checked by the existing steps 1 and 2 with the sites SA-ARCH-ADOPT-01 allows.

### Acceptance Criteria (Gherkin, added)

```gherkin
  Scenario: Error — a workflow step names a person-only update command
    Given a crate other than ordin-service-api and the dispatch modules holds the string literal "setup.install_update"
    When person_only_commands_not_reachable_by_agents runs
    Then it fails with ARCH_PERSON_ONLY_NAMED naming that crate, the file and the line
```

### Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (a new crate, Source Access, is the knowledge agent's only client of the document systems besides Git fetching). Phase 2B row: `phase-2-architecture.md` amendment January 2026.

### Allowed-dependency table — edits

Insert after the `ordin-caller-auth` row (:93, phase 2):

| Crate | Phase | Programs | Allowed workspace dependencies (besides `ordin-types`) |
|---|---|---|---|
| `ordin-source-access` | 2 | A | `ordin-agent-configuration`, `ordin-journal`, `ordin-telemetry` |

Replace two rows:

| Crate | Phase | Programs | Allowed workspace dependencies (besides `ordin-types`) |
|---|---|---|---|
| `ordin-knowledge-query-server` | 6 | A | `ordin-knowledge-query-client`, `ordin-memory-service`, `ordin-caller-auth`, `ordin-agent-configuration`, `ordin-source-access`, `ordin-journal`, `ordin-telemetry` |
| `ordin-knowledge-reader` | 8 | A | `ordin-agent-configuration`, `ordin-memory-service`, `ordin-knowledge`, `ordin-duty-timer`, `ordin-source-access`, `ordin-journal`, `ordin-telemetry` |

Add to the paragraph under the table: "*Amended January 2026 (HLD 0.63.0, ADR-115):* new crate `ordin-source-access` (phase 2, programs A); edges Knowledge Query Server → Source Access (6 → 2) and Knowledge Reader → Source Access (8 → 2). Both point down, so no phase changes. Source Access is given its secrets as values in its settings by the server program, so it has no edge to `ordin-identity`."

### Added assertion

**AT-PD-1** No crate other than `ordin-source-access` and the programs depends on `ordin-source-access` except `ordin-knowledge-query-server` and `ordin-knowledge-reader`; in particular no crate built for programs D or P depends on it (the desktop and the personal server never construct it).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Architecture rules for Source Access (ADR-115)

  Scenario: Happy path — the two knowledge crates may depend on Source Access
    Given the workspace graph read with --all-features
    When the Architecture Test checks ordin-knowledge-query-server and ordin-knowledge-reader
    Then their edges to ordin-source-access are allowed

  Scenario: Error — a desktop crate depends on Source Access
    Given a crate built for program D declares a dependency on ordin-source-access
    When the Architecture Test runs
    Then it fails naming that crate and AT-PD-1

  Scenario: Error — Source Access depends on Identity
    Given ordin-source-access declares a dependency on ordin-identity
    When the Architecture Test runs
    Then it fails naming the edge as not in the allowed-dependency table
```
