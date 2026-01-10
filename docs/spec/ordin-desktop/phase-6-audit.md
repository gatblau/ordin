# Phase 6 — Self-Audit

> **Spec set:** `ordin-desktop` · **Phase:** 6 — Self-Audit (twenty-third run, first after the January 2026 amendment *Per-document read access for knowledge agents*, ADR-115, decision 94) · **Status:** Passed — the amended specs promoted to *Approved* by the user January 2026 · **Mode:** greenfield
> **derivedFromHld:** 0.63.0 · **Date:** January 2026 · **Supersedes:** the twenty-second audit of January 2026 (Passed, for the set amended by ADR-114) and every earlier run (see *Earlier audits*)
> **Checklist:** `.claude/skills/specgen/references/audit-checklist.md` (every item binary), plus the additional checks in the Phase 6 brief.
> **Scope:** the 100 files of the set, plus this file. New: `components/SourceAccess.md` (crate `ordin-source-access`, build phase 2, agent profile, prefix `SACC_`). Amended by ADR-115: `components/KnowledgeQueryServer.md`, `KnowledgeQueryClient.md`, `KnowledgeReader.md`, `AgentConfiguration.md`, `MemoryService.md`, `CallerApi.md`, `CallerAuthentication.md`, `IdentityAndCredentials.md`, `OrdinServer.md`, `OrdinDesktop.md`, `ServiceApi.md`, `ScreensModule.md`, `ArchitectureTest.md`, `Telemetry.md`; `cross-cutting/ErrorHandling.md`, `DatabaseMigrations.md`, `Configuration.md`, `Logging.md`, `Metrics.md`, `RateLimiting.md`, `AuthenticationAndAuthorisation.md`, `Tracing.md`, `GracefulShutdown.md`, `InputValidation.md`; `phase-1-analysis.md`, `phase-2-architecture.md`, `phase-2c-shared-types-wire.md`, `phase-5-playbook.md`; every other file only for its `derivedFromHld` pin.

This is a Review-mode audit by an independent reviewer, run three times against the amendment before it was applied; its findings were fixed in Plan mode between runs. Paths are relative to `docs/spec/ordin-desktop/` unless they start with `docs/` or `.claude/`.

## Verdict

**Passed.** Every checklist item and every additional check is ticked for the amended set. The first run failed on 40 defects, five of them blocking: the Git access unit kept the branch and sub-path while Source Access matched on the repository alone, so every per-person Git check would have been withheld; a proxy's SharePoint facts could never reach the front door for checking; a missing delegation reported unchecked sources and so revealed that a restricted item matched; unlinking a missing link answered differently in two specs; and the Phase 1 shortlist cap exceeded the memory component's maximum of 50. The second run left eight items (the `Delegation` copy, account-identifier bounds, error-surface wording, the personal server's Confluence scope, three glossary and count slips, two log events); the third run confirmed them fixed. Applying the amendment then found that the prefix `SRC_` already belonged to the Workspace Service; the ten Source Access codes were renamed `SACC_` before the registry was written.

## Checklist

| # | Item | Result |
|---|---|---|
| 1 | Complete data models | Pass |
| 2 | Actions: inputs, outputs, steps, errors | Pass |
| 3 | No banned phrases | Pass |
| 4 | At least three Gherkin scenarios per component, including an error path | Pass |
| 5 | Error table of at least two rows | Pass |
| 6 | Every code in the cross-cutting registry with its retryability | Pass |
| 7 | Cross-component interactions documented on both sides | Pass |
| 8 | Phase 2B is a DAG | Pass |
| 9 | Configuration values listed | Pass |
| 10 | Self-contained specs | Pass |
| 11 | Assumptions complete; open questions blocking only | Pass |
| 12 | Example input and output | Pass |
| 13 | Shared types defined once in 2C and copied verbatim | Pass |
| 14 | Security for every entry point | Pass |
| 15 | Performance targets | Pass |
| 16 | en-GB | Pass |
| 17 | Rollback for destructive schema changes | Pass |
| 18 | `derivedFromHld` on new and amended specs | Pass |
| 19 | No ADR-shaped paragraph | Pass |
| 20 | Spec glossary terms in the HLD glossary | Pass |
| 21 | Every `derivedFromHld` matches the HLD revision | Pass — every file of the set reads 0.63.0 (the user's decision of January 2026) |
| 22 | Revision bumps have changelog lines | Pass |

### Additional checks

- **Intent coverage.** Each of the eighteen acceptance criteria of the change is covered by a Gherkin scenario or an error row, mostly in `components/KnowledgeQueryServer.md` and `components/SourceAccess.md`.
- **Consistency.** Names of types, fields, routes 41–45, error codes, metric families 59–64, constants, tables, origins and configuration keys agree across every amended file.
- **Registers.** Every new code is in an error table and in the registry of `cross-cutting/ErrorHandling.md`; every new or changed table (`kq_linked_accounts`, `src_notice_seen`, `kq_connection_accounts`, `kr_document_sources`, the `kr_documents` rebuild, the `memory_recall_wider` version widening) has a `cross-cutting/DatabaseMigrations.md` row.
- **Identifiers.** 88 identifiers the amendments name as existing were found in `docs/spec/` or `memory/docs/spec/`.
- **Anchors.** Every edit anchor was found verbatim before the edit was written.

### Open items (not failures)

- Confirmation items C-101 to C-107 (HLD 10) gate Playbook steps, not the spec; each has a stated fallback.
- `make doc-ref-check`, named by the spec-anchors layer rule, is not yet present in the repository; anchors added by `/apply` are checked when it is.

## Earlier audits

- **Twenty-second audit (January 2026): Passed** for the set before this amendment (releases and updates, ADR-114, decision 93); the user promoted the set to *Approved*.
- **Twenty-first audit (January 2026): Passed** for the set before this amendment (knowledge agents and the Open Knowledge Format, ADR-112 and ADR-113); the user promoted the set to *Approved*. Its open items O-3…O-24 and advisories A-17…A-48 are carried below.
- **Twentieth audit (January 2026): Passed** for the set before this amendment (agentic memory as a core capability, ADR-111); the amended specs waited for the user's promotion and are covered again by this run.
- **Nineteenth audit (January 2026): Passed** for the set before this amendment (reading other teams' knowledge, ADR-110); the user promoted the set to *Approved*.
- **Eighteenth audit (January 2026): Passed** for the set before this amendment (Ordin's language and language files, ADR-109); the user promoted the set to *Approved*.
- **Seventeenth audit (January 2026): Passed** for the set before this amendment (continuing an earlier conversation, ADR-108); the user promoted the set to *Approved*.
- **Sixteenth audit (January 2026): Passed** for the set before this amendment (every run done by ordin, ADR-107); the user promoted the set to *Approved*.
- **Fifteenth audit (January 2026): Passed** for the set before this amendment (Jira issues and handovers, ADR-106); the user promoted the set to *Approved*.
- **Fourteenth audit (January 2026): Passed** for the set before the Jira amendment (page links, ADR-104); the user promoted the set to *Approved*.
- **Thirteenth audit (January 2026): Passed** for the set before the page-links amendment (team-knowledge fixes J-1…J-4 verified); the user promoted the set to *Approved*.
- **Twelfth audit (January 2026): Failed**, J-1…J-4 — fixed.
- **Eleventh audit (January 2026): Passed** for the set before the team-knowledge amendment.
- **Tenth audit: Failed**, H-1…H-4 and advisory A-26 — fixed.
- **Ninth audit: Failed**, G-1, G-2 — fixed.
- **Eighth audit: Failed**, F-1…F-4 — fixed.
- **Seventh audit (first after the Slack amendment): Failed**, D-1…D-5 — fixed.
- **Sixth audit (January 2026): Passed** for the set before the Slack amendment. Earlier runs: every defect fixed and still fixed.

## What happens next

The user promoted the amended specs to *Approved* on January 2026. `/breakdown` on the per-document read access change plans its build: Source Access (Step 79, build phase 2) first, then the amended steps of the Knowledge Query Server, the Knowledge Reader and the desktop.

## Decisions needed from you

None outstanding from this run.
