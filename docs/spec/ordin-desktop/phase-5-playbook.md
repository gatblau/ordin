# Phase 5 — Generation Playbook

> **Spec set:** ordin-desktop · **Phase:** 5 — Generation Playbook · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-108; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · amended 2026-01-28 (the memory models download on first use, PB-REL-11 to PB-REL-14, OQ-REL-3 resolved, OQ-REL-4; Approved by the user 2026-01-28) · amended January 2026 (the Windows installer built on the Mac as an option, PB-REL-15 to PB-REL-20, PB-REL-10 amended; Approved by the user 2026-01-29) · amended January 2026 (the gate's evidence: the release computer runs no test, PB-REL-21 to PB-REL-23, PB-REL-6, PB-REL-10 and PB-REL-19 amended; Approved by the user 2026-01-29) · amended January 2026 (Windows programs need no runtime the PC may lack, PB-REL-24 to PB-REL-26, PB-REL-6, PB-REL-10 and PB-REL-19 amended; Approved by the user 2026-01-29) · **Mode:** greenfield · **derivedFromHld:** 0.69.0 · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-114 build switches and updates; promoted to Approved by the user January 2026) · amended January 2026 (a task's form, PB-TF-1 to PB-TF-3, ADR-119; Approved by the user January 2026) · amended January 2026 (ADR-118, ADR-120: facts as triples, words, conflicts, passages and the memory diagram; Approved by the user January 2026) · amended January 2026 (a team workspace, §7.14 PB-TW-1 to PB-TW-3, Steps 3, 49, 51, 53, 57, 71, 72 and 74, the desktop's team-knowledge switch on by default, ADR-121; Approved by the user January 2026)
> **Source HLD:** `docs/design/ordin-desktop/` · **Inputs:** `phase-2-architecture.md` (2B-1, 2B-3), `components/*.md` (79 specs; *amended January 2026, ADR-115: one new*; *amended January 2026, ADR-113: three new*; *amended January 2026, ADR-102, ADR-103, ADR-106 and ADR-107*), `cross-cutting/*.md` (15 specs; `VisualDesign.md` added January 2026, ADR-116) and `phase-4-cross-cutting.md`, HLD 05 — Tech stack · **Authored:** January 2026

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
| P7 | The user confirms the commands of this Playbook and names the four that no input fixes: the Rust toolchain version for `rust-toolchain.toml`; the line-coverage command for X3; the container tool for the PostgreSQL 18 container of the `[pg]` scenarios (Steps 5, 21, 24, 33, 60, 64) and for building the image of `deploy/container/Containerfile`; the Tauri bundling command for the desktop installer. | Met (decision 81, January 2026): Rust toolchain = the current stable release, pinned in `rust-toolchain.toml` at Step 0; coverage = `cargo llvm-cov` (X3); container tool = Docker (`[pg]` scenarios and `deploy/container/Containerfile`; Colima provides it on macOS; amended January 2026, decision 97); desktop installer = `cargo tauri build`. Recorded in HLD 05 *Build / test toolchain*. | HLD 05 *Build / test toolchain* ("commands are confirmed by the user before use"); RunQueue.md `[pg]`; OrdinServer.md *Container image* |
| P8 | The parity and conformance fixtures are ordin's own, written from the specs and committed in this repository: `testdata/parity/agent-model/` (Step 8), `testdata/parity/workspaces/` (Step 15), `testdata/parity/checks/` (Step 30), `testdata/parity/installers/` (Step 31) and `testdata/conformance/rules/` (Step 27). No test reads another repository, and nothing is copied from one. A test whose fixture folder is missing fails and never skips. | Met (the user's decision of January 2026): the fixtures are committed. | AgentModel.md, ResolverAndCompiler.md, Checker.md, Installers.md; `testdata/README.md` |

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
| X4 | The crate exports `ALL_CODES: &[&str]` beside its error enum (ErrorHandling.md *Internal Logic* 11); a unit test asserts that `ordin_types::error_registry::lookup` finds every entry and that `owns_prefix(<package>, <prefix>)` is true for each. `ordin-arch-test` has no workspace dependency, so `ordin-types` makes these two assertions for it over a copy of its codes (ArchitectureTest.md *Public Interface*). |
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
3. **Member manifests** *(chosen here)*. For each of the 77 Rust crates of 2B-3 (the 73 module crates, `ordin-hook`, `ordin-server`, `ordin-desktop` and `ordin-arch-test`; Step 79 adds `ordin-source-access`), a `Cargo.toml` that declares exactly its 2B-3 *Dependencies* (as workspace path dependencies, plus `ordin-types`) and the third-party crates its spec names, with one exception: `ordin-memory-service` declares its `memory` path dependency at Step 42, because without the Step E-1 change M-3 `memory` carries SurrealDB, which would stop the licence gate. `ordin-sandbox` declares `sandboxd` here, with an empty `src/lib.rs` (`src/main.rs` with an empty `main` for `apps/hook`, `apps/server`, `apps/desktop`). Reason: LicenceCheck.md *Internal Logic* 8 requires the licence gate to run on the `Cargo.lock` this step writes, before any later step; only a lock that resolves every crate's dependencies lets it confirm the nine named crates and the transitive licences. It also lets the Architecture Test check every edge from Step 1. Each later step owns its crate's contents and may correct its manifest; X5 and gate item 4 re-check any change.
4. **Toolchain pin:** `rust-toolchain.toml` naming the version the user gives under P7.
5. **Lint and format configuration:** none beyond the tools' defaults. Neither HLD 05 nor any spec names a `rustfmt.toml` or `clippy.toml` setting; the gates run `cargo fmt --check` and `cargo clippy … -- -D warnings`.
6. **`deny.toml`** at the workspace root, verbatim from LicenceCheck.md *Public Interface*: `[graph] all-features = true`; `[licenses] allow = ["MIT", "Apache-2.0", "BSD-3-Clause", "ISC", "Unicode-3.0"]`, `confidence-threshold = 0.93`, `exceptions` for `libgit2-sys` (E1) and `notify` (E3); `[advisories] ignore = []`, `yanked = "deny"`; `[sources] unknown-registry = "deny"`, `unknown-git = "deny"`.
7. **Screens module skeleton** `apps/desktop/ui/`: `package.json` with the versions ScreensModule.md pins (`svelte` 5.57.2, `vite` 8.3.4, `@sveltejs/vite-plugin-svelte` 7.3.1, `typescript` 6.0.3, `@tauri-apps/api` 2.12.4, `@fluent/bundle` 0.19.1, `echarts` 6.1.0, `@xterm/xterm` 6.0.0; Monaco Editor pinned at lock time, HLD 05), `vitest` and `svelte-check` as development dependencies (HLD 05), the scripts `gen:api` and `gen:check` (ScreensModule.md *Generation*), and `package-lock.json`.
8. **`Makefile`** with target `check`, stopping at the first failure, in this order (the first three from LicenceCheck.md; the rest *chosen here* from the tools HLD 05 and the specs name): `cargo deny check licenses advisories sources`; in `apps/desktop/ui/`, `npx license-checker-rseidelsohn --production --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC"` and `npm audit --omit=dev` (LicenceCheck.md); `cargo fmt --check`; `cargo clippy --workspace -- -D warnings`; `cargo test --workspace` (which runs the Architecture Test, ArchitectureTest.md); `cargo run -p ordin-service-api --bin ordin-service-api-gen -- check apps/desktop/ui` (ServiceApi.md); in `apps/desktop/ui/`, `npx svelte-check` and `npx vitest run` (HLD 05). Entries whose crate or files do not exist yet are added by the step that creates them (Step 3 for the generator check, Step 71 for the screens checks).
9. **CI:** the CI job runs `make check` and nothing else, so the local and CI gates are one list.
10. **Licence gate** (LicenceCheck.md *Internal Logic* 8), on the `Cargo.lock` that `cargo metadata --format-version 1` writes from the manifests of item 3:
    1. Read the SPDX expression `cargo-deny` reports for the crate bundling libgit2: `libgit2-sys` declares `MIT OR Apache-2.0`, so E1's `exceptions` entry matches nothing, is reported as a warning and stays as written (LicenceCheck.md *Internal Logic* 8).
    2. Add each OSI-approved licence of a transitive crate not yet on `allow` (the Wasmtime family behind the sandbox, the ONNX Runtime bindings behind `fastembed`, the ICU4X data crates, any other) with a one-line comment naming the crate and the reason.
    3. Confirm that the seven crates HLD 05 marks "to be confirmed by cargo-deny before Phase 5" — `thiserror`, `async-trait`, `serde_json`, `tokio-util`, `unicode-normalization` (05 — *Utility and cryptography crates*), `tokio-tungstenite` (05 — *Always on and Slack*), `jsonwebtoken` (05 — *Caller token verification*) — and `rand` and `base64` (ChannelGateway.md) report expressions inside `allow`.
    4. A licence the OSI does not approve, or the GPL or the AGPL, on any crate other than E1 and E3 stops Step 0: the dependency is removed or replaced, or the user records a new named exception through an HLD decision. No such licence is added to `allow`.

**Gate:** `cargo metadata --format-version 1 --locked` exits 0 · `cargo deny check licenses advisories sources` exits 0 after item 10 · in `apps/desktop/ui/`: `npx license-checker-rseidelsohn --production --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC"` and `npm audit --omit=dev` exit 0 · `cargo fmt --check` exits 0.

**Exit:** the workspace resolves with one `Cargo.lock`; the licence gate's four items are done and recorded in `deny.toml` comments; LicenceCheck.md's Gherkin *the workspace passes with the two crate exceptions* holds.

### Step 1 — Architecture Test

Built first, ahead of its 2B-3 phase 12, because it depends on no workspace crate and every later gate runs it.

- **Crate:** `crates/ordin-arch-test` (package `ordin-arch-test`) · **Programs:** build only · **Spec:** `components/ArchitectureTest.md`
- **Needs done:** Step 0
- **Conforms to:** ErrorHandling (`ARCH_` codes, 12 in the registry); Logging
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
- **Amended January 2026 (ADR-121; Approved by the user January 2026):** the ten `team_knowledge.*` commands of a team workspace, all local only, the three added to `PERSON_ONLY_COMMANDS`, and the wire types of 2C §18 (`components/ServiceApi.md` SVC-TW-1 to SVC-TW-4) are built in this step; see §7.14.

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
- **Gate:** `cargo build -p ordin-agent-model` · `cargo test -p ordin-agent-model` · `cargo clippy -p ordin-agent-model -- -D warnings` · `cargo test -p ordin-arch-test` · `--features testkit`: `InMemorySource` passes `assert_part_source_contract` · table-driven parity over the committed fixtures in `testdata/parity/agent-model/`
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
- **Gate:** `cargo build -p ordin-resolver` · `cargo test -p ordin-resolver` · `cargo clippy -p ordin-resolver -- -D warnings` · `cargo test -p ordin-arch-test` · parity over every WorkspaceProfile in `testdata/parity/workspaces/`
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
- **Gate:** `cargo build -p ordin-rules` · `cargo test -p ordin-rules` · `cargo clippy -p ordin-rules -- -D warnings` · `cargo test -p ordin-arch-test` · rules-engine conformance suite driven from the committed scenarios in `testdata/conformance/rules/` (ADR-007)
- **Conditional on:** C-26 (passed), C-27, C-78, C-80 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-107):** the content check (`ContentChecker`, `RulesHost::content_checker`, `check_content`, `RULE_CHECKER_UNREACHABLE`) and its control lines in the start-up report; the gate runs its seven new scenarios against an HTTP stand-in that answers allow, block, or never.

#### Step 28 — Brand

- **Crate:** `crates/ordin-brand` (package `ordin-brand`) · **Programs:** D · **Spec:** `components/Brand.md`
- **Needs done:** steps 0–1; 5, 6, 12 (Organisation Profile, Localisation, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations; VisualDesign (*amended January 2026, ADR-117*: the build pack's icon set of 50 named icons and version 1.2.0, Brand amendment *The prototype's icons in the build pack*; *amended January 2026, ADR-116*: the build pack's 37 tokens and version 1.1.0, the optional roles and the added contrast pairs of `components/Brand.md`'s January 2026 amendment)
- **Gate:** `cargo build -p ordin-brand` · `cargo test -p ordin-brand` · `cargo clippy -p ordin-brand -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.

#### Step 29 — Dictation Service

- **Crate:** `crates/ordin-dictation` (package `ordin-dictation`) · **Programs:** D · **Spec:** `components/DictationService.md`
- **Needs done:** steps 0–1; 12 (Localisation)
- **Conforms to:** ErrorHandling; Logging
- **Gate:** `cargo build -p ordin-dictation` · `cargo test -p ordin-dictation` · `cargo clippy -p ordin-dictation -- -D warnings` · `cargo test -p ordin-arch-test` · build on macOS (Swift static library through `build.rs` and `swift build`) and on a non-macOS host (`#[cfg(target_os = "macos")]` path excluded)
- **Exit:** X1–X5.

#### Step 80 — Transcription Service

*Added January 2026 (amended January 2026; user decision; Approved by the user January 2026). Step numbers are identifiers; this step sits in build phase 3 and is built after Step 29.*

- **Crate:** `crates/ordin-dictation` (package `ordin-dictation`), module `transcription` behind the crate feature `transcription` · **Programs:** D · **Spec:** `components/TranscriptionService.md`
- **Needs done:** steps 0–1; 12, 29 (Localisation; the crate's own Step 29)
- **Conforms to:** ErrorHandling (`TRX_` codes); Logging (no text, speaker or path); InputValidation (sizes, durations, line lengths); Configuration (`speech-models.json`)
- **Gate:** `cargo build -p ordin-dictation --features transcription` · `cargo test -p ordin-dictation --features transcription` (the fixtures `meeting.vtt`, `meeting.docx`, `meeting.txt`, `speech-60s.{m4a,mp3,wav}` with `speech-60s.txt`, `two-track.mkv`; a local test model host for download, resume, checksum and cancel; the word-error-rate test runs on the test server with the larger model) · `cargo clippy -p ordin-dictation --features transcription -- -D warnings` · `cargo test -p ordin-arch-test` · the licence gate (`cargo deny check licenses`)
- **Conditional on:** C-108, C-109, C-110 (§7.8)
- **Exit:** X1–X5.

#### Step 81 — OBS Recorder Adapter

*Added January 2026 (amended January 2026; user decision; Approved by the user January 2026). Build phase 3.*

- **Crate:** `crates/ordin-obs` (package `ordin-obs`) · **Programs:** D · **Spec:** `components/ObsRecorderAdapter.md`
- **Needs done:** steps 0–1; 7, 12 (Identity and Credentials with ID-OBS-1; Localisation)
- **Conforms to:** ErrorHandling (`OBS_` codes); Logging (no password, authentication string or path)
- **Gate:** `cargo build -p ordin-obs` · `cargo test -p ordin-obs` (against a fake obs-websocket v5 server written by the tests: authentication, StartRecord, StopRecord with an output path, scene and inputs created only when absent, the person's profile and scene restored, each failure state, OBS 29 on macOS) · `cargo clippy -p ordin-obs -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-111 (§7.8)
- **Exit:** X1–X5.

#### Step 82 — Minutes Service

*Added January 2026 (amended January 2026; user decision; Approved by the user January 2026). Build phase 4.*

- **Crate:** `crates/ordin-minutes` (package `ordin-minutes`) · **Programs:** D · **Spec:** `components/MinutesService.md`
- **Needs done:** steps 0–2; 5, 12, 80, 81 (shared types with WIRE-MIN-1 to WIRE-MIN-6; Journal with JA-MIN-1 and JA-MIN-2; Localisation; Transcription Service; OBS Recorder Adapter)
- **Conforms to:** ErrorHandling (`MIN_` codes); Logging; Configuration (minutes settings); GracefulShutdown (a running recording stopped and queued at quit)
- **Gate:** `cargo build -p ordin-minutes` · `cargo test -p ordin-minutes` (a scripted `MinutesWriter`, the fake OBS of Step 81, the fixtures of Step 80; every scenario of the spec) · `cargo clippy -p ordin-minutes -- -D warnings` · `cargo test -p ordin-arch-test`
- **Exit:** X1–X5.

#### Step 30 — Checker

- **Crate:** `crates/ordin-checker` (package `ordin-checker`) · **Programs:** D P A · **Spec:** `components/Checker.md`
- **Needs done:** steps 0–1; 5, 6, 8, 15 (Agent Model, Resolver and Compiler, Organisation Profile, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (SQLite and PostgreSQL slices); InputValidation (every part before it is offered (`CHK_`))
- **Gate:** `cargo build -p ordin-checker` · `cargo test -p ordin-checker` · `cargo clippy -p ordin-checker -- -D warnings` · `cargo test -p ordin-arch-test` · parity: the committed fixtures in `testdata/parity/checks/` give the expected findings (R-69)
- **Exit:** X1–X5.

#### Step 31 — Installers

- **Crate:** `crates/ordin-installers` (package `ordin-installers`) · **Programs:** D P A · **Spec:** `components/Installers.md`
- **Needs done:** steps 0–1; 8, 13, 15 (Agent Folder Manager, Resolver and Compiler, Agent Model)
- **Conforms to:** ErrorHandling; Logging; Configuration
- **Gate:** `cargo build -p ordin-installers` · `cargo test -p ordin-installers` · `cargo clippy -p ordin-installers -- -D warnings` · `cargo test -p ordin-arch-test` · golden-tree tests of each installer's pure render against the committed trees in `testdata/parity/installers/`
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
- **Amended January 2026 (ADR-121; Approved by the user January 2026):** the team workspace is built with this step: the results folder, results recorded with the run that wrote them, links made from a description's places, waiting grants, the result path rule, keeping both results of one name and the end of a team workspace, with migration step 4 (`components/WorkspaceService.md` WS-TW-1 to WS-TW-9).

#### Step 50 — Catalogue Service

- **Crate:** `crates/ordin-catalogue` (package `ordin-catalogue`) · **Programs:** D P A · **Spec:** `components/CatalogueService.md`
- **Needs done:** steps 0–1; 5, 6, 8, 14, 30, 45 (Organisation Profile, Project Service, Format Plug-ins, Checker, Agent Model, Journal)
- **Conforms to:** ErrorHandling; Logging; Configuration; DatabaseMigrations (own SQLite file in A (DatabaseMigrations Internal Logic 13)); Pagination; GracefulShutdown; LicenceCheck (`git2`, exception E1)
- **Gate:** `cargo build -p ordin-catalogue` · `cargo test -p ordin-catalogue` · `cargo clippy -p ordin-catalogue -- -D warnings` · `cargo test -p ordin-arch-test` · Gherkin *The PartSource contract test passes*: `assert_part_source_contract` against the catalogue over the contract fixture repositories
- **Conditional on:** C-63, C-66, C-67 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-106):** the Jira amendment of `components/CatalogueService.md` (lifecycle place type `jira`, `LIFE_PLACE_INVALID`, migration step 2, a widening rebuild under DatabaseMigrations rule 12a) is built in this step.
- **Amended January 2026 (Approved by the user January 2026):** the software-engineering agent's profile lists the plan index as its eleventh task (`components/CatalogueService.md` CAT-TF-5); no code, the step's scenarios run over the changed bundled catalogue, and the agents repository's commit `RELEASE_BUNDLED_AGENTS_COMMIT` names moves with it, as PB-TF-1 states.

#### Step 51 — Push Service

- **Crate:** `crates/ordin-push` (package `ordin-push`) · **Programs:** D P · **Spec:** `components/PushService.md`
- **Needs done:** steps 0–1; 5, 11, 16, 38, 43, 44, 45 (Approval Service, SharePoint Adapter, Confluence Adapter, Project Service, Craft Toolkit, File Service, Journal)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; GracefulShutdown; LicenceCheck (`git2`, exception E1)
- **Gate:** `cargo build -p ordin-push` · `cargo test -p ordin-push` · `cargo clippy -p ordin-push -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-13, C-28, C-42 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-103):** the team-knowledge amendment of `components/PushService.md` (team shares: card, push of a prepared commit or a review request, resend after the branch moved, withdrawal, recovery, and its additive migration step) is built in this step.
- **Amended January 2026 (ADR-121; Approved by the user January 2026):** no code: a team workspace's results, activity and description travel through the team-share and published-copy paths already built here (`components/PushService.md` PS-TW-1); the step's scenarios gain PS-TW-1's.

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
- **Amended January 2026 (ADR-121; Approved by the user January 2026):** nothing: `components/KnowledgeService.md` KS-TW-1 is withdrawn (a team workspace writes no `log.md` line), so this step does not run again for the team workspace.

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
- **Amended January 2026 (ADR-119; Approved by the user January 2026):** a prompt with `title` and `inputs` is checked at prepare and released in the Installers' INST-TF-1 form with `${input:arguments}` (`components/ReleaseBuilder.md` RB-TF-1 to RB-TF-3, `PRE_TASK_INVALID`); Step 52's scenarios run again unchanged.

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
- **Amended January 2026 (ADR-121; Approved by the user January 2026):** the results folder named in a team workspace's tasks (`components/OrdinHost.md` OH-TW-1, OH-TW-2) is built in this step.

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
- **Amended January 2026 (ADR-110)** — *not in the first build (ADR-121 item 9; built later, see the ADR-121 line below)*: the read-folder amendment of `components/TeamKnowledgeService.md`: bindings keyed by `TeamFolderId` with role and origins (forward-only table rebuild with a migration test proving every binding, item record, problem and share row kept), read folders and **Stop reading**, company-named folders reconciled from the profile, the three-answer review check, one repository copy and one fetch per repository with the random timer offset, descriptions, checks files and the recall check, Adopt, **Adopt again**, **Keep as it is**, the adoption records and their files, and the adopted items listed in a share's review request. The gate's tests include every scenario of the amendment against the test Git host.
- **Amended January 2026 (ADR-111):** relationship files now carry links, with no format change (`components/TeamKnowledgeService.md` amendment January 2026, SA-AMEM-04).
- **Amended January 2026 (ADR-112)** — *not in the first build (ADR-121 item 9; built later; its migration step 3 is applied in the first build with its table empty)*: `index.md` and `log.md` in the own folder at each share, reserved files skipped on a get, plain OKF bundles as read folders, and migration step 3 (`components/TeamKnowledgeService.md` amendment January 2026).
- **Amended January 2026 (ADR-121 items 9 and 10; Approved by the user January 2026):** **this step's first build is of the workspace's own folder and the team workspace together** (`crates/ordin-team-knowledge` holds no code before it). It builds the base specification of `components/TeamKnowledgeService.md` for the own folder (ADR-103 items 1–10, §A to §H with §B step 4's clone per workspace), the amendments ADR-104, ADR-111 and ADR-118 as they apply to the own folder, and the team workspace amendment (January 2026, ADR-121): the workspace description, **Share as a team workspace…**, **Open a team workspace…**, a colleague's changes merged setting by setting with the later share winning, waiting grants and **Allow**, names checked against the repository's history (TW-J), activity files (one per entry under `activity/`, no `log.md` line) and the run hook, results shared after each run with same-name results both kept, **Leave the team workspace**, and the module's migration steps 1 to 4 — steps 2 and 3 applied as their amendments write them, their tables left empty. **Not in this build:** the ADR-110 amendment (read folders, company-named folders, descriptions, adoption, the per-repository clone and timer of §J) and the ADR-112 amendment (`index.md`, `log.md`, plain bundles); each is built in a later build of this step, with its own gate, adding code and no migration step; the build notes in `components/TeamKnowledgeService.md` mark them. **Needs done** for the first build: steps 0–1; 5, 7, 42, 45, 49, 51, 53 (step 6, the Organisation Profile, is needed only by the ADR-110 amendment); the Session Supervisor, Package Inspector, Catalogue Service and Package Catalogue are reached only through the program's `TeamRuns` and `TeamAgents` (the crate's tests use fakes of both). The gate's tests include every scenario of the base specification for the own folder, of ADR-104, ADR-111 and ADR-118 for the own folder, and of the team workspace amendment. The `team-knowledge` switch is on by default in the desktop program (§9 item 2, OrdinDesktop.md DSK-TW-1).

#### Step 58 — Home and Search

- **Crate:** `crates/ordin-home-search` (package `ordin-home-search`) · **Programs:** D P · **Spec:** `components/HomeAndSearch.md`
- **Needs done:** steps 0–1; 5, 16, 38, 44, 49, 53 (Journal, Knowledge Service, File Service, Workspace Service, SharePoint Adapter, Confluence Adapter)
- **Conforms to:** ErrorHandling; Logging; DatabaseMigrations; Pagination
- **Gate:** `cargo build -p ordin-home-search` · `cargo test -p ordin-home-search` · `cargo clippy -p ordin-home-search -- -D warnings` · `cargo test -p ordin-arch-test`
- **Conditional on:** C-36 (§7.8)
- **Exit:** X1–X5.
- **Amended January 2026 (ADR-106):** the Jira amendment of `components/HomeAndSearch.md` (changed marks from the Workspace Service's Jira origin changes; no copy-update row for Jira) is built in this step.
- **Amended January 2026 (ADR-121; Approved by the user January 2026):** a colleague's added agent or access waits in *Needs you* (`components/HomeAndSearch.md` HS-TW-1 to HS-TW-3: `OpenTarget::TeamGrant`, `needs-you-team-grant`, `needs-you-team-grant-unconfirmed`, `needs-you-team-grant-gone`); see §7.14.

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
- **Amended January 2026 (ADR-121; Approved by the user January 2026):** the share and open dialogs, the access card, the **Team activity** row and screen and **Leave the team workspace** (`components/ScreensModule.md` SM-TW-1 to SM-TW-12) are built in this step.

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
- **Amended January 2026 (ADR-121; Approved by the user January 2026):** `default = ["team-knowledge"]`, the `TeamAgents`, `TeamRuns`, `TeamWorkspaces` and `TeamPush` delegations and the event-sink registration of row 3.41a (`components/OrdinDesktop.md` DSK-TW-1 to DSK-TW-3) are built in this step.

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

- Stop order and bounds (GracefulShutdown.md, P4-2): on `SIGTERM` the personal server stops the desktop's modules in the desktop's order after its own inputs; runs may finish until 30 s after the signal and are then cancelled and recorded `interrupted`; the Channel Gateway drain is bounded at 10 s, the Remote API wait at 5 s, every other step at 2 s; the whole stop ends within 54 s, inside `TimeoutStopSec=60s` and the container's 60 s stop period (`docker run --restart unless-stopped --stop-timeout 60`). The agent profile never cuts a run short. One test per bound, with a module double that overruns it.
- Exit statuses of every command: 0, 1 with `error[<CODE>]: <message>`, 2, 3, 4, 70 (OrdinServer.md *Exit statuses*); one test per Error Table row.
- Control socket `<ORDIN_DATA_DIR>/run/control.sock`, mode `0600`, peer user checked with `SO_PEERCRED`.
- **Amended January 2026 (ADR-107):** wiring of `EnvironmentSecrets`, `ContentChecker`, `SupervisorHost::check_content`, `HostedAgentCore::context_lookup` and `context_recent`; the Helm values `podAnnotations` and `roles.<role>.podAnnotations`.

### 6.3 Screens module — Step 71

`apps/desktop/ui/` (npm package `@ordin/screens`, private), TypeScript with Svelte 5 and Vite, depending only on the TypeScript types `ordin-service-api-gen` writes (2B-1 rule 6). Built before Step 72 because the desktop's `tauri.conf.json` takes `frontendDist: "ui/dist"` (relative to the folder that holds `tauri.conf.json`, `apps/desktop`; the build command runs inside `apps/desktop/ui`, so `--prefix` names that same folder) (OrdinDesktop.md).

- **Needs done:** Step 0 (package skeleton) and Step 3 (generator); *amended January 2026 (ADR-117):* Step 28 (Brand: the build pack's icon set the module copies).
- **Conforms to:** ErrorHandling (draws the sentence a reply carries; records `UI_CHART_RENDER_FAILED`); LicenceCheck (**lands here**: `license-checker-rseidelsohn` and `npm audit` over the module's production dependencies).
- **Gate** (in `apps/desktop/ui/`): `npm run gen:api` then `npm run gen:check` exits 0 (TypeScript type generation check; runs `ordin-service-api-gen check .`) · `npx svelte-check` exits 0 · `npx vitest run` exits 0 · `npx license-checker-rseidelsohn --production --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC"` and `npm audit --omit=dev` exit 0.
- **Tests the spec names** (ScreensModule.md, *Tests (vitest)*): one per Error Table row; one per block kind with the 2C JSON example as input; store tests table-driven over event sequences (in order, repeat, gap, reset, unknown kind); a source-scan test that fails on `{@html`, `innerHTML`, `fetch(`, `XMLHttpRequest`, `WebSocket`, `EventSource`, and on any import of `@tauri-apps/api` outside `lib/backend/tauri.ts`; a catalogue test that every message id used in the source exists in the en-GB `.ftl` files; an accessibility test of the approval screen and the confirmation dialog (focus on open, Tab trapped, Escape).
- **Performance checks:** initial JavaScript ≤ 600 KB gzipped, with Monaco, xterm.js and ECharts loaded on first use; a chart of 5,000 points drawn ≤ 300 ms (C-40).
- **Conditional on:** C-40 (ECharts provisional), C-34 (Monaco tokenisers; named by Phase 1, not by the spec).
- **Exit:** X1, X2 and X3 for the module (coverage ≥ 70 %, ScreensModule.md); X4 and X5 do not apply (no Rust crate).
- **Amended January 2026 (ADR-116):** also **conforms to** `cross-cutting/VisualDesign.md`; the tests that spec names (computed styles in both themes, `brandTokens` over every mapping row, every fallback chain, every contrast pair of V7) join the gate; a lint step fails on a colour literal or a `font-family` in a component's style block; each changed screen or block is reviewed beside its scene (ScreensModule.md amendment January 2026, item 6) and the scene id recorded in the change's description. Needs Step 28's January 2026 amendment (the build pack 1.1.0) for the default values to reach an installed computer.
- **Amended January 2026 (ADR-117):** the icons of `cross-cutting/VisualDesign.md` V8 and ScreensModule.md amendment *Icons* (SM-IC-1 to SM-IC-4); their tests join the gate, among them the byte comparison of `apps/desktop/ui/src/assets/icons/ordin-icons.svg` with `crates/ordin-brand/build-brand/icons-light.svg`, so Step 28 runs before this step; each changed screen part is reviewed beside its scene with its icons.

### 6.4 ordin desktop — Step 72

`apps/desktop` (package `ordin-desktop`, binary `ordin`), the Tauri 2 composition root for macOS and Windows (OrdinDesktop.md).

- `tauri.conf.json` as OrdinDesktop.md gives it: `frontendDist: "ui/dist"` (relative to the folder that holds `tauri.conf.json`, `apps/desktop`; the build command runs inside `apps/desktop/ui`, so `--prefix` names that same folder), `devUrl: "http://localhost:5173"`, `beforeBuildCommand: "npm --prefix ../ui run build"`, the content security policy (Cors.md), `capabilities: ["main"]`; `apps/desktop/capabilities/main.json` limits the WebView to the `ordin` plugin's Service API commands and to listening for events.
- Integration test stubs the spec names (one each): construction with fake modules records the order of *Internal Logic* 3.1–3.49; a fake `ServiceApi` call through the real Command Bridge plugin under the `main` capability; the WebView cannot call a command outside `ordin:default`.
- Stop: `Core::shutdown` (*Internal Logic* steps 10–11); quit with no running session ≤ 3 s, with sessions ≤ 20 s.
- `WordToMarkdown` is passed to the Confluence Adapter only once C-13 confirms the conversion, else `None`.
- The installer, with its per-user default and whole-computer choice (ADR-072), and the updater (C-46, C-108, Setup Assistant) are built with the Tauri bundling command the user names under P7.

---
- **Amended January 2026 (ADR-107):** `SupervisorHost::check_content` answers passed on the desktop.

### 6.5 Release pipeline (*added January 2026, ADR-114; amended January 2026: run by a maintainer with `make`, not by CI*)

Built before the first version is published, and used for every version. It is three make targets a maintainer runs on their own computer when they judge a version ready (ADR-114 item 9); each takes `VERSION=<version>`. No CI job builds, signs or publishes a release, and no CI service holds the update key.

- `make release-check` runs step 1.
- `make release-build` runs step 1, then steps 2 to 4 for the platform of the computer it runs on. A macOS installer is built on a macOS computer and a Windows installer on a Windows computer, so the maintainer runs it once on each and gathers the outputs in one folder.
- `make release-publish` runs steps 5 to 7 over that folder, and creates the version's tag `v<version>` with its release.

The private half of the update key is a file on the maintainer's computer, outside version control (the repository's `.gitignore` refuses every `*.key` file); the targets pass its path to the Tauri signer in `TAURI_SIGNING_PRIVATE_KEY` and its password in `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and a target started without them stops before building anything.

1. **Gate.** `make check` over the workspace with the default switches of the version (§9), then the ship gate the roadmap names for that version (kept by the maintainers outside the repository). A failure stops the target and nothing is built or published.
2. **Desktop installers.** `cargo tauri build` (P7) with the version's default switches, signed with the update key (ADR-114 item 8; ADR-072), and with no Apple or Microsoft certificate. Each installer gets a detached signature file beside it, which a person checks with `minisign -Vm <installer> -p <public key file>`; the public half of the update key is committed in the repository. The macOS app also takes Apple's free ad-hoc signature (`codesign -s -`), which involves no Apple account. A SHA-256 hash file lists every installer, for IT hash rules (R-61). The key and its password are never written to the output, to a log or to an artefact.
3. **Server archives** (when the version turns on `personal-profile` or `agent-profile`). `ordin-server-<version>-linux-<x86_64|aarch64>.tar.gz` as §6.2 item 1, each signed with the update key in the signature form the Tauri updater checks (C-99).
4. **Container image and Helm chart** (when the version turns on `agent-profile`), as §6.2 items 4 and 5, published to the project's image registry.
5. **Feed.** One feed file in Tauri's updater format: the version, the release notes `/releasegen` drafted (at most 4,000 characters, SetupAssistant.md `UpdateOffer.notes`), the date, and one entry per platform with its download address and signature: the two desktop installers, and the two server archives when step 3 built them.
6. **Publish.** Artefacts, with their detached signatures and the hash file, first, then the feed file at the stable address the unbranded build's installer carries (`InstallerSettings.update_feed_url`, A-69), so the feed never names a file that is not yet there.
7. **Check after publishing.** The target reads the published feed back, downloads every entry, verifies each signature against the public half of the update key built into the programs, and checks each downloaded installer against the hash file. A failure withdraws the feed file by restoring the previous one and stops the release.

**Gate:** steps 1–7 pass for a test version published to a test feed address before the first real version. **Exit:** an installation on macOS and one on Windows, pointed at the test feed, offer the test version, download it, install it with **Restart to update**, and show its **What's new**, with no system warning (C-108). A whole-computer install offers the same version, downloads nothing, and asks the person to ask their administrator.

#### 6.5 amendment January 2026 — the pipeline's files, inputs and checks

> **Status:** written 2026-01-10 under the user's standing instruction ("proceed with recommendations"); the user has not read this text. The rules PB-REL-1 to PB-REL-10 restate §6.5 as files and commands and choose only what the section leaves open. OQ-REL-1 to OQ-REL-3 are the user's decisions. Counterpart: `components/OrdinDesktop.md`, DSK-REC-46.
>
> Evidence read: §6.5 above; `apps/desktop/tauri.conf.json` (`plugins.updater.pubkey` empty, `endpoints` empty, targets app, dmg, nsis); `Makefile`; `.gitignore` (`*.key` refused); `apps/desktop/src/updater.rs` (`SPEC-GAP-33`); `components/OrdinDesktop.md` DSK-REC-39, DSK-REC-39a and OQ-REC-13; `git remote -v` (`origin` is `https://github.com/gatblau/ordin.git`).

- **PB-REL-1 Files and commands.** The three make targets call `scripts/release/check.sh`, `scripts/release/build.sh` and `scripts/release/publish.sh`, each with `VERSION=<version>`; `scripts/release/lib.sh` holds what they share and `scripts/release/leak-check.sh` is PB-REL-9. The pipeline's settings are the `KEY=value` lines of `release/release.conf` (keys named `RELEASE_*`; a value already in the environment wins over the file). `scripts/release/test.sh` tests all of them against stand-ins for `cargo tauri`, `minisign` and `make`, and `make ci` runs it. The scripts need `jq`, `curl`, `git`, `base64` and `shasum` or `sha256sum`, and `release-publish` also `minisign`; a missing tool stops them with its name.
- **PB-REL-2 Version.** `VERSION` must match `MAJOR.MINOR.PATCH` with an optional `-` suffix of letters, digits, dots and hyphens, and must equal the version of the package `ordin-desktop`. Any other value stops the target before anything is built or written.
- **PB-REL-3 The key.** `release-check` needs no key, because it builds nothing. `release-build` and `release-publish` stop at once, building and writing nothing, when `TAURI_SIGNING_PRIVATE_KEY` is unset or does not name a readable file, or when `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is unset. The messages name the variable and never print a value. The password reaches the Tauri signer only through its environment variable, never as a command-line argument, and no target runs with shell tracing on. *Amended 2026-01-10 (user request): `make release VERSION=<version>` asks for the password once, on the terminal and without echo, when `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is unset; it signs a small file and verifies it against `release/ordin-update.pub` before building, so a wrong password or another key stops before the gate; it then runs `release-build` and, unless `PUBLISH=0`, `release-publish`. It stops with nothing built when there is no terminal. With no `VERSION` it asks for it, what to do (build only, build and publish to the test feed, build and publish to the real feed, which asks for the version to be typed again), the single-platform and no-bundled-agents choices where they apply, and one line of release notes when publishing.* *Amended 2026-01-30 (user request): `make publish VERSION=<version>` runs the same password prompt and key check with `BUILD=0`, then `release-publish` alone, building nothing; it stops when `VERSION` is not given or `PUBLISH=0` is set.*
- **PB-REL-4 The public key (resolves how `SPEC-GAP-33` is closed; the key itself is the maintainer's).** `release/ordin-update.pub` holds the public half in minisign's file form (a comment line and one base64 line), which `minisign -Vm <installer> -p release/ordin-update.pub` reads. The string Tauri's configuration needs is the base64 of that whole file; `release-build` computes it and passes it to the build through `--config`, so `tauri.conf.json` stays without a key and a development build still refuses every update. `release-build` and `release-publish` stop when the file is missing or empty. The maintainer makes the pair once: `cargo tauri signer generate -w ~/ordin-update.key` (it asks for a password and writes `~/ordin-update.key` and `~/ordin-update.key.pub`), then `base64 -d < ~/ordin-update.key.pub > release/ordin-update.pub`, and commits only `release/ordin-update.pub`. The private file stays outside the repository.
- **PB-REL-5 Channels.** `RELEASE_CHANNEL` is `test` or `real` and defaults to `test`; `real` has to be written out. The channel's settings are `RELEASE_<TEST|REAL>_FEED_URL` (the stable address of `latest.json`), `RELEASE_<TEST|REAL>_ARTEFACT_BASE_URL` (artefacts are served at `<base>/v<VERSION>/<file>`) and `RELEASE_<TEST|REAL>_UPLOAD`, which is `dir:<folder>` (copy into a folder that is served at the base address; also used by the tests) or `github:<owner>/<repo>:<feed-tag>` (artefacts become the release `v<VERSION>`, which creates the tag on the releases repository's default branch, and whose description is the notes followed by the source commit's hash; `latest.json` is an asset of the release `<feed-tag>`). All six ship empty (OQ-REL-1); `release-publish` stops, naming the empty one. A feed or base address must begin with `https://`; `file://` is accepted only when `RELEASE_ALLOW_FILE_URLS=1`, for the tests.
- **PB-REL-6 Build.** `release-build` runs, in order: the gate (`make check`; a failure stops), the key and public-key checks, the bundled agents input (PB-REL-7), `make desktop-build-inputs`, then `cargo tauri build` in `apps/desktop` with `--config` carrying `bundle.createUpdaterArtifacts = true`, `plugins.updater.pubkey` (PB-REL-4), `plugins.updater.endpoints` = the real channel's feed address when it is set, the `catalogue` resource when PB-REL-7 produced one, and, on macOS, `bundle.macOS.signingIdentity = "-"` (Apple's free ad-hoc signature, no account). It stops when the version it was given differs from the package's. It collects, into `RELEASE_OUT_DIR` (default `target/release-out/<VERSION>`), `ordin-<VERSION>-<platform>.<ext>` for each installer (the `.dmg` and the `.app.tar.gz` on macOS, the NSIS `-setup.exe` on Windows; `<platform>` is `darwin-aarch64`, `darwin-x86_64`, `windows-x86_64` or `windows-aarch64`), signs each with `cargo tauri signer sign -f <the key file>`, run with `TAURI_SIGNING_PRIVATE_KEY` left out of the signer's environment because the signer reads that variable as the key's text and not as a path, which writes `<file>.sig` (base64 of a minisign signature, the form the feed carries), and writes `<file>.minisig` (the same signature decoded, the form `minisign -Vm` reads beside the file). It writes `SHA256SUMS.<platform>` (`<hash>  <file name>` for every installer and updater artefact) and `platform-<platform>.json` (`platform`, `file`, `signature`: the updater artefact and its `.sig` text).
- **PB-REL-7 The bundled agents input (DSK-REC-39a; FU-145).** `RELEASE_BUNDLED_AGENTS_SOURCE` is a local clone of the bundled agents repository and `RELEASE_BUNDLED_AGENTS_COMMIT` the commit's full 40-character hash. Both set: the commit must exist in the clone, or the build stops; `git archive` of that commit is extracted into `apps/desktop/catalogue/` (emptied first, ignored by git) and that folder becomes the bundle's `catalogue` resource. Neither set: the build stops and says so, unless `RELEASE_ALLOW_NO_BUNDLED_AGENTS=1` is written out, in which case it builds without them and prints that the application will answer `PKG_BUNDLED_AGENT_MISSING`. Only one set, or a hash that is not 40 hexadecimal characters, stops it. After the build the pipeline lists the built application's `catalogue` resource folder (macOS: `ordin.app/Contents/Resources/catalogue`; on Windows, where the build leaves no unpacked application, the staged `apps/desktop/catalogue`) and fails when it holds no file, unless the allow-flag was written. `release-check` runs the same commit test when the two are set. Nothing is rewritten, and nothing reads the earlier implementation.
- **PB-REL-8 Publish.** `release-publish` works over `RELEASE_OUT_DIR`, which must hold a `platform-*.json` for at least one macOS and one Windows platform. *Amended 2026-01-10 (user decision, no Windows computer available): on the test channel only, when `RELEASE_ALLOW_SINGLE_PLATFORM=1` is set, a folder with a macOS platform and no Windows platform is accepted; the publish prints a warning that names the platforms the feed carries, and the feed carries only those. The real channel always requires both and refuses with that reason whatever the setting. A folder with no macOS platform is refused on both channels.* It: (1) checks each installer against its `.minisig` with `minisign -Vm <file> -p release/ordin-update.pub` and each hash file; a failure stops with nothing uploaded; (2) merges the `SHA256SUMS.*` into `ordin-<VERSION>-SHA256SUMS.txt`; (3) reads the notes from `RELEASE_NOTES_FILE` (default `<out dir>/notes.md`; the `/releasegen` draft) and stops when they are over 4,000 characters (it does not cut them); (4) writes `latest.json` in Tauri's updater format: `version`, `notes`, `pub_date` (UTC, RFC 3339), and `platforms.<platform>` with `signature` and `url` = `<base>/v<VERSION>/<file>`; (5) uploads the artefacts, their `.sig` and `.minisig` files and the hash file first; (6) keeps the previous `latest.json` if there is one, then replaces it; (7) reads the feed back from the channel's feed address and fails unless its `version` is `<VERSION>`, then downloads every file the hash file lists, checks its hash, and checks every signature against the committed public key, using the feed's own `signature` text for each feed entry and the `.minisig` file for the other installers. Any failure in (7) restores the previous `latest.json` (or removes the feed when there was none), says so, and exits non-zero; the uploaded artefacts stay, and deleting a test release and tag is the maintainer's action.
- **PB-REL-9 The leak check.** `scripts/release/leak-check.sh <path>...` stops with status 1 when any file under the paths holds a line of the key file or the key's password. `release-build` runs it over its output folder and its build log, which it keeps in the output folder as `build.log`, and `release-publish` runs it over the output folder before uploading.
- **PB-REL-10 Tests.** `scripts/release/test.sh` covers: a version refused; `release-check` with no key; `release-build` and `release-publish` with each key variable unset (and nothing built); a missing or empty public key file; the bundled agents input unset, set, with a bad hash and with a hash the clone lacks; the key found in the build log; the notes over 4,000 characters; a changed installer refused before upload; a full publish to a `dir:` target with the feed, the hash file and the signatures read back; a read-back failure that restores the previous feed.

**Open questions (the user's decisions).**

- **OQ-REL-1 The release feed's address and where artefacts are hosted (OA-6).** Nothing in the repository fixes either. **Recommendation:** the project's GitHub releases at `gatblau/ordin-releases` (a repository of their own, separate from the source repository `gatblau/ordin`): artefacts at `https://github.com/gatblau/ordin-releases/releases/download/v<VERSION>/<file>`, the real feed at `https://github.com/gatblau/ordin-releases/releases/download/feed/latest.json` and the test feed at `https://github.com/gatblau/ordin-releases/releases/download/feed-test/latest.json`, with `RELEASE_*_UPLOAD` = `github:gatblau/ordin-releases:feed` and `github:gatblau/ordin-releases:feed-test`. It costs nothing, serves https, needs no service of ours and keeps the address stable. Blocks `release-publish` only; until the six values are written the target stops. **Resolved 2026-01-10 (user, "proceed with recommendations"):** as recommended. `release/release.conf` carries `RELEASE_TEST_FEED_URL=https://github.com/gatblau/ordin-releases/releases/download/feed-test/latest.json`, `RELEASE_TEST_ARTEFACT_BASE_URL=https://github.com/gatblau/ordin-releases/releases/download`, `RELEASE_TEST_UPLOAD=github:gatblau/ordin-releases:feed-test`, and the same three for `real` with `feed`. The base address has no trailing slash; PB-REL-5 adds `/v<VERSION>/<file>`. **Amended 2026-01-10 (user decision):** releases are stored in their own repository, `gatblau/ordin-releases`, decoupled from the source repository `gatblau/ordin`; every address above reads `gatblau/ordin-releases`.
- **OQ-REL-2 The help, support and privacy addresses (FU-033).** `crates/ordin-brand/build-brand/brand.json` still carries `https://ordin.invalid/help`, `/support` and `/privacy`. **Recommendation:** help = the repository's README, support = its issue tracker, privacy = a privacy page the maintainer writes in the repository (none exists yet). **Resolved 2026-01-10 (user, "proceed with recommendations"):** `brand.json` carries `help_url` = `https://github.com/gatblau/ordin#readme`, `support_url` = `https://github.com/gatblau/ordin/issues` and `privacy_url` = `https://github.com/gatblau/ordin/blob/main/docs/privacy.md`. `docs/privacy.md` is a short placeholder limited to what the design and the app data folder show (local storage; no telemetry, assumption A-10; the update check reads the feed address). The maintainer still writes and reviews the final wording; the privacy page is not final until then.
- **OQ-REL-3 Where the release gets the memory models (DSK-REC-13).** *(Source decided 2026-01-28, user decision: the models come from the releases repository `gatblau/ordin-releases` on GitHub, release `models-1`; they do not share the bundled agents' repository. The file names, digests and the pipeline input are settled by the resolution below.)* The application looks for `embedding/` and `reranker/` in a `models` resource folder and a `ModelManifest` compiled in by the release build, and no document or file records the models' source, file names or digests. A release built without them starts, and recall answers `MEM_MODEL_UNAVAILABLE`. **Recommendation:** the maintainer names the source and digests, then a spec amendment adds them to this pipeline as one more input like PB-REL-7. Blocks only a release that includes memory recall.
  **Resolved 2026-01-28 (user decision; PB-REL-11 to PB-REL-14 below, Approved 2026-01-28):** the models are not bundled. They are fetched the first time memory is used, after the person agrees, from one zip published as the release `models-1` of `gatblau/ordin-releases`. The release build carries the zip's address, its size, its SHA-256 and the manifest of the ten file digests as build inputs, and copies no model into the bundle. The file names and digests are the ten lines of `release/memory-models/files.txt`.

#### 6.5 amendment 2026-01-28 — the memory models as a release input (resolves OQ-REL-3)

> **Status:** Approved by the user 2026-01-28 ("proceed with recommendations"). Source: the user's decisions of 28 January 2026 (fetched on first use of memory, not bundled; one zip on `gatblau/ordin-releases` carrying both licences; the person is asked first). It changes how ADR-033's models reach the computer, not which models they are; the HLD records the change separately (see *Open questions* below). Counterparts: `components/OrdinDesktop.md` amendment 2026-01-28 (DSK-MD-1 to DSK-MD-12), `components/MemoryService.md` amendment 2026-01-28 (MEM-MD-1 to MEM-MD-4).
>
> Evidence read: OQ-REL-3 above; `release/release.conf` (no model key); `release/memory-models/files.txt` (ten lines `<sha256> <path in the zip> <source address>`); `scripts/release/memory-models.sh` (writes `ordin-memory-models-<N>.zip` holding `LICENSES.md`, `SHA256SUMS`, `embedding/` and `reranker/`, with a fixed file date and order); `scripts/release/check.sh`, `scripts/release/build.sh` (no model copied today); `crates/ordin-memory-service/src/config.rs` (`ModelManifest { files: Vec<ModelFileDigest> }`, `ModelFileDigest { path, sha256 }`); `crates/ordin-memory-service/src/providers.rs` (`check_models`, the ten files of `bundled_files()`); `apps/desktop/src/window.rs` (`models_dir: resources.join("models")`, `model_manifest: Default::default()`). The published zip, read 2026-01-28: `https://github.com/gatblau/ordin-releases/releases/download/models-1/ordin-memory-models-1.zip`, 173,982,577 bytes, SHA-256 `c97ba4951a33720e317d1b95f5859f0fba075e2c0b5fa4e19bdb87694ac995d7`.

- **PB-REL-11 The models' settings.** `release/release.conf` gains three keys: `RELEASE_MEMORY_MODELS_URL` (the zip's `https://` address), `RELEASE_MEMORY_MODELS_SHA256` (64 lowercase hexadecimal characters) and `RELEASE_MEMORY_MODELS_BYTES` (the zip's exact size in bytes, a whole number above 0, no separators). For `models-1` they are `https://github.com/gatblau/ordin-releases/releases/download/models-1/ordin-memory-models-1.zip`, `c97ba4951a33720e317d1b95f5859f0fba075e2c0b5fa4e19bdb87694ac995d7` and `173982577`. The manifest is not a setting: it is read from `release/memory-models/files.txt`, first and second columns, comment and empty lines skipped. A value already in the environment wins over the file, as PB-REL-1 states.
- **PB-REL-12 The check of the models input.** `release-check` and `release-build` (before the gate, so a bad input stops before anything is built) stop, naming the setting or the line, when: any of the three settings is empty; the address does not begin with `https://`; the SHA-256 is not 64 lowercase hexadecimal characters; the size is not a whole number above 0; `release/memory-models/files.txt` holds no entry (the empty-manifest case); it holds other than exactly ten entries; its paths are not exactly the ten files the Memory Service checks — `model.onnx`, `tokenizer.json`, `config.json`, `special_tokens_map.json` and `tokenizer_config.json` under each of `embedding/` and `reranker/`; a path appears twice; or a digest is not 64 lowercase hexadecimal characters. There is no flag to build without the models: a release always carries the input. `release-check` reads no network address; whether the zip at the address has that fingerprint is the maintainer's check when publishing it (PB-REL-13).
- **PB-REL-13 What the build receives, and what it does not copy.** `release-build` sets four build-time environment variables for `cargo tauri build`: `ORDIN_MEMORY_MODELS_URL`, `ORDIN_MEMORY_MODELS_SHA256` and `ORDIN_MEMORY_MODELS_BYTES` from PB-REL-11, and `ORDIN_MEMORY_MODELS_MANIFEST`, the ten entries as `<path>=<sha256>` joined by `,` in the file's order. The program reads them as DSK-MD-2 states. The `--config` of PB-REL-6 names no `models` resource and the pipeline copies no model file anywhere; the installers' size does not include the models. A build made any other way has none of the four and behaves as DSK-MD-2's "no models input" (memory answers `MEM_MODEL_UNAVAILABLE`, as before this amendment).
- **PB-REL-14 Publishing a models zip.** The zip is made by `make memory-models VERSION=<N>` (`scripts/release/memory-models.sh`) and published by the maintainer as the release `models-<N>` of `gatblau/ordin-releases`, outside `release-publish`; the maintainer downloads it back from its address and compares its SHA-256 with `SHA256SUMS` before writing PB-REL-11's three settings. A published `models-<N>` is never replaced: a changed model or a corrected zip is `models-<N+1>`, new settings, a new `files.txt` when the files change, and a new application version. An installed application keeps fetching the zip its build names.
- **PB-REL-10 Tests (amended 2026-01-28).** `scripts/release/test.sh` adds: `release-check` with `release/memory-models/files.txt` holding no entry (stops, naming the file); with nine entries; with a path outside the ten; with each of the three settings empty; with an `http://` address; and `release-build` passing `ORDIN_MEMORY_MODELS_MANIFEST` with ten pairs and naming no `models` resource in its `--config`.

**Open questions (amended 2026-01-28).**

- **OQ-REL-4 The HLD's word "bundled" (ADR-033).** *(Resolved 2026-01-28: ADR-033 amended in the HLD, January 2026; the 05 Memory models row updated; HLD revision unchanged at 0.66.0.)* ADR-033 and HLD 05's *Memory models* row say the models are bundled with the application. After this amendment they are fetched once, on first use, after the person agrees; which models, and that they run on the computer, are unchanged. **Recommendation:** amend ADR-033 in the HLD (its *Decision* and *Consequences*: "the installer grows by the size of both models" no longer holds; one download of 174 MB on first use of memory from the project's releases host does), and raise `derivedFromHld` on the specs this amendment touches. Does not block this Draft; blocks its promotion to Approved.

#### 6.5 amendment, January 2026 — the Windows installer built on the Mac, as an option

> **Status:** Approved by the user 2026-01-29 ("proceed with recommendations"). Source: the user's decisions of 29 January 2026: the release scripts gain an option to build the Windows NSIS installer on the maintainer's Mac with cargo-xwin; the build on a Windows computer stays the default until one installer built on the Mac has been installed and checked on a clean Windows computer; the user accepted Microsoft's Visual Studio Build Tools licence terms (https://go.microsoft.com/fwlink/?LinkId=2086102) for this use, on the condition that the files cargo-xwin downloads from Microsoft are never published. It amends §6.5's "a Windows installer on a Windows computer" for this option only; every other rule of PB-REL-1 to PB-REL-14 holds for it unchanged.
>
> Evidence read: `scripts/release/build.sh`, `scripts/release/lib.sh`, `scripts/release/leak-check.sh`, `scripts/release/publish.sh`, `scripts/release/test.sh`; `Makefile` (`desktop-build-inputs`); `rust-toolchain.toml` (channel `1.96.0`). A trial on 2026-01-29, on a Linux computer standing in for the Mac (Rust 1.96.0 with the target `x86_64-pc-windows-msvc`, cargo-xwin, tauri-cli 2.12.4, clang, lld, llvm, NSIS), built `ordin-hook` with `cargo xwin build --release -p ordin-hook --target x86_64-pc-windows-msvc` and the installer with `cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis --config <config>`, which wrote it to `<target dir>/x86_64-pc-windows-msvc/release/bundle/nsis`.

- **PB-REL-15 How the option is chosen, and what it writes.** `make release-build-windows VERSION=<version>` runs `scripts/release/build.sh` with `RELEASE_CROSS=windows-x86_64`. `windows-x86_64` is the only value `RELEASE_CROSS` takes; any other stops the build. It runs on macOS only (`uname -s` is `Darwin`) and stops elsewhere, saying to run `make release-build` on Windows. `make release-build` without `RELEASE_CROSS` is unchanged: the native build stays the default on both computers. The option writes into the same `RELEASE_OUT_DIR` the macOS build uses, with PB-REL-6's names (`ordin-<VERSION>-windows-x86_64.setup.exe`, its `.sig` and `.minisig`, `SHA256SUMS.windows-x86_64`, `platform-windows-x86_64.json`), and keeps its log as `build-windows-x86_64.log` so the macOS build's `build.log` stays. Its `platform-windows-x86_64.json` carries one more field, `cross: true`; a native build's never carries it.
- **PB-REL-16 Prerequisites.** Before the gate, the build stops, naming the first missing one and the command that installs it, unless all are present: `clang` and `llvm-rc` (`brew install llvm`), `lld-link` (`brew install lld`), `makensis` (`brew install nsis`), `npm` (`brew install node`), `jq` (`brew install jq`, already PB-REL-1's), `cargo-xwin` (`cargo install --locked cargo-xwin`), `rustup` with the target `x86_64-pc-windows-msvc` installed for the toolchain `rust-toolchain.toml` pins (`rustup target add x86_64-pc-windows-msvc`, run in the repository folder), and tauri-cli at exactly 2.12.4 (`cargo install tauri-cli --version 2.12.4 --locked`), the version the trial proved. Homebrew keeps its `llvm` out of `PATH`, so when `brew` is present the `bin` folders of its `llvm` and `lld` go first on `PATH` for the Windows steps only; the gate keeps the `PATH` it had.
- **PB-REL-17 The licence answer.** cargo-xwin downloads Microsoft's Windows SDK and C runtime under the Visual Studio Build Tools licence. The build stops before the gate, with nothing built or downloaded and a message naming the licence and its address, unless `RELEASE_ACCEPT_MSVC_LICENSE=1` is in the environment (written on the command line by the maintainer, each time); no other value accepts it. The key is refused, with the build stopped, when `release/release.conf` (or the file `RELEASE_CONF` names) holds it, so a committed file can never accept the licence for whoever clones the repository; it has no default. With the answer given, `XWIN_ACCEPT_LICENSE=1` reaches the two Windows steps of PB-REL-19 only, never the gate and never a native build. The condition the user attached to the acceptance is PB-REL-18.
- **PB-REL-18 Microsoft's files are never published.** cargo-xwin keeps them in `XWIN_CACHE_DIR`, which the build sets to `RELEASE_XWIN_CACHE_DIR` or, when that is unset, `~/.cache/cargo-xwin`. The build stops before the gate when that folder is inside the repository or inside `RELEASE_OUT_DIR`, or when `RELEASE_OUT_DIR` is inside it. `scripts/release/leak-check.sh` gains a second check, run wherever PB-REL-9 runs it (after `release-build`, `release-build-windows`, and in `release-publish` before uploading): it fails with status 1, printing file names only, when a file under its paths has one of the kinds the cache holds and a release never does (`.lib`, `.h`, `.hpp`, `.idl`, `.winmd`, `.cab`, `.msi`, `.vsix`, any case); or, when the cache folder (`RELEASE_XWIN_CACHE_DIR`, default `~/.cache/cargo-xwin`) exists, when the file lies inside it or the cache holds a file of the same name.
- **PB-REL-19 The build, and its signing.** In order: PB-REL-2 to PB-REL-4, PB-REL-7 and PB-REL-12's checks, then PB-REL-15 to PB-REL-18's; the gate (`make check`, run natively on the Mac); `make desktop-build-inputs-windows`, which builds the screens (`npm run build`), then `ordin-hook` with `cargo xwin build --release -p ordin-hook --target x86_64-pc-windows-msvc`, and copies it beside the program as `ordin-hook-x86_64-pc-windows-msvc.exe` in `apps/desktop`'s `binaries` folder; then, in `apps/desktop`, `cargo tauri build --config <config> --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis`, with PB-REL-6's `--config` for Windows (no `bundle.macOS`) and PB-REL-13's four variables. It takes the installer from `<target dir>/x86_64-pc-windows-msvc/release/bundle/nsis` unless `RELEASE_BUNDLE_DIR` names another bundle folder. Signing, the `.sig` and `.minisig` files, the hash file, PB-REL-7's catalogue check (the staged `apps/desktop/catalogue`, as on Windows) and the leak check are PB-REL-6, PB-REL-7 and PB-REL-9's, unchanged: the updater signature of an installer built on the Mac is made and checked exactly as a native one's.
- **PB-REL-20 Which feed takes it.** On the test channel, `release-publish` accepts a `platform-windows-x86_64.json` with `cross: true` as it accepts a native one, and prints that the Windows installer was built on the Mac. On the real channel it refuses the folder, with nothing uploaded and a message saying why, while any `platform-windows-*.json` in it carries `cross: true`. The refusal is lifted by amending this assertion once one installer built on the Mac has been installed and checked on a clean Windows computer (installed, started, and updated from the test feed); until then a real release's Windows installer is built on Windows.
- **PB-REL-10 Tests (amended January 2026).** `scripts/release/test.sh` adds, with stand-ins for `uname`, `rustup`, `cargo-xwin`, `clang`, `llvm-rc`, `lld-link`, `makensis` and `npm`: a `RELEASE_CROSS` value other than `windows-x86_64`; the option off macOS; no licence answer, an answer other than `1`, and the answer written in the settings file (each stops with nothing built); a missing `makensis`, a missing target and another tauri-cli version; the cache inside the repository, inside the output folder, and the output folder inside the cache; a native build given the answer, which does not pass it on; a build that passes, checking that the gate runs without the answer, that the Windows steps and the tauri build get it and the cache folder, the tauri build's arguments, its `--config`, the models input, every file of PB-REL-15 in the shared folder beside the macOS ones, the kept `build.log`, the `cross` mark, and the signature; the leak check failing on a `.lib` file, on a file named as one in the cache, on a file inside the cache, and passing the cross-built output; and a publish of the shared folder that the test channel accepts with both platforms and the real channel refuses with nothing uploaded.

#### 6.5 amendment, January 2026 — the gate's evidence: the release computer runs no test

> **Status:** Approved by the user 2026-01-29 ("proceed with recommendations"). Source: the user's decision of 29 January 2026: every test runs on a separate computer kept for testing, so the release build stops running the full gate on the computer that builds the release; it checks instead that the full gate already passed on exactly the commit it builds, and refuses when it did not. The repository is open source and names no maintainer's computer. It amends step 1 of §6.5 (where the gate runs, not what it is), PB-REL-6's and PB-REL-19's "the gate (`make check`)" and `release-check`; every other rule of PB-REL-1 to PB-REL-20 holds unchanged.
>
> Evidence read: `Makefile` (`check` = `make ci` then `make test`; `release-check`, `release-build`, `release-build-windows`); `scripts/release/check.sh`, `scripts/release/build.sh` (the only two places that ran `make -C "$ROOT" check`), `scripts/release/release.sh` (runs `build.sh`), `scripts/release/lib.sh`, `scripts/release/test.sh`; `.gitignore` (the build output, the bundled programs, the built screens and the staged catalogue are ignored, so a build leaves the tree clean).

- **PB-REL-21 The gate's evidence, the default.** `release-check`, `release-build` and `release-build-windows` (and so `make release`) run no test when `RELEASE_GATE` is unset or `evidence`. In the gate's place in their order (PB-REL-6, PB-REL-19), each: (1) stops when the repository folder is not a git working tree, or when `git status --porcelain` lists anything (a changed tracked file, or an untracked file git does not ignore), naming up to five of the paths; (2) fetches `refs/notes/ordin-gate` from the git remote named by `RELEASE_GATE_REMOTE` (default `origin`) into the same local ref, replacing it, and stops naming the remote and git's last error line when the fetch fails (an unreachable remote, or one holding no such ref); (3) reads the note on `HEAD` in that ref and goes on only when its first line begins `check passed `, printing the commit and the line; otherwise it stops naming the commit, saying to run `make gate-record` on the test computer at that commit, or to give `RELEASE_GATE=local`. Every stop builds nothing. The committed `release/release.conf` sets neither `RELEASE_GATE` nor `RELEASE_GATE_REMOTE`: which computer runs the tests is the maintainer's own set-up, given in the environment (a value there wins, PB-REL-1) and in their clone's git remotes.
- **PB-REL-22 Recording the evidence.** `make gate-record` runs `scripts/release/gate-record.sh` on the computer that runs the tests. It stops, recording nothing, when the tree is not clean as PB-REL-21 (1) states; it runs `make check` on `HEAD` and stops, recording nothing, when that fails; after a pass it stops, recording nothing, when `HEAD` moved or a tracked file differs from `HEAD`. Otherwise it writes, replacing any earlier note on that commit, one note in `refs/notes/ordin-gate`: `check passed <UTC time, YYYY-MM-DDTHH:MM:SSZ> <uname -s> <uname -m>, <rustc --version>`. The line names no computer, user or path. When the repository has no `user.email`, the notes commit is made as `ordin-gate <ordin-gate@localhost>`. It pushes nothing: the release computer fetches the ref from the test computer as a git remote, or the maintainer pushes the ref to the remote PB-REL-21 reads.
- **PB-REL-23 Running the gate on the release computer.** `RELEASE_GATE=local` runs `make check` where the step 1 of PB-REL-6 and PB-REL-19 ran it before this amendment, with no clean-tree or note check; a failure stops. Any value of `RELEASE_GATE` other than `evidence` or `local` stops the target with nothing built.
- **PB-REL-10 Tests (amended January 2026, the gate's evidence).** `scripts/release/test.sh` runs its earlier tests with `RELEASE_GATE=local`, and adds, over a throw-away repository and a clone of it standing in for the test computer: a remote that cannot be fetched (stops, runs no gate); `gate-record` on a tree with an untracked file (stops, runs no gate), with `make check` failing (writes no note), and passing (runs `make check`, writes a `check passed <time>` note); `release-check`, `release-build`, the Windows build on the Mac and `make release` on the recorded commit (each passes without running `make check`); a changed tracked file and an untracked file (each stops, naming the file, with nothing built); a later commit with no note (stops naming the commit and `make gate-record`, nothing built); `RELEASE_GATE=local` on that commit (runs `make check`), with `make check` failing (stops); and another `RELEASE_GATE` value (stops, runs no gate).

#### 6.5 amendment, January 2026 — Windows programs need no runtime the PC may lack

> **Status:** Approved by the user 2026-01-29 ("proceed with recommendations"). Source: a check on a clean Windows 11 computer on 29 January 2026: the Windows installer installed, but `ordin.exe` and `ordin-hook.exe` stopped at once with status 0xC0000135 (a DLL was not found). `ordin.exe` imported `MSVCP140.dll` and `MSVCP140_1.dll` (which need `VCRUNTIME140.dll`), `ordin-hook.exe` imported `VCRUNTIME140.dll`, and a fresh Windows has none of them. The user's decisions of 29 January 2026: link Microsoft's C and C++ runtime statically into ordin's Windows programs; where a C++ dependency, notably ONNX Runtime, cannot link that way, ship the runtime DLLs beside `ordin.exe` instead, taken afresh for each release; ship ONNX Runtime's own `DirectML.dll` beside `ordin.exe`; keep the per-person install that needs no administrator rights (DSK-UP-18 unchanged); and check at each release that the built programs need no Visual C++ runtime DLL the installer does not carry. It amends PB-REL-6 and PB-REL-19 (the Windows `--config`, a step before and a step after the Tauri build) and PB-REL-10; every other rule of PB-REL-1 to PB-REL-23 holds unchanged.
>
> Evidence read: `apps/desktop/build.rs`, `apps/hook/Cargo.toml`, `Cargo.toml` (`ort` at `=2.0.0-rc.15` with no default features; `fastembed` with `ort-download-binaries-native-tls`), `Cargo.lock` (`static_vcruntime` 3.0.0 through `tauri-build`), `scripts/release/build.sh`, `scripts/release/lib.sh`, `release/release.conf`. ort-sys 2.0.0-rc.15's build script links ONNX Runtime 1.31.0 statically from its prebuilt archive for `x86_64-pc-windows-msvc` with DirectML, which holds `onnxruntime.lib` and `DirectML.dll` and is linked against `DirectML`, `D3D12`, `DXGI`, `DXCORE` and `dxguid`. Every object in that `onnxruntime.lib` is compiled with `RuntimeLibrary=MD_DynamicRelease` and asks for `MSVCRT` and `msvcprt`, the C and C++ runtime as DLLs. Its `DirectML.dll` is version 1.15.4 (SHA-256 `9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1`), byte for byte the `bin/x64-win/DirectML.dll` of Microsoft's package `Microsoft.AI.DirectML` 1.15.4. Tauri's build step (tauri-build 2.7.4) already links the Visual C++ runtime statically and the Universal C runtime dynamically for the desktop program (`static_vcruntime`). Two trials on 2026-01-29, cross-building with cargo-xwin: (1) `-C target-feature=+crt-static` for the whole workspace does not link `ordin.exe`: Tauri's build step excludes the static Universal C runtime the flag asks for, and the link fails on undefined C functions (`strlen`, `round` and others); (2) linking `ordin.exe` against Microsoft's static C++ standard library (`libcpmt.lib`) in place of `msvcprt.lib` fails on the linker's runtime check: `/failifmismatch: mismatch detected for 'RuntimeLibrary'`, ONNX Runtime's objects `MD_DynamicRelease` against `libcpmt.lib`'s `MT_StaticRelease`. ONNX Runtime's prebuilt library therefore cannot link with a static C++ runtime, and the user's fallback applies to `ordin.exe`. The newest x64 Visual C++ redistributable package the Visual Studio 2022 release channel (`https://aka.ms/vs/17/release/channel`) listed on that day was `Microsoft.VC.14.44.17.14.CRT.Redist.X64.base`, version 14.44.35211; its `msvcp140.dll` imports `VCRUNTIME140.dll` and `VCRUNTIME140_1.dll`, its `msvcp140_1.dll` imports `MSVCP140.dll` and `VCRUNTIME140.dll`, and its `vcruntime140_1.dll` imports `VCRUNTIME140.dll`.

**Assertions.**

- **PB-REL-24 Windows programs need no runtime the PC may lack.** On Windows the ordin programs start on a PC with nothing installed but Windows 10 or 11 (and WebView2, as before this amendment). The Universal C runtime (`ucrtbase.dll` and the `api-ms-win-crt-*` names) is part of Windows and is used as a DLL. (1) `ordin-hook.exe` carries the Visual C++ runtime inside it: its build step (`apps/hook/build.rs`) makes the split Tauri makes for the desktop program, the Visual C++ runtime static and the Universal C runtime dynamic, with `static_vcruntime` 3.0.0, so it imports no `MSVCP140*.dll` and no `VCRUNTIME140*.dll`. (2) `ordin.exe` keeps Tauri's split for its own code. ONNX Runtime's prebuilt library cannot link with a static C++ runtime (the evidence above), so `ordin.exe` imports `MSVCP140.dll` and `MSVCP140_1.dll`, and the installer carries them and the two DLLs they need, `VCRUNTIME140.dll` and `VCRUNTIME140_1.dll`, beside `ordin.exe` (PB-REL-25). Windows looks for a program's DLLs in the program's own folder first, so the copies beside `ordin.exe` are used whether or not the PC has a Visual C++ redistributable installed, and nothing is installed system-wide. `-C target-feature=+crt-static` is not used for any Windows program. Both build steps apply to a build on Windows and to the cargo-xwin build on the Mac alike, because they live in the programs' build scripts and not in the release scripts.
- **PB-REL-25 The files beside `ordin.exe`.** Every Windows installer carries five files in the install folder, beside `ordin.exe`: `DirectML.dll`, `msvcp140.dll`, `msvcp140_1.dll`, `vcruntime140.dll` and `vcruntime140_1.dll`. `release-build` and `release-build-windows`, for a Windows platform: (1) before the gate, stop, with nothing built, when the platform is not `windows-x86_64` (the files are chosen for that platform only), when any of the settings `RELEASE_WINDOWS_DIRECTML_URL`, `RELEASE_WINDOWS_DIRECTML_SHA256` and `RELEASE_WINDOWS_VC_CHANNEL_URL` is empty, when an address does not begin with `https://` (`file://` under PB-REL-5's test allowance), when the SHA-256 is not 64 lowercase hexadecimal characters, or when `curl` or `unzip` is missing; (2) after the gate and before `make desktop-build-inputs` (or `desktop-build-inputs-windows`), empty and fill the folder `apps/desktop/windows-runtime` (ignored by git). `DirectML.dll` is the member `bin/x64-win/DirectML.dll` of the package at `RELEASE_WINDOWS_DIRECTML_URL`, and its SHA-256 must equal `RELEASE_WINDOWS_DIRECTML_SHA256`. The four Visual C++ runtime DLLs come from the Visual Studio channel at `RELEASE_WINDOWS_VC_CHANNEL_URL`: the build reads the channel's `Manifest` item, takes from that manifest the package whose id matches `Microsoft.VC.<version>.CRT.Redist.X64.base` with the highest `version` (compared number by number), downloads its one payload, checks its SHA-256 against the payload's `sha256` in the manifest, and takes each DLL from the member that ends `/x64/Microsoft.VC<n>.CRT/<name>`; the build log names the version and the package's SHA-256. So each release carries the newest runtime Microsoft publishes on that channel. Each download is kept in `RELEASE_DOWNLOAD_CACHE_DIR` (default `~/.cache/ordin-release`) under its SHA-256 and reused while the fingerprint matches; a download that fails, a package without the member, or a fingerprint that differs stops the build, keeping nothing, before the Tauri build. (3) The Windows `--config` of PB-REL-6 and PB-REL-19 gains, in `bundle.resources`, the five entries `windows-runtime/<name>` → `<name>`, which Tauri's NSIS installer puts in the install folder beside `ordin.exe`; the macOS `--config` names none of them. `release/release.conf` holds `RELEASE_WINDOWS_DIRECTML_URL=https://www.nuget.org/api/v2/package/Microsoft.AI.DirectML/1.15.4`, `RELEASE_WINDOWS_DIRECTML_SHA256=9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1` (the DirectML version ONNX Runtime's prebuilt library ships with; both change when that version changes) and `RELEASE_WINDOWS_VC_CHANNEL_URL=https://aka.ms/vs/17/release/channel`. These files are Microsoft's redistributable runtime files, shipped by the user's decision of 29 January 2026; PB-REL-18's rule covers the files cargo-xwin downloads into its cache (the Windows SDK and the C runtime's libraries and headers), none of which is among the five, and the leak check is unchanged.
- **PB-REL-26 The release check of the programs' imports.** After the Tauri build and before the installer is copied to `RELEASE_OUT_DIR`, `release-build` and `release-build-windows` list the DLLs each Windows program imports with `llvm-objdump -p` or `objdump -p` (the first found on the Windows steps' `PATH`, else the `llvm-objdump` of the pinned toolchain's `llvm-tools-preview` component) and stop, naming the file and the DLLs and keeping no installer, when: `apps/desktop/binaries/ordin-hook-<target triple>.exe` imports any DLL whose name matches `MSVCP140*.dll` or `VCRUNTIME140*.dll` in any case; or `ordin.exe` (beside the bundle folder) or one of the five files of PB-REL-25 imports such a DLL that is not one of those five files. A program whose imports cannot be read, or a missing program, also stops the build. On success the build log says the Windows programs need no Visual C++ runtime beyond the files beside `ordin.exe`.
- **PB-REL-10 Tests (amended January 2026, the files beside `ordin.exe`).** `scripts/release/test.sh` adds, with stand-ins for `llvm-objdump` (a program's imports are lines of the stand-in file) and `unzip`, and a stand-in DirectML package, Visual Studio channel, manifest (an older and a newer x64 redistributable, a non-base and an ARM64 package) and redistributable package (with a debug copy of `msvcp140.dll`) at `file://` addresses: `release/release.conf`'s three values; a cross build and a native Windows build that stage the five files and nothing stale, map each beside `ordin.exe` in the `--config`, take the x64 `DirectML.dll`, the newest base package and the release (not debug) `msvcp140.dll`, name the version, check the imports and keep both downloads in the cache; a macOS `--config` naming none of them; a native build that uses the cache when both packages are gone; with nothing cached, an unreadable DirectML package and an unreadable redistributable package (each stops, building nothing); a `DirectML.dll` with another fingerprint (stops, nothing cached); a package without the x64 `DirectML.dll`; a redistributable whose fingerprint differs from the manifest's (stops, building nothing); a channel naming no manifest; a manifest with no x64 redistributable; a redistributable without `vcruntime140_1.dll`; each of `RELEASE_WINDOWS_DIRECTML_SHA256` and `RELEASE_WINDOWS_VC_CHANNEL_URL` empty (stops, runs no gate); an `http://` DirectML address; `windows-aarch64` (stops, builds nothing); a program importing `MSVCP140_2.dll` (stops, keeps no installer); a hook importing `vcruntime140.dll`; a cross build whose program imports `VCRUNTIME140_threads.dll`; and a program importing Windows' own `msvcrt.dll` (passes).

#### 6.5 amendment, January 2026 — the journeys' evidence

> **Status:** written under the user's decision of January 2026 ("proceed with recommendations", plan 04 Phase 7): the release build refuses a commit on which the journeys (§7.12) have not passed, as it refuses one on which the gate has not. It adds to PB-REL-21 and PB-REL-23 and amends PB-REL-10; every other rule of PB-REL-1 to PB-REL-26 holds unchanged.
>
> Evidence read: `scripts/release/lib.sh` (`require_gate_evidence`, `release_gate`), `scripts/journeys-windows.sh`, `apps/desktop/journeys/playwright.config.ts` (the JSON report at `run/results.json`), `Makefile` (`journeys-windows`).

- **PB-REL-27 The journeys' evidence.** With `RELEASE_GATE` unset or `evidence`, after the gate's note of PB-REL-21 (3) is found, `release-check`, `release-build` and `release-build-windows` fetch `refs/notes/ordin-journeys` from the same remote into the same local ref, replacing it, and stop, naming the remote and git's last error line, when the fetch fails; then read the note on `HEAD` in that ref and go on only when its first line begins `journeys passed `, printing the commit and the line; otherwise they stop naming the commit and saying to run `make journeys-windows COMMIT=<commit>` on the test computer. Every stop builds nothing. With `RELEASE_GATE=local` (PB-REL-23) no note is read; after `make check` passes the target prints a warning that nothing shows the journeys passed on the commit.
- **PB-REL-28 Recording the journeys.** `make journeys-windows` (`scripts/journeys-windows.sh`) builds `COMMIT` (default `HEAD`) on the Windows build machine with the `journeys` switch (OrdinDesktop.md DSK-JRN-1) and runs every journey of §7.12 against it. When Playwright exits 0 and `GREP` is unset, it writes, replacing any earlier note on that commit, one note in `refs/notes/ordin-journeys`: `journeys passed <UTC time, YYYY-MM-DDTHH:MM:SSZ> windows, <number of passed journeys from run/results.json, or ?> journeys`, made as `ordin-gate <ordin-gate@localhost>` when the repository has no `user.email`. A failed run, or a run of some journeys only (`GREP` set), writes no note. It pushes nothing, as PB-REL-22.
- **PB-REL-10 Tests (amended January 2026, the journeys' evidence).** `scripts/release/test.sh` adds, with a stand-in for the SSH command to the Windows build machine: a commit whose gate passed but which has no journeys note stops `release-check` and `release-build`, naming `make journeys-windows`, with nothing built; failed journeys, and journeys run with `GREP`, write no note; a full passing run builds the commit with the `journeys` switch and writes the note; `release-check` then passes and names the note.

#### 6.5 amendment, January 2026 — the Windows build started from this computer

> **Status:** written under the user's request of January 2026 ("can the commands not be done by a make target?"). Source: building the Windows installer meant copying the update key to the Windows build machine, running the build there with the password, taking the key away and copying the files back by hand. Every step in PB-REL-6 and PB-REL-15 stays in force; the Windows computer stays the place the real feed's Windows installer is built.
>
> Evidence read: `scripts/journeys-windows.sh` (how the Windows machine is reached), `scripts/release/release.sh` (the key and password check), `scripts/release/lib.sh` (`sign_file`), `scripts/release/publish.sh` (the real feed refuses an installer built on the Mac).

- **PB-REL-29 `make release-build-windows-remote`.** `scripts/release/build-windows-remote.sh` builds the Windows installer for `VERSION` from the commit this repository is at (`HEAD`), on the Windows build machine reached as `make journeys-windows` reaches it (`RELEASE_WINDOWS_SSH`, default `JOURNEYS_WINDOWS_SSH`, else `ssh -p 2222 ordin@127.0.0.1`; `RELEASE_WINDOWS_REPO`, default `/c/o`; `RELEASE_WINDOWS_REMOTE`, default `server`). In order: (1) the version is valid and is the package's; the key is `TAURI_SIGNING_PRIVATE_KEY` (default `~/ordin-update.key`) and the password is `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` or asked for on the terminal without echo; the key signs a small file that verifies against `release/ordin-update.pub`, as PB-REL-3 does, so a wrong password stops here with nothing sent (the check is `check_signing_key` in `lib.sh`); (2) the Windows machine fetches its remote and must hold the commit, else the script stops naming the commit and saying to push it, with nothing built; (3) the key goes to `~/.ordin-release-key` on the Windows machine, written under `umask 077`; (4) the build runs there: the commit is checked out, the tree is cleaned (`node_modules` and `target` kept), `target/release-out/<VERSION>` is removed, and `make release-build VERSION=<VERSION>` runs with the key file named and the password passed through standard input, never as an argument, with `RELEASE_GATE_REMOTE` (default `server`) naming where its records are read; (5) the key file is removed from the Windows machine when the build ends, whether it passed or failed, and a failure to remove it says so and names the file; (6) the installer, its `.sig` and `.minisig`, `SHA256SUMS.windows-*`, `platform-windows-*.json` and `build-windows-*.log` are copied into `target/release-out/<VERSION>` here; each installer must carry a signature that `minisign -Vm` verifies against `release/ordin-update.pub`, the hashes must match, and `leak-check.sh` passes over the folder. Uncommitted changes in this repository are not part of the build, and the script says so. The real feed takes the installer, as it was built on Windows (PB-REL-15, `publish.sh`).
- **PB-REL-10 Tests (amended January 2026, the Windows build started from this computer).** `scripts/release/test.sh` adds, with the stand-in for the SSH command and the stand-in signer: a bad version is refused; a wrong password stops before anything reaches the Windows machine; a commit the Windows machine cannot read builds nothing; a successful run puts the key there under `umask 077`, builds the commit with `make release-build`, removes the key and brings the files back; the password is in no logged command; a failed build still removes the key and copies no file; a file that comes back without its signature is refused.

#### 6.5 amendment, January 2026 — trying a build by hand on Windows

> **Status:** written under the user's request of January 2026 ("I should be able to build without going through all the tests, so I can smoke test and get early feedback"). Source: `make local-build` builds an unsigned installer of the working tree for the computer it runs on, with no gate, key or records, but only on that computer; the Windows installer could only be made through the release build, which refuses a commit without the gate's and the journeys' records.

- **PB-REL-30 `make local-build-windows-remote`.** `scripts/release/local-build-windows-remote.sh` builds, on the Windows build machine reached as PB-REL-29 reaches it (`RELEASE_WINDOWS_SSH`, `RELEASE_WINDOWS_REPO`, `RELEASE_WINDOWS_REMOTE`), the unsigned installer of this repository's working tree, as `make local-build` does for the computer it runs on. In order: (1) the tree's tracked files are recorded as a commit by `git stash create` (HEAD when nothing differs); files not yet added to git are not part of it and the script names up to three; (2) that commit is pushed to the repository the Windows machine fetches from (`RELEASE_WINDOWS_PUSH`, default `testbox`) under the branch `local-build`, replacing it; (3) the Windows machine stops any `ordin.exe`, fetches, checks the commit out, cleans the tree (`node_modules` and `target` kept), installs the screens' packages and runs `make local-build` with `AGENTS` as given; (4) the installer comes back into `target/local-build/windows/` here. It asks for no key and no password, runs no gate and no journeys, checks no record, makes no update file and publishes nothing; its last lines say the installer is not signed and no record says it passed anything. `make release` and `make release-build` still refuse a commit without its records (PB-REL-21 to PB-REL-23).
- **PB-REL-31 The GitHub App client id** (amended January 2026; user decision; Approved by the user January 2026). `release/release.conf` holds `RELEASE_GITHUB_APP_CLIENT_ID`, empty by default: the client id of a GitHub App the maintainer registered on their own GitHub account with its device flow enabled; it is public, not a secret. `make local-build`, `make release-build` (and `make release`, which runs it) call `github_app_env` (`scripts/release/lib.sh`) before the build: a value of letters, digits, `.`, `_` and `-` (at most 128) is exported as `GITHUB_APP_CLIENT_ID`, which the desktop program compiles in; any other value stops the build with a sentence naming the setting and builds nothing; an empty value removes `GITHUB_APP_CLIENT_ID` from the build's environment, and the build says that people connect GitHub with a personal access token (ScreensModule.md SM-NS-2.6). `make local-build-windows-remote` and `make release-build-windows-remote` check the value the same way and pass it to the Windows machine's `make` as `RELEASE_GITHUB_APP_CLIENT_ID`. The release scripts' tests (PB-REL-10) cover an empty value, a value that reaches the build, a refused value, and both Windows scripts.
- **PB-REL-10 Tests (amended January 2026, trying a build by hand on Windows).** `scripts/release/test.sh` adds, with the stand-in for the SSH command: a build of the working tree sends a commit to the Windows machine's repository and runs `make local-build` there with no key, no password and no gate; the installer comes back into the local build folder; a failed build there fails the script and brings nothing back; an `AGENTS` value other than `commit`, `worktree` and `none` is refused before anything is sent.

- **PB-REL-32 A test build with no gate** (amended 2026-02-05; user decision 2026-02-05, "the test channel should be able to build without this restriction"; Approved by the user 2026-02-05). `RELEASE_GATE=none` is accepted when `RELEASE_CHANNEL` is `test` (its default): in the gate's place of PB-REL-6 and PB-REL-19, `release-check`, `release-build` and `release-build-windows` stop when the tree is not clean, as PB-REL-21 (1) states, then run no test, fetch and read no record of PB-REL-21 or PB-REL-27, and print a warning naming the commit and saying no test was run or checked. With `RELEASE_CHANNEL=real` it stops, with nothing built, saying a real release needs the gate and the journeys. The committed `release/release.conf` does not set it. `release-build-windows-remote` (PB-REL-29) checks `RELEASE_GATE` (default `evidence`; only `evidence`, `local` or `none`) and `RELEASE_CHANNEL` (default `test`) the same way before anything is sent, and exports both to the build on the Windows machine. PB-REL-23's list of accepted values gains `none`; PB-REL-10's tests gain: `none` passes on the test channel with no record and runs no gate, and is refused on the real channel.
- **PB-REL-33 Signatures are not searched for the key** (amended February 2026; user decision; Approved by the user February 2026). PB-REL-9's check does not search signature files (`*.sig`, `*.minisig`), and searches a `*.json` file with the value of every `"signature"` field emptied; every other byte of every file is still searched. A signature is made with the key to be published, and its random text can hold a short password or key line by chance (a 0.2.1-test.6 build was stopped this way). PB-REL-10's tests gain: a signature file and a feed signature holding the password pass; the password elsewhere in the feed fails.
- **PB-REL-34 The key's password from the macOS keychain** (amended February 2026; user decision; Approved by the user February 2026). When `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is unset and `RELEASE_KEY_KEYCHAIN` names a keychain item's service, `make release` and `make publish` read the password with `security find-generic-password -a "$USER" -s <service> -w` before PB-REL-3's prompt, never print it, and go on to PB-REL-3's key check; an item that cannot be read stops them with nothing built, naming the item and the `security add-generic-password` line that adds it. `release/release.conf` does not set it. PB-REL-10's tests gain: the password read from the keychain with no terminal, never printed; an unreadable item stops with nothing built.
- **PB-REL-35 Moving the key's password into the keychain** (amended February 2026; user decision; Approved by the user February 2026). `make release-key-keychain` (`scripts/release/key-keychain.sh`) needs a terminal and changes nothing without one. It decodes the key named by `TAURI_SIGNING_PRIVATE_KEY` (default `~/ordin-update.key`) into a private temporary folder, has `minisign -C` change its password (the person types the current and the new one at the terminal), stores the new password with `security add-generic-password -U` under the service in `RELEASE_KEY_KEYCHAIN` (default `ordin-update-key`, typed again by the person), and replaces the key file only after the new key and the keychain's password sign a file that verifies against `release/ordin-update.pub`; the old key is kept once as `<key>.bak`. It adds `export RELEASE_KEY_KEYCHAIN=<service>` to `~/.zshrc` when no such line exists. No password passes through its arguments, environment or output; the temporary folder is deleted on exit. Any failed step stops it with the key file unchanged.
- **PB-REL-36 The Microsoft client id** (amended February 2026, from ADR-126; user decision; Approved by the user February 2026). `release/release.conf` holds `RELEASE_MICROSOFT_CLIENT_ID`, empty by default: the client id of a multi-tenant Microsoft app registration the maintainer owns (a public client with a loopback redirect and the SharePoint Adapter's delegated permissions); it is public, not a secret. It is checked and passed exactly as PB-REL-31's value, by `microsoft_app_env` (`scripts/release/lib.sh`) beside `github_app_env`: letters, digits, `.`, `_` and `-` (at most 128) are exported as `MICROSOFT_CLIENT_ID`, which the desktop program compiles in (IdentityAndCredentials.md ID-MS-BUILT-1); any other value stops the build with a sentence naming the setting; an empty value removes `MICROSOFT_CLIENT_ID` from the build's environment and the build says that Microsoft 365 needs a company profile. Both Windows scripts check it and pass it as `RELEASE_MICROSOFT_CLIENT_ID`. PB-REL-10's tests gain the same four cases as PB-REL-31.

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
- Programs run unmodified as separate programs — Claude Code (E2), the Copilot CLI (the person's own install, never bundled; *amended 2026-02-02, ADR-123; Approved by the user 2026-02-02*), `jdtls`, `gh`, `glab`, the ArtCraft apps — are outside both checks and recorded in HLD 05 with their licences (LicenceCheck.md *Internal Logic* 6).

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
| C-46 | Per-user install, update and removal with no administrator; the install choice | 59 Setup Assistant, 72 ordin desktop |
| C-108 (*added January 2026, ADR-072*) | An update the app downloads itself installs with no system warning | 59 Setup Assistant, §6.5 Release pipeline (fallback: none; the install guide then gives the steps for each update) |
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
| C-63 | SQLite full-text ranking matches the earlier implementation's gap-check results | 50 Catalogue Service |
| C-64 | A pack changed mid-session takes effect next turn | 13 Agent Folder Manager, 31 Installers, 55 Package Catalogue |
| C-66 | The earlier implementation ignores `withdrawn.yaml` | 50 Catalogue Service |
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
| C-108 (*added January 2026 (amended January 2026; user decision; Approved by the user January 2026)*) | `whisper-rs` passes the licence gate, or the user accepts it as a named exception | 80 Transcription Service (fallback: the crate's own declarations of the whisper.cpp functions, whisper.cpp built from a pinned source archive) |
| C-109 (*added January 2026 (amended January 2026; user decision; Approved by the user January 2026)*) | The two addresses of `speech-models.json` serve the files with the listed SHA-256 and size over `https`, with no sign-in | 80 Transcription Service (fallback: the `minutes` switch stays off, §9 item 6) |
| C-110 (*added January 2026 (amended January 2026; user decision; Approved by the user January 2026)*) | The time budgets of SA-MIN-03 and a word error rate of at most 20 % on the 60-second fixture hold on both reference machines | 80 Transcription Service, 82 Minutes Service (fallback: SA-MIN-03 restated from the measurement; the default model stays the larger one) |
| C-111 (*added January 2026 (amended January 2026; user decision; Approved by the user January 2026)*) | OBS 28 to 31 accept the profile parameters and input kinds of ObsRecorderAdapter.md §C and record a two-track Matroska file | 81 OBS Recorder Adapter (fallback: one mixed track, no speaker labels) |
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

### 7.12 The screens against the backend, and the journeys (*added January 2026*)

Four checks keep the screens honest against the real backend. The first three are part of `make check`, and so of the gate of PB-REL-21; the fourth is the evidence of PB-REL-27.

1. **The screens' requests** (`make ui-contract-check`). The screen tests record every request they send to their stand-in backend; `ordin-service-api-check-requests` runs each through the Service API's own input rules (`check_request`: the command is known, the input normalises and matches its schema) and fails on any refusal not listed, with a reason, in `apps/desktop/ui/tests/support/refused-on-purpose.txt`.
2. **No control does nothing** (`apps/desktop/ui/tests/dead-controls.test.ts`). Every registered screen is opened over sample data whose replies match the result schemas, and every element a person can operate is clicked; each click sends a request, changes the open screens, opens a dialog or menu, or changes the page. A disabled control carries a reason a person can read. Exceptions are listed, with a reason, in `apps/desktop/ui/tests/support/inert-on-purpose.txt`.
3. **A first start with the agents that ship** (`make first-start-check`, `apps/desktop/tests/first_start.rs`). The real core starts on an empty data root with the catalogue taken from `RELEASE_BUNDLED_AGENTS_SOURCE` at `RELEASE_BUNDLED_AGENTS_COMMIT`; both bundled agents install and are listed, and no warning or error is logged beyond those listed, with a reason, in `apps/desktop/tests/fixtures/first-start-expected.txt`. A missing agents checkout fails the check.
4. **The journeys** (`make journeys-windows`, `apps/desktop/journeys`). Playwright drives the real window of a `journeys` build on the Windows build machine, attached over WebView2's debugging port, through nine journeys (`apps/desktop/journeys/journeys/`): getting ready on a first start; adding a workspace and its documents; opening each kind of document; a task with Ordin given to an agent; approving a change sent back to an online document; recovery after a stop mid-task; remembering a fact; the settings and updates; and keeping or setting aside data a different version left. Claude Code is the real one; the model and the online services are stand-ins on 127.0.0.1, so nothing leaves the machine. A journey fails on any warning or error the program logs that it does not expect. A data root the run did not make is renamed and kept, never deleted.

#### 7.12 amendment, January 2026 — What people see, and Ordin's judgement on demand

> **Status:** Approved by the user, January 2026 ("approve"; decisions of January 2026: no evals in the release gate; the maintainer tests by hand before each release; a coordinator eval run started by hand). Source: the person's own use of the first release found what the checks above could not: they drive Ordin with a scripted stand-in model whose replies are plain text, so neither Ordin's Markdown nor Ordin's own choices were ever exercised.

- **PB-CHK-1 The first-start check** (item 3) also requires that the document agent serves the workspace the check creates (PackageCatalogue.md PKG-DOC-2, OrdinDesktop.md DSK-DOC-1).
- **PB-CHK-2 The journeys** (item 4) are ten: journey 10 asks Ordin about a PDF in a linked folder on a first start (the attachment line names its folder, Ordin delegates a brief with no command to the document agent, the person ticks the folder on Ordin's access card, and the agent's summary is shown formatted). The stand-in model's replies are written in Markdown, as the real model writes. Journey 04 asserts the working line (ScreensModule.md SM-WK-1). `make journeys-windows` stops every `ordin.exe` it started on the Windows machine when the run ends, whether it passed or failed, so a later build on that machine finds no file in use.
- **PB-CHK-3 Ordin's judgement, on demand** (`make coordinator-evals`). Not a release gate and recorded nowhere: the maintainer runs it when the coordinator's instructions or the model change. It runs each evaluation under `models/@default/evals/coordinator/` of the agents repository at `RELEASE_BUNDLED_AGENTS_COMMIT` against the real model, through the real program on the Windows build machine with Claude Code signed in: a workspace is made from the evaluation's `context`, its `prompt` is sent to Ordin, and the tool calls and the reply are recorded. Each evaluation runs three times. A run passes when every `toolsRequired` tool was called, no `toolsForbidden` tool was called and no `containsNone` text appears in the reply; the `pass` predicate is printed beside the recorded calls and reply for the maintainer to judge. The output is a table of evaluation, run, pass or fail, and the reason.
- **PB-CHK-4 An evaluation states its starting workspace** *(Approved by the user, January 2026, "proceed with recommendations")*. PB-CHK-3's "a workspace is made from the evaluation's `context`" is made exact. An evaluation that the runner runs carries `input.setup`, read by the runner and ignored by the Checker's other rules:
  - `agents`: a list of agent identifiers (for example `urn:ordin:default:workspaces:document-agent`), the workspace holds exactly these agents besides Ordin: each is installed from the bundled catalogue before the prompt is sent, and any other agent the start sequence installed (DSK-DOC-1) is removed first, through the same operation as the Agents screen's **Remove…**. An identifier the bundled catalogue does not hold stops the run with the reason "agent not in the catalogue: <identifier>".
  - `folders`: a list of `{ name, files: [{ path, text }] }`; each is written under a temporary folder, linked to the workspace as a place called `name`, and each file is created with `text` as its content (an `.pdf` file is created with the text as a one-page PDF).
  - `grants`: a list of `{ agent, folders: [name] }`; each listed agent is granted read access to each listed folder before the prompt is sent. An agent absent from `grants` has no grant (OH-ACC-1's card is then the expected behaviour).
  - `attach`: an optional `{ folder, path }`; the prompt is sent with that file attached, so Ordin's turn carries the attachment line of OH-ATT-1.
  - `context` stays as prose for the maintainer reading the result; the runner does not parse it.
  An evaluation with no `input.setup` is listed in the table as "not run: no setup" and does not fail the run. The coordinator evaluations that name agents the catalogue does not hold (`delegates-to-the-fitting-agent`, `asks-only-what-it-cannot-answer`, `proposes-a-crew-when-no-agent-covers-the-work`) are listed that way until they are given a setup from catalogue agents. The three evaluations of PB-CHK-3's source change (`finds-a-reading-agent-without-asking`, `briefs-an-agent-with-no-commands`, `reports-a-task-that-could-not-start`) each carry a `setup`. An evaluation states everything it needs in `input.prompt` and `setup`; it never relies on an earlier conversation or an earlier run that the runner cannot replay.

  ```gherkin
  Scenario: Happy path — an evaluation's setup becomes a workspace (PB-CHK-4)
    Given an evaluation whose setup installs the document agent, links a folder "docs" holding "plans/strategy.pdf" and attaches that file
    When the runner runs it
    Then the workspace has the document agent installed, the folder "docs" linked and no grant
    And Ordin's turn carries "Attached: strategy.pdf (in docs: plans/strategy.pdf)"

  Scenario: Error path — an agent the catalogue does not hold (PB-CHK-4)
    Given an evaluation whose setup names an identifier the bundled catalogue lacks
    When the runner runs it
    Then the run is not started and the table reads "agent not in the catalogue: <identifier>"

  Scenario: Edge — an evaluation with no setup (PB-CHK-4)
    Given an evaluation with no input.setup
    When the runner runs the set
    Then its row reads "not run: no setup" and the set's other rows are unaffected
  ```

#### 7.12 amendment, January 2026 — A task's form (ADR-119)

> **Status:** Approved by the user January 2026. Source: HLD 09 ADR-119 (decision 101). The amendments are AgentModel.md AM-TF-1 to AM-TF-6, FormatPlugins.md FP-TF-1 to FP-TF-3, Checker.md CHK-TF-1 and CHK-TF-2, Installers.md INST-TF-1, SessionSupervisor.md SUP-TF-1, PackageInspector.md PKI-TF-1, CatalogueService.md CAT-TF-1 to CAT-TF-6, AgentLoader.md AL-TF-1, PackageCatalogue.md PKG-TF-1, OrdinHost.md OH-TF-1 to OH-TF-4, ServiceApi.md SVC-TF-1 and SVC-TF-2, 2C wire WIRE-TF-1 and WIRE-TF-2, and ScreensModule.md SM-AT-2 and SM-TF-1 to SM-TF-8. No build switch: the change is part of the core.

- **PB-TF-1 The steps that run again,** each with its own gate of §4, in dependency order: Step 3 (Service API: `runs.start_task`, person only, the restated `TaskForm` and `TaskInput`, the form block's `task` and `Documents`), Step 8 (Agent Model), Step 14 (Format Plug-ins), Step 30 (Checker), Step 31 (Installers), Step 32 (Session Supervisor), Step 37 (Package Inspector), Step 50 (Catalogue Service: no code; its scenarios run over the new bundled catalogue), Step 52 (Agent Loader: no code; its scenarios are unchanged), Step 55 (Package Catalogue), Step 57 (Ordin Host), then the Screens module (Step 71) and the desktop (Step 72). The bundled catalogue's files change as CatalogueService.md CAT-TF-1 to CAT-TF-6 fix them, in the agents repository at the commit `RELEASE_BUNDLED_AGENTS_COMMIT` names; the commit is moved in the same change.
- **PB-TF-2 The first-start check** (item 3) also requires that the document agent of the workspace the check creates lists four task forms, titled as CAT-TF-1 titles them and each `startable`, and that the check's ingest of the bundled catalogue reports no `task_title_invalid`, `task_inputs_invalid` or `task_body_invalid`.
- **PB-TF-3 The journeys** (item 4) gain journey 11: on a first start, the person opens the document agent under **Agents** in the tree, presses **Summarise a document**, chooses a document in a linked folder, reads the access line, presses the verb, and the run's screen shows the summary; the conversation holds nothing from the form. The stand-in model's reply for the run names the new file.

  ```gherkin
  Scenario: Happy path — a task started from the tree (PB-TF-3)
    Given a first start with the document agent installed and the folder "docs" linked
    When journey 11 runs
    Then the run's screen shows the summary file and the conversation holds no message from the form
  ```

### 7.13 Facts as triples and the memory diagram (*added January 2026, ADR-118, ADR-120; Approved by the user January 2026*)

Lands after memory Step 6h (`memory/docs/spec/phase-5-playbook.md`). Touches `ordin-memory-service`, `ordin-host`, `ordin-knowledge`, `ordin-team-knowledge`, `ordin-service-api`, `ordin-types` and the desktop UI. No build switch is added; the change is in every build that carries memory.

- [ ] **Memory Service** (MemoryService.md amendment ADR-118, ADR-120): M-10 (component step 13 `0011_triples`), own step 14 `facts as triples`, MEM-MIG-6 table rows 13 and 14; `SEED_TERMS` (26 words); T1 `remember_facts` with MEM-TR-11's check of a new word; FV′; §9.1, K, L changes; §9.4 correction by object; C1, V1–V3, P1, G1; §8, R and AI passages; §12 tool schema and texts; C0 (conflicts across a workspace's own, every-workspace and team facts); E and F terms, with F's names for ids and its differing-word rule.
- [ ] **Ordin Host** (OrdinHost.md amendment ADR-118): `RememberArgs`, the Link words seed section, the fixed instructions and `FIXED_INSTRUCTIONS_VERSION = "1.1.0"`, step 15, OH-AM step 3 text, OH-TR-1, OH-TR-2 (a document source's working-copy version).
- [ ] **Knowledge Service** KS-TR-3 migration step; **Knowledge Reader** unchanged in code (KRD-TR-1); **Team Knowledge Service** TK-TR-1 to TK-TR-5 (format 2, `TeamTerm`, a colleague's word that differs).
- [ ] **Service API** SVC-TR-1, SVC-TR-2 (six rows, `PERSON_ONLY_COMMANDS`); **types** WIRE-TR-1 to WIRE-TR-6, regenerated `types.gen.ts`.
- [ ] **Screens** SM-SF-13 to SM-SF-24 (`FactLine`, the Words tab, What Ordin read, summaries, `MemoryDiagram.svelte` with `GraphChart` added to `loadEcharts`), the string table.
- [ ] **Gate:** `cargo test -p ordin-memory-service -p ordin-host -p ordin-knowledge -p ordin-team-knowledge -p ordin-service-api`; the Ordin Host tool-text test (description 1–120, `argument_form` 1–300, `plain` 1–200); the UI unit tests and the screens-against-backend journey of 7.12 extended with: remember two facts, settle a conflict, merge a word, open What Ordin read, centre the diagram; every test runs on the test server.

### 7.14 A team workspace (*added January 2026, ADR-121; Approved by the user January 2026*)

Source: decision 103; HLD revision 0.68.0, ADR-121 (items 9 and 10 settled after the spec review). The amendments are TeamKnowledgeService.md (amendment January 2026, TW-A to TW-J, and the build notes on its ADR-110 and ADR-112 amendments), WorkspaceService.md WS-TW-1 to WS-TW-9, PushService.md PS-TW-1 and PS-TW-2, HomeAndSearch.md HS-TW-1 to HS-TW-3, OrdinHost.md OH-TW-1 and OH-TW-2, OrdinDesktop.md DSK-TW-1 to DSK-TW-3, ServiceApi.md SVC-TW-1 to SVC-TW-4, 2C wire §18 (WIRE-TW-1 to WIRE-TW-4) and ScreensModule.md SM-TW-1 to SM-TW-12. The team workspace is built with the team-knowledge steps; it adds no build switch of its own and is present in every build that has `team-knowledge`, which the desktop program turns on by default (§9 item 2).

- **PB-TW-1 The steps built,** each with its own gate of §4, in dependency order: Step 3 (Service API: the ten commands, local only), Step 49 (Workspace Service, migration step 4), Step 51 (Push Service: scenarios only), Step 57 (Ordin Host), Step 58 (Home and Search: `OpenTarget::TeamGrant`, the two strings and the scenarios of HS-TW-1 to HS-TW-3; the program's `waiting_items` reading of waiting grants is built with Step 72), **Step 74 (Team Knowledge Service: its first build — the own folder and the team workspace, migration steps 1 to 4 — as Step 74's ADR-121 line states; this is new code, not a step run again)**, then the Screens module (Step 71) and the desktop (Step 72). Step 53 (Knowledge Service) does not run again: KS-TW-1 is withdrawn. The ADR-110 and ADR-112 amendments of the Team Knowledge Service are not built here (ADR-121 item 9).
- **PB-TW-2 The journeys** (item 4) gain journey 12, run against two installations on the test server sharing one test Git host: installation A shares a workspace with one SharePoint stand-in link, one local folder and the document agent as a team workspace and approves the card; installation B opens it, sees the local folder left out and the document agent added with its access; A runs **Summarise a document**, approves the results card; B gets and sees the entry in **Team activity** with the result opening from its results folder; A adds an agent with access to the link and shares; B gets, sees the access card, presses **Allow**; B leaves, and the team folder is unchanged.

  ```gherkin
  Scenario: Happy path — two people in one team workspace (PB-TW-2)
    Given installations A and B signed in to the test Git host as "asha-patel" and "priya-shah"
    When journey 12 runs
    Then B's Team activity lists "asha-patel ran Summarise a document with Document agent" with the summary opening on B
    And B's agent added by A holds no grant until B presses Allow
    And no file of the team folder holds conversation text, a task's inputs or a credential
  ```

- **PB-TW-3 Gate:** `cargo test -p ordin-team-knowledge -p ordin-workspaces -p ordin-push -p ordin-host -p ordin-home-search -p ordin-service-api`, the UI unit tests, and journey 12; `make doc-ref-check`; every test runs on the test server.

### 7.15 Meeting minutes (*added January 2026 (amended January 2026; user decision; Approved by the user January 2026)*)

Source: the user's decisions of January 2026 on meeting minutes from transcripts, recordings and OBS. The amendments are TranscriptionService.md, ObsRecorderAdapter.md and MinutesService.md (new), DictationService.md DICT-TRX-1, IdentityAndCredentials.md ID-OBS-1, JournalAndArtefactStore.md JA-MIN-1 to JA-MIN-3, OrdinHost.md OH-MIN-1 to OH-MIN-3, ServiceApi.md SVC-MIN-1 to SVC-MIN-4, OrdinDesktop.md DSK-MIN-1 to DSK-MIN-5, 2C wire §23 (WIRE-MIN-1 to WIRE-MIN-6), cross-cutting/ErrorHandling.md (43 codes) and Configuration.md (minutes settings), and ScreensModule.md SM-MIN-1 to SM-MIN-10. Everything is behind the build switch `minutes` (§9), except the Journal's migration step 4 and the ids of WIRE-MIN-1, which are in every build.

- **PB-MIN-1 The steps,** each with its own gate of §4, in dependency order: Step 2 (shared types: WIRE-MIN-1 to WIRE-MIN-6 in `ordin-types`, the §23 types beside `crates/ordin-types/src/wire/`), Step 5 (Journal: migration step 4 and the minutes methods), Step 7 (Identity: ID-OBS-1), Step 80, Step 81, Step 82 (new code), Step 3 (Service API: the twenty rows, `PERSON_ONLY_COMMANDS`, `WebPage::ObsDownload`, regenerated `types.gen.ts` and `schema.gen.json` in both a build with and one without `minutes`), Step 57 (Ordin Host: OH-MIN-1 to OH-MIN-3 behind its feature `minutes`), then the Screens module (Step 71) and the desktop (Step 72).
- **PB-MIN-2 The journeys** (item 4) gain journey 13, run on the macOS and the Windows test machines with a build that has `minutes`: import `meeting.vtt` and accept the notice; open the minutes, edit, save, reopen; export as Word; import `speech-60s.m4a`, press **Not now** on the model prompt and see nothing downloaded; with the fake OBS of Step 81 serving the WebSocket port, press **Record**, see the reminder and the indicator, press **Stop**, and see minutes arrive with no further input. A second run with a build without `minutes` checks that no Minutes row, Record button or Meeting minutes settings appear.

  ```gherkin
  Scenario: Happy path — minutes from a transcript and from a recording (PB-MIN-2)
    Given a build with the minutes switch and the fake OBS on port 4455
    When journey 13 runs
    Then two minutes pages are listed under Minutes, each with the headings Summary, Decisions, Actions and Open questions
    And the models folder is empty after Not now was pressed
  ```

- **PB-MIN-3 Gate:** `cargo test -p ordin-journal -p ordin-identity -p ordin-dictation --features ordin-dictation/transcription -p ordin-obs -p ordin-minutes -p ordin-host --features ordin-host/minutes -p ordin-service-api`, the UI unit tests, journey 13 on both machines, the grep of test logs and the data folder for the fixture OBS password (none found), and `make doc-ref-check`; every test runs on the test server and its Windows machine.
- **PB-MIN-4 Ordin's minutes tools** *(Approved by the user, 2026-11-21 ("approve as written"))*: Step 2 (WIRE-MIN-7, WIRE-RUN-SRV-1), Step 4's MCP Front (SA-MCP-SRV-01), the Session Supervisor and both local adapters (SUP-SRV-1, the copy into `SessionGrant`), Step 82 (MIN-TR-1 to MIN-TR-5), Step 57 (OH-MIN-4 to OH-MIN-11), Step 3 (SVC-MIN-2 amended, SVC-MIN-5, regenerated minutes files), Step 71 (SM-MIN-11), Step 72 (DSK-MIN-6). Gate: PB-MIN-3's, plus `cargo test -p ordin-mcp-front -p ordin-session-supervisor -p ordin-claude-adapter -p ordin-copilot-adapter`, a test that a minutes run's whole offer through the MCP Front is exactly `ordin/keep_minutes` while another provider offers Ordin sessions tools, and `cargo test -p ordin-host` without the feature.

- **PB-MIN-5 The speech model chosen by the machine** *(Approved by the user, 2026-11-22 ("approve as written"))*: Step 2 (WIRE-MIN-8), Step 80 (TRX-MACH-1 to TRX-MACH-4; `windows-sys` gains `Win32_System_SystemInformation` in `ordin-dictation`), Step 82 (MIN-MACH-1 to MIN-MACH-4), Step 3 (regenerated minutes files), Step 71 (SM-MIN-12). Gate: PB-MIN-3's, plus `cargo test -p ordin-dictation --features transcription` with the TRX-MACH-3 table run against `recommend`, and `cargo test -p ordin-minutes` with the four MIN-MACH scenarios.

- **PB-MIN-6 OBS loads Ordin's profile after its settings are written** *(Approved by the user, 2026-11-22 ("fix it, amend the spec"))*: Step 81 (ObsRecorderAdapter.md OBS-PRO-1 to OBS-PRO-3, OBS-STOP-1). Gate: PB-MIN-3's, plus `cargo test -p ordin-obs` with the three OBS-PRO scenarios against the fake OBS, which applies a profile's parameters only when the profile is selected, and one real recording on macOS and on Windows whose file is `.mkv` on the first `start` after the profile is created.

## 8. Greenfield note

This spec set is in greenfield mode. Every assertion is derived from the HLD and none is classified *Required*, *Observed*, *Not implemented* or *Gap*, so the Playbook has no remediation phase and no continuity phase: Steps 0 to 78, with E-1 and E-2, are one build in the order above. The ports of the earlier agent-machinery code and wrightrun code (Agent Model, Resolver and Compiler, Checker, Installers, Catalogue Service, Authoring Service, Rules Engine vocabulary, Caller API) are new code checked by the parity and conformance tests named in their steps, not carried-forward code.

*Amended January 2026 (ADR-114):* the build is still one dependency order. §9 fixes the build switches that let it ship in increments; which increment carries which step is planned in the maintainers' release roadmap, kept outside the repository, outside this spec set.

---

## 9. Build switches

*Added January 2026 (ADR-114). Draft until the Phase 6 audit of this amendment passes and the user promotes it.*

The steps of §4 are built in their dependency order, and every step is built whole (X1–X5). Which steps and switches reach people in which release is planned outside this spec set, in the maintainers' release roadmap kept outside the repository; this section fixes only the mechanism the plan uses, so that changing the plan changes no spec.

1. **Switches.** The programs `apps/desktop` and `apps/server` have Cargo features named after what they give the person: `sandbox`, `dictation`, `copilot`, `slack`, `mail`, `team-knowledge`, `authoring`, `handover` (turns on `mail` and `slack`), `telegram`, `hosted-agents`, `knowledge-agents` and `minutes` (turns on `dictation`; *amended January 2026 (amended January 2026; user decision; Approved by the user January 2026)*, desktop only), and on the server `personal-profile` and `agent-profile` (on the server `telegram` turns on `personal-profile`, which turns on `slack` because the Trigger Service takes the Slack Service). The crates each switch adds, and the rows of the programs' start that it constructs, are fixed in OrdinDesktop.md and OrdinServer.md (amendments January 2026). Every crate a switch does not name is always built.
2. **Defaults.** The specs set `default = []`. A shipped version's manifests list as default features the switches its release turns on (kept by the maintainers outside the repository). *Amended January 2026 (ADR-121; Approved by the user January 2026):* the desktop program sets `default = ["team-knowledge"]` (OrdinDesktop.md DSK-TW-1), because the team workspace is part of the desktop wherever it ships; every other switch, and every switch of the server, keeps `default = []`.
3. **What a build answers.** A backend answers only the command groups of the modules its build constructs and refuses the others with `SVC_NOT_IN_THIS_VERSION`; the screens show no entry for them (ServiceApi.md *Command groups by build switch*; ScreensModule.md SM-UP-1).
4. **Gates.** Every gate of §4 and §7 runs with the switches of the build being checked. `make check` also runs once with `--all-features`, so every switch's code stays built and linted; the Architecture Test reads the graph with `--all-features` (ArchitectureTest.md amendment January 2026), and `deny.toml` keeps `[graph] all-features = true`.
5. **A step and its switch.** A step whose crate a switch names may be built before that switch is turned on in any shipped version; its gate runs as in §4.
6. **A failed spike.** When a confirmation item that gates a whole step fails (C-17 for Step 41; C-81 for the agent profile), the switch that names the step stays off in every shipped version. No spec changes.
