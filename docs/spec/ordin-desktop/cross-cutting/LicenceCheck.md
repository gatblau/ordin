# SPEC: LicenceCheck

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-114; promoted to Approved by the user January 2026) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** build configuration, not a crate — `deny.toml` at the Cargo workspace root (*chosen here*; the file name `cargo-deny` reads by default), the `make check` target and the container image build (SA-LIC-01, HLD 05 — *Licence and vulnerability checks*), and the screens module's npm check under `apps/desktop/ui/` (SA-LAYOUT-01).

## Purpose

Fails the build when any dependency ordin ships carries a licence the Open Source Initiative does not approve, or the GPL or the AGPL, or a known vulnerability, so that the open-source rule ordin carried from wrightrun (ADR-098 item 6; wrightrun ADR-071, ADR-072, ADR-077) is enforced by a machine on every build rather than by review. Exactly three named exceptions exist, each decided by the user and recorded here with its reason.

## Approach

Two tools, one per ecosystem, both reading lock files: `cargo-deny` 0.20.2 (`MIT OR Apache-2.0`) for the Rust workspace — licences and RustSec advisories in one pass over `Cargo.lock` — and `license-checker-rseidelsohn` 5.0.1 (BSD-3-Clause) with `npm audit` for the screens module's npm dependencies (SA-LIC-01). The allow list names licences explicitly; a licence not on it fails the build until it is added with a reason. Exceptions are per crate, never per licence, so an exception cannot admit a second crate. Rejected: `cargo-license` with a script (no advisory check, a second tool); allowing "any OSI-approved licence" by category (cargo-deny's `allow-osi-fsf-free` style switches also admit FSF-free licences such as the GPL family, which the rule denies).

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | `make check` over the whole workspace (one `Cargo.lock`) and over `apps/desktop/ui/` npm dependencies; the desktop installer is built only after the check passes. | SA-LIC-01; 05 |
| Personal server (P) | Yes | Same workspace check; the container image build runs it again before the image is written. | SA-LIC-01; 05 |
| Agent profile (A) | Yes | Same image as P. The harness (Claude Code) is outside the image and outside the check (OQ-62). | SA-LIC-01; 05; ADR-098 item 7 |

The embedded crates `memory/` and `sandbox/sandbox-host/` keep their own lock files (2B-1 rule 7); they are path dependencies of `ordin-memory-service` and `ordin-sandbox`, so their dependencies appear in the workspace's `Cargo.lock` and are checked there.

## Shared Context

**Rule (HLD 05, copied).** "A build check that lists every dependency with its licence and fails on one the Open Source Initiative does not approve, or on the GPL or the AGPL, and an audit of locked dependencies for known vulnerabilities … both run in `make check` and in the image build." *Amended in revision 0.49.0 (decision 80):* three named exceptions — libgit2, the harness of OQ-62, and `notify` 8.2.0. SurrealDB is not part of the build (ADR-101), so it is not an exception.

**The three named exceptions.**

| # | Component | Licence | Why it is excepted | Decision | How the check carries it |
|---|---|---|---|---|---|
| E1 | libgit2 1.9.x, bundled and statically linked by `git2` 0.21.0 (through its `-sys` crate), unmodified | GPL-2.0 with the linking exception | The only Rust git library that can push; `gix` has no push (SA-GIT-01). | OQ-67 (HLD) / OQ-GIT-LICENCE (1B), the user's decision of January 2026, option (a) | a per-crate exception on the crate that bundles libgit2; no other crate may carry a GPL expression |
| E2 | The harness of OQ-62 (Claude Code), installed by the deploying organisation from Anthropic's installer at the version ordin pins | proprietary (not open source) | Hosted agents use the same harness as every ordin agent; ordin does not redistribute it. | OQ-62, decision 74 (January 2026) | not in `Cargo.lock` or `package-lock.json`; outside the image and outside the check — recorded here only |
| E3 | `notify` 8.2.0 | CC0-1.0 (a public-domain dedication the OSI does not list) | File watching on macOS, Windows and Linux without ordin's own per-platform watchers. | OQ-68, the user's decision of January 2026 | a per-crate exception on `notify` |

**Crates whose licence HLD 05 marks "to be confirmed by cargo-deny before Phase 5".**

| Crate | Named by | Check that must pass |
|---|---|---|
| `thiserror` 1 | 05 — Utility and cryptography crates | `cargo deny check licenses` reports its expression inside the allow list |
| `async-trait` 0.1 | 05 — Utility and cryptography crates | as above |
| `serde_json` 1 | 05 — Utility and cryptography crates | as above |
| `tokio-util` | 05 — Utility and cryptography crates | as above |
| `unicode-normalization` | 05 — Utility and cryptography crates | as above |
| `tokio-tungstenite` 0.28.0 | 05 — Always on and Slack | as above |
| `jsonwebtoken` | 05 — Caller token verification | as above |
| `minisign-verify` *(amended January 2026, ADR-114)* | SetupAssistant.md SET-UP-18 (a personal server verifies its own update archive) | as above |

ChannelGateway.md lists `rand` and `base64` (R22); since the correction round of January 2026 it points their confirmation at this licence gate rather than leaving them marked unconfirmed, and Phase 1A SA-LIC-01 now names the three exceptions and the gate (both former defects resolved). These nine crates, the transitive crates and the E1 expression are settled by the *licence gate* (Internal Logic step 8), not left open.

## Public Interface

**`deny.toml`** (workspace root; keys as `cargo-deny` 0.20.2 reads them):

```toml
[graph]
all-features = true            # every feature combination the programs build (desktop, server, extra-guests)

[licenses]
# OSI-approved licences named in HLD 05 and Phase 1A for crates ordin builds. A licence found
# that is not listed fails the build until it is added here with its reason (SA-LIC-01).
allow = [
  "MIT",
  "Apache-2.0",
  "BSD-3-Clause",
  "ISC",
  "Unicode-3.0",
]
confidence-threshold = 0.93

# E1 — libgit2 (OQ-67 / OQ-GIT-LICENCE): the crate that bundles libgit2 only.
# E3 — notify 8.2.0 (OQ-68): CC0-1.0.
exceptions = [
  { crate = "libgit2-sys", allow = ["GPL-2.0"] },   # narrowed at the licence gate to the SPDX expression cargo-deny reports (Internal Logic step 8)
  { crate = "notify", allow = ["CC0-1.0"] },
]

[advisories]
ignore = []                    # an ignored advisory needs an entry here with its RUSTSEC id and reason
yanked = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

**`make check`** runs, in this order, and stops at the first failure: `cargo deny check licenses advisories sources`; then, in `apps/desktop/ui/`, `npx license-checker-rseidelsohn --production --onlyAllow "MIT;Apache-2.0;BSD-3-Clause;ISC"` and `npm audit --omit=dev` (any advisory fails, *chosen here*). The image build (`deploy/container/Containerfile`) runs `cargo deny check licenses advisories sources` before compiling.

### Example

A dependency added under a licence not on the list:

```
$ make check
error[rejected]: failed to satisfy license requirements
  ┌─ some-crate 1.2.0 (registry+https://github.com/rust-lang/crates.io-index)
  │  license = "BUSL-1.1"
  │  ✗ BUSL-1.1 - not explicitly allowed
make: *** [check] Error 1
```

## Internal Logic

1. `cargo-deny` resolves the dependency graph from `Cargo.lock` with all features, reads each crate's declared licence expression (and its licence files where the declaration is absent, at the confidence threshold), and evaluates it against `allow` plus the crate's own `exceptions` entry. An expression satisfied only by a licence outside the list fails the build with the crate and licence named.
2. GPL and AGPL never pass: neither is on `allow`, and the only GPL entry is E1's per-crate exception.
3. Advisories: every locked crate is checked against the RustSec advisory database; a vulnerability or a yanked version fails the build. An advisory may be ignored only by an `ignore` entry with its identifier and a written reason.
4. Sources: a crate from a registry or git repository other than crates.io fails the build.
5. Interface: `license-checker-rseidelsohn` lists every production npm dependency of the screens module with its licence and fails on any outside `--onlyAllow`; `npm audit` fails on any advisory against `package-lock.json`.
6. The harness (E2) and external programs ordin installs or runs unmodified as separate programs — language servers such as `jdtls` (EPL-2.0, SA-LSP-01), `gh`, `glab`, the ArtCraft apps, the Copilot CLI sidecar — are not linked and are outside both checks; each is recorded in HLD 05 with its licence.
7. The check runs in the first Playbook step, so a finding stops the build before any module is generated (HLD 10).
8. **Licence gate (first run).** The first `cargo deny check licenses` run, in the Phase 5 scaffolding step that writes `Cargo.lock`, is a gate the scaffolding step must pass before any later step starts:
   1. It reads the SPDX expression `cargo-deny` derives for the crate that bundles libgit2 and narrows E1's `exceptions` entry to exactly that expression (for example `GPL-2.0 WITH GCC-exception-2.0` if that is what it reports), so the exception admits nothing broader.
   2. For every transitive crate whose licence is OSI-approved but not yet on `allow` (the Wasmtime family behind the sandbox, the ONNX Runtime bindings behind `fastembed`, the ICU4X data crates and any other), it adds that licence to `allow` with a one-line comment naming the crate that needs it and the reason.
   3. It confirms that the seven crates HLD 05 marks "to be confirmed by cargo-deny before Phase 5", and `rand` and `base64`, report expressions inside `allow`.
   4. A licence that the OSI does not approve, or the GPL or the AGPL, on any crate other than the three named exceptions (E1 the libgit2 crate, E2 outside the check, E3 `notify`) stops the build: the scaffolding step fails and the dependency is removed or replaced, or the user records a new named exception through an HLD decision. No such licence is added to `allow`.

## Data Model

N/A — configuration files only (`deny.toml`, `Makefile`); no table.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| A Rust crate's licence is outside the allow list and has no exception | `make check` exit 1 | — (`cargo-deny` `rejected`) | `error[rejected]: failed to satisfy license requirements` naming crate and licence |
| A locked Rust crate has a RustSec advisory or is yanked | `make check` exit 1 | — (`cargo-deny` `vulnerability` / `yanked`) | the advisory identifier and the crate |
| A crate comes from an unknown registry or git source | `make check` exit 1 | — (`cargo-deny` sources) | the crate and its source |
| An npm production dependency's licence is outside `--onlyAllow` | `make check` exit 1 | — | `license-checker-rseidelsohn` names the package and licence |
| Licence gate: a non-OSI, GPL or AGPL licence on a crate other than the three named exceptions | scaffolding step fails | — (`cargo-deny` `rejected`) | the crate and licence; the build does not continue |
| `npm audit` reports an advisory | `make check` exit 1 | — | the advisory and the package |

No runtime error code: the check runs at build time only.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: LicenceCheck

  Scenario: Happy path — the workspace passes with the two crate exceptions
    Given Cargo.lock contains git2 0.21.0 with its bundled libgit2 and notify 8.2.0
    When make check runs
    Then cargo deny check licenses advisories sources exits 0
    And the npm licence check and npm audit exit 0

  Scenario: Edge case — the libgit2 exception admits no other GPL crate
    Given a second crate declaring "GPL-2.0" is added to the workspace
    When make check runs
    Then cargo-deny rejects that crate
    And make check exits 1

  Scenario: Error path — a licence the OSI does not approve fails the build
    Given a crate declaring "BUSL-1.1" is added to the workspace
    When make check runs
    Then cargo-deny reports "not explicitly allowed" for BUSL-1.1
    And the image build does not start

  Scenario: Edge case — the licence gate narrows the libgit2 exception
    Given the first cargo deny check licenses run reports an SPDX expression for libgit2-sys
    When the Phase 5 scaffolding step completes the licence gate
    Then the libgit2-sys exception in deny.toml allows exactly that expression
    And every OSI-approved licence added to allow carries a comment naming its crate and reason

  Scenario: Error path — a known vulnerability fails the build
    Given a locked crate has a RustSec advisory that is not in the ignore list
    When make check runs
    Then cargo-deny reports the advisory and make check exits 1
```

## Performance, Security, Observability

- **Performance targets:** `cargo deny check` with a cached advisory database completes within 60 s on a developer machine; the npm checks within 30 s.
- **Security:** advisories are checked on every build and in the image build; unknown sources are refused, so a dependency cannot be swapped for a fork without review; the exception list is per crate and visible in one file under review.
- **Observability:** the tool output names each finding; no runtime metric.

*Closed gaps and defects.* The three earlier unconfirmed assumptions (the exact SPDX expression of the libgit2 exception, the licences of transitive crates, and the licences of the nine crates marked "to be confirmed by cargo-deny") can only be settled against a lock file; they are now the licence gate of *Internal Logic* step 8, which the Phase 5 scaffolding step must pass, with a fixed outcome for every case.

## Gaps

None.
