# Phase 6 — Self-Audit

> **Spec set:** `ordin-desktop` · **Phase:** 6 — Self-Audit (twenty-fifth run, first after the January 2026 amendment *Icons*, ADR-117, decision 99) · **Status:** Passed — promoted to *Approved* by the user, 2026-01-28 ("approved proceed with next stage") · **Mode:** greenfield
> **derivedFromHld:** 0.66.0 · **Date:** January 2026 · **Supersedes:** the twenty-fourth audit of January 2026 (Passed, for the set amended by ADR-116) and every earlier run (see *Earlier audits*)
> **Checklist:** `.claude/skills/specgen/references/audit-checklist.md` (every item binary), plus the additional checks in the Phase 6 brief.
> **Scope:** the 100 files of the set, plus this file. Amended by ADR-117: `cross-cutting/VisualDesign.md` (V8 Icons, Internal Logic step 8), `components/Brand.md` (amendment *The prototype's icons in the build pack*), `components/ScreensModule.md` (amendment *Icons*, SM-IC-1 to SM-IC-4), `cross-cutting/ErrorHandling.md` (registry, `UI_` prefix count), `phase-1-analysis.md` (SA-VIS-06 to SA-VIS-09), `phase-4-cross-cutting.md` (registry count), `phase-5-playbook.md` (Steps 28 and 71). Every other file: `derivedFromHld` moved from 0.65.0 to 0.66.0 with no other change.

This is a Review-mode audit by an independent reviewer, run three times against the amendment before it was applied; its findings were fixed in Plan mode between runs. Paths are relative to `docs/spec/ordin-desktop/` unless they start with `docs/` or `.claude/`.

## Verdict

**Passed.** Every checklist item and every additional check is ticked for the amended set. The first run failed on nineteen defects, five of them blocking: the HLD's Branding row lost its new sentence to a broken table cell; 11 §3 still allowed icon-only buttons on tree rows alone; the drafts named two keyboard shortcuts that exist nowhere; tree and menu actions dropped icons the prototype draws; and the run-state, problems-line and **Outside the company** icons the specs already required were unmapped (the prototype draws them as text glyphs, which V8 now states). The second run failed on one blocking defect, fifteen prototype actions missing from V8's *Actions* table, and three small differences. The third run found none blocking; its two optional fixes (the document screen's **More actions** row and a hosted agent's **Open**) were applied.

## Checklist

| # | Item | Result |
|---|---|---|
| 1 | Complete data models | Pass — no entity added |
| 2 | Actions: inputs, outputs, steps, errors | Pass — VisualDesign Internal Logic step 8 |
| 3 | No banned phrases | Pass |
| 4 | At least three Gherkin scenarios per component, including an error path | Pass — VisualDesign 10 icon scenarios, ScreensModule 7, Brand 4 (Brand adds no error) |
| 5 | Error table of at least two rows | Pass |
| 6 | Every code in the cross-cutting registry with its retryability | Pass — `UI_BRAND_ICONS_IGNORED` added (internal, not retryable), in alphabetical order; the `UI_` prefix count recounted to 16 |
| 7 | Cross-component interactions documented on both sides | Pass — `brand.get_asset` for the icon sets, Brand amendment ↔ VisualDesign step 8 |
| 8 | Phase 2B is a DAG | Pass — Step 71 now needs Step 28; no cycle |
| 9 | Configuration values listed | Pass — none added |
| 10 | Self-contained specs | Pass |
| 11 | Assumptions complete; open questions blocking only | Pass — SA-VIS-06 to SA-VIS-09 |
| 12 | Example input and output | Pass — step 8's sanitising example |
| 13 | Shared types defined once in 2C and copied verbatim | Pass — none added |
| 14 | Security for every entry point | Pass — a pack's SVG enters the document only as listed elements and attributes, through the DOM, after Brand's checks |
| 15 | Performance targets | Pass — ≤ 20 ms to build the sprite, about 12 KB |
| 16 | en-GB | Pass |
| 17 | Rollback for destructive schema changes | Pass — no schema change |
| 18 | `derivedFromHld` on new and amended specs | Pass |
| 19 | No ADR-shaped paragraph | Pass |
| 20 | Spec glossary terms in the HLD glossary | Pass — no new term |
| 21 | Every `derivedFromHld` matches the HLD revision | Pass — every file of the set reads 0.66.0 |
| 22 | Revision bumps have changelog lines | Pass — revision 0.66.0 has two changelog lines |

### Additional checks

- **Icon names.** The 50 names of V8, of 08 — Brand pack and of the Brand amendment equal the `i-` symbols of `docs/design/ordin-desktop/user-interface.html`'s sprite, in order, each with `viewBox="0 0 16 16"`.
- **Placement and actions.** Every row of V8 *Where icons are drawn* and *Actions* matches the prototype's `ico(…)` calls, tree `icon:` properties, `acts` entries and the maps `KICO`, `ORIGIN_ICO`, `FEEDICO`, `KIND_ICO` and `ptIco`; every scene id cited exists.
- **Sizes and stroke.** V8 *Drawing* equals the prototype's `.ico` rules (16, 13, 12, 12, 14, 11 and 24-in-44 px; stroke 1.6, 2.6 on `i-more`).
- **Consistency.** ADR-117, 11 §3, UX-A83, V8, the Brand and ScreensModule amendments agree on the icon-only buttons, the mirrored icons, the PNG and per-name fallbacks, the error code and its four details, and build pack version 1.2.0.
- **Catalogue ids.** The seven new message ids are unused in every catalogue; `conv-tab-close` is reused as intended.
- **Intent coverage.** Each of the eleven acceptance criteria of the icon-set change, decided as ADR-117 (HLD `docs/design/ordin-desktop/09-decisions.md`), is met by a section, a table or a Gherkin scenario of the amended files.

### Open items (not failures)

- The screen tab's **Close** tooltip differs from the prototype's "Close · Delete", because no Delete shortcut for tabs is specified (recorded in SM-IC-3).
- The screens draw today only some of the places V8 lists; the rest get their icons in the amendment that adds each part.

## Earlier audits

- **Twenty-fourth audit (January 2026): Passed** for the set before this amendment (the look of the screens, ADR-116, decision 98), after two failing runs (eleven, then seven defects); the user promoted the set to *Approved*.
- **Twenty-third audit (January 2026): Passed** for the set before this amendment (per-document read access for knowledge agents, ADR-115, decision 94); the user promoted the set to *Approved*.
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

`/breakdown` plans the build: Step 28 (Brand: the 50-icon build pack, version 1.2.0), then Step 71 (screens: the sprite, the icon part, the icons in place, the diagnostic and the catalogue messages).

## Decisions needed from you

None. The user promoted the icon amendment to *Approved* on 2026-01-28.
