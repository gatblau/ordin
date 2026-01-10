# HLD changelog (`sandbox-host`)

One line per HLD revision bump (folder-form requirement, D-HLD-6).

> **Note on history.** This file was absent for revisions 0.1.0–0.4.0; their bumps are
> recorded in the Phase 6 audit amendments (`../../spec/sandbox-host/phase-6-audit.md`)
> and in git history. It is started here at 0.4.1 and maintained going forward.

> **Erratum (January 2026, no revision bump).** `05-tech-stack.md`, `02-architecture.md`,
> `10-risks.md`, and ADR-020/ADR-024/ADR-030 (`09-decisions.md`) described the Windows
> named-pipe peer check as `GetNamedPipeClientProcessId`→token-SID. That PID lookup
> has a reuse race and is forbidden for the authz decision by
> `docs/spec/sandbox-host/windows-peer-auth.md` §6; corrected in place to
> `ImpersonateNamedPipeClient`→`TokenUser` SID→`EqualSid`, matching the spec and the
> implementation. A prose correction of a security mechanism — the intent (ADR-024 local
> peer-auth boundary) is unchanged — so **no revision bump and no spec `derivedFromHld` re-pin**.

- **0.4.1** (January 2026) — ADR-033: resource-audiencing for pass-through. The daemon
  requests a per-capability resource `audience` at sign-in so the IdP issues an access token
  audienced for the resource server, which validates it (RFC 01). No new component; consumes
  the existing `ResolvedCapability.audience`.

> **Erratum (January 2026, no revision bump).** The auth-mode taxonomy the JSON schema
> (`schema/pysandbox.policy.schema.json`) and the implementation already carried — per-capability
> `authMode` (`exchange`/`passthrough`/`none`/`api_key`), the api_key `secretRef` + `keyPlacement`,
> and the `allowWrite` write gate (shared-register ADR-036/ADR-037/ADR-039, recorded in
> [`../sandboxd-server-mode/09-decisions.md`](../sandboxd-server-mode/09-decisions.md)) — was absent
> from the `11-policy-schema.md` prose and the LLD specs (`phase-2` shared types; C1/C4/C11).
> Reconciled in place (code → spec via `/sync-spec`) so the prose, the JSON schema, and the code
> agree. A correction of docs that lagged an already-decided taxonomy — so **no revision bump and
> no spec `derivedFromHld` re-pin**.

- **0.4.2** (January 2026) — ADR-040: keychain-backed `SecretResolver` for `api_key`. Adds
  `KeychainSecretResolver` (C1) selected by `PYS_SECRET_RESOLVER`, the `PYS_KEYCHAIN_SERVICE`
  env var, and a new **C18 CredentialCli** (`sandboxd credential set|rm|list`) for self-service
  per-user enrolment (resolves OQ-KC-1). The `api_key` auth mode / `secretRef` / `keyPlacement`
  (ADR-036) are unchanged. All new assertions are [Not implemented] (no code yet). All component
  pins re-pinned 0.4.1 → 0.4.2.

- **0.5.0** (January 2026) — ADR-041: a per-run working folder at `/work` with a change record
  (amends ADR-013/ADR-019's no-preopens stance), from RFC `docs/wip/rfc/01-agent-sandbox-features.md`
  F1, accepted by the user. Adds **C19 WorkDir** (`src/workdir.rs`), `RunRequest.files` and
  `RunResult.changes`, `SandboxRuntime::run_in`, the `files` input on `python_sandbox`, a guest
  rebuilt with RustPython `host_env` (re-pinned digest), threat-model rows on TB2 and RR-10, and
  OQ-WD-1 (engine-wide epoch stops concurrent runs) and OQ-WD-2 (guest build not reproducible).
  Implemented and tested. All component pins re-pinned 0.4.2 → 0.5.0.

- **0.6.0** (January 2026) — ADR-042: broker egress address policy (RFC 01 F2) — forbidden ranges
  always refused, private ranges only with the new capability field `allowPrivateNetwork`,
  checked at connect time through a policy resolver; new wire code `EGRESS_DENIED`; new module
  `src/egress.rs`. OQ-WD-1 resolved (per-run stop flag; one run's deadline no longer stops
  another). OQ-WD-2 resolved (reproducible guest: pinned toolchain 1.96.0, `--locked`, path
  remapping, patched `rustpython-derive-impl` and `rustpython-vm`, `make guest-verify`); the
  guest no longer embeds the build environment. Guest digest re-pinned. All component pins
  re-pinned 0.5.0 → 0.6.0.

- **0.7.0** (January 2026) — ADR-043 to ADR-046: the HLD-impact pass for RFC `docs/wip/rfc/01-agent-sandbox-features.md`
  features F3 to F8, accepted January 2026 as ordin ADR-040. Adds a shell guest and a JavaScript guest
  (ADR-043), brokered `curl` and `fetch()` for every guest (ADR-044), work sessions with a cumulative
  change record (ADR-045), caller-supplied non-secret environment variables and once-per-process guest
  compilation (ADR-046). Touches `00-overview`, `02-architecture`, `04-domain`, `05-tech-stack`,
  `07-cross-cutting`, `08-interfaces`, `10-risks` (OQ-GU-1 to OQ-GU-5, six risks) and the threat model
  (TB2, TB3, RR-11). Design only: **nothing is implemented**. Verified while writing: the Python guest
  is already compiled once per process (`src/runtime.rs:116`, `src/main.rs:403`). The LLD spec set under
  `docs/spec/sandbox-host/` is **not** updated and its `derivedFromHld` pins still read 0.6.0; `/spec`
  must run next.
