# Phase 5 — Generation Playbook

> **Spec set:** ordin-desktop · **Phase:** 5 — Generation Playbook · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-108; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0 · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114 build switches and updates; promoted to Approved by the user January 2026)
> **Source HLD:** `docs/design/ordin-desktop/` · **Inputs:** `phase-2-architecture.md` (2B-1, 2B-3), `components/*.md` (79 specs; *amended January 2026, ADR-115: one new*; *amended January 2026, ADR-113: three new*; *amended January 2026, ADR-102, ADR-103, ADR-106 and ADR-107*), `cross-cutting/*.md` (14 specs) and `phase-4-cross-cutting.md`, HLD 05 — Tech stack · **Authored:** January 2026

The ordered build checklist for the ordin desktop, the ordin server (personal and agent profiles) and the ordin hook. `codegen` consumes one step per invocation, named as `"<step number>: <component>"`. Every step after Step 0 is one Component spec; the order is the 2B-3 build phase (0 → 12) and, within a phase, the 2B-3 table order. Two exceptions are stated where they occur: the Architecture Test is built first (Step 1) and the screens module is built before the ordin desktop (Step 71 before Step 72).

The repository declares no Practice Pack. Every command named below comes from HLD 05 (*Build / test toolchain*, *Licence and vulnerability checks*) or from a component or cross-cutting spec, and HLD 05 states that commands are confirmed by the user before use (precondition P7).

## Table of contents

- [1. Preconditions](#1-preconditions)
- [2. Conventions used by every step](#2-conventions-used-by-every-step)
- [3. Step 0 — Scaffolding, and Step 1 — Architecture Test](#3-step-0--scaffolding-and-step-1--architecture-test)
- [4. Build steps in dependency order](#4-build-steps-in-dependency-order)
- [5. Embedded-component steps](#5-embedded-component-steps)
- [6. Programs and screens](#6-programs-and-screens)
- [7. Integration and verification](#7-integration-and-verification)
- [8. Greenfield note](#8-greenfield-note)
- [9. Build switches](#9-build-switches)

---

## 1. Preconditions

No step after Step 0 starts until P1 to P6 hold. P7 and P8 must hold before the step that first needs them.

| # | Precondition | State on January 2026 | Evidence |
|---|---|---|---|
| P1 | The Phase 6 self-audit (`docs/spec/ordin-desktop/phase-6-audit.md`, `references/audit-checklist.md`) has passed every item, and the user has promoted the spec set from *Draft* to *Approved*. | Met for the set before the January 2026 Slack amendment: the sixth Phase 6 audit passed every item and the user promoted the set to *Approved*. Open for the amended set: the seventh audit (January 2026, after the Slack amendment, ADR-102) found D-1…D-5, which are fixed; the Slack Service spec and the 19 amended specs are promoted once the audit after that fix passes. *Met for the January 2026 team-knowledge amendment (ADR-103, decision 83):* the thirteenth Phase 6 audit passed every item and the user promoted the Team Knowledge Service spec and the specs amended for it to *Approved* on January 2026. *Met for the January 2026 Jira amendment (ADR-106, decision 86):* the fifteenth Phase 6 audit passed every item and the user promoted the Jira Adapter spec and the specs amended for it to *Approved* on January 2026. | `phase-6-audit.md` § *Verdict* (seventh audit, January 2026) |
| P2 | Every Component spec's *Gaps* section reads `None.` | Met: 72 of 72 before the January 2026 amendments; *amended January 2026 (ADR-103):* 74 specs, re-checked by the Phase 6 audit of the team-knowledge amendment (the Team Knowledge Service reads `None.`); *amended January 2026 (ADR-106):* 75 specs (the Jira Adapter reads `None.`), re-checked by the fifteenth Phase 6 audit. | `components/*.md` § Gaps |
| P3 | Every Cross-cutting spec's *Gaps* section reads `None.` | Met: 14 of 14 (InputValidation closed by P4-1, GracefulShutdown by P4-2, LicenceCheck by moving its three open items to the licence gate of Step 0). | `cross-cutting/*.md` § Gaps |
| P4 | The defects Phase 4 routed to the Phase 3 correction round are closed, because three gates below fail on them: the error-code registry check (§7.7) on ErrorHandling.md EH-D5, EH-D7, EH-D8 and EH-D13; the migration registration test (Step 5) on DatabaseMigrations.md M-4 (module-name strings); the Metrics closed-list test (Step 10) on Metrics.md N-8 (ten families with the `ordin_` prefix) and the span checks (§7.4) on Tracing.md T-D2. Configuration.md C-1 (`gitlab.host` with two owners), C-3 (model-name length 128 against 100) and C-5 (three `ORDIN_MCP_*` variables missing from Phase 2D's 2D-1c), and the two listings with no sort key (Pagination.md), are closed in the same round. | Met: every defect is recorded as resolved, confirmed by the Phase 6 audit's spot-checks of EH-D8 (`RUNST_*`), M-4 (module-name strings), N-8 (no `ordin_` metric), C-3 (`OrganisationProfile.md`, 1–100) and C-5 (the `ORDIN_MCP_*` rows in 2D-1c). | `phase-4-cross-cutting.md` § *Defects found in earlier phases*; `phase-6-audit.md` item *Phase 4's open-defects list is empty* |
| P5 | The memory component's changes M-1 to M-6 (MemoryService.md, ADR-101) land in `memory/` before Step 42, as Step E-1. | Not started. | MemoryService.md, *The change ordin needs* |
| P6 | The `sandboxd` changes the Sandbox spec names — per-agent guest compile, one person credential per run, the shell and JavaScript guests and the run environment — land in `sandbox/sandbox-host/` as Step E-2. Until they land, Step 41 builds the Sandbox with only `python_sandbox`, and `shell_sandbox`, `javascript_sandbox` and the `environment` argument stay behind Cargo feature `extra-guests`, off. | Not started. | Sandbox.md, *Approach* (C-17 paragraph), MCP tools table |
| P7 | The user confirms the commands of this Playbook and names the four that no input fixes: the Rust toolchain version for `rust-toolchain.toml`; the line-coverage command for X3; the container tool for the PostgreSQL 18 container of the `[pg]` scenarios (Steps 5, 21, 24, 33, 60, 64) and for building the image of `deploy/container/Containerfile`; the Tauri bundling command for the desktop installer. | Met (decision 81, January 2026): Rust toolchain = the current stable release, pinned in `rust-toolchain.toml` at Step 0; coverage = `cargo llvm-cov` (X3); container tool = Podman (`[pg]` scenarios and `deploy/container/Containerfile`); desktop installer = `cargo tauri build`. Recorded in HLD 05 *Build / test toolchain*. | HLD 05 *Build / test toolchain* ("commands are confirmed by the user before use"); RunQueue.md `[pg]`; OrdinServer.md *Container image* |
| P8 | The parity fixtures are reachable from the tests: frozen crucible at commit `e270904` (`crucible/models/@default/`, and the authoringcheck, compositioncheck and artefactbuild test fixtures) for Steps 8, 15, 30, 31; wrightrun's spec scenarios (`agent-wright-runtime`) for the Rules Engine conformance suite of Step 27. Both are sibling repositories; where the tests read them from (a copied fixture folder or a path to the sibling) is the user's choice. | Open. | AgentModel.md, ResolverAndCompiler.md, Checker.md, Installers.md; HLD 05 *Build / test toolchain* |

The blocking confirmation item C-61 is settled (result (b), Phase 1B OQ-C61). Every other confirmation item is non-blocking and bounds conditional assertions; §7.8 lists which step each one gates.

---

## 2. Conventions used by every step

**Package names.** Module crates live at `crates/<package>`; the programs are `apps/hook` (package `ordin-hook`), `apps/server` (package `ordin-server`) and `apps/desktop` (package `ordin-desktop`, binary `ordin`) (ArchitectureTest.md, *Package names of the programs*). The screens module is the npm package `@ordin/screens` at `apps/desktop/ui/`.

**Gate.** Each Rust step's gate is the four commands written in the step, in this order, then the extra items the step lists:

1. `cargo build -p <package>`
2. `cargo test -p <package>`
3. `cargo clippy -p <package> -- -D warnings`
4. `cargo test -p ordin-arch-test` — the crate graph still matches 2B-3

A gate failure is a stop: the cause is fixed in the code (Patch) or in the spec (Plan), never by relaxing a flag.

**Exit criteria X1–X5**, the same for every step:

| # | Criterion |
|---|---|
| X1 | Every row of the spec's *Error Table* has a unit test that provokes the condition and asserts the code, the retryability and, where stated, the HTTP status or exit status. |
| X2 | Every Gherkin scenario of the spec's *Acceptance Criteria* has a test of the same name; scenarios marked `[pg]` run against PostgreSQL 18 as well as SQLite. |
| X3 | Line coverage of the crate is at least 70 %, measured with the command the user names under P7. |
| X4 | The crate exports `ALL_CODES: &[&str]` beside its error enum (ErrorHandling.md *Internal Logic* 11); a unit test asserts that `ordin_types::error_registry::lookup` finds every entry and that `owns_prefix(<package>, <prefix>)` is true for each. |
| X5 | `cargo fmt --check` and `cargo deny check licenses advisories sources` pass for the workspace after the step's changes (a dependency added by the step enters the licence check here). |

**Cross-cutting conformance.** Each step names the cross-cutting specs it conforms to, by file name under `cross-cutting/`, with the part that lands in that crate. ErrorHandling and Logging apply to every Rust crate. Metrics and Tracing apply to agent-profile code only (SA-XC-04, SA-XC-05; Metrics.md and Tracing.md *Applicability by program*). Cors applies to no program; only the `Origin` refusal and the desktop's content security policy are built (Cors.md). LicenceCheck applies to the whole workspace through X5 and is named in a step only where the crate carries a named exception or a licence fact. The other ten are named where the step's spec uses them: DatabaseMigrations for each table owner of DatabaseMigrations.md *Modules that register migrations*, Pagination for each `Page<T>` reader, GracefulShutdown for each crate with a `shutdown()` or a cancellation token, and the remaining concerns for the crates their *Implemented in* line names.

**Where each shared cross-cutting part lands** (before the first module that needs it):

| Concern | Lands in | Step | First consumer |
|---|---|---|---|
| LicenceCheck (`deny.toml`, `make check`, licence gate) | workspace root | 0 | every later step (X5) |
| ErrorHandling — `CodedError`, `error_registry` | `ordin-types` | 2 | Step 3 onwards (X4) |
| ErrorHandling — Service API envelope (`ServiceError`, `CommandReply`, `handle`) | `ordin-service-api` | 3 | Steps 34, 35 |
| ErrorHandling — hook protocol | `apps/hook`, `ordin-hook-relay` | 26, 39 | Steps 46, 47 |
| ErrorHandling — Caller API envelope and panic layer | `ordin-caller-api` | 65 | Step 70 |
| Logging — module `log` | `ordin-types` | 2 | every module; installed by Steps 70, 72 |
| InputValidation — text, path, URL, cursor and YAML helpers | `ordin-types` | 2 | Step 3 onwards |
| Pagination — `ordin_types::paging` | `ordin-types` | 2 | Steps 3, 5 |
| DatabaseMigrations — migration registry | `ordin-journal` | 5 | Step 6 (exports its steps as data), Step 11 onwards |
| Configuration — profile loader (D, P) | `ordin-organisation-profile` | 6 | Step 12 |
| Configuration — secret backend | `ordin-identity` | 7 | Step 16 |
| Configuration — agent configuration loader (A) | `ordin-agent-configuration` | 9 | Step 22 |
| Configuration — server environment (`ServerEnv::read`) | `apps/server` | 70 | — |
| Metrics, Tracing | `ordin-telemetry` | 10 | Steps 21, 27, 32, 33, 36 |
| AuthenticationAndAuthorisation | `ordin-identity`, `ordin-caller-auth`, `ordin-rules`, `ordin-command-bridge`, `ordin-remote-api`, `ordin-crew-orchestrator`, `ordin-caller-api` | 7, 23, 27, 34, 35, 40, 65 | — |
| GracefulShutdown — run window and stop order | `ordin-session-supervisor`; programs | 32; 70, 72 | — |
| HealthChecks | `ordin-setup` (D), `ordin-caller-api` (A), `apps/server` (P) | 59, 65, 70 | — |
| RateLimiting | each owning crate | 20, 21, 27, 32, 35, 36, 40, 42, 65, 68, adapters | — |
| Cors (`Origin` refusal, CSP) | `ordin-remote-api`, `ordin-caller-api`, `apps/desktop` | 35, 65, 72 | — |

---

## 3. Step 0 — Scaffolding, and Step 1 — Architecture Test

### Step 0 — Scaffolding

Produces the workspace every later step builds into. No product behaviour.

1. **Root manifest** `Cargo.toml` (SA-LAYOUT-01, 2B-1 rules 1, 3, 7):
   - `[workspace]` with `members = ["crates/*", "apps/desktop", "apps/server", "apps/hook"]` and `exclude = ["memory", "sandbox"]` — the embedded components keep their own `Cargo.lock`, and `sandboxd` its own workspace.
   - `[workspace.package]`: `edition = "2021"` (the edition ArchitectureTest.md fixes for its own crate, applied workspace-wide), `license = "Apache-2.0"`, `publish = false`.
   - `[workspace.dependencies]`: every third-party crate and version HLD 05 and the component specs name, so that each crate takes its version from one place.
2. **Folders:** `crates/`, `apps/desktop`, `apps/server`, `apps/hook`, `apps/desktop/ui/`, `deploy/container/`, `deploy/helm/ordin-server/`.
3. **Member manifests** *(chosen here)*. For each of the 72 Rust crates of 2B-3 (*amended January 2026, ADR-102: + `ordin-slack-service`; ADR-103: + `ordin-team-knowledge`*), a `Cargo.toml` that declares exactly its 2B-3 *Dependencies* (as workspace path dependencies, plus `ordin-types`) and the third-party crates its spec names, with an empty `src/lib.rs` (`src/main.rs` with an empty `main` for `apps/hook`, `apps/server`, `apps/desktop`). Reason: LicenceCheck.md *Internal Logic* 8 requires the licence gate to run on the `Cargo.lock` this step writes, before any later step; only a lock that resolves every crate's dependencies lets it confirm the nine named crates and the transitive licences. It also lets the Architecture Test check every edge from Step 1. Each later step owns its crate's contents and may correct its manifest; X5 and gate item 4 re-check any change.
4. **Toolchain pin:** `rust-toolchain.toml` naming the version the user gives under P7.
5. **Lint and format configuration:** none beyond the tools' defaults. Neither HLD 05 nor any spec names a `rustfmt.toml` or `clippy.toml` setting; the gates run `cargo fmt --check` and `cargo clippy … -- -D warnings`.
6. **`deny.toml`** at the workspace root, verbatim from LicenceCheck.md *Public Interface*: `[graph] all-features = true`; `[licenses] allow = ["MIT", "Apache-2.0", "BSD-3-Clause", "ISC", "Unicode-3.0"]`, `confidence-threshold = 0.93`, `exceptions` for `libgit2-sys` (E1) and `notify` (E3); `[advisories] ignore = []`, `yanked = "deny"`; `[sources] unknown-registry = "deny"`, `unknown-git = "deny"`.
7. **Screens module skeleton** `apps/desktop/ui/`: `package.json` with the versions ScreensModule.md pins (`svelte` 5.57.2, `vite` 8.3.4, `@sveltejs/vite-plugin-svelte` 7.3.1, `typescript` 7.0.2, `@tauri-apps/api` 2.12.4, `@fluent/bundle` 0.19.1, `echarts` 6.1.0, `@xterm/xterm` 6.0.0; Monaco Editor pinned at lock time, HLD 05), `vitest` and `svelte-check` as development dependencies (HLD 05), the scripts `gen:api` and `gen:check` (ScreensModule.md *Generation*), and `package-lock.json`.
8. **`Makefile`** with target `check`, stopping at the first failure, in this order (the first three from LicenceCheck.md; the rest *chosen here* from the tools HLD 05 and the specs name): `cargo deny check licenses advisories sources`; in `apps/desktop/ui/`, `npx license-checker-rseidelsohn --production --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC"` and `npm audit --omit=dev` (LicenceCheck.md); `cargo fmt --check`; `cargo clippy --workspace -- -D warnings`; `cargo test --workspace` (which runs the Architecture Test, ArchitectureTest.md); `cargo run -p ordin-service-api --bin ordin-service-api-gen -- check apps/desktop/ui` (ServiceApi.md); in `apps/desktop/ui/`, `npx svelte-check` and `npx vitest run` (HLD 05). Entries whose crate or files do not exist yet are added by the step that creates them (Step 3 for the generator check, Step 71 for the screens checks).
9. **CI:** the CI job runs `make check` and nothing else, so the local and CI gates are one list.
10. **Licence gate** (LicenceCheck.md *Internal Logic* 8), on the `Cargo.lock` that `cargo metadata --format-version 1` writes from the manifests of item 3:
    1. Read the SPDX expression `cargo-deny` reports for the crate bundling libgit2 and narrow E1's `exceptions` entry to exactly that expression.
    2. Add each OSI-approved licence of a transitive crate not yet on `allow` (the Wasmtime family behind the sandbox, the ONNX Runtime bindings behind `fastembed`, the ICU4X data crates, any other) with a one-line comment naming the crate and the reason.
    3. Confirm that the seven crates HLD 05 marks "to be confirmed by cargo-deny before Phase 5" — `thiserror`, `async-trait`, `serde_json`, `tokio-util`, `unicode-normalization` (05 — *Utility and cryptography crates*), `tokio-tungstenite` (05 — *Always on and Slack*), `jsonwebtoken` (05 — *Caller token verification*) — and `rand` and `base64` (ChannelGateway.md) report expressions inside `allow`.
    4. A licence the OSI does not approve, or the GPL or the AGPL, on any crate other than E1 and E3 stops Step 0: the dependency is removed or replaced, or the user records a new named exception through an HLD decision. No such licence is added to `allow`.

**Gate:** `cargo metadata --format-version 1 --locked` exits 0 · `cargo deny check licenses advisories sources` exits 0 after item 10 · in `apps/desktop/ui/`: `npx license-checker-rseidelsohn --production --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC"` and `npm audit --omit=dev` exit 0 · `cargo fmt --check` exits 0.

**Exit:** the workspace resolves with one `Cargo.lock`; the licence gate's four items are done and recorded in `deny.toml` comments; LicenceCheck.md's Gherkin *the workspace passes with the two crate exceptions* holds.

### Step 1 — Architecture Test

Built first, ahead of its 2B-3 phase 12, because it depends on no workspace crate and every later gate runs it.

- **Crate:** `crates/ordin-arch-test` (package `ordin-arch-test`) · **Programs:** build only · **Spec:** `components/ArchitectureTest.md`
- **Needs done:** Step 0
- **Conforms to:** ErrorHandling (`ARCH_` codes, 9 in the registry); Logging
- **Gate:** `cargo build -p ordin-arch-test` · `cargo test -p ordin-arch-test` · `cargo clippy -p ordin-arch-test -- -D warnings` · the unit test of `check_table` (every allowed edge points to a lower phase) · the integration test `crate_graph_matches_phase_2b3` (`tests/architecture.rs`) passes over the Step 0 manifests with no violation
- **Exit:** X1–X5. From here every step's gate item 4 holds the workspace to 2B-3.
- **Amended January 2026 (ADR-106):** the table of `components/ArchitectureTest.md` gains the row `ordin-jira` (phase 2, D) and lets `ordin-workspaces` and `ordin-handover` depend on it.

---
- **Amended January 2026 (ADR-111):** the agentic-memory amendment of `components/ArchitectureTest.md`: `PERSON_ONLY_COMMANDS` gains eight `memory.*` names (ServiceApi.md); no new test.
- **Amended January 2026 (ADR-114):** the test reads `cargo metadata` with `--all-features`, so the programs' optional dependencies behind build switches stay in the checked graph (`components/ArchitectureTest.md` amendment January 2026).

## 4. Build steps in dependency order

Steps 2 to 73, grouped by 2B-3 build phase (Step 73 sits in build phase 7). *Needs done* lists the steps whose crates this crate depends on (2B-3 *Dependencies*, with the 2B-3 names in brackets); Steps 0 and 1 precede every step. *Conditional on* names the confirmation items whose conditional assertions the step implements with the spec's fallback; §7.8 says when each must run.

### Build phase 0

#### Step 2 — Shared types

- **Crate:** `crates/ordin-types` (package `ordin-types`) · **Programs:** D P A · **Spec:** `components/SharedTypes.md`
- **Needs done:** steps 0–1
- **Conforms to:** ErrorHandling (**lands here**: `CodedError`, `RepairAction`, `TypesError`, module `error_registry` (`lookup`, `owns_prefix`)); Logging (**lands here**: module `log` (line form, field rules, argument digest)); Configuration (2D constants copied); InputValidation (**lands here**: `text.rs`, `paging.rs`, `error.rs` helpers and the restricted YAML reader `ordin_types::yaml::read_restricted`, `read_restricted_node`); Pagination (**lands here**: `ordin_types::paging`)
- **Gate:** `cargo build -p ordin-types` · `cargo test -p ordin-types` · `cargo clippy -p ordin-types -- -D warnings` · `cargo test -p ordin-arch-test` · `cargo bench -p ordin-types` for the *Performance targets* (`parse_v7`, `parse_rfc3339_ms_utc`, `normalise_text`)
- **Exit:** X1–X5.
- *Amended January 2026 (ADR-109):* `EventKind::LanguageChanged` (`language_changed`), 19 variants; the round-trip test covers it.
- **Amended January 2026 (ADR-111):** `MemoryRecallId`, `MemoryThingId`, `MemoryLinkId` (with `ID_MEMORY_REF_INVALID`) and `MessageBlock.memory_used` (2C domain and wire, `components/SharedTypes.md` amendment January 2026).


### Build phase 1

#### Step 3 — Service API

- **Crate:** `crates/ordin-service-api` (package `ordin-service-api`) · **Programs:** D P A · **Spec:** `components/ServiceApi.md`
- **Needs done:** steps 0–1; 2 (Shared types)
- **Conforms to:** ErrorHandling (**lands here**: `ServiceError`, `CommandReply`, `ErrorSentences`, `handle`); Logging; InputValidation (`accept`, `Input::check`, `check_block`); Pagination (`Page<T>` in command results)
- **Gate:** `cargo build -p ordin-service-api` · `cargo test -p ordin-service-api` · `cargo clippy -p ordin-service-api -- -D warnings` · `cargo test -p ordin-arch-test` · catalogue test: `CATALOGUE` holds 400 rows with unique names (*amended January 2026, ADR-102, ADR-103, ADR-104 and ADR-106; amended January 2026, ADR-108, ADR-109 and ADR-110; amended January 2026: 400, the total after the ADR-111 to ADR-114 amendments of ServiceApi.md*) · `cargo run -p ordin-service-api --bin ordin-service-api-gen -- write apps/desktop/ui` then `... -- check apps/desktop/ui` exits 0 (TypeScript type generation check) · `cargo bench -p ordin-service-api`
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-109):** the ten `localisation.*` language commands, the three `ordin.*` drafting commands and the event `language_changed` (`ServiceEvent::LanguageChanged`) of `components/ServiceApi.md` are built in this step, with `localisation.import_language` in the 9 MiB body list; the TypeScript types are regenerated. `EventKind::LanguageChanged` lands in Step 2.
- **Amended January 2026 (ADR-106):** the `jira` group (`jira.connect_token`, sensitive input, and `jira.find_issues`) and the changed handover, workspace and sign-in rows of `components/ServiceApi.md` are built in this step; the TypeScript types are regenerated before Step 71.
- **Amended January 2026 (ADR-110):** the seven `team_knowledge.*` commands (`add_read_folder`, `stop_reading`, `adopt`, `adopt_again`, `keep_as_is`, `set_description`, `check_recall`), the optional `folder_id` of `team_knowledge.check_now` and `team_knowledge.get`, and `knowledge.list_read_pages` of `components/ServiceApi.md` are built in this step; the three adopt commands are registered for the interface only and the catalogue test asserts that no MCP Front tool and no run-reachable command list names them. The TypeScript types are regenerated. No event kind is added.
- **Amended January 2026 (ADR-111):** the 13 `memory.*` commands of `components/MemoryService.md` (agentic memory) as catalogue rows (390 commands in 38 groups) and `PERSON_ONLY_COMMANDS` extended (`components/ServiceApi.md`).
- **Amended January 2026 (ADR-114):** the releases-and-updates amendment of `components/ServiceApi.md`: five `setup.*` rows (`check_update`, `whats_new`, `whats_new_seen`, `server_update`, `update_server`), `ServiceHello.groups`, the refusal `SVC_NOT_IN_THIS_VERSION` in `accept`, `SettingsSection::About` and the table of command groups by build switch; `ServiceApi::groups()` and `Incoming.groups`. The catalogue test asserts 400 rows after this amendment (the 377 of the gate line above predates ADR-108 to ADR-113). The TypeScript types are regenerated.

#### Step 4 — Runtime Adapter interface

- **Crate:** `crates/ordin-runtime-adapter` (package `ordin-runtime-adapter`) · **Programs:** D P A · **Spec:** `components/RuntimeAdapterInterface.md`
- **Needs done:** steps 0–1; 2 (Shared types)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (adapter answers parsed into typed shapes); RateLimiting (each adapter for its provider's limits)
- **Gate:** `cargo build -p ordin-runtime-adapter` · `cargo test -p ordin-runtime-adapter` · `cargo clippy -p ordin-runtime-adapter -- -D warnings` · `cargo test -p ordin-arch-test` · the test kit is reachable only through `[dev-dependencies]` (the architecture test reads `cargo metadata` without dev edges)
- **Conditional on:** C-31, C-37, C-41, C-80 (§7.8)
- **Exit:** X1–X5.

#### Step 5 — Journal and Artefact Store

- **Crate:** `crates/ordin-journal` (package `ordin-journal`) · **Programs:** D P A · **Spec:** `components/JournalAndArtefactStore.md`
- **Needs done:** steps 0–1; 2 (Shared types)
- **Conforms to:** ErrorHandling; Logging; Configuration (`history.*` clamp); DatabaseMigrations (**lands here**: `MigrationRegistry`, `ModuleMigrations`, `MigrationStep`, `ApplyMode`, `MigrationReport`, `schema_migrations`); Pagination; GracefulShutdown (`Journal::shutdown`)
- **Gate:** `cargo build -p ordin-journal` · `cargo test -p ordin-journal` · `cargo clippy -p ordin-journal -- -D warnings` · `cargo test -p ordin-arch-test` · registration unit test: every module of each program registers once (`JRNL_MIGRATION_DUPLICATE` otherwise) · migration steps 1–9 on SQLite; on PostgreSQL for the scaled shape
- **Exit:** X1–X5.

#### Step 6 — Organisation Profile

- **Crate:** `crates/ordin-organisation-profile` (package `ordin-organisation-profile`) · **Programs:** D P A · **Spec:** `components/OrganisationProfile.md`
- **Needs done:** steps 0–1; 2 (Shared types)
- **Conforms to:** ErrorHandling; Logging; Configuration (**lands here** (D, P): `OrganisationProfileService`, `ProfileSnapshot`, `InstallerSettings`); DatabaseMigrations (`MODULE_NAME` + `SQLITE_MIGRATIONS`); AuthenticationAndAuthorisation (profile signature); InputValidation (profile at load (`PROF_`)); GracefulShutdown (`shutdown()`)
- **Gate:** `cargo build -p ordin-organisation-profile` · `cargo test -p ordin-organisation-profile` · `cargo clippy -p ordin-organisation-profile -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-12, C-14 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-110):** the `team_knowledge` section of `components/OrganisationProfile.md` (`TeamKnowledgeSection`, `CompanyReadFolder`, `AppliesTo`) and its validation-table row are built in this step; the gate's tests include each refusal scenario of that amendment.

#### Step 7 — Identity and Credentials

- **Crate:** `crates/ordin-identity` (package `ordin-identity`) · **Programs:** D P A · **Spec:** `components/IdentityAndCredentials.md`
- **Needs done:** steps 0–1; 2 (Shared types)
- **Conforms to:** ErrorHandling; Logging; Configuration (**lands here**: `server_secret_backend`, `SecretBackendConfig`); DatabaseMigrations (`MODULE_NAME` + `SQLITE_MIGRATIONS`); AuthenticationAndAuthorisation (sign-ins and tokens)
- **Gate:** `cargo build -p ordin-identity` · `cargo test -p ordin-identity` · `cargo clippy -p ordin-identity -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-06, C-10, C-12, C-14, C-43, C-44, C-49, C-50, C-87 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-106):** the Jira amendment of `components/IdentityAndCredentials.md` (`CredentialKind::Jira`, `jira_access`, `save_jira_token`, migration step 2, a widening rebuild under DatabaseMigrations rule 12a with its migration test) is built in this step.
- **Amended January 2026 (ADR-115):** `access_token_for_scope` (step H.13, Microsoft only, memory cache, consent-required → `CRED_SCOPE_MISSING`); `read:me` asked for only on the Knowledge Query Client's linking path (the Confluence Adapter's scopes unchanged); conditional on C-104.

#### Step 8 — Agent Model

- **Crate:** `crates/ordin-agent-model` (package `ordin-agent-model`) · **Programs:** D P A · **Spec:** `components/AgentModel.md`
- **Needs done:** steps 0–1; 2 (Shared types)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (agent parts at load (`AM_`); YAML anchors, aliases, tags refused)
- **Gate:** `cargo build -p ordin-agent-model` · `cargo test -p ordin-agent-model` · `cargo clippy -p ordin-agent-model -- -D warnings` · `cargo test -p ordin-arch-test` · `--features testkit`: `InMemorySource` passes `assert_part_source_contract` · table-driven parity over frozen crucible's fixtures (`crucible/models/@default/`, 516 files, commit `e270904`)
- **Exit:** X1–X5.

#### Step 9 — Agent Configuration

- **Crate:** `crates/ordin-agent-configuration` (package `ordin-agent-configuration`) · **Programs:** A · **Spec:** `components/AgentConfiguration.md`
- **Needs done:** steps 0–1; 2 (Shared types)
- **Conforms to:** ErrorHandling; Logging; Configuration (**lands here** (A): `load_config`); InputValidation (agent configuration at load (`CFG_`))
- **Gate:** `cargo build -p ordin-agent-configuration` · `cargo test -p ordin-agent-configuration` · `cargo clippy -p ordin-agent-configuration -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-75, C-77, C-83 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** `model.environment` with the reserved names, `controls.content-checks`, context steps (`ChatContext` as `Fixed` or `Steps`, the earlier keys read as the first three steps), the hand-off route keys and list-backed links of `components/AgentConfiguration.md`; codes `CFG_MODEL_ENVIRONMENT_INVALID`, `CFG_CHECKER_UNKNOWN`, `CFG_CONTEXT_STEP_INVALID`, `CFG_ACTION_LIST_INVALID`.
- **Amended January 2026 (ADR-113):** the `knowledge-agent` section, its bounds and `CFG_KNOWLEDGE_AUDIENCE_UNATTESTED` (`components/AgentConfiguration.md` amendment January 2026).
- **Amended January 2026 (ADR-115):** `audience-may-read: false` accepted with `host-kind` and `check-every-question`; `confluence`, `sharepoint`, `access-checks` and `notices` keys; five `CFG_` codes (`components/AgentConfiguration.md` amendment January 2026).

#### Step 10 — Telemetry

- **Crate:** `crates/ordin-telemetry` (package `ordin-telemetry`) · **Programs:** A · **Spec:** `components/Telemetry.md`
- **Needs done:** steps 0–1; 2 (Shared types)
- **Conforms to:** ErrorHandling; Logging; Configuration; Metrics (**lands here**: registry, closed family list, OpenMetrics rendering); Tracing (**lands here**: span allow list, OTLP exporter, counting span queue, `traceparent`)
- **Gate:** `cargo build -p ordin-telemetry` · `cargo test -p ordin-telemetry` · `cargo clippy -p ordin-telemetry -- -D warnings` · `cargo test -p ordin-arch-test` · closed-list test: no family outside the 49 of Metrics.md; span allow list of five (Tracing.md)
- **Conditional on:** C-77 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** family 50 `wrightrun_content_checks_total{point, verdict}`; family 37 also counts one thread failing in ingest.


### Build phase 2
- **Amended January 2026 (ADR-115):** families 59–64 and their `TelemetryEvent` variants; family 40's four route names.

#### Step 11 — Approval Service

- **Crate:** `crates/ordin-approval` (package `ordin-approval`) · **Programs:** D P A · **Spec:** `components/ApprovalService.md`
- **Needs done:** steps 0–1; 5 (Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; AuthenticationAndAuthorisation (approver checks (A)); Pagination (`list_pending`)
- **Gate:** `cargo build -p ordin-approval` · `cargo test -p ordin-approval` · `cargo clippy -p ordin-approval -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.

#### Step 12 — Localisation

- **Crate:** `crates/ordin-localisation` (package `ordin-localisation`) · **Programs:** D P A · **Spec:** `components/Localisation.md`
- **Needs done:** steps 0–1; 5, 6 (Organisation Profile, Journal)
- **Conforms to:** ErrorHandling (error sentences (Fluent message per code)); Logging; Configuration (interface-language clamp); DatabaseMigrations
- **Gate:** `cargo build -p ordin-localisation` · `cargo test -p ordin-localisation` · `cargo clippy -p ordin-localisation -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.
- *Amended January 2026 (ADR-109):* the person layer of `components/Localisation.md` (LOC-LF-1…): `SAFETY_IDS` and `MESSAGE_AREAS` from `build.rs`, the fixed check `check_entry`, the served rule, `LocalisationConfig.app_version` and the re-check on a version change, the language, message, export, import and draft methods, `language_changes()`, and migration `localisation` step 2 (`person_languages`, `person_messages`). The gate's tests include one failing and one passing message per check of `check_entry`, and a person language formatted within the 0.5 ms p95 budget with no file read outside the journal database.

#### Step 13 — Agent Folder Manager

- **Crate:** `crates/ordin-agent-folders` (package `ordin-agent-folders`) · **Programs:** D P A · **Spec:** `components/AgentFolderManager.md`
- **Needs done:** steps 0–1; 5 (Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (`migrations()`)
- **Gate:** `cargo build -p ordin-agent-folders` · `cargo test -p ordin-agent-folders` · `cargo clippy -p ordin-agent-folders -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-64 (§7.8)
- **Exit:** X1–X5.

#### Step 14 — Format Plug-ins

- **Crate:** `crates/ordin-format-plugins` (package `ordin-format-plugins`) · **Programs:** D P A · **Spec:** `components/FormatPlugins.md`
- **Needs done:** steps 0–1; 8 (Agent Model)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (parts in either format (`FMT_`))
- **Gate:** `cargo build -p ordin-format-plugins` · `cargo test -p ordin-format-plugins` · `cargo clippy -p ordin-format-plugins -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-62, C-70 (§7.8)
- **Exit:** X1–X5.

#### Step 15 — Resolver and Compiler

- **Crate:** `crates/ordin-resolver` (package `ordin-resolver`) · **Programs:** D P A · **Spec:** `components/ResolverAndCompiler.md`
- **Needs done:** steps 0–1; 8 (Agent Model)
- **Conforms to:** ErrorHandling; Logging; Configuration
- **Gate:** `cargo build -p ordin-resolver` · `cargo test -p ordin-resolver` · `cargo clippy -p ordin-resolver -- -D warnings` · `cargo test -p ordin-arch-test` · Gherkin *parity with frozen crucible* over every WorkspaceProfile in `crucible/models/@default/workspaces/`
- **Exit:** X1–X5.

#### Step 16 — SharePoint Adapter

- **Crate:** `crates/ordin-sharepoint` (package `ordin-sharepoint`) · **Programs:** D P · **Spec:** `components/SharePointAdapter.md`
- **Needs done:** steps 0–1; 5, 7 (Identity, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; InputValidation (adapter answers (`SP_TOO_LARGE`)); Pagination; RateLimiting (provider limits)
- **Gate:** `cargo build -p ordin-sharepoint` · `cargo test -p ordin-sharepoint` · `cargo clippy -p ordin-sharepoint -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-14, C-42, C-69 (§7.8)
- **Exit:** X1–X5.

#### Step 75 — Jira Adapter

*Added January 2026 (ADR-106). Step numbers are identifiers; this step sits in build phase 2 and is built after Step 16.*

- **Crate:** `crates/ordin-jira` (package `ordin-jira`) · **Programs:** D · **Spec:** `components/JiraAdapter.md`
- **Needs done:** steps 0–1; 5, 7 (Journal, Identity)
- **Conforms to:** ErrorHandling (`JIRA_` codes, 15 in the registry); Logging (no token, no issue text in a log line); InputValidation (addresses, query bounds, response and attachment size limits); RateLimiting (Jira's 429 and 503 with `Retry-After`: back-off with jitter, at most 3 attempts)
- **Gate:** `cargo build -p ordin-jira` · `cargo test -p ordin-jira` (against the Jira Cloud test double of the spec's scenarios; no network) · `cargo clippy -p ordin-jira -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-86, C-87, C-88 (§7.8)
- **Exit:** X1–X5.

#### Step 76 — Knowledge Query Client

*Added January 2026 (ADR-113). Step numbers are identifiers; this step sits in build phase 2 and is built after Step 25.*

- **Crate:** `crates/ordin-knowledge-query-client` (package `ordin-knowledge-query-client`) · **Programs:** D P A · **Spec:** `components/KnowledgeQueryClient.md`
- **Needs done:** steps 0–1; 5, 7 (Journal, Identity)
- **Conforms to:** ErrorHandling (`KQ_` codes of the client); Logging (no query text, no token, counts and timings only); Configuration (2D-4 knowledge deadlines and hops; 2D-2 `knowledge_agents.front_doors[]`); DatabaseMigrations (`kq_connections`, `kq_connection_contents`; SQLite and PostgreSQL slices); InputValidation (https addresses, request and answer bounds); GracefulShutdown (`close()` ends open connections)
- **Gate:** `cargo build -p ordin-knowledge-query-client` · `cargo test -p ordin-knowledge-query-client` (against a test double of the knowledge route group; deadlines, backup request, hop and visited rules, rank fusion; no network) · `cargo clippy -p ordin-knowledge-query-client -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-96 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-115):** linking on connect and on sign-in changes, the per-question SharePoint delegation, `asker` and `pending`, `kq_connection_accounts` (version 2); conditional on C-104.

#### Step 79 — Source Access

*Added January 2026 (ADR-115). Step numbers are identifiers; this step sits in build phase 2 and is built after Step 76.*

- **Crate:** `crates/ordin-source-access` (package `ordin-source-access`) · **Programs:** A · **Spec:** `components/SourceAccess.md`
- **Needs done:** steps 0–1; 5, 9, 10 (Journal, Agent Configuration, Telemetry)
- **Conforms to:** ErrorHandling (`SACC_` codes, 10 in the registry); Logging (R-7c: no token, delegation, account, subject or unit of a refused check; counts only); Metrics (families 59–63 of `components/Telemetry.md` amendment January 2026); Configuration (2D-1d Confluence, SharePoint and notice secret names; 2D-4 access-check constants); DatabaseMigrations (`src_notice_seen`, SQLite and PostgreSQL); InputValidation (whoami answers, notice bodies ≤ 1 MiB, constant-time signature comparison); RateLimiting (L30–L33); GracefulShutdown (`forget_all` on stop)
- **Gate:** `cargo build -p ordin-source-access` · `cargo test -p ordin-source-access` (against recorded-response test doubles of GitHub, GitLab, Confluence Cloud and Microsoft Graph, including 403, 404, 429 and slow answers; no network) · `cargo clippy -p ordin-source-access -- -D warnings` · `cargo test -p ordin-arch-test` · `[pg]` scenarios against a PostgreSQL 18 container
- **Conditional on:** C-101, C-102, C-103, C-104, C-106, C-107 (§7.8)
- **Exit:** X1–X5.

#### Step 17 — Hosted Agent Adapter

- **Crate:** `crates/ordin-hosted-agent-adapter` (package `ordin-hosted-agent-adapter`) · **Programs:** D P · **Spec:** `components/HostedAgentAdapter.md`
- **Needs done:** steps 0–1; 4, 7 (Identity, Runtime Adapter interface)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (adapter answers); RateLimiting (provider limits); GracefulShutdown
- **Gate:** `cargo build -p ordin-hosted-agent-adapter` · `cargo test -p ordin-hosted-agent-adapter` · `cargo clippy -p ordin-hosted-agent-adapter -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-37 (§7.8)
- **Exit:** X1–X5.

#### Step 18 — Slack Thread Adapter

- **Crate:** `crates/ordin-slack-thread-adapter` (package `ordin-slack-thread-adapter`) · **Programs:** D P · **Spec:** `components/SlackThreadAdapter.md`
- **Needs done:** steps 0–1; 4, 7 (Identity, Runtime Adapter interface)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (adapter answers); RateLimiting (provider limits); GracefulShutdown
- **Gate:** `cargo build -p ordin-slack-thread-adapter` · `cargo test -p ordin-slack-thread-adapter` · `cargo clippy -p ordin-slack-thread-adapter -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-04, C-37 (§7.8)
- **Exit:** X1–X5.

#### Step 19 — NATS Adapter

- **Crate:** `crates/ordin-nats` (package `ordin-nats`) · **Programs:** P A · **Spec:** `components/NatsAdapter.md`
- **Needs done:** steps 0–1; 6, 7 (Identity, Organisation Profile)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (adapter answers (`NATS_PAYLOAD_TOO_LARGE`)); RateLimiting (provider limits); GracefulShutdown
- **Gate:** `cargo build -p ordin-nats` · `cargo test -p ordin-nats` · `cargo clippy -p ordin-nats -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-53 (§7.8)
- **Exit:** X1–X5.

#### Step 20 — Usage Planner

- **Crate:** `crates/ordin-usage-planner` (package `ordin-usage-planner`) · **Programs:** D P A · **Spec:** `components/UsagePlanner.md`
- **Needs done:** steps 0–1; 4, 5, 6 (Organisation Profile, Journal, Runtime Adapter interface)
- **Conforms to:** ErrorHandling; Logging; Configuration (`usage.*` clamp); DatabaseMigrations; RateLimiting (person's usage frame)
- **Gate:** `cargo build -p ordin-usage-planner` · `cargo test -p ordin-usage-planner` · `cargo clippy -p ordin-usage-planner -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-29, C-37 (§7.8)
- **Exit:** X1–X5.

#### Step 21 — Run Queue

- **Crate:** `crates/ordin-run-queue` (package `ordin-run-queue`) · **Programs:** A · **Spec:** `components/RunQueue.md`
- **Needs done:** steps 0–1; 5 (Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations (SQLite and PostgreSQL slices); Pagination; RateLimiting (one run per thread); Metrics (families 1–3, 6, 30 (sink))
- **Gate:** `cargo build -p ordin-run-queue` · `cargo test -p ordin-run-queue` · `cargo clippy -p ordin-run-queue -- -D warnings` · `cargo test -p ordin-arch-test` · `[pg]` scenarios against a PostgreSQL 18 container · parity scenario: `SqliteRunStore` and `PgRunStore` return the same values and codes
- **Conditional on:** C-80 (§7.8)
- **Exit:** X1–X5.

#### Step 22 — Model Policy

- **Crate:** `crates/ordin-model-policy` (package `ordin-model-policy`) · **Programs:** A · **Spec:** `components/ModelPolicy.md`
- **Needs done:** steps 0–1; 7, 9 (Agent Configuration, Identity)
- **Conforms to:** ErrorHandling; Logging; Configuration
- **Gate:** `cargo build -p ordin-model-policy` · `cargo test -p ordin-model-policy` · `cargo clippy -p ordin-model-policy -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-50, C-74, C-77 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** `ModelProvider` as a string, form-only MP-2, MP-2a secret resolution through `EnvironmentSecrets`, `MPOL_SECRET_MISSING`; `MPOL_PROVIDER_NOT_READY` and its scenario removed.

#### Step 23 — Caller Authentication

- **Crate:** `crates/ordin-caller-auth` (package `ordin-caller-auth`) · **Programs:** A · **Spec:** `components/CallerAuthentication.md`
- **Needs done:** steps 0–1; 9 (Agent Configuration)
- **Conforms to:** ErrorHandling; Logging; Configuration; AuthenticationAndAuthorisation (**lands here** (A): token, roles, route table)
- **Gate:** `cargo build -p ordin-caller-auth` · `cargo test -p ordin-caller-auth` · `cargo clippy -p ordin-caller-auth -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-111):** the operator route `POST /operator/memory/<verb>` is authorised as an operator route (`components/CallerAuthentication.md` amendment January 2026).
- **Amended January 2026 (ADR-113):** `verify_knowledge`, `KnowledgePrincipal` and the role `none` for route 37 (`components/CallerAuthentication.md` amendment January 2026).
- **Amended January 2026 (ADR-115):** least roles of routes 41–44 (member for 41–42, none for the two notice routes) and route 45 `POST /operator/knowledge/forget-checks` (least role operator) behind `knowledge forget-checks`.

#### Step 24 — Settings Store

- **Crate:** `crates/ordin-settings-store` (package `ordin-settings-store`) · **Programs:** A · **Spec:** `components/SettingsStore.md`
- **Needs done:** steps 0–1; 5, 9 (Agent Configuration, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (SQLite and PostgreSQL slices); InputValidation (setting values); Pagination; Metrics (families 38–39 (sink))
- **Gate:** `cargo build -p ordin-settings-store` · `cargo test -p ordin-settings-store` · `cargo clippy -p ordin-settings-store -- -D warnings` · `cargo test -p ordin-arch-test` · `[pg]` scenarios against a PostgreSQL 18 container
- **Exit:** X1–X5.

#### Step 25 — Server Connector

- **Crate:** `crates/ordin-server-connector` (package `ordin-server-connector`) · **Programs:** D · **Spec:** `components/ServerConnector.md`
- **Needs done:** steps 0–1; 3, 5, 7 (Service API, Identity, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (`MIGRATIONS` const (exempt, SA-CLOSE-05)); GracefulShutdown
- **Gate:** `cargo build -p ordin-server-connector` · `cargo test -p ordin-server-connector` · `cargo clippy -p ordin-server-connector -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-71 (§7.8)
- **Exit:** X1–X5.

#### Step 26 — ordin hook (program)

- **Crate:** `apps/hook` (package `ordin-hook`) · **Programs:** D P A · **Spec:** `components/OrdinHook.md`
- **Needs done:** steps 0–1; 2 (Shared types)
- **Conforms to:** ErrorHandling (**lands here** with step 39: hook protocol (deny, exit 0)); Logging; Configuration (hook environment (2D-1c)); InputValidation (hook input size and shape)
- **Gate:** `cargo build -p ordin-hook` · `cargo test -p ordin-hook` · `cargo clippy -p ordin-hook -- -D warnings` · `cargo test -p ordin-arch-test` · C-27 conformance tests of the Copilot payload mapping (D.2)
- **Conditional on:** C-26 (passed), C-27, C-74, C-78 (§7.8)
- **Exit:** X1–X5.


### Build phase 3

#### Step 27 — Rules Engine

- **Crate:** `crates/ordin-rules` (package `ordin-rules`) · **Programs:** D P A · **Spec:** `components/RulesEngine.md`
- **Needs done:** steps 0–1; 5, 6, 11 (Approval Service, Journal, Organisation Profile)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (SQLite and PostgreSQL slices); AuthenticationAndAuthorisation (layer 2); RateLimiting (declared start and repeat limits (A)); Metrics (families 11–15 (sink))
- **Gate:** `cargo build -p ordin-rules` · `cargo test -p ordin-rules` · `cargo clippy -p ordin-rules -- -D warnings` · `cargo test -p ordin-arch-test` · rules-engine conformance suite driven from wrightrun's spec scenarios (HLD 05 — *Build / test toolchain*; ADR-007)
- **Conditional on:** C-26 (passed), C-27, C-78, C-80 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** the content check (`ContentChecker`, `RulesHost::content_checker`, `check_content`, `RULE_CHECKER_UNREACHABLE`) and its control lines in the start-up report; the gate runs its seven new scenarios against an HTTP stand-in that answers allow, block, or never.

#### Step 28 — Brand

- **Crate:** `crates/ordin-brand` (package `ordin-brand`) · **Programs:** D · **Spec:** `components/Brand.md`
- **Needs done:** steps 0–1; 5, 6, 12 (Organisation Profile, Localisation, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations
- **Gate:** `cargo build -p ordin-brand` · `cargo test -p ordin-brand` · `cargo clippy -p ordin-brand -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.

#### Step 29 — Dictation Service

- **Crate:** `crates/ordin-dictation` (package `ordin-dictation`) · **Programs:** D · **Spec:** `components/DictationService.md`
- **Needs done:** steps 0–1; 12 (Localisation)
- **Conforms to:** ErrorHandling; Logging
- **Gate:** `cargo build -p ordin-dictation` · `cargo test -p ordin-dictation` · `cargo clippy -p ordin-dictation -- -D warnings` · `cargo test -p ordin-arch-test` · build on macOS (Swift static library through `build.rs` and `swift build`) and on a non-macOS host (`#[cfg(target_os = "macos")]` path excluded)
- **Exit:** X1–X5.

#### Step 30 — Checker

- **Crate:** `crates/ordin-checker` (package `ordin-checker`) · **Programs:** D P A · **Spec:** `components/Checker.md`
- **Needs done:** steps 0–1; 5, 6, 8, 15 (Agent Model, Resolver and Compiler, Organisation Profile, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (SQLite and PostgreSQL slices); InputValidation (every part before it is offered (`CHK_`))
- **Gate:** `cargo build -p ordin-checker` · `cargo test -p ordin-checker` · `cargo clippy -p ordin-checker -- -D warnings` · `cargo test -p ordin-arch-test` · parity: frozen crucible's authoringcheck, compositioncheck and artefactbuild fixtures at `e270904` give the same findings (R-69)
- **Exit:** X1–X5.

#### Step 31 — Installers

- **Crate:** `crates/ordin-installers` (package `ordin-installers`) · **Programs:** D P A · **Spec:** `components/Installers.md`
- **Needs done:** steps 0–1; 8, 13, 15 (Agent Folder Manager, Resolver and Compiler, Agent Model)
- **Conforms to:** ErrorHandling; Logging; Configuration
- **Gate:** `cargo build -p ordin-installers` · `cargo test -p ordin-installers` · `cargo clippy -p ordin-installers -- -D warnings` · `cargo test -p ordin-arch-test` · golden-tree tests of each installer's pure render against frozen crucible's converter output
- **Conditional on:** C-01, C-26 (passed), C-60, C-61 (settled, result b), C-64 (§7.8)
- **Exit:** X1–X5.

#### Step 32 — Session Supervisor

- **Crate:** `crates/ordin-session-supervisor` (package `ordin-session-supervisor`) · **Programs:** D P A · **Spec:** `components/SessionSupervisor.md`
- **Needs done:** steps 0–1; 4, 5, 20, 21 (Runtime Adapter interface, Journal, Usage Planner, Run Queue)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (SQLite and PostgreSQL slices); Pagination; RateLimiting (local sessions, run limits); GracefulShutdown (**lands here**: `shutdown`, `stop_taking_runs`, `wait_for_runs`); Metrics (families 5, 16, 17, 31 (sink)); Tracing (opens `ordin.run_stretch`, `ordin.model_turn`, `ordin.subagent_call` (A))
- **Gate:** `cargo build -p ordin-session-supervisor` · `cargo test -p ordin-session-supervisor` · `cargo clippy -p ordin-session-supervisor -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-05, C-29, C-74, C-80 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** J.7 input and output checks around the run, the retry without gathered context, and the run summary's `checks`, `checks_blocked` and per-step `context`.
- **Amended January 2026 (ADR-111):** `SupervisorHost::run_starting` before every agent task session, which registers or clears the agent's memory rules (`components/SessionSupervisor.md` amendment January 2026).

#### Step 33 — Duty Timer

- **Crate:** `crates/ordin-duty-timer` (package `ordin-duty-timer`) · **Programs:** A · **Spec:** `components/DutyTimer.md`
- **Needs done:** steps 0–1; 5, 9, 10, 21 (Agent Configuration, Run Queue, Telemetry, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (SQLite and PostgreSQL slices); GracefulShutdown; Metrics (family 4)
- **Gate:** `cargo build -p ordin-duty-timer` · `cargo test -p ordin-duty-timer` · `cargo clippy -p ordin-duty-timer -- -D warnings` · `cargo test -p ordin-arch-test` · `[pg]` scenarios against a PostgreSQL 18 container
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-113):** `BuiltInDuty` and `register_built_in`, with the firing guard keyed `built-in/<kind>/<interval seconds>` (`components/DutyTimer.md` amendment January 2026).

#### Step 34 — Command Bridge

- **Crate:** `crates/ordin-command-bridge` (package `ordin-command-bridge`) · **Programs:** D · **Spec:** `components/CommandBridge.md`
- **Needs done:** steps 0–1; 3, 5, 25 (Service API, Server Connector, Journal)
- **Conforms to:** ErrorHandling; Logging; AuthenticationAndAuthorisation (layer 1 (with the Tauri capability of `apps/desktop`)); InputValidation (every IPC command); GracefulShutdown
- **Gate:** `cargo build -p ordin-command-bridge` · `cargo test -p ordin-command-bridge` · `cargo clippy -p ordin-command-bridge -- -D warnings` · `cargo test -p ordin-arch-test` · performance: 1,000 events a second for 60 s with no `BRG_WINDOW_LAGGING` on the reference machine (C-73)
- **Conditional on:** C-73 (§7.8)
- **Exit:** X1–X5.

#### Step 35 — Remote API

- **Crate:** `crates/ordin-remote-api` (package `ordin-remote-api`) · **Programs:** P · **Spec:** `components/RemoteApi.md`
- **Needs done:** steps 0–1; 3, 5, 6, 7, 11 (Service API, Approval Service, Journal, Identity, Organisation Profile)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (`migrations()`); AuthenticationAndAuthorisation (desktop key, client certificates); InputValidation; Pagination (`SRV_BAD_CURSOR`, status 422); RateLimiting (command bucket, failed-key lockout); GracefulShutdown; Cors (refuses a request carrying `Origin` with `SRV_REFUSED`)
- **Gate:** `cargo build -p ordin-remote-api` · `cargo test -p ordin-remote-api` · `cargo clippy -p ordin-remote-api -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-71 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-111):** the new `memory.*` commands pass through the existing command route on the personal server; a hosted agent's scope is refused there with `MEM_NOT_SUPPORTED` (`components/RemoteApi.md` amendment January 2026).


### Build phase 4

#### Step 36 — MCP Front

- **Crate:** `crates/ordin-mcp-front` (package `ordin-mcp-front`) · **Programs:** D P A · **Spec:** `components/McpFront.md`
- **Needs done:** steps 0–1; 3, 5, 27 (Rules Engine, Journal, Service API)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (MCP tool calls); RateLimiting (calls in flight); GracefulShutdown; Metrics (families 48–49 (sink)); Tracing (opens `ordin.tool_call` with `origin = "ordin_mcp_front"` (A))
- **Gate:** `cargo build -p ordin-mcp-front` · `cargo test -p ordin-mcp-front` · `cargo clippy -p ordin-mcp-front -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-22, C-76, C-80 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-110):** no new tool. The `recall` input schemas of server `memory` (registered by the Memory Service, Step 42) and server `ordin` (Ordin Host, Step 57) gain `searches` and `folders`; the MCP Front's schema check accepts them unchanged. No adopt tool exists on any server.
- **Amended January 2026 (ADR-111):** no new tool. Server `memory` is offered to agent task sessions whose agent has memory rules; the `remember` schemas of servers `memory` and `ordin` gain `entities`, `links` and (Ordin only) `made_from` (`components/McpFront.md` amendment January 2026).

#### Step 37 — Package Inspector

- **Crate:** `crates/ordin-package-inspector` (package `ordin-package-inspector`) · **Programs:** D P A · **Spec:** `components/PackageInspector.md`
- **Needs done:** steps 0–1; 8, 14, 27 (Rules Engine, Format Plug-ins, Agent Model)
- **Conforms to:** ErrorHandling; Logging; InputValidation (package declarations (`PKI_`))
- **Gate:** `cargo build -p ordin-package-inspector` · `cargo test -p ordin-package-inspector` · `cargo clippy -p ordin-package-inspector -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.

#### Step 38 — File Service

- **Crate:** `crates/ordin-files` (package `ordin-files`) · **Programs:** D P A · **Spec:** `components/FileService.md`
- **Needs done:** steps 0–1; 5, 13, 27 (Agent Folder Manager, Rules Engine, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations (SQLite and PostgreSQL slices); Pagination; GracefulShutdown; LicenceCheck (`notify` 8.2.0, exception E3)
- **Gate:** `cargo build -p ordin-files` · `cargo test -p ordin-files` · `cargo clippy -p ordin-files -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-36 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-104):** the moving-a-file amendment of `components/FileService.md` (`move_file`, keeping version rows, the index row, held edits and published-copy keys with the file; renames seen by the watcher carry their history; `FILE_MOVE_TARGET_EXISTS`) is built in this step.

#### Step 39 — Hook Relay

- **Crate:** `crates/ordin-hook-relay` (package `ordin-hook-relay`) · **Programs:** D P A · **Spec:** `components/HookRelay.md`
- **Needs done:** steps 0–1; 5, 11, 27, 32 (Rules Engine, Approval Service, Session Supervisor, Journal)
- **Conforms to:** ErrorHandling (**lands here** with step 26: hook protocol); Logging; Configuration; DatabaseMigrations (SQLite and PostgreSQL slices); InputValidation (hook requests); GracefulShutdown; Tracing (opens `ordin.tool_call` with `origin = "built_in"` (A))
- **Gate:** `cargo build -p ordin-hook-relay` · `cargo test -p ordin-hook-relay` · `cargo clippy -p ordin-hook-relay -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-78 (§7.8)
- **Exit:** X1–X5.

#### Step 40 — Crew Orchestrator

- **Crate:** `crates/ordin-crew-orchestrator` (package `ordin-crew-orchestrator`) · **Programs:** D P · **Spec:** `components/CrewOrchestrator.md`
- **Needs done:** steps 0–1; 4, 5, 6, 11, 20, 27, 32 (Session Supervisor, Runtime Adapter interface, Rules Engine, Approval Service, Usage Planner, Organisation Profile, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; AuthenticationAndAuthorisation (layer 3); RateLimiting (crew guardrails); GracefulShutdown
- **Gate:** `cargo build -p ordin-crew-orchestrator` · `cargo test -p ordin-crew-orchestrator` · `cargo clippy -p ordin-crew-orchestrator -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.


### Build phase 5

#### Step 41 — Sandbox

- **Crate:** `crates/ordin-sandbox` (package `ordin-sandbox`) · **Programs:** D P · **Spec:** `components/Sandbox.md`
- **Needs done:** steps 0–1; 5, 7, 27, 36 (MCP Front, Rules Engine, Identity, Journal); E-2 is scheduled first but does not block: without it `extra-guests` stays off (§5). The C-17 spike must have passed (§7.8).
- **Conforms to:** ErrorHandling; Logging; Configuration (2D-1f map); InputValidation (typed arguments, unknown fields refused); GracefulShutdown
- **Gate:** `cargo build -p ordin-sandbox` · `cargo test -p ordin-sandbox` · `cargo clippy -p ordin-sandbox -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-17, C-23, C-38 (§7.8)
- **Exit:** X1–X5.

#### Step 42 — Memory Service

- **Crate:** `crates/ordin-memory-service` (package `ordin-memory-service`) · **Programs:** D P A · **Spec:** `components/MemoryService.md`
- **Needs done:** steps 0–1; 5, 36, 76 (MCP Front, Journal, Knowledge Query Client — *76 amended January 2026 (ADR-114 audit): the crate depends on `ordin-knowledge-query-client` since ADR-113*); E-1 (`memory/` changes M-1 to M-6, §5; *amended January 2026, ADR-103:* and M-7; *amended January 2026, ADR-110:* and M-8; *amended January 2026, ADR-111:* and M-9)
- **Conforms to:** ErrorHandling; Logging; Configuration (2D-1e map); DatabaseMigrations (`mem_*` tables, `memory_fact_refs`; SQLite and PostgreSQL slices); RateLimiting (memory rate); GracefulShutdown; HealthChecks (readiness input (A)); LicenceCheck (no SurrealDB in the build (ADR-101))
- **Gate:** `cargo build -p ordin-memory-service` · `cargo test -p ordin-memory-service` · `cargo clippy -p ordin-memory-service -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-15, C-18, C-35, C-82 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-103):** the team-knowledge amendment of `components/MemoryService.md` (export and import of a workspace's facts for the Team Knowledge Service, the personal-data check, and its additive migration step) is built in this step, after E-1 item 7.
- **Amended January 2026 (ADR-104):** `FactView.page_section` (the page-section amendment of `components/MemoryService.md`) is built in this step; no migration.
- **Amended January 2026 (ADR-110):** the read-folder amendment of `components/MemoryService.md`: `TeamImportBatch.read_folder`, `memory_fact_refs.read_folder_id`, export excluding read-folder facts, `remove_read_folder`, `adopt_fact`, `index_read_page_sections`, `count_workspace_facts`, `check_content`, the trait `MemoryFolders` (implemented by the programs, Steps 70 and 72), the `FactView` additions, and `searches` and `folders` on both `recall` tools. The gate's tests include the amendment's scenarios at this module's boundary.
- **Amended January 2026 (ADR-111):** the agentic-memory amendment of `components/MemoryService.md`: `MemoryCaller::Agent`, things and links on `remember`, summaries with `made_from`, the recall record and `recall_id`, fading and **Keep**, history and origins, things and links for the person, `MemoryPages`, `decay_k` 864,000, and the additive migration step "agentic memory" with the M-9 SQL; built after E-1 carries M-9 (memory spec set Step 6g). Its Gherkin and the §7.10 runs are its acceptance.
- **Amended January 2026 (ADR-112, ADR-113):** the knowledge-agents amendment of `components/MemoryService.md`: page trust by `PageTier`, deprecated pages last, `wider` recall through the Knowledge Query Client (needs Step 76), `more_found`, the recall record's outside rows (`memory_recall_wider`), `MemoryCaller::KnowledgeAgent`, `index_knowledge_document`, `remove_knowledge_document`, `top_things`, and the additive migration step.
- **Amended January 2026 (ADR-115):** knowledge-scope `result_cap` up to 50 for the shortlist; `derived_from` and `derived_origins` on each recalled fact; `top_things_within`; `MemoryUsedView.wider_unchecked`.

#### Step 43 — Craft Toolkit

- **Crate:** `crates/ordin-craft-toolkit` (package `ordin-craft-toolkit`) · **Programs:** D P · **Spec:** `components/CraftToolkit.md`
- **Needs done:** steps 0–1; 5, 6, 27, 36, 38 (MCP Front, Rules Engine, File Service, Organisation Profile, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations; GracefulShutdown; LicenceCheck (ArtCraft apps run as separate programs, outside the check (C-08))
- **Gate:** `cargo build -p ordin-craft-toolkit` · `cargo test -p ordin-craft-toolkit` · `cargo clippy -p ordin-craft-toolkit -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-07, C-08, C-09, C-13, C-21, C-28, C-47 (§7.8)
- **Exit:** X1–X5.

#### Step 44 — Confluence Adapter

- **Crate:** `crates/ordin-confluence` (package `ordin-confluence`) · **Programs:** D · **Spec:** `components/ConfluenceAdapter.md`
- **Needs done:** steps 0–1; 5, 7, 11, 38 (Identity, Approval Service, File Service, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; InputValidation (adapter answers (`CONF_TOO_LARGE`)); Pagination; RateLimiting (provider limits)
- **Gate:** `cargo build -p ordin-confluence` · `cargo test -p ordin-confluence` · `cargo clippy -p ordin-confluence -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-11, C-12, C-13, C-42 (§7.8)
- **Exit:** X1–X5.

#### Step 45 — Project Service

- **Crate:** `crates/ordin-projects` (package `ordin-projects`) · **Programs:** D P · **Spec:** `components/ProjectService.md`
- **Needs done:** steps 0–1; 5, 7, 27, 36, 38 (MCP Front, File Service, Rules Engine, Identity, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations; GracefulShutdown; LicenceCheck (`git2` with libgit2, exception E1)
- **Gate:** `cargo build -p ordin-projects` · `cargo test -p ordin-projects` · `cargo clippy -p ordin-projects -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-10, C-30, C-32, C-33 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-110):** `GithubAccess::branch_review_status` (`Required`, `NotRequired`, `CannotTell`) over GitHub rulesets and branch protection and GitLab protected branches and approval rules, with `branch_review_rules` derived from it so that a 403 or 404 never reads as "no review" (fixes `ProjectService.md:754`). The gate's tests cover each answer against recorded host replies, including a read-only token on classic protection (`CannotTell`).

#### Step 46 — Claude Local Adapter

- **Crate:** `crates/ordin-claude-adapter` (package `ordin-claude-adapter`) · **Programs:** D P A · **Spec:** `components/ClaudeLocalAdapter.md`
- **Needs done:** steps 0–1; 4, 27, 36, 39 (Runtime Adapter interface, MCP Front, Hook Relay, Rules Engine)
- **Conforms to:** ErrorHandling; Logging; Configuration (2D-1c variables set or removed); InputValidation (harness output parsed into typed events); RateLimiting (provider limits); GracefulShutdown
- **Gate:** `cargo build -p ordin-claude-adapter` · `cargo test -p ordin-claude-adapter` · `cargo clippy -p ordin-claude-adapter -- -D warnings` · `cargo test -p ordin-arch-test` · init-event conformance test: the CLI's tool list equals `DESKTOP_BUILT_IN_TOOLS` at the pinned Claude Code version
- **Conditional on:** C-25, C-26 (passed), C-37, C-41, C-61 (settled, result b), C-74, C-77, C-79, C-80 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** step 8 sets the declared provider settings in the child only; `CLA_ENV_RESERVED`; the gate's child-environment test uses a local stand-in model server that records requests.

#### Step 47 — Copilot Local Adapter

- **Crate:** `crates/ordin-copilot-adapter` (package `ordin-copilot-adapter`) · **Programs:** D P · **Spec:** `components/CopilotLocalAdapter.md`
- **Needs done:** steps 0–1; 4, 7, 27, 36, 39 (Runtime Adapter interface, MCP Front, Hook Relay, Rules Engine, Identity)
- **Conforms to:** ErrorHandling; Logging; Configuration (2D-1c variables set or removed); InputValidation (harness output parsed into typed events); RateLimiting (provider limits); GracefulShutdown
- **Gate:** `cargo build -p ordin-copilot-adapter` · `cargo test -p ordin-copilot-adapter` · `cargo clippy -p ordin-copilot-adapter -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-01, C-02, C-03, C-05, C-25, C-27, C-37, C-41, C-48, C-60, C-75 (§7.8)
- **Exit:** X1–X5.

#### Step 48 — Connection Binder

- **Crate:** `crates/ordin-connection-binder` (package `ordin-connection-binder`) · **Programs:** A · **Spec:** `components/ConnectionBinder.md`
- **Needs done:** steps 0–1; 9, 10, 27, 36 (Agent Configuration, MCP Front, Rules Engine, Telemetry)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (answers of outbound connections); GracefulShutdown; Metrics (families 9, 10, 24); Tracing (opens `ordin.http_out` (A))
- **Gate:** `cargo build -p ordin-connection-binder` · `cargo test -p ordin-connection-binder` · `cargo clippy -p ordin-connection-binder -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-74, C-76 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** B-5a binds a checker operation for the Rules Engine and keeps it off every tool list; `TOOL_CHECKER_UNFIT`; B-18 availability of list-backed links.


### Build phase 6

#### Step 77 — Knowledge Query Server

*Added January 2026 (ADR-113). Step numbers are identifiers; this step sits in build phase 6 and is built after Step 42.*

- **Crate:** `crates/ordin-knowledge-query-server` (package `ordin-knowledge-query-server`) · **Programs:** A · **Spec:** `components/KnowledgeQueryServer.md`
- **Needs done:** steps 0–1; 5, 9, 10, 23, 42, 76 (Journal, Agent Configuration, Telemetry, Caller Authentication, Memory Service, Knowledge Query Client)
- **Conforms to:** ErrorHandling (`KQ_` codes of the server); Logging (no query text, no asker identity); Metrics (the knowledge families of `components/Telemetry.md` amendment January 2026); Tracing (span `ordin.knowledge_query`, no content); Configuration (`knowledge-agent` section; 2D-4 cache and change-stream constants); DatabaseMigrations (`kq_cache`, `kq_cache_commits`, `kq_contents`, `kq_reader_status`, `kq_proxy_asks`, `kq_changes`, `kq_answer_sources`, `kq_answer_source_items`); AuthenticationAndAuthorisation (audience check after `verify_knowledge`); HealthChecks (readiness input (A))
- **Gate:** `cargo build -p ordin-knowledge-query-server` · `cargo test -p ordin-knowledge-query-server` · `cargo clippy -p ordin-knowledge-query-server -- -D warnings` · `cargo test -p ordin-arch-test` · `[pg]` scenarios against a PostgreSQL 18 container
- **Conditional on:** C-95, C-96 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-115):** the per-person filter before ranking, routes 41–44, `kq_linked_accounts` (version 2) and the cache purge, `unchecked_sources`, `pending`; needs done gains 79; conditional on C-105.

#### Step 49 — Workspace Service

- **Crate:** `crates/ordin-workspaces` (package `ordin-workspaces`) · **Programs:** D P · **Spec:** `components/WorkspaceService.md`
- **Needs done:** steps 0–1; 5, 16, 27, 38, 44, 75 (File Service, Confluence Adapter, SharePoint Adapter, Jira Adapter, Rules Engine, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; Pagination; GracefulShutdown
- **Gate:** `cargo build -p ordin-workspaces` · `cargo test -p ordin-workspaces` · `cargo clippy -p ordin-workspaces -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-103):** the team-knowledge amendment of `components/WorkspaceService.md` (the removed-link notice the Team Knowledge Service listens to) is built in this step.
- **Amended January 2026 (ADR-106):** the Jira amendment of `components/WorkspaceService.md` (`SourceLinkKind::JiraIssue`, §J reading, origin check and **Bring in the new version**, migration step 3, a widening rebuild under DatabaseMigrations rule 12a with its migration test) is built in this step.

#### Step 50 — Catalogue Service

- **Crate:** `crates/ordin-catalogue` (package `ordin-catalogue`) · **Programs:** D P A · **Spec:** `components/CatalogueService.md`
- **Needs done:** steps 0–1; 5, 6, 8, 14, 30, 45 (Organisation Profile, Project Service, Format Plug-ins, Checker, Agent Model, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (own SQLite file in A (DatabaseMigrations Internal Logic 13)); Pagination; GracefulShutdown; LicenceCheck (`git2`, exception E1)
- **Gate:** `cargo build -p ordin-catalogue` · `cargo test -p ordin-catalogue` · `cargo clippy -p ordin-catalogue -- -D warnings` · `cargo test -p ordin-arch-test` · Gherkin *The PartSource contract test passes*: `assert_part_source_contract` against the catalogue over the contract fixture repositories
- **Conditional on:** C-63, C-66, C-67 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-106):** the Jira amendment of `components/CatalogueService.md` (lifecycle place type `jira`, `LIFE_PLACE_INVALID`, migration step 2, a widening rebuild under DatabaseMigrations rule 12a) is built in this step.

#### Step 51 — Push Service

- **Crate:** `crates/ordin-push` (package `ordin-push`) · **Programs:** D P · **Spec:** `components/PushService.md`
- **Needs done:** steps 0–1; 5, 11, 16, 38, 43, 44, 45 (Approval Service, SharePoint Adapter, Confluence Adapter, Project Service, Craft Toolkit, File Service, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; GracefulShutdown; LicenceCheck (`git2`, exception E1)
- **Gate:** `cargo build -p ordin-push` · `cargo test -p ordin-push` · `cargo clippy -p ordin-push -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-13, C-28, C-42 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-103):** the team-knowledge amendment of `components/PushService.md` (team shares: card, push of a prepared commit or a review request, resend after the branch moved, withdrawal, recovery, and its additive migration step) is built in this step.

#### Step 52 — Agent Loader

- **Crate:** `crates/ordin-agent-loader` (package `ordin-agent-loader`) · **Programs:** A · **Spec:** `components/AgentLoader.md`
- **Needs done:** steps 0–1; 9, 22, 27, 36, 38, 48 (Agent Configuration, Model Policy, Connection Binder, MCP Front, Rules Engine, File Service)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (agent release at load (`REL_`)); Metrics (family 29 set by the server after `AgentLoader::load`)
- **Gate:** `cargo build -p ordin-agent-loader` · `cargo test -p ordin-agent-loader` · `cargo clippy -p ordin-agent-loader -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-61 (settled, result b), C-74, C-77, C-78, C-79 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** `ConfiguredControls.content_checks` filled at L-18; the Model Policy copy updated.


### Build phase 7

#### Step 53 — Knowledge Service

- **Crate:** `crates/ordin-knowledge` (package `ordin-knowledge`) · **Programs:** D P A · **Spec:** `components/KnowledgeService.md`
- **Needs done:** steps 0–1; 5, 27, 36, 38, 42, 49, 51 (MCP Front, File Service, Workspace Service, Memory Service, Rules Engine, Push Service, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (SQLite and PostgreSQL slices); Pagination (`notes`, `entries`); GracefulShutdown; Metrics (families 25, 26, 35–37 (sink))
- **Gate:** `cargo build -p ordin-knowledge` · `cargo test -p ordin-knowledge` · `cargo clippy -p ordin-knowledge -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-35 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-103):** the team-knowledge amendment of `components/KnowledgeService.md` (page export and import, the optional `shared_by` and `shared_at` header keys, the newer-edit-wins rule with the other kept as an earlier version, pages citing places of another computer) is built in this step.
- **Amended January 2026 (ADR-104):** the page-links amendment of `components/KnowledgeService.md` (extra header entries kept, `tags` and `aliases`, the link graph and backlinks, renames that move the file and rewrite linking pages, adoption of new files, removal seen in the folder, suggestions, filter words, migration step 4) is built in this step, after Step 38's `move_file`.
- **Amended January 2026 (ADR-106):** the Jira amendment of `components/KnowledgeService.md` (`SourceLinkKind::JiraIssue` in its copy; a Jira origin change marks no page) is built in this step.
- **Amended January 2026 (ADR-107):** `KnowledgeIndex::lookup` (section H step 9); ingest reads thread replies with the checkpoint's `threads` (section I); older checkpoints still load.
- **Amended January 2026 (ADR-110):** the read-folder amendment of `components/KnowledgeService.md`: the read-pages table and its full-text index (migration step), `import_read_page`, `remove_read_page`, `remove_read_folder_pages`, `adopt_page`, read pages in the knowledge tools and page list with their folder, links resolved within one folder, and `knowledge.list_read_pages`.
- **Amended January 2026 (ADR-111):** `PageGraph` (title, other names, linked pages) passed to `index_page_sections` and `index_read_page_sections`; `page_for_names` serving **Explained in**; a page linking to a removed or renamed page is re-indexed (`components/KnowledgeService.md` amendment January 2026).
- **Amended January 2026 (ADR-112):** the Open Knowledge Format amendment of `components/KnowledgeService.md`: the `okf` module, page keys `type`, `description`, `generated`, `verified` and the new `sources` fields, reserved `index.md` and `log.md`, the written `index.md`, links written as `[Title](/path.md)`, plain bundles read as pages, and migration step 6.

#### Step 54 — Mail Service

- **Crate:** `crates/ordin-mail` (package `ordin-mail`) · **Programs:** D P · **Spec:** `components/MailService.md`
- **Needs done:** steps 0–1; 5, 6, 7, 11, 27, 36, 38, 49 (MCP Front, Identity, Rules Engine, Approval Service, File Service, Workspace Service, Organisation Profile, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; InputValidation (adapter answers); RateLimiting (provider limits); GracefulShutdown
- **Gate:** `cargo build -p ordin-mail` · `cargo test -p ordin-mail` · `cargo clippy -p ordin-mail -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-43, C-44, C-45 (§7.8)
- **Exit:** X1–X5.

#### Step 73 — Slack Service

*Added January 2026 (ADR-102). Step numbers are identifiers; this step sits in build phase 7 and is built after Step 54.*

- **Crate:** `crates/ordin-slack-service` (package `ordin-slack-service`) · **Programs:** D P · **Spec:** `components/SlackService.md`
- **Needs done:** steps 0–1; 5, 6, 7, 11, 36, 49 (Identity, Approval Service, MCP Front, Workspace Service, Organisation Profile, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations; Pagination; RateLimiting (**lands here**: L27–L29); InputValidation (Slack answers); GracefulShutdown
- **Gate:** `cargo build -p ordin-slack-service` · `cargo test -p ordin-slack-service` · `cargo clippy -p ordin-slack-service -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-84 (§7.8)
- **Exit:** X1–X5.

#### Step 55 — Package Catalogue

- **Crate:** `crates/ordin-package-catalogue` (package `ordin-package-catalogue`) · **Programs:** D P · **Spec:** `components/PackageCatalogue.md`
- **Needs done:** steps 0–1; 5, 6, 13, 15, 30, 31, 37, 50 (Catalogue Service, Resolver and Compiler, Installers, Checker, Organisation Profile, Agent Folder Manager, Package Inspector, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; Pagination; GracefulShutdown
- **Gate:** `cargo build -p ordin-package-catalogue` · `cargo test -p ordin-package-catalogue` · `cargo clippy -p ordin-package-catalogue -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-64 (§7.8)
- **Exit:** X1–X5.

#### Step 56 — Release Builder

- **Crate:** `crates/ordin-release-builder` (package `ordin-release-builder`) · **Programs:** A · **Spec:** `components/ReleaseBuilder.md`
- **Needs done:** steps 0–1; 9, 15, 30, 52 (Agent Configuration, Checker, Resolver and Compiler, Agent Loader)
- **Conforms to:** ErrorHandling; Logging; Configuration; InputValidation (agent releases (`REL_`))
- **Gate:** `cargo build -p ordin-release-builder` · `cargo test -p ordin-release-builder` · `cargo clippy -p ordin-release-builder -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-61 (settled, result b), C-75, C-81 (§7.8)
- **Exit:** X1–X5.


### Build phase 8

#### Step 57 — Ordin Host

- **Crate:** `crates/ordin-host` (package `ordin-host`) · **Programs:** D P · **Spec:** `components/OrdinHost.md`
- **Needs done:** steps 0–1; 3, 4, 5, 12, 20, 30, 32, 36, 37, 38, 40, 42, 45, 49, 51, 53, 55 (Runtime Adapter interface, Service API, MCP Front, Checker, File Service, Package Inspector, Package Catalogue, Crew Orchestrator, Session Supervisor, Usage Planner, Memory Service, Knowledge Service, Push Service, Project Service, Workspace Service, Journal, Localisation; *12 amended January 2026, ADR-109*)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations; InputValidation (charters (`CHARTER_`), workflow files (`WFF_`)); Pagination; GracefulShutdown (shutdown token)
- **Gate:** `cargo build -p ordin-host` · `cargo test -p ordin-host` · `cargo clippy -p ordin-host -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-29, C-61 (settled, result b) (§7.8)
- **Exit:** X1–X5.
- *Amended January 2026 (ADR-108):* Internal Logic steps 48–49 (OH-CC-1…10) and migration `ordin_host/0002_continue`. The continuity scenario's last line (Ordin's reply) runs only where both harnesses are installed; the gate checks the seed.
- *Amended January 2026 (ADR-109):* the language amendment of `components/OrdinHost.md` (OH-LG-1…): `OrdinHostDeps.localisation`, the seed's language line and the language-changed prefix, the tool `change_setting`, the draft offer, the commands `ordin.estimate_language_draft`, `ordin.draft_language` and `ordin.stop_language_draft`, the drafting job and the template `language-draft`. No Ordin Host migration. The gate checks two openings with the same inputs give byte-identical seeds.
- **Amended January 2026 (ADR-110):** Ordin's `recall` tool on server `ordin` passes `searches` and `folders` to `MemoryService::recall` and returns the `FactView` folder fields; no tool, command or seed text lets Ordin adopt.
- **Amended January 2026 (ADR-111):** Ordin's `remember` gains `entities`, `links` and `made_from`; the seed's instruction on links; recall ids on answers (`MessageBlock.memory_used`); summaries at run end (`consolidation_turn`, at most 3 a run) (`components/OrdinHost.md` amendment January 2026).

#### Step 78 — Knowledge Reader

*Added January 2026 (ADR-112, ADR-113). Step numbers are identifiers; this step sits in build phase 8 and is built after Step 53.*

- **Crate:** `crates/ordin-knowledge-reader` (package `ordin-knowledge-reader`) · **Programs:** A · **Spec:** `components/KnowledgeReader.md`
- **Needs done:** steps 0–1; 5, 9, 10, 33, 42, 53 (Journal, Agent Configuration, Telemetry, Duty Timer, Memory Service, Knowledge Service with its `okf` module)
- **Conforms to:** ErrorHandling (`KRD_` codes); Logging (no document text, no alias text); Metrics (the reader families of `components/Telemetry.md` amendment January 2026); Configuration (`knowledge-agent` repositories, glossaries, budget, interval; 2D-1d credential names; 2D-4 reading constants); DatabaseMigrations (`kr_*` tables); GracefulShutdown (the cycle stops at a document boundary and releases its lease)
- **Gate:** `cargo build -p ordin-knowledge-reader` · `cargo test -p ordin-knowledge-reader` (against local bare repositories made by the tests and a scripted `ReaderModel`; no network) · `cargo clippy -p ordin-knowledge-reader -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-98 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-115):** Confluence spaces and SharePoint libraries read through Source Access; `kr_document_sources` and the widened outcome check (step 2); table-of-contents things from audience-open facts only; needs done gains 79; conditional on C-103.

#### Step 74 — Team Knowledge Service

*Added January 2026 (ADR-103). Step numbers are identifiers; this step sits in build phase 8 and is built after Step 57.*

- **Crate:** `crates/ordin-team-knowledge` (package `ordin-team-knowledge`) · **Programs:** D P · **Spec:** `components/TeamKnowledgeService.md`
- **Needs done:** steps 0–1; 5, 6, 7, 42, 45, 49, 51, 53 (Journal, Organisation Profile, Identity, Memory Service, Project Service, Workspace Service, Push Service, Knowledge Service; *6 amended January 2026, ADR-110*)
- **Conforms to:** ErrorHandling; Logging; Configuration (2D-4 team-knowledge constants); DatabaseMigrations (`team_knowledge_*` tables); GracefulShutdown; InputValidation (team folder files: size bounds, `deny_unknown_fields`, file name equals item id)
- **Gate:** `cargo build -p ordin-team-knowledge` · `cargo test -p ordin-team-knowledge` · `cargo clippy -p ordin-team-knowledge -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-110):** the read-folder amendment of `components/TeamKnowledgeService.md`: bindings keyed by `TeamFolderId` with role and origins (forward-only table rebuild with a migration test proving every binding, item record, problem and share row kept), read folders and **Stop reading**, company-named folders reconciled from the profile, the three-answer review check, one repository copy and one fetch per repository with the random timer offset, descriptions, checks files and the recall check, Adopt, **Adopt again**, **Keep as it is**, the adoption records and their files, and the adopted items listed in a share's review request. The gate's tests include every scenario of the amendment against the test Git host.
- **Amended January 2026 (ADR-111):** relationship files now carry links, with no format change (`components/TeamKnowledgeService.md` amendment January 2026, SA-AMEM-04).
- **Amended January 2026 (ADR-112):** `index.md` and `log.md` in the own folder at each share, reserved files skipped on a get, plain OKF bundles as read folders, and migration step 3 (`components/TeamKnowledgeService.md` amendment January 2026).

#### Step 58 — Home and Search

- **Crate:** `crates/ordin-home-search` (package `ordin-home-search`) · **Programs:** D P · **Spec:** `components/HomeAndSearch.md`
- **Needs done:** steps 0–1; 5, 16, 38, 44, 49, 53 (Journal, Knowledge Service, File Service, Workspace Service, SharePoint Adapter, Confluence Adapter)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; Pagination
- **Gate:** `cargo build -p ordin-home-search` · `cargo test -p ordin-home-search` · `cargo clippy -p ordin-home-search -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-36 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-106):** the Jira amendment of `components/HomeAndSearch.md` (changed marks from the Workspace Service's Jira origin changes; no copy-update row for Jira) is built in this step.

#### Step 59 — Setup Assistant

- **Crate:** `crates/ordin-setup` (package `ordin-setup`) · **Programs:** D P · **Spec:** `components/SetupAssistant.md`
- **Needs done:** steps 0–1; 5, 6, 7, 43, 50, 55 (Organisation Profile, Identity, Package Catalogue, Catalogue Service, Craft Toolkit, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations; HealthChecks (**lands here** (D): check runner)
- **Gate:** `cargo build -p ordin-setup` · `cargo test -p ordin-setup` · `cargo clippy -p ordin-setup -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-25, C-46, C-47, C-48, C-58, C-59 (§7.8)
- **Exit:** X1–X5.

- **Amended January 2026 (ADR-114):** the releases-and-updates amendment of `components/SetupAssistant.md` (the changed `AppUpdater`, the check the person starts, background download, **What's new**, and a personal server's own update with its platform trait and migration step) is built in this step.

#### Step 60 — Retention and Erasure

- **Crate:** `crates/ordin-retention` (package `ordin-retention`) · **Programs:** A · **Spec:** `components/RetentionAndErasure.md`
- **Needs done:** steps 0–1; 5, 21, 24, 38, 42, 53 (Knowledge Service, Memory Service, File Service, Run Queue, Settings Store, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations (SQLite and PostgreSQL slices); Metrics (families 27–28 (sink))
- **Gate:** `cargo build -p ordin-retention` · `cargo test -p ordin-retention` · `cargo clippy -p ordin-retention -- -D warnings` · `cargo test -p ordin-arch-test` · `[pg]` scenarios against a PostgreSQL 18 container
- **Exit:** X1–X5.


### Build phase 9

#### Step 61 — Authoring Service

- **Crate:** `crates/ordin-authoring` (package `ordin-authoring`) · **Programs:** D P · **Spec:** `components/AuthoringService.md`
- **Needs done:** steps 0–1; 5, 8, 14, 30, 36, 50, 51, 57 (Agent Model, Catalogue Service, Checker, Format Plug-ins, MCP Front, Ordin Host, Push Service, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; InputValidation (archives (`AUTHR_ARCHIVE_INVALID`)); Pagination; GracefulShutdown; LicenceCheck (`git2`, exception E1)
- **Gate:** `cargo build -p ordin-authoring` · `cargo test -p ordin-authoring` · `cargo clippy -p ordin-authoring -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-38, C-67 (§7.8)
- **Exit:** X1–X5.

#### Step 62 — Channel Gateway

- **Crate:** `crates/ordin-channel-gateway` (package `ordin-channel-gateway`) · **Programs:** P A · **Spec:** `components/ChannelGateway.md`
- **Needs done:** steps 0–1; 5, 6, 11, 49, 57, 60 (Ordin Host, Approval Service, Workspace Service, Organisation Profile, Journal, Retention and Erasure)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (SQLite and PostgreSQL slices); InputValidation (chat updates); Pagination; GracefulShutdown (outbox drain); HealthChecks (chat links as readiness input (A))
- **Gate:** `cargo build -p ordin-channel-gateway` · `cargo test -p ordin-channel-gateway` · `cargo clippy -p ordin-channel-gateway -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-56 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** context steps at K.3.7, the hand-off route and fact (K.3.8, K.8), list-backed links (K.5.d), the working signal, `replace_ephemeral`; `HostedAgentCore` has fourteen methods.
- **Amended January 2026 (ADR-111):** **What I used** / **What Ordin used** on answers in Slack and Telegram, and the words "what did you use?" (`components/ChannelGateway.md` amendment January 2026).

#### Step 63 — Handover Service

- **Crate:** `crates/ordin-handover` (package `ordin-handover`) · **Programs:** D P · **Spec:** `components/HandoverService.md`
- **Needs done:** steps 0–1; 5, 7, 11, 16, 30, 45, 49, 50, 51, 54, 57, 73, 75 (Catalogue Service, Checker, Approval Service, Push Service, Project Service, SharePoint Adapter, Mail Service, Slack Service, Workspace Service, Jira Adapter, Ordin Host, Identity, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; Pagination; GracefulShutdown
- **Gate:** `cargo build -p ordin-handover` · `cargo test -p ordin-handover` · `cargo clippy -p ordin-handover -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-68, C-84, C-85, C-86, C-88 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-106):** the Jira amendment of `components/HandoverService.md` (place type `jira`, record comments, receiving, accept, return, take back, restart recovery from `jira_pending_writes`, migration step 3, a widening rebuild under DatabaseMigrations rule 12a with its migration test) is built in this step.

#### Step 64 — Job Runner

- **Crate:** `crates/ordin-job-runner` (package `ordin-job-runner`) · **Programs:** A · **Spec:** `components/JobRunner.md`
- **Needs done:** steps 0–1; 5, 10, 53, 60 (Knowledge Service, Retention and Erasure, Journal, Telemetry)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations (SQLite and PostgreSQL slices); Pagination; GracefulShutdown (**lands here**: `JOB_STOP_GRACE_SECONDS`); Metrics (families 21–23)
- **Gate:** `cargo build -p ordin-job-runner` · `cargo test -p ordin-job-runner` · `cargo clippy -p ordin-job-runner -- -D warnings` · `cargo test -p ordin-arch-test` · `[pg]` scenarios against a PostgreSQL 18 container
- **Exit:** X1–X5.

#### Step 65 — Caller API

- **Crate:** `crates/ordin-caller-api` (package `ordin-caller-api`) · **Programs:** A · **Spec:** `components/CallerApi.md`
- **Needs done:** steps 0–1; 10, 11, 21, 23, 24, 27, 38, 53, 60, 77 (Caller Authentication, Run Queue, File Service, Settings Store, Knowledge Service, Retention and Erasure, Rules Engine, Telemetry, Approval Service, Knowledge Query Server — *77 amended January 2026 (ADR-114 audit): the crate depends on `ordin-knowledge-query-server` since ADR-113*)
- **Conforms to:** ErrorHandling (**lands here** (A): wrightrun envelope, panic layer); Logging; Configuration; AuthenticationAndAuthorisation (**lands here** (A): intake signature, own-runs rule, approver flag); InputValidation (wrightrun bodies and limits); Pagination; RateLimiting (intake runs in progress, load shedding); GracefulShutdown (**lands here**: `serve`, `SHUTDOWN_SECONDS`); HealthChecks (**lands here** (A): `health_router`, routes 28–30); Metrics (route 30 `GET /metrics`; families 7, 8, 40–43); Cors (**lands here** (A): `Origin` refusal (`build_router` step 4))
- **Gate:** `cargo build -p ordin-caller-api` · `cargo test -p ordin-caller-api` · `cargo clippy -p ordin-caller-api -- -D warnings` · `cargo test -p ordin-arch-test` · conformance tests over `verify_signature`, `wire_time`, `wire_state`
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-111):** route 33 `POST /operator/memory/<verb>` over the `MemoryDesk` trait, for a hosted agent's owners (`components/CallerApi.md` amendment January 2026).
- **Amended January 2026 (ADR-113):** routes 34–40 — the knowledge route group and the knowledge MCP endpoint from the Knowledge Query Server's router (needs Step 77), and the operator's knowledge status — mounted only for a `knowledge-agent` section (`components/CallerApi.md` amendment January 2026).

### Build phase 10
- **Amended January 2026 (ADR-115):** routes 41–44 mounted from the Knowledge Query Server's router; the notice routes read the body untouched for signature checks.

#### Step 66 — Telegram Adapter

- **Crate:** `crates/ordin-telegram-adapter` (package `ordin-telegram-adapter`) · **Programs:** D P · **Spec:** `components/TelegramAdapter.md`
- **Needs done:** steps 0–1; 5, 7, 62 (Channel Gateway, Identity, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations; InputValidation (adapter answers (`TGA_FILE_TOO_LARGE`)); RateLimiting (Telegram limits (C-52)); GracefulShutdown
- **Gate:** `cargo build -p ordin-telegram-adapter` · `cargo test -p ordin-telegram-adapter` · `cargo clippy -p ordin-telegram-adapter -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-50, C-52 (§7.8)
- **Exit:** X1–X5.

#### Step 67 — Slack Channel Adapter

- **Crate:** `crates/ordin-slack-channel-adapter` (package `ordin-slack-channel-adapter`) · **Programs:** P A · **Spec:** `components/SlackChannelAdapter.md`
- **Needs done:** steps 0–1; 5, 7, 62 (Channel Gateway, Identity, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations (SQLite and PostgreSQL slices); InputValidation (adapter answers (`SCA_FILE_TOO_LARGE`)); RateLimiting (provider limits); GracefulShutdown; Metrics (families 18–20, 32–34, 44–46 (sink))
- **Gate:** `cargo build -p ordin-slack-channel-adapter` · `cargo test -p ordin-slack-channel-adapter` · `cargo clippy -p ordin-slack-channel-adapter -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-56 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** `replies`, rich-message text (F.11), canvas tabs and HTML conversion (F.5, F.6), `workflow_button`, `replace_ephemeral` (F.12), `SCA_RESPONSE_URL_INVALID`; the gate's new scenarios run against a Slack stand-in that records Web API calls (C-89).

#### Step 68 — Trigger Service

- **Crate:** `crates/ordin-triggers` (package `ordin-triggers`) · **Programs:** P · **Spec:** `components/TriggerService.md`
- **Needs done:** steps 0–1; 5, 6, 19, 57, 62, 73 (Ordin Host, Channel Gateway, NATS Adapter, Slack Service, Organisation Profile, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; InputValidation (NATS events, at most 4,000 characters per input); InputValidation (Slack messages, at most 4,000 characters per input); RateLimiting (**lands here**: trigger limits); GracefulShutdown
- **Gate:** `cargo build -p ordin-triggers` · `cargo test -p ordin-triggers` · `cargo clippy -p ordin-triggers -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.


### Build phase 11

#### Step 69 — Telegram Service

- **Crate:** `crates/ordin-telegram-service` (package `ordin-telegram-service`) · **Programs:** D P · **Spec:** `components/TelegramService.md`
- **Needs done:** steps 0–1; 5, 6, 11, 27, 36, 38, 49, 66 (Telegram Adapter, MCP Front, Rules Engine, Approval Service, File Service, Workspace Service, Organisation Profile, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations; Pagination; GracefulShutdown
- **Gate:** `cargo build -p ordin-telegram-service` · `cargo test -p ordin-telegram-service` · `cargo clippy -p ordin-telegram-service -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-52, C-55, C-58, C-59 (§7.8)
- **Exit:** X1–X5.


### Build phase 12

#### Step 70 — ordin server (program)

- **Crate:** `apps/server` (package `ordin-server`) · **Programs:** P A · **Spec:** `components/OrdinServer.md`
- **Needs done:** steps 0–1 and every module step whose *Programs* column includes P or A: 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 27, 30, 31, 32, 33, 35, 36, 37, 38, 39, 40, 41, 42, 43, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 73, 74
- **Conforms to:** ErrorHandling (start refusals `error[<CODE>]`, exit statuses, panic hook); Logging (**installs** the line form, OrdinServer.md *Internal Logic* step 3); Configuration (**lands here** (A): `apps/server/src/env.rs` `ServerEnv::read`); DatabaseMigrations (registers and applies every module's migrations; own module `server`); GracefulShutdown (**lands here**: orderly stop, OrdinServer.md *Internal Logic* step 11); HealthChecks (**lands here** (P): watchdog, control socket, `ordin-server health`; readiness check for A); Metrics (installs Telemetry (A); sets family 29); Tracing (installs Telemetry (A)); LicenceCheck (image build runs `cargo deny check licenses advisories sources`)
- **Gate:** `cargo build -p ordin-server` · `cargo test -p ordin-server` · `cargo clippy -p ordin-server -- -D warnings` · `cargo test -p ordin-arch-test` · program contract test: a sample of every module type serialises into the `ordin-service-api` restated type and back with no difference (ServiceApi.md, *Command input and result types*) · registration unit test of DatabaseMigrations.md Internal Logic 1 for the personal and the agent profile · see §6.2 for the shipped files, the image and the units
- **Conditional on:** C-49, C-50, C-51, C-84, C-104, C-107 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-109):** row 5.5 of `components/OrdinServer.md` passes `LocalisationConfig.app_version`, row 5.32 passes `OrdinHostDeps.localisation`, and the personal profile forwards `language_changed`, in this step.
- **Amended January 2026 (ADR-106):** rows 5.27 and 5.35 of `components/OrdinServer.md` pass `jira: None` to the Workspace and Handover Services; the server lists no `ordin-jira` dependency (Step 1 checks it).
- **Amended January 2026 (ADR-110):** the personal profile implements the Memory Service's `MemoryFolders` over the Team Knowledge Service, passes the verified profile's `team_knowledge.read_folders` to the Team Knowledge Service on each `subscribe()` change, and answers the new `team_knowledge.*` and `knowledge.list_read_pages` commands. The agent profile builds none of it.
- **Amended January 2026 (ADR-111):** wiring of `set_pages`, `run_starting`, `set_recall_log_days`, `recalls_of_run`, `MemoryDesk` and the gateway's "what was used" methods (`components/OrdinServer.md` amendment January 2026).
- **Amended January 2026 (ADR-114):** the program's build switches (`personal-profile`, `agent-profile` with its `agent` subcommands, `knowledge-agents`, and the module switches the personal profile wires; §9); `ordin-server update` and `ordin-server update --check`; the Setup Assistant's `ServerPlatform` implemented by the program; `setup.server_update` and `setup.update_server` answered by the personal profile and refused by the agent profile (`components/OrdinServer.md` amendment January 2026).
- **Amended January 2026 (ADR-112, ADR-113):** the Knowledge Query Client in every profile; in the agent profile with a `knowledge-agent` section, the knowledge scope, the Knowledge Query Server (`requests`) and the Knowledge Reader (`runs`), with `ReaderModel`, `ReaderOutlet`, `HotCopy` and `CredentialSource` implemented and the built-in duty registered (`components/OrdinServer.md` amendment January 2026).
- **Amended January 2026 (ADR-115):** Source Access constructed in every role of a knowledge agent with settings resolved from `knowledge-agent`; `ordin-server knowledge sign-in sharepoint` and `ordin-server knowledge forget-checks`; the Microsoft scope union in the personal profile; needs done gains 79; conditional on C-104 and C-107.

#### Step 71 — Screens module (App Shell, Block Renderer drawing side)

- **Crate:** `apps/desktop/ui` (npm package `@ordin/screens`, private) · **Programs:** D · **Spec:** `components/ScreensModule.md`
- **Needs done:** steps 0 and 3 (Service API: generated TypeScript types)
- **Conforms to:** ErrorHandling (draws the sentence a reply carries; `UI_CHART_RENDER_FAILED`); LicenceCheck (**lands here**: `license-checker-rseidelsohn`, `npm audit`)
- **Gate:** the checks of §6.3: `npm run gen:check`, `npx svelte-check`, `npx vitest run`, the npm licence check and `npm audit --omit=dev`
- **Conditional on:** C-40, C-34 (§7.8)
- **Exit:** X1–X3 (§6.3).
- **Amended January 2026 (ADR-104):** the page-links amendment of `components/ScreensModule.md` (links drawn and opened, *Pages that link here*, other names and tags, suggestions, the filter box, files that are not pages, a fact opened at its section) is built in this step, after Step 3 regenerates the types.
- **Amended January 2026 (ADR-106):** the Jira amendment of `components/ScreensModule.md` (Settings › Jira, **Add to this workspace › Jira issue…**, a linked issue in the tree, the handover draft's **Which issue?**) is built in this step, after Step 3 regenerates the types.
- **Amended January 2026 (ADR-108):** the continue amendment of `components/ScreensModule.md` (the conversation picker, **Continue this conversation** on the read-only tab and the tree node, its confirmation, the continued conversation's fold and moved items; SM-CC-1…7).
- **Amended January 2026 (ADR-109):** the language amendment of `components/ScreensModule.md` (SM-LG-1…): catalogue text loaded from the core with `localisation.get_catalogue` and reloaded on `language_changed`, Settings › Language, the Language screen, **Make a new language…**, **Import…** and **Export…**, and **Languages…** for a server; built after Step 3 regenerates the types.
- **Amended January 2026 (ADR-110):** the read-folder amendment of `components/ScreensModule.md` (11 §5.31a as amended, UX-A78): *Also reads*, **Read another team's knowledge…**, **Stop reading**, *Set by your company*, blocked folders with **Ask IT**, **Describe this folder…**, the Adopt dialog, the source-change marks with **Adopt again** and **Keep as it is**, the recall check line, and folder names on facts and pages; built after Step 3 regenerates the types. A test checks no new text holds a Git word.
- **Amended January 2026 (ADR-111):** *What Ordin remembers* Facts and Things, **What Ordin used**, **Remembered during this run**, and a hosted agent's memory on the operator screen (`components/ScreensModule.md` amendment January 2026).
- **Amended January 2026 (ADR-114):** the releases-and-updates amendment of `components/ScreensModule.md` (SM-UP-1…): no entry for a group the backend does not answer, Settings › About with **Check for updates**, the update line with **Restart to update**, the **What's new** card, and a paired server's update row in Settings › Servers; built after Step 3 regenerates the types.
- **Amended January 2026 (ADR-112, ADR-113):** Settings › Knowledge agents, the connect dialog and details, agent facts under **What Ordin used**, **More found**, the page editor's Type and Description, the marks, and the operator's knowledge status (`components/ScreensModule.md` amendment January 2026).
- **Amended January 2026 (ADR-115):** **What Ordin used** draws the unchecked-sources sentence; Settings › Knowledge agents › Details draws one line per checked system.

#### Step 72 — ordin desktop (program)

- **Crate:** `apps/desktop` (package `ordin-desktop`) · **Programs:** D · **Spec:** `components/OrdinDesktop.md`
- **Needs done:** steps 0–1 and every module step whose *Programs* column includes D: 2, 3, 4, 5, 6, 7, 8, 11, 12, 13, 14, 15, 16, 17, 18, 20, 25, 27, 28, 29, 30, 31, 32, 34, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 49, 50, 51, 53, 54, 55, 57, 58, 59, 61, 63, 66, 69, 73, 74, 75, and Step 71 (the screens module's `dist/`)
- **Conforms to:** ErrorHandling (panic hook); Logging (**installs** the line form, OrdinDesktop.md *Internal Logic* step 2); Configuration; DatabaseMigrations (registers and applies every module's migrations); AuthenticationAndAuthorisation (Tauri capability (layer 1, with step 34)); GracefulShutdown (**lands here**: `Core::shutdown`, OrdinDesktop.md *Internal Logic* steps 10–11); Cors (`tauri.conf.json` content security policy and the `main` capability); LicenceCheck (`tauri-plugin-deep-link`)
- **Gate:** `cargo build -p ordin-desktop` · `cargo test -p ordin-desktop` · `cargo clippy -p ordin-desktop -- -D warnings` · `cargo test -p ordin-arch-test` · program contract test as step 70 · registration unit test of DatabaseMigrations.md Internal Logic 1 for the desktop · Gherkin *start wires every module and shows the window* · see §6.4
- **Conditional on:** C-13, C-85 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-109):** row 3.5 of `components/OrdinDesktop.md` passes `LocalisationConfig.app_version`, row 3.41 passes `OrdinHostDeps.localisation`, the program forwards `language_changed`, and `DesktopServiceApi` answers the new `localisation.*` and `ordin.*` commands, in this step.
- **Amended January 2026 (ADR-106):** wiring row 3.30a of `components/OrdinDesktop.md` builds the Jira Adapter and passes it to the Workspace and Handover Services, and the program answers the two `jira.*` commands, in this step.
- **Amended January 2026 (ADR-110):** the program implements `MemoryFolders` over the Team Knowledge Service, passes the verified profile's `team_knowledge.read_folders` on each `subscribe()` change, and `DesktopServiceApi` answers the new `team_knowledge.*` and `knowledge.list_read_pages` commands, in this step.

---
- **Amended January 2026 (ADR-111):** wiring of `set_pages`, `run_starting` and `set_recall_log_days` (`components/OrdinDesktop.md` amendment January 2026).
- **Amended January 2026 (ADR-114):** the program's build switches (§9), the rows of *Internal Logic* 3 each switch constructs, the seams that work without a switched-off module, the groups `DesktopServiceApi` answers, and the `TauriUpdater` over the changed `AppUpdater`, with a ready update installed on quitting or with **Restart to update** (`components/OrdinDesktop.md` amendment January 2026).
- **Amended January 2026 (ADR-112, ADR-113):** the Knowledge Query Client constructed after Identity and the Organisation Profile, company front doors passed from the profile, `MemoryService::set_knowledge_client`, and the Knowledge Service's `KnowledgeIdentity` (`components/OrdinDesktop.md` amendment January 2026).
- **Amended January 2026 (ADR-115):** the Microsoft scope union for `identity.sign_in` (`KnowledgeQueryClient::microsoft_check_scopes`).

## 5. Embedded-component steps

The two embedded components sit outside the root workspace (`exclude`), keep their own lock files, and are path dependencies of one crate each (2B-1 rule 7). Their changes are made in this repository's copies (ADR-034) and are gated by their own test suites.

### Step E-1 — `memory/` storage port and embedding (before Step 42)

**Spec:** `components/MemoryService.md`, *The change ordin needs* (ADR-101). **Needs done:** Step 0. **Precondition:** P5.

Order *(chosen here: each item consumes the one before it)*:

| Order | Item | Contract (MemoryService.md) |
|---|---|---|
| 1 | M-1 — storage port: one supertrait over `MemoryStore` and the four stores that reach SurrealDB; `Service`, `WritePipeline`, `MaintenanceWorker`, `LeaseReaper`, `RetrievalEngine` hold `Arc<dyn StoragePort>` | `StoragePort` |
| 2 | M-2 — SQLite and PostgreSQL implementations in `memory::store::sql` (feature `sql`, `sqlx` 0.9, DDL of *Data Model* §B); vector stage 1 as an exact cosine scan | `SqlStorage::new(pool, SqlDialect, embed_dim)` |
| 3 | M-3 — `surrealdb` optional behind feature `surreal`; the HTTP edge and the OIDC `Authenticator` behind feature `http`; both on by default for the standalone binaries | ordin depends on `memory = { path = "memory", default-features = false, features = ["sql", "embedded"] }` |
| 4 | M-4 — `Service` takes `Arc<dyn CallerAuthenticator>`; the OIDC `Authenticator` implements it | `CallerAuthenticator` |
| 5 | M-5 — `memory::embedded::build(config, deps) -> EmbeddedMemory`, never calling `Config::load`, `build_state` or `obs::log::init_logging` | `EmbeddedDeps`, `EmbeddedMemory` |
| 6 | M-6 — `RememberRequest.visibility` for `client = Some("ordin-in-process")` only; `Service::list_facts` (keyset, 100 a page); `StoragePort::erase_scope` with a re-count proof | signatures in MemoryService.md |
| 7 | M-7 (*added January 2026, ADR-103*) — `export_scope` (a scope's facts with the entities, sources and relationships they name, for team knowledge), `import_facts` (structured facts keeping their identifiers and validity times, through the personal-data scan and the write gate, without extraction), `clear_pii_review` (the person's check of a flagged fact) | signatures in MemoryService.md, amendment January 2026 |
| 8 | M-8 (*added January 2026, ADR-110; memory ADR-020*) — `Fact.collection` and `RememberRequest.collection`; `RecallFilters.collections` applied before every signal; `RecallRequest.searches` (typed `lex` / `vec` sub-queries, first weighted 2); stage-1 rank fusion per signal list and collection (k = 60, rank bonus, per-collection quota) replacing the cap by fact id, with `fused_norm` on the rerank fallback and degraded path; `StoragePort::erase_collection`; `Service::check_content`; `mem_fact.collection` with its index and the per-scope in-memory embedding cache; the benchmark's per-collection leg | MemoryService.md amendment January 2026; the memory spec set's amendments January 2026 |

**Gate:**
- `cargo test --manifest-path memory/Cargo.toml` — the default features (`surreal`, `http`): the standalone binaries behave as before.
- `cargo test --manifest-path memory/Cargo.toml --no-default-features --features sql,embedded` — the component's own suite against the SQLite and the PostgreSQL implementation (ADR-101 item 4; PostgreSQL through the container tool of P7).
- `cargo clippy --manifest-path memory/Cargo.toml --no-default-features --features sql,embedded -- -D warnings`.
- In the ordin workspace, once Step 42's manifest points at the changed `memory/`: `cargo deny check licenses advisories sources` passes with no `BUSL-1.1` crate, which shows SurrealDB is out of ordin's build (LicenceCheck.md: SurrealDB is not an exception).

**Exit:** M-1 to M-8 each have tests in `memory/` (*M-8 amended January 2026, ADR-110*); the M-8 benchmark runs of §7.9 are recorded; C-15 (in-process without HTTP or OIDC) and C-82 (PostgreSQL behind the port) are met by construction, as MemoryService.md states.

### Step E-2 — `sandboxd` changes (before Step 41)

**Spec:** `components/Sandbox.md`, *Approach*, the C-17 paragraph. **Needs done:** Step 0. **Precondition:** P6.

The three changes the Sandbox spec works around: (1) one guest compile shared across agent controllers in place of one compile per controller (Sandbox.md *Internal Logic* D step 8); (2) one person credential per run; (3) the shell and JavaScript guests (`sandboxd` ADR-043) and the run environment (`sandboxd` ADR-046). Sandbox.md says these are "listed at the end of this spec"; its end holds only *Gaps: None.*, so the list above is taken from the C-17 paragraph and the MCP tools table.

**Gate:** `cargo test --manifest-path sandbox/sandbox-host/Cargo.toml` · `cargo clippy --manifest-path sandbox/sandbox-host/Cargo.toml -- -D warnings`.

**Relation to Step 41.** Step 41 does not wait for E-2: it builds against the current `sandboxd` with `extra-guests` off, which Sandbox.md specifies. When change (3) lands, `extra-guests` is switched on and gated by `cargo test -p ordin-sandbox --features extra-guests`; changes (1) and (2) remove the per-controller compile and the credential workaround without changing the Sandbox's interface. Step 41 itself waits for the C-17 spike (§7.8), because every Sandbox assertion is conditional on it and the HLD names no fallback.

---

## 6. Programs and screens

### 6.1 ordin hook — Step 26

- `apps/hook`, binary `ordin-hook`, depends on `ordin-types` only (2B-1 rule 3).
- On any failure of its own it exits 0 with the harness's deny verdict and logs the code; a bad command line exits 64 (ErrorHandling.md *Public Interface*; OrdinHook.md).
- The Copilot payload mapping (OrdinHook.md D.2) is built and kept for the C-27 conformance tests although no adapter writes a Copilot hook file in the first release (OrdinHook.md SA-RECON-03).
- Shipped inside the server archive and image (`bin/ordin-hook`, Step 70) and beside the desktop binary (Step 72).

### 6.2 ordin server — Step 70 (personal and agent profiles)

Builds `apps/server` (package `ordin-server`, binary `ordin-server`), the composition root of the personal profile and of the agent profile's single and scaled shapes (OrdinServer.md).

Items the step produces besides the binary, from OrdinServer.md *Shipped files*:

1. **Archive** `ordin-server-<version>-linux-<x86_64|aarch64>.tar.gz` holding `bin/ordin-server`, `bin/ordin-hook` (Step 26), `share/systemd/ordin-server.service`, `share/systemd/ordin-server-agent.service`, `LICENSE`, `NOTICE`.
2. **`ordin-server.service`** (personal profile): `Type=notify`, `NotifyAccess=main`, `ExecStart=%h/.local/bin/ordin-server start`, `Restart=on-failure`, `RestartSec=5s`, `WatchdogSec=90s`, `TimeoutStopSec=60s`.
3. **`ordin-server-agent.service`** (agent profile, single shape): `ExecStartPre=%h/.local/bin/ordin-server agent release obtain`, `ExecStart=%h/.local/bin/ordin-server agent start`, `TimeoutStopSec=` the run limit plus 60 s, default `3660s`.
4. **Container image** `deploy/container/Containerfile` as OrdinServer.md gives it (`debian:bookworm-slim`, user 10001, `VOLUME ["/var/lib/ordin"]`, `STOPSIGNAL SIGTERM`, `HEALTHCHECK … CMD ["ordin-server", "health"]`, `ENTRYPOINT ["ordin-server"]`, `CMD ["start"]`); the image build runs `cargo deny check licenses advisories sources` before compiling (LicenceCheck.md). Claude Code is not in the image (ADR-098 item 7).
5. **Helm chart** `deploy/helm/ordin-server/` and `deploy/argocd/application.yaml` for the scaled shape, with the values 2D-3 fixes.

Checks particular to this step, besides its row in §4:

- Stop order and bounds (GracefulShutdown.md, P4-2): on `SIGTERM` the personal server stops the desktop's modules in the desktop's order after its own inputs; runs may finish until 30 s after the signal and are then cancelled and recorded `interrupted`; the Channel Gateway drain is bounded at 10 s, the Remote API wait at 5 s, every other step at 2 s; the whole stop ends within 54 s, inside `TimeoutStopSec=60s` and the container's 60 s stop period (`podman run --restart unless-stopped --stop-timeout 60`). The agent profile never cuts a run short. One test per bound, with a module double that overruns it.
- Exit statuses of every command: 0, 1 with `error[<CODE>]: <message>`, 2, 3, 4, 70 (OrdinServer.md *Exit statuses*); one test per Error Table row.
- Control socket `<ORDIN_DATA_DIR>/run/control.sock`, mode `0600`, peer user checked with `SO_PEERCRED`.
- **Amended January 2026 (ADR-107):** wiring of `EnvironmentSecrets`, `ContentChecker`, `SupervisorHost::check_content`, `HostedAgentCore::context_lookup` and `context_recent`; the Helm values `podAnnotations` and `roles.<role>.podAnnotations`.

### 6.3 Screens module — Step 71

`apps/desktop/ui/` (npm package `@ordin/screens`, private), TypeScript with Svelte 5 and Vite, depending only on the TypeScript types `ordin-service-api-gen` writes (2B-1 rule 6). Built before Step 72 because the desktop's `tauri.conf.json` takes `frontendDist: "../ui/dist"` (OrdinDesktop.md).

- **Needs done:** Step 0 (package skeleton) and Step 3 (generator).
- **Conforms to:** ErrorHandling (draws the sentence a reply carries; records `UI_CHART_RENDER_FAILED`); LicenceCheck (**lands here**: `license-checker-rseidelsohn` and `npm audit` over the module's production dependencies).
- **Gate** (in `apps/desktop/ui/`): `npm run gen:api` then `npm run gen:check` exits 0 (TypeScript type generation check; runs `ordin-service-api-gen check .`) · `npx svelte-check` exits 0 · `npx vitest run` exits 0 · `npx license-checker-rseidelsohn --production --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC"` and `npm audit --omit=dev` exit 0.
- **Tests the spec names** (ScreensModule.md, *Tests (vitest)*): one per Error Table row; one per block kind with the 2C JSON example as input; store tests table-driven over event sequences (in order, repeat, gap, reset, unknown kind); a source-scan test that fails on `{@html`, `innerHTML`, `fetch(`, `XMLHttpRequest`, `WebSocket`, `EventSource`, and on any import of `@tauri-apps/api` outside `lib/backend/tauri.ts`; a catalogue test that every message id used in the source exists in the en-GB `.ftl` files; an accessibility test of the approval screen and the confirmation dialog (focus on open, Tab trapped, Escape).
- **Performance checks:** initial JavaScript ≤ 600 KB gzipped, with Monaco, xterm.js and ECharts loaded on first use; a chart of 5,000 points drawn ≤ 300 ms (C-40).
- **Conditional on:** C-40 (ECharts provisional), C-34 (Monaco tokenisers; named by Phase 1, not by the spec).
- **Exit:** X1, X2 and X3 for the module (coverage ≥ 70 %, ScreensModule.md); X4 and X5 do not apply (no Rust crate).

### 6.4 ordin desktop — Step 72

`apps/desktop` (package `ordin-desktop`, binary `ordin`), the Tauri 2 composition root for macOS and Windows (OrdinDesktop.md).

- `tauri.conf.json` as OrdinDesktop.md gives it: `frontendDist: "../ui/dist"`, `devUrl: "http://localhost:5173"`, `beforeBuildCommand: "npm --prefix ../ui run build"`, the content security policy (Cors.md), `capabilities: ["main"]`; `apps/desktop/capabilities/main.json` limits the WebView to the `ordin` plugin's Service API commands and to listening for events.
- Integration test stubs the spec names (one each): construction with fake modules records the order of *Internal Logic* 3.1–3.49; a fake `ServiceApi` call through the real Command Bridge plugin under the `main` capability; the WebView cannot call a command outside `ordin:default`.
- Stop: `Core::shutdown` (*Internal Logic* steps 10–11); quit with no running session ≤ 3 s, with sessions ≤ 20 s.
- `WordToMarkdown` is passed to the Confluence Adapter only once C-13 confirms the conversion, else `None`.
- The per-user installer and updater (C-46, Setup Assistant) are built with the Tauri bundling command the user names under P7.

---
- **Amended January 2026 (ADR-107):** `SupervisorHost::check_content` answers passed on the desktop.

### 6.5 Release pipeline (*added January 2026, ADR-114; amended January 2026: run by a maintainer with `make`, not by CI*)

Built before the first version is published, and used for every version. It is three make targets a maintainer runs on their own computer when they judge a version ready (ADR-114 item 9); each takes `VERSION=<version>`. No CI job builds, signs or publishes a release, and no CI service holds the update key.

- `make release-check` runs step 1.
- `make release-build` runs step 1, then steps 2 to 4 for the platform of the computer it runs on. A macOS installer is built on a macOS computer and a Windows installer on a Windows computer, so the maintainer runs it once on each and gathers the outputs in one folder.
- `make release-publish` runs steps 5 to 7 over that folder, and creates the version's tag `v<version>` with its release.

The private half of the update key is a file on the maintainer's computer, outside version control (the repository's `.gitignore` refuses every `*.key` file); the targets pass its path to the Tauri signer in `TAURI_SIGNING_PRIVATE_KEY` and its password in `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and a target started without them stops before building anything.

1. **Gate.** `make check` over the workspace with the default switches of the version (§9), then the ship gate the roadmap names for that version (kept by the maintainers outside the repository). A failure stops the target and nothing is built or published.
2. **Desktop installers.** `cargo tauri build` (P7) with the version's default switches, signed with the platform code-signing identity (ADR-072) and with the update key (ADR-114 item 8). The key and its password are never written to the output, to a log or to an artefact.
3. **Server archives** (when the version turns on `personal-profile` or `agent-profile`). `ordin-server-<version>-linux-<x86_64|aarch64>.tar.gz` as §6.2 item 1, each signed with the update key in the signature form the Tauri updater checks (C-99).
4. **Container image and Helm chart** (when the version turns on `agent-profile`), as §6.2 items 4 and 5, published to the project's image registry.
5. **Feed.** One feed file in Tauri's updater format: the version, the release notes `/releasegen` drafted (at most 4,000 characters, SetupAssistant.md `UpdateOffer.notes`), the date, and one entry per platform with its download address and signature: the two desktop installers, and the two server archives when step 3 built them.
6. **Publish.** Artefacts first, then the feed file at the stable address the unbranded build's installer carries (`InstallerSettings.update_feed_url`, A-69), so the feed never names a file that is not yet there.
7. **Check after publishing.** The target reads the published feed back, downloads every entry and verifies each signature against the public half of the update key built into the programs. A failure withdraws the feed file by restoring the previous one and stops the release.

**Gate:** steps 1–7 pass for a test version published to a test feed address before the first real version. **Exit:** an installation on macOS and one on Windows, pointed at the test feed, offer the test version, download it, install it with **Restart to update**, and show its **What's new**.

## 7. Integration and verification

Runs after Step 72, and again before every release. Each item is a gate: a failure stops the release.

### 7.1 Unit tests

`cargo test --workspace` and, in `apps/desktop/ui/`, `npx vitest run`. Together they hold X1 and X2 for every step. X3 (≥ 70 % lines per crate and for the screens module) is re-measured here with the command of P7.

### 7.2 Integration tests

An integration test lives in the crate that depends on both sides: the caller's crate where 2B-3 gives it the edge, otherwise the program crate that injects the trait (Steps 70, 72). One test per interaction named in a spec's *called by* or *who calls what* table where both sides are built, and at least these:

| Hub trait (owner) | Implemented or registered by | Test |
|---|---|---|
| `ToolProvider` (MCP Front, Step 36) | Sandbox, Memory Service, Craft Toolkit, Ordin Host, Knowledge Service, Mail Service, Telegram Service, Authoring Service, Project Service, Connection Binder, Agent Loader (2B-2 D4) | each provider registers at start, is offered, and answers one call that the Rules Engine allowed and refuses one it denied; the desktop's start registers nine providers before `McpFront.start` (OrdinDesktop.md Gherkin); *amended January 2026 (ADR-114):* in a build with switches off, the providers the build's switches construct (OrdinDesktop.md amendment January 2026) |
| `Coordinator` (Crew Orchestrator, Step 40) | Ordin Host (Step 57; 2B-2 D7) | a crew run calls the Ordin Host at "Ordin checks the result", a member question and the end |
| `PartSource` (Agent Model, Step 8) | Catalogue Service (Step 50), injected into Resolver and Compiler and Checker (2B-2 D8) | `assert_part_source_contract` passes against the catalogue; the Resolver and the Checker resolve the same part through the injected source |
| `EventSink` (Journal, Step 5) | Command Bridge, Remote API, Channel Gateway subscribe (2B-2 D1) | an Approval Service event reaches each subscriber once, in sequence-number order |
| `ServiceApi` (Service API, Step 3) | the two programs | the same command through the Command Bridge (desktop) and the Remote API (personal server) gives the same reply; the restated-type contract test of each program (ServiceApi.md) |
| Program-implemented traits | `RulesHost`, `SandboxGrants` (Rules Engine, Sandbox); `SupervisorHost` (Session Supervisor); `CrewHost` (Crew Orchestrator); `WorkingCopyStore` (File Service, Push Service, Workspace Service); `RefResolver` (MCP Front); `PlaceFiles` (Sandbox); `HarnessProbe`, `SessionSetupSource`, `SessionRecordStore` (local adapters); `CompanyTokenSource` (Hosted Agent Adapter); `SlackUserTokenSource` (Slack Thread Adapter); `JournalReader`, `SearchSource` (Home and Search); `InstanceActivity` (Package Catalogue); `HandoverReach` (Authoring Service); `PlaceReadCheck` (Telegram Service); `ServerInstaller` (Setup Assistant); `WordToMarkdown` (Confluence Adapter); `KnowledgeFiles`, `ChatHistory` (Knowledge Service); `HostedAgentCore`, `PersonalCore`, `JobOutcomeNotifier` (Channel Gateway); `JobDesk` (Job Runner); `MetricsSink` (Run Queue, Settings Store); `SelectionLookup`, `SettingRuleCheck` (Settings Store); `LoaderStore`, `WorkFolderOpener` (Agent Loader); `RunHost`, `ScopeSettings`, `ChannelDocuments` (Connection Binder); `HarnessLayoutWriter`, `PackageReader` (Release Builder); `ErrorSentences` (Service API); `TeamMemory`, `TeamPages`, `TeamPush`, `TeamWorkspaces`, `TeamRepos` (Team Knowledge Service; *amended January 2026, ADR-103*) | one test per trait in each program crate that implements it, calling the owning module through the program's implementation |

Tests marked `[pg]` run against a PostgreSQL 18 container (P7). No test calls a shared database or an external service; adapters are tested against doubles of Slack, Microsoft Graph, Confluence, Telegram, NATS, GitHub and the harnesses, except the conformance tests named in §4 that need the pinned harness. *Amended January 2026 (ADR-103):* the Team Knowledge Service's share, get, branch-moved, review-required and read-only scenarios run against a **test Git host**: a local bare repository served over HTTPS on loopback inside the test, with a double of `GithubAccess` answering `repository_permission` and `branch_review_rules`; two installations' data roots share it to test Get, Share, deletions both ways, corrections and the same-page rule (ADR-103 items 4–7) and the byte-identical round trip (item 2).

### 7.3 Smoke tests per program

| Program | Smoke test | Pass when |
|---|---|---|
| ordin desktop | Start `ordin` on an empty data root with no organisation profile | the modules are constructed in *Internal Logic* step 3's order with every profile key at its default; `window_shown` is logged within 2,000 ms of start; the Setup Assistant's start checks run; quitting with nothing running and no downloaded update waiting ends within 3 s (*amended January 2026, ADR-114*) |
| ordin server, personal profile | On a Linux test host: `ordin-server setup profile`, `ordin-server setup listen`, `ordin-server setup pair-desktop`, then `ordin-server start` under `ordin-server.service`; pair one desktop through the Server Connector | `ordin-server health` exits 0; the paired desktop lists the server and runs one command over the Remote API; `SIGTERM` ends the stop within 54 s with `run/clean-stop` written |
| ordin server, agent profile (single shape) | `ordin-server setup release REFERENCE` with a test release, then `ordin-server agent release obtain` and `ordin-server agent start` under `ordin-server-agent.service` | `GET /healthz` and `GET /readyz` answer 200; `GET /metrics` answers OpenMetrics text; one caller-started run reaches a final state |
| ordin desktop, team knowledge (*added January 2026, ADR-103*) | On the reference laptop (A-03), a Team Knowledge workspace bound to the test Git host gets 5,000 facts shared from a second data root | `recall` p95 stays within the Memory Service's budget (≤ 600 ms with models warm) over 100 queries, with no read of the clone during recall (ADR-103 item 9); a check with nothing new ends within 5 s |

### 7.4 Observability checks (agent profile only)

- `GET /metrics` lists exactly the 50 families of Metrics.md's closed list (49 before ADR-107), each with its `HELP` text, `wrightrun_` prefix and closed label values; no other family appears (Metrics.md N-1; P4 closes N-8).
- Spans exported to an OTLP receiver in the test are only the five of Tracing.md's allow list — `ordin.run_stretch`, `ordin.model_turn`, `ordin.tool_call`, `ordin.subagent_call`, `ordin.http_out` — with the resource holding exactly `service.name`, `service.version`, `ordin.agent`; a full span queue increments `wrightrun_spans_dropped_total`.
- Desktop and personal server: no metrics route and no span export (SA-XC-04, SA-XC-05); the test asserts `ordin-telemetry` is absent from `ordin-desktop`'s dependency graph (2B-3: Telemetry is A only) and that the personal profile opens no metrics listener.
- Logs on every program follow Logging.md's line form; a test greps a smoke run's log for the fields Logging.md forbids (message content, file content, tool arguments, tokens) and fails on any.

### 7.5 Security scans

- `cargo deny check licenses advisories sources` over the workspace: licences against `deny.toml` with exceptions E1 (narrowed in Step 0) and E3 only; RustSec advisories; yanked versions; sources other than crates.io. The nine crates confirmed at Step 0 — `thiserror`, `async-trait`, `serde_json`, `tokio-util`, `unicode-normalization`, `tokio-tungstenite`, `jsonwebtoken`, `rand`, `base64` — must still pass at their locked versions.
- In `apps/desktop/ui/`: `npx license-checker-rseidelsohn --production --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC"` and `npm audit --omit=dev`.
- The image build of `deploy/container/Containerfile` runs the same `cargo deny` check before compiling.
- Programs run unmodified as separate programs — Claude Code (E2), the Copilot CLI sidecar, `jdtls`, `gh`, `glab`, the ArtCraft apps — are outside both checks and recorded in HLD 05 with their licences (LicenceCheck.md *Internal Logic* 6).

### 7.6 Lint

`cargo fmt --check` · `cargo clippy --workspace -- -D warnings` · in `apps/desktop/ui/`: `npx svelte-check` and `npx vitest run`. All four exit 0.

### 7.7 Error-code registry check

The check that every code emitted in source appears in ErrorHandling.md's *Error-code registry* (1,888 codes, 75 owners; *amended January 2026, ADR-103, ADR-104 and ADR-106*), in three parts:

1. **Per crate (X4, every step).** Each crate's `ALL_CODES` is found by `ordin_types::error_registry::lookup`, and each prefix is owned by the crate (`owns_prefix`).
2. **Enumerated from source.** `grep -rhoE '"[A-Z]{2,6}_[A-Z0-9_]{2,60}"' crates apps --include='*.rs' --include='*.ts' --include='*.svelte' | sort -u` lists every string literal shaped as a code (the 2C pattern `^[A-Z]{2,6}_[A-Z0-9_]{2,60}$`, which every registered code matches after EH-D9). Each literal is either a registered code or a name in Configuration.md's 2D-1 environment-variable tables; any other literal fails the check. Each registered code appears in some crate's `ALL_CODES`; a registered code no crate exports fails the check. *Amended January 2026 (ADR-114):* "no crate" means no crate built with the switches being checked; a code whose owner is behind a switch that is off is not counted (ErrorHandling.md amendment January 2026).
3. **At run time.** No smoke run of §7.3 logs `svc.unregistered_code` (ServiceApi.md) or answers `SVC_INTERNAL` for a module code.

ErrorHandling.md places the test `error_registry::every_code_registered` in `ordin-types`, which depends on no workspace crate (2B-1 rule 2) and so cannot read another crate's `ALL_CODES`. Part 1 runs that half inside each crate; part 2 replaces the cross-crate walk. ErrorHandling.md should record this placement in the next correction round.

### 7.8 Confirmation items and the steps they gate

A step may be built before its items are confirmed, using the fallback its spec states; the conditional assertion is relied on — the fallback switched off — only after the item is confirmed. HLD 10 *Rollout* runs the compatibility proof first: C-19, C-20, C-22, C-25 and C-26 on Claude Code, then C-01 to C-03, C-05 and C-27 on the Copilot route. Four items gate more than an assertion:

- **C-17** gates Step 41 entirely: every Sandbox assertion is conditional on it and the HLD names no fallback; a failed spike returns the decision to the HLD (ADR-031).
- **C-81** gates shipping the agent profile (ReleaseBuilder.md; Phase 1: no fallback).
- **C-75** keeps Copilot refused for hosted agents (Steps 9, 47) until settled; **C-83** keeps Gemini models refused (Step 9) until settled.

Items named in a component spec:

| Item | What it bounds | Gated steps |
|---|---|---|
| C-01 | Copilot CLI loads agent files from the session's folder | 31 Installers, 47 Copilot Local Adapter |
| C-02 | Every Copilot tool request reaches the permission handler or a hook | 47 Copilot Local Adapter |
| C-03 | One Copilot SDK client runs several sessions with different folders | 47 Copilot Local Adapter |
| C-04 | A Slack ephemeral reply cannot be read back | 18 Slack Thread Adapter |
| C-05 | Copilot sessions resume after restart | 32 Session Supervisor, 47 Copilot Local Adapter |
| C-06 | `apm` accepts a GitHub App token (largely moot since ADR-082) | 7 Identity and Credentials |
| C-07 | ArtCraft CLI names, modes, commands, platforms | 43 Craft Toolkit |
| C-08 | Each pinned ArtCraft app's licence | 43 Craft Toolkit |
| C-09 | File formats each ArtCraft app reads and writes | 43 Craft Toolkit |
| C-10 | One `gh` sign-in serves `gh` and git | 7 Identity and Credentials, 45 Project Service |
| C-11 | Confluence token can read trees, write versioned pages, attach | 44 Confluence Adapter |
| C-12 | Atlassian OAuth works for a desktop without a stored secret | 6 Organisation Profile, 7 Identity and Credentials, 44 Confluence Adapter |
| C-13 | `wordcraft-cli` converts Word to Markdown or HTML headless | 43 Craft Toolkit, 44 Confluence Adapter, 51 Push Service, 72 ordin desktop |
| C-14 | Graph public-client sign-in and SharePoint versioned writes | 6 Organisation Profile, 7 Identity and Credentials, 16 SharePoint Adapter |
| C-15 | Memory runs in-process without HTTP or OIDC | 42 Memory Service |
| C-17 | `sandboxd` library embeds without its daemon | 41 Sandbox |
| C-18 | Memory can use local embedding and reranking models | 42 Memory Service |
| C-21 | Each ArtCraft CLI serves MCP with the needed tools | 43 Craft Toolkit |
| C-22 | Core can serve MCP to a separate harness process locally | 36 MCP Front |
| C-23 | Sandbox compiles guests ahead of time and caches them | 41 Sandbox |
| C-25 (*partly*) | `copilot login` completes with no terminal (Claude half passed) | 46 Claude Local Adapter, 47 Copilot Local Adapter, 59 Setup Assistant |
| C-26 | `--safe-mode`/`--restricted` keeps the sign-in, runs `--settings` hooks, loads nothing of the person's own — passed with `--restricted` (Phase 1); the assertions resting on it are unconditional | 26 ordin hook, 27 Rules Engine, 31 Installers, 46 Claude Local Adapter |
| C-27 | Copilot runs a hook for every tool call and honours deny | 26 ordin hook, 27 Rules Engine, 47 Copilot Local Adapter |
| C-28 | ArtCraft MCP tools make Word, PDF and slides from Markdown | 43 Craft Toolkit, 51 Push Service |
| C-29 | Context-window use is readable; a fresh seeded session can start | 20 Usage Planner, 32 Session Supervisor, 57 Ordin Host |
| C-30 | `glab` signs in with no terminal and serves git | 45 Project Service |
| C-31 | ACP permission requests and IDE support (OQ-33) | 4 Runtime Adapter interface |
| C-32 | Installed IDEs found and started with a folder | 45 Project Service |
| C-33 | Pinned language servers work in a worktree | 45 Project Service |
| C-35 | Memory indexes Markdown pages by section, no LLM on read | 42 Memory Service, 53 Knowledge Service |
| C-36 | Home, changed marks and search within A-03 and A-21 without a model | 38 File Service, 58 Home and Search |
| C-37 | Model and thinking level per session; usage reported | 4 Runtime Adapter interface, 17 Hosted Agent Adapter, 18 Slack Thread Adapter, 20 Usage Planner, 46 Claude Local Adapter, 47 Copilot Local Adapter |
| C-38 | One-line tool descriptions keep Ordin's tool choice reliable | 41 Sandbox, 61 Authoring Service |
| C-40 | Charting library meets A-34 in 300 ms | 71 Screens module |
| C-41 | Harness to-do lists arrive in the event stream | 4 Runtime Adapter interface, 46 Claude Local Adapter, 47 Copilot Local Adapter |
| C-42 | Graph and Confluence report who can see a file or page | 16 SharePoint Adapter, 44 Confluence Adapter, 51 Push Service |
| C-43 | Entra ID grants mail read, draft and send; change queries work | 7 Identity and Credentials, 54 Mail Service |
| C-44 | Gmail internal app without external verification | 7 Identity and Credentials, 54 Mail Service |
| C-45 | Company mail server's IMAP and SMTP features | 54 Mail Service |
| C-46 | Per-user install, update and removal with no administrator | 59 Setup Assistant |
| C-47 | ArtCraft apps signed and start without a system prompt | 43 Craft Toolkit, 59 Setup Assistant |
| C-48 (*partly*) | Copilot device sign-in on a Linux server | 47 Copilot Local Adapter, 59 Setup Assistant |
| C-49 (*partly*) | GitHub and Entra device sign-in under company rules | 7 Identity and Credentials, 70 ordin server |
| C-50 (*partly*) | Encrypted `0600` secret file with a systemd or container key | 7 Identity and Credentials, 22 Model Policy, 66 Telegram Adapter, 70 ordin server |
| C-51 (*partly*) | Lingering without an administrator on company servers | 70 ordin server |
| C-52 (*partly*) | Telegram limits (file sizes, button data, rate, text length) | 66 Telegram Adapter, 69 Telegram Service |
| C-53 (*partly*) | `async-nats` JetStream, credentials, TLS; no proxy | 19 NATS Adapter |
| C-55 (*partly*) | Bot privacy mode and what a bot receives in groups | 69 Telegram Service |
| C-56 (*partly*) | Slack Socket Mode for an internal app | 62 Channel Gateway, 67 Slack Channel Adapter |
| C-58 (*partly*) | Telegram install found without reading its data; `tg://` link; one receiver | 59 Setup Assistant, 69 Telegram Service |
| C-59 | Company distributions ship systemd 256+; `tg:` handler readable | 59 Setup Assistant, 69 Telegram Service |
| C-60 | Copilot loads what the vscode or agents-md Installer writes | 31 Installers, 47 Copilot Local Adapter |
| C-61 | Claude Code under `--restricted` loads what the Claude Code Installer writes — settled January 2026, result (b) | 31 Installers, 46 Claude Local Adapter, 52 Agent Loader, 56 Release Builder, 57 Ordin Host |
| C-62 | APM plug-in round-trips the company's APM versions | 14 Format Plug-ins |
| C-63 | SQLite full-text ranking matches crucible's gap-check results | 50 Catalogue Service |
| C-64 | A pack changed mid-session takes effect next turn | 13 Agent Folder Manager, 31 Installers, 55 Package Catalogue |
| C-66 | Frozen crucible ignores `withdrawn.yaml` | 50 Catalogue Service |
| C-67 | Required reviewers readable without repository admin rights | 50 Catalogue Service, 61 Authoring Service |
| C-68 | Converted Word and Confluence headings match OutputContract sections | 63 Handover Service |
| C-69 | Directory search and conditional SharePoint write | 16 SharePoint Adapter |
| C-70 | Claude Code and Copilot set-up layouts stable enough to map | 14 Format Plug-ins |
| C-71 | SSE over HTTP/2 passes company proxies with 20 s comments | 25 Server Connector, 35 Remote API |
| C-73 | Tauri IPC carries the Service API at the desktop's targets | 34 Command Bridge |
| C-74 | Organisation key not inherited by hooks, MCP servers, tools | 22 Model Policy, 26 ordin hook, 32 Session Supervisor, 46 Claude Local Adapter, 48 Connection Binder, 52 Agent Loader |
| C-75 | A non-person GitHub account can hold a Copilot seat | 9 Agent Configuration, 47 Copilot Local Adapter, 56 Release Builder |
| C-76 | Prefixed MCP tool names still match bare names in package text | 36 MCP Front, 48 Connection Binder |
| C-77 | Claude Code options with an organisation key (model, fallback, gateway, a provider's settings in the child's environment; settled by local test T1, ADR-107) | 9 Agent Configuration, 10 Telemetry, 22 Model Policy, 46 Claude Local Adapter, 52 Agent Loader |
| C-78 | Hook input names the sub-agent that made a call | 26 ordin hook, 27 Rules Engine, 39 Hook Relay, 52 Agent Loader |
| C-79 | With file-writing tools off, instructions and skills still load | 46 Claude Local Adapter, 52 Agent Loader |
| C-80 | Session record moves between machines and resumes | 4 Runtime Adapter interface, 21 Run Queue, 27 Rules Engine, 32 Session Supervisor, 36 MCP Front, 46 Claude Local Adapter |
| C-81 | Anthropic's terms allow an organisation-installed Claude Code with its own key | 56 Release Builder |
| C-82 | Memory keeps its store in PostgreSQL or behind ordin's storage trait | 42 Memory Service |
| C-83 | Gemini CLI runs headless with hooks and MCP under an open licence | 9 Agent Configuration |
| C-84 | Slack Web API names, scopes and PKCE sign-in used by the Slack Service | 73 Slack Service, 7 Identity and Credentials, 63 Handover Service, 70 ordin server |
| C-85 | Per-user `ordin://` link handler; Slack renders it as a link | 72 ordin desktop, 63 Handover Service |
| C-86 | Jira returns comments in creation order; an edited comment has a later `updated` | 75 Jira Adapter, 63 Handover Service |
| C-87 | A pasted Atlassian API token reaches every Jira endpoint used, with no admin set-up | 75 Jira Adapter, 7 Identity and Credentials |
| C-88 | Assignable-user search returns account id and display name; email may be hidden | 75 Jira Adapter, 63 Handover Service |
| C-89 | Slack canvas tabs, canvas HTML, `workflow_button` and `response_url` replace (ADR-107) | 67 Slack Channel Adapter, 62 Channel Gateway |
| C-91 | Recall p95 ≤ 200 ms with 4 typed sub-queries at 3 × 20,000 facts, exact scan over the in-memory cache (ADR-110) | E-1 M-8, 42 Memory Service (A-58 lowered by measurement if it fails) |
| C-92 | GitHub ruleset rules and GitLab approval rules readable with read access (ADR-110; spike of January 2026, documentation only) | 45 Project Service, 74 Team Knowledge Service (fallback: `CannotTell`, the folder is not taken in) |
| C-99 (*added January 2026, ADR-114*) | Tauri's updater format carries Linux archive entries the desktop ignores, and their signatures verify with `minisign-verify` | 59 Setup Assistant, 70 ordin server, §6.5 Release pipeline (fallback: none; the server keeps announcing only, as before ADR-114) |
| C-100 (*added January 2026, ADR-114*) | A user unit's main process can ask systemd to restart its own unit, with lingering on and no administrator | 70 ordin server (fallback: none; the server keeps announcing only) |
| C-101 (*added January 2026, ADR-115*) | GitHub's per-user repository permission answers with a read credential, and the permission it needs | 79 Source Access (fallback: a classic token with `repo` scope from the agent's own machine account; the operator is told at start) |
| C-102 (*added January 2026, ADR-115*) | GitLab effective member level and the external flag without an administrator | 79 Source Access (fallback: internal projects treated as private) |
| C-103 (*added January 2026, ADR-115*) | Confluence per-user permission check needs the administrator permission; page restrictions readable by a viewer | 79 Source Access, 78 Knowledge Reader (fallback: no page of a space read while person checks are off and restrictions cannot be read) |
| C-104 (*added January 2026, ADR-115*) | Entra user-consented app, on-behalf-of exchange for delegated `Files.Read.All`, Graph 403/404 for unreadable items | 7 Identity and Credentials, 76 Knowledge Query Client, 79 Source Access, 70 ordin server (fallback: the company-wide application permission when the company grants it, otherwise SharePoint is not offered to knowledge agents) |
| C-105 (*added January 2026, ADR-115*) | 20 parallel Confluence checks and one Graph batch of 20 within 300 ms p95 | 77 Knowledge Query Server, 79 Source Access — spike before Step 77's amendment (fallback: deadline and batch retuned; late checks under More found) |
| C-106 (*added January 2026, ADR-115*) | Repository-owner webhooks of GitHub and GitLab reach the agent with no organisation administrator | 79 Source Access, 65 Caller API (fallback: notices off; the remember period bounds the gap) |
| C-107 (*added January 2026, ADR-115*) | A Microsoft refresh token issued to a confidential client stays valid after a newer one is returned, until its own lifetime ends | 79 Source Access, 70 ordin server (fallback: the operator signs the agent in again after a restart that finds the stored copy refused, R-132) |
Items Phase 1 assigns to a component but no component spec names; they bound the same steps:

| Item | What it bounds | Gated steps |
|---|---|---|
| C-16 (*partly*) | Memory's third-party licences permit embedding | 42 Memory Service — moot for SurrealDB after ADR-101 (E-1, M-3); the licence check of X5 covers the rest |
| C-20 | With shell, code and web tools off, remaining built-in calls reach the hook | 39 Hook Relay, 46 Claude Local Adapter |
| C-34 | Monaco tokenisers cover the listed languages | 71 Screens module |
| C-39 | Ordin follows tone, register and length on both harnesses | 57 Ordin Host |
| C-54 (*partly*) | Company NATS maximum payload at or under 1 MB | 19 NATS Adapter, 68 Trigger Service |
| C-57 (*partly*) | Watchdog in a user unit; Claude sign-in expiry readable | 70 ordin server |
| C-65 | Hook sees every Bash and MCP call with flags under `--restricted` | 27 Rules Engine, 39 Hook Relay |
| R-123 (*added January 2026, ADR-113*) | The memory component confirms more than one instance over one store (its RISK-014) | 70 ordin server: the scaled shape of a knowledge agent and the `[pg]` runs of §7.11 (single shape first) |
| C-95 (*added January 2026, ADR-113*) | Recall in one knowledge agent within 200 ms p95 over 100,000 facts, SQLite and PostgreSQL | 42 Memory Service, 77 Knowledge Query Server (§7.11 *Fan-out bench*) |
| C-96 (*added January 2026, ADR-113*) | Fan-out first result 1 s p95; front-door overhead 50 ms; cached 50 ms; change notice 10 s | 76 Knowledge Query Client, 77 Knowledge Query Server (§7.11 *Fan-out bench*, *Cache*) |
| C-97 (*added January 2026, ADR-112*) | The Open Knowledge Format's licence permits stating conformance | documentation step of §7.11 (wording only; no build step gated) |
| C-98 (*added January 2026, ADR-113*) | One fixed model turn per document yields facts that pass ordin's checks, at a measured cost | 78 Knowledge Reader (§7.11 *Reading*) |

Confirmed or withdrawn, and so gating nothing: C-19 (passed for streamed JSON, progress and resume), C-24 (withdrawn, ADR-078), C-26 (passed with `--restricted`), C-61 (settled, result (b)), C-72 (withdrawn, decision 78).

---

### 7.9 Acceptance runs — reading other teams' knowledge (*added January 2026, ADR-110*)

Run after Steps E-1, 42, 53, 74 and 71, on the memory benchmark rig and the reference laptop (A-03), with the test Git host. Each run records today's behaviour (the build before M-8) as its baseline in the same run, and each result is written to the step's record.

| Run name | Run | Pass when |
|---|---|---|
| Fairness | memory benchmark, per-collection leg: own folder 20,000 facts, read folder 200, 20 fixed queries whose single answer is in the small folder; run again with the small folder company-named | the answer is among the reranker's candidates in ≥ 19 of 20 queries, the same with `set_by_company` |
| Exact match kept | benchmark fixture's exact-term queries | the fact holding the rare term ranks in the top 3 at least as often as the baseline |
| Filtered recall | recall restricted to the 200-fact folder with 20,000 in the other | `min(result_cap, matches)` facts returned |
| Plain query unchanged | the memory component's recall regression suite with `query` only | passes unchanged |
| Budget | up to 4 typed sub-queries, 20,000 facts per folder × 3 folders | recall p95 ≤ 200 ms excluding caller network; the figure is recorded and confirms or lowers A-58 (C-91) |
| One fetch per repository | three bindings into one repository on one installation, counted at the test Git host | one copy on disk and one fetch per check cycle |
| Spread checks | 1,000 simulated installations started at the same instant | no one-second window holds more than 2 % of checks |
| Anchors | `make doc-ref-check` | no `DANGLING` finding; every new assertion anchored at its implementation site |

**Documentation step (a `/breakdown` phase after the user approves the specs):** `README.md` (the *Context* row and the `memory/` description where it mentions sharing), `docs/features/team-knowledge.md` (title line, *What you get*, *How it works*, *You stay in charge*, *Good to know*: reading other teams' folders, the read-only rule, **Adopt**, folders set by the company, the folder-split convention), `docs/features/memory.md` and `docs/features/knowledge-pages.md` (the sentences that point at team knowledge) and `docs/features/README.md` (the Team knowledge row). Pass when each describes reading other teams' folders and that nothing passes from one team's folder to another unless a person adopts it, and none contains branch, commit, push, pull, merge or rebase. The clickable prototype follows as its own step (UI-D111).

### 7.10 Acceptance runs — agentic memory (*added January 2026, ADR-111*)

Run after Steps E-1 (with M-9), 42, 53, 57, 62 and 71, on the memory benchmark rig and the reference laptop (A-03). Each result is written to the step's record.

| Run name | Run | Pass when |
|---|---|---|
| Graph budget | memory benchmark graph leg: 10,000 facts, 20,000 links, 2,000 things, 200 queries naming 1–3 things, one step out, SQLite and PostgreSQL | recall p95 ≤ 200 ms excluding caller network (C-93); two steps measured and recorded |
| Linked fact found | the leg's 1,000-fact fixture where no fact's text holds "supplies" | "who supplies packaging?" returns the fact linked to packaging in the first 5, on both stores |
| Hop limit | chain A–B–C–D with one step | facts about B returned; none about C or D |
| Scope | two workspaces, a link whose facts are in the other | never followed |
| Page first | equal text relevance, one fact from a page section and one from a document | the page-backed fact ranks first |
| Upgrade | a copy of a pilot installation's store from before revision 0.60.0 | every fact visible and attached to its subject's thing, no links, no error; name index done within 60 s for 20,000 things (C-94) |
| Offline | remembering facts with links with networking denied | passes; no outbound call |
| Anchors | `make doc-ref-check` | no `DANGLING` finding; every new assertion anchored at its implementation site |

**Documentation step (a `/breakdown` phase after the user approves the specs):** `README.md` (the *Context* row), `docs/features/memory.md` and `docs/features/README.md` are already updated by this amendment; the step checks they still match what is built and removes the card's "not yet built" line when it is.

### 7.11 Acceptance runs — knowledge agents and the Open Knowledge Format (*added January 2026, ADR-112, ADR-113*)

Run after Steps 42, 53, 65, 70, 71, 72, 74, 76, 77 and 78, on the reference laptop (A-03) and a test network of four agent-profile servers in the single shape, plus one scaled deployment for the `[pg]` runs. Each result is written to the step's record.

| Run name | Run | Pass when |
|---|---|---|
| Unchanged desktop | the agentic-memory and read-folder suites with no front door connected | pass unchanged |
| Connect and remove | add and remove a front door with the person's company sign-in | both journalled with ids only; no company set-up beyond the front door |
| Local first | a question the local graph answers confidently, with a packet capture | no request to the front door |
| Nothing but the query | a wider question, with a packet capture | only the query, sub-queries and limit leave; no conversation text, workspace fact or file content |
| Fan-out bench | front door and three proxies, 1,000 questions over 100,000 facts per proxy, SQLite and PostgreSQL | first result p95 ≤ 1 s; front-door overhead ≤ 50 ms p95 beyond the slowest answering proxy; per-proxy recall p95 ≤ 200 ms (C-95, C-96) |
| No model, no fetch | the bench with the model client and Git fetcher instrumented | zero model calls and zero fetches inside a query span |
| Slow proxy | one proxy delayed 3 s | the result arrives by the first deadline naming it "did not answer in time"; its answer arrives as **More found** before 5 s |
| Backup | two proxies over one repository, the first delayed past 300 ms | the second's answer is used, within the first deadline |
| Ring | three agents asking each other, hop budget 1 | no forwarding; a request whose visited list holds an agent is refused by it |
| Cache | the same question twice from one access group; then a commit moves | second answer p95 ≤ 50 ms from cache; after the move, not from cache; the notice drops entries within 10 s of the fetch (C-96) |
| Two groups | the same question from two access groups | no cached answer crosses groups |
| Audience | an asker outside the audience; an MCP client with no sign-in | refused with a plain reason, no facts |
| Content as data | a document holding "ignore previous instructions" | returned as a fact; no behaviour of front door, proxy or desktop changes |
| Reading | a knowledge agent over three repositories, then a commit changing one document | facts name repository, path and commit; only the changed document re-read; its replaced facts closed; no page written; nothing pushed; the memory component makes no outbound call |
| Pages and glossary | the Borealis page with other name and link; a glossary "Borealis Ltd" with aliases "Borealis" and "BRL" read by two proxies; two glossaries disagreeing on "BRL"; a desktop with the glossary as a read folder | thing, other name, link and section sources as stated; one thing across proxies; first glossary wins and the status names both; the desktop resolves "BRL" to "Borealis Ltd" |
| Trust order | three equally matching concepts verified by a person, by a process only, and not at all; a deprecated and a stable concept; a passed `stale_after` | person first, process second, unverified third; deprecated not returned; "may be out of date" shown |
| OKF round trip | every page ordin writes checked by a conformance checker written from the OKF 0.2 specification; a foreign bundle with an unknown type, an unknown key, a broken link and no `index.md`; a rename rewriting that bundle's link | every page passes; the bundle reads with no error; unknown key and `sources` fields kept byte for byte |
| Reserved files | a `knowledge/` folder and a team folder after a page is added and a share runs | `index.md` and `log.md` never pages; `index.md` lists the page with its description and `okf_version: "0.2"`; one dated `log.md` entry with no statement text |
| MCP | an MCP client of the audience | exactly `knowledge_search` and `knowledge_contents`; search returns the desktop path's ranked facts within the same deadlines; with `mcp.enabled: false` the route does not answer |
| Sources record | record off, then on with 1-day retention | off: no per-answer rows; on: one row per answer with answer id, time, access group and sources, no query text, no asker identity; older rows deleted; **What Ordin used** and the journal show the answer id; the connect dialog states the policy |
| Hot copy | a front door asking one proxy about one repository past the threshold in a day | it starts reading the repository and stops asking the proxy once its fetch completes |
| Anchors | `make doc-ref-check` | no `DANGLING` finding; every new assertion anchored at its implementation site |

**Documentation step (a `/breakdown` phase after the user approves the specs):** the feature cards `docs/features/knowledge-agents.md` and `docs/features/open-knowledge-format.md` (the second written ahead of this step, January 2026; at this step it is checked against the promoted specs); updates to `docs/features/knowledge-pages.md`, `team-knowledge.md`, `memory.md`, `hosted-agent.md`, `docs/features/README.md` and the top-level `README.md` *Context* row; the paper `docs/knowledge-fabric.md` linked from `docs/why-ordin.md`. Until C-97 is confirmed they say ordin "writes and reads the Open Knowledge Format 0.2", never "conforms"; none names a private product or vendor.

## 8. Greenfield note

This spec set is in greenfield mode. Every assertion is derived from the HLD and none is classified *Required*, *Observed*, *Not implemented* or *Gap*, so the Playbook has no remediation phase and no continuity phase: Steps 0 to 78, with E-1 and E-2, are one build in the order above. The ports of crucible and wrightrun code (Agent Model, Resolver and Compiler, Checker, Installers, Catalogue Service, Authoring Service, Rules Engine vocabulary, Caller API) are new code checked by the parity and conformance tests named in their steps, not carried-forward code.

*Amended January 2026 (ADR-114):* the build is still one dependency order. §9 fixes the build switches that let it ship in increments; which increment carries which step is planned in the maintainers' release roadmap, kept outside the repository, outside this spec set.

---

## 9. Build switches

*Added January 2026 (ADR-114). Draft until the Phase 6 audit of this amendment passes and the user promotes it.*

The steps of §4 are built in their dependency order, and every step is built whole (X1–X5). Which steps and switches reach people in which release is planned outside this spec set, in the maintainers' release roadmap kept outside the repository; this section fixes only the mechanism the plan uses, so that changing the plan changes no spec.

1. **Switches.** The programs `apps/desktop` and `apps/server` have Cargo features named after what they give the person: `sandbox`, `dictation`, `copilot`, `slack`, `mail`, `team-knowledge`, `authoring`, `handover` (turns on `mail` and `slack`), `telegram`, `hosted-agents` and `knowledge-agents`, and on the server `personal-profile` and `agent-profile` (on the server `telegram` turns on `personal-profile`, which turns on `slack` because the Trigger Service takes the Slack Service). The crates each switch adds, and the rows of the programs' start that it constructs, are fixed in OrdinDesktop.md and OrdinServer.md (amendments January 2026). Every crate a switch does not name is always built.
2. **Defaults.** The specs set `default = []`. A shipped version's manifests list as default features the switches its release turns on (kept by the maintainers outside the repository).
3. **What a build answers.** A backend answers only the command groups of the modules its build constructs and refuses the others with `SVC_NOT_IN_THIS_VERSION`; the screens show no entry for them (ServiceApi.md *Command groups by build switch*; ScreensModule.md SM-UP-1).
4. **Gates.** Every gate of §4 and §7 runs with the switches of the build being checked. `make check` also runs once with `--all-features`, so every switch's code stays built and linted; the Architecture Test reads the graph with `--all-features` (ArchitectureTest.md amendment January 2026), and `deny.toml` keeps `[graph] all-features = true`.
5. **A step and its switch.** A step whose crate a switch names may be built before that switch is turned on in any shipped version; its gate runs as in §4.
6. **A failed spike.** When a confirmation item that gates a whole step fails (C-17 for Step 41; C-81 for the agent profile), the switch that names the step stays off in every shipped version. No spec changes.
