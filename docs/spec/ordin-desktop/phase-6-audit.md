# Phase 6 — Self-Audit

> **Spec set:** `ordin-desktop` · **Phase:** 6 — Self-Audit (twenty-sixth run, first after the January 2026 amendment *Meeting minutes*) · **Status:** **Passed** on the second run, 2026-01-31, after the user approved the HLD delta (revision 0.69.0, ADR-122); the first run failed on items 20 and *HLD consistency* only · **Mode:** greenfield
> **derivedFromHld:** 0.69.0 · **Date:** 2026-01-31 · **Supersedes:** the twenty-fifth audit of January 2026 (Passed, for the set amended by ADR-117) and every earlier run (see *Earlier audits*)
> **Checklist:** `.claude/skills/specgen/references/audit-checklist.md` (every item binary).
> **Scope:** new — `components/TranscriptionService.md`, `components/ObsRecorderAdapter.md`, `components/MinutesService.md`; amended (amended January 2026; user decision; Approved by the user January 2026) — `components/DictationService.md` (DICT-TRX-1), `components/IdentityAndCredentials.md` (ID-OBS-1), `components/JournalAndArtefactStore.md` (JA-MIN-1 to JA-MIN-3), `components/OrdinHost.md` (OH-MIN-1 to OH-MIN-3), `components/ServiceApi.md` (SVC-MIN-1 to SVC-MIN-4), `components/OrdinDesktop.md` (DSK-MIN-1 to DSK-MIN-5), `components/ScreensModule.md` (SM-MIN-1 to SM-MIN-10), `cross-cutting/ErrorHandling.md` (43 codes, four prefixes), `cross-cutting/Configuration.md` (minutes settings), `phase-1-analysis.md` (SA-MIN-01 to SA-MIN-12, C-108 to C-111, four glossary terms), `phase-2-architecture.md` (three inventory rows), `phase-2c-shared-types-wire.md` (§23), `phase-5-playbook.md` (Steps 80–82, §7.8 rows, §7.15, §9 item 1). Every other file: unchanged.

Paths are relative to `docs/spec/ordin-desktop/` unless they start with `docs/` or `.claude/`.

## Verdict

**Passed (second run, 2026-01-31).** Every item of the checklist and every additional check passes. The first run failed on item 20 and on *HLD consistency*, both waiting on the HLD delta; the user approved it ("yes, approve and run breakdown"), it was applied as HLD revision 0.69.0 with ADR-122 and one changelog line, and every file in *Scope* now reads `derivedFromHld: 0.69.0`.

## Checklist

| # | Item | Result |
|---|---|---|
| 1 | Complete data models | Pass — `minutes` table (both engines), `Transcript`, settings file, recording and transcript files |
| 2 | Actions: inputs, outputs, steps, errors | Pass — each new public method and each of the twenty commands |
| 3 | No banned phrases | Pass — added text scanned case-sensitively |
| 4 | At least three Gherkin scenarios per component, including an error path | Pass — Transcription 9, OBS 6, Minutes 8; amendments: Identity 4, Journal 4, Ordin Host 4, Service API 4, Desktop 4, Screens 7 (DICT-TRX-1 changes no behaviour) |
| 5 | Error table of at least two rows | Pass |
| 6 | Every emitted code in the registry with its retryability | Pass — enumerated from the new specs' Error Tables and enums (no source emits them yet): 13 `TRX_`, 12 `OBS_`, 12 `MIN_`, 3 `MINUTES_`, 3 `OH_MINUTES_*`; each owner named; the `CRED_`, `DSK_`, `JRNL_`, `SVC_` codes reused are already registered |
| 7 | Cross-component interactions on both sides | Pass — Minutes ↔ Transcription, OBS, Journal (JA-MIN-2), Ordin Host (`MinutesWriter` ↔ OH-MIN-1), Identity (ID-OBS-1 ↔ OBS §B2), Service API ↔ Desktop ↔ Screens; Dictation ↔ Transcription (DICT-TRX-1) |
| 8 | Phase 2B is a DAG | Pass — Transcription 3 → Localisation 2; OBS 3 → Identity 1, Localisation 2; Minutes 4 → 3, 3, 2, 1; no edge to the Ordin Host |
| 9 | Configuration values listed | Pass — Configuration.md *Minutes settings*; no environment variable or profile key added |
| 10 | Self-contained specs | Pass — shared types and interfaces copied into each new spec |
| 11 | Assumptions complete; open questions blocking only | Pass — SA-MIN-01 to SA-MIN-12 resolve the six non-blocking questions; no blocking question |
| 12 | Example input and output | Pass |
| 13 | Shared types once in 2C, copied | Pass — §23 |
| 14 | Security for every entry point | Pass — twenty `Local` rows, five person-only, the password never echoed or logged, loopback-only WebSocket, transcript text as content in the minutes run |
| 15 | Performance targets | Pass — SA-MIN-03 budgets, command latencies, memory ceiling |
| 16 | en-GB | Pass |
| 17 | Rollback for destructive schema changes | Pass — migration step 4 only adds a table and an index |
| 18 | `derivedFromHld` on new and amended specs | Pass |
| 19 | No ADR-shaped paragraph in a spec | Pass — the trade-off is drafted as ADR-122 in the held delta, not in a spec |
| 20 | Spec glossary terms in the HLD glossary | Pass — *Minutes*, *Transcript*, *Speech model*, *Recording indicator* added to `docs/design/ordin-desktop/00-overview.md` (first run: Fail, closed by H-3) |
| 21 | Every `derivedFromHld` matches the HLD revision | Pass — the eighteen files in *Scope* read 0.69.0; the other files keep their earlier value and change nothing derived from ADR-122 (their move to 0.69.0 rides with their next amendment) |
| 22 | Revision bumps have changelog lines | Pass — 0.69.0 has its line in `99-changelog.md` |

### Additional checks

- **HLD consistency — Pass (first run: Fail).** Closed by ADR-122, which amends ADR-078 for meeting recordings only. HLD 09 ADR-078 ("Dictation uses macOS's own on-device speech recognition only; it is off on Windows and needs no bundled model") and ADR-043's amendment state there is no speech model. The new Transcription Service downloads one, with consent, for meeting recordings only. The specs keep dictation unchanged (DICT-TRX-1), recorded in the HLD as item H-1 (ADR-122).
- **Identifiers.** Every existing identifier cited was read in this run: `apps/desktop/Cargo.toml:19-32`; `crates/ordin-dictation/{Cargo.toml,src/lib.rs}`; `crates/ordin-identity/src/{accounts.rs,secret.rs,error.rs}` (`CRED_KEYCHAIN_UNAVAILABLE`); `crates/ordin-host/src/{crews.rs,types.rs,task_form.rs,conversation.rs}`; `crates/ordin-journal/src/sql/` (version 3); `crates/ordin-service-api/src/commands/mod.rs` (`WebPage`); `crates/ordin-types/src/id.rs` (`WorkspaceId`, `RunId`, `DashboardId`); `crates/ordin-craft-toolkit/src/definitions.rs`; `deny.toml` (`MPL-2.0` allowed); `Cargo.lock` (`tokio-tungstenite`, `zip`, `quick-xml`, `sha2`, `reqwest` present; `symphonia`, `whisper-rs`, `rubato` absent). New identifiers are spec intent.
- **Counts.** Catalogue 451 → 471 rows, `Interactive` 435 → 453, groups 48 → 49 (10 behind a switch). Registry header 2118 → 2161 and owners 79 → 82.

### Open items (not failures)

- **O-MIN-1** The registry header count was already eight below the registry's rows before this amendment (2118 stated, 2126 rows at `HEAD`, 2128 with the in-flight Confluence rows); this run adds 43 rows and 43 to the header, and leaves the earlier difference for a recount.
- **O-MIN-2** The OBS sentences quote OBS's own menu labels ("Tools › WebSocket Server Settings", "Server Password"), which hold a word EH-SEN-2 lists. EH-SEN-1 binds codes of crates built with no switch on; the `OBS_` crate is behind `minutes`, so the rule does not bind them, and the quotation is what the person must find on screen.
- **O-MIN-3** SA-MIN-09 places **Record** on the workspace's Minutes screen because the request's "team view" names no screen of this spec set.

## HLD-impact-pass — approved by the user and applied, 2026-01-31 (revision 0.69.0)

Applied to `docs/design/ordin-desktop/` (`README.md` revision 0.68.0 → 0.69.0; 00, 01, 02, 03, 05, 06, 09, 99).

- **H-1 `09-decisions.md` — ADR-122 (new): Meeting minutes are made on the computer from transcripts and recordings; OBS records; Ordin writes.** *Decision:* Ordin makes minutes from `.vtt`, `.docx` and `.txt` transcripts and from `.m4a`, `.mp3` and `.wav` recordings; recordings are turned into text on the computer by a downloaded whisper.cpp-class speech model (larger, about 1.5 GB, by default; smaller, about 470 MB, by setting), downloaded only after the person agrees and checked by checksum; Ordin records meetings by controlling the person's own OBS over its local WebSocket; Ordin's agent writes the minutes, sending the transcript text — never audio — to the AI service the person's agents already use, after a one-time notice. *Context:* meeting outcomes are lost or written by hand; meeting-tool integrations need company consent. *Consequences:* ADR-078's "no speech model" stays true for dictation and gains an exception for meeting recordings; a new build switch `minutes`; the person is reminded to tell participants each time they record. *Alternatives:* cloud transcription (breaks local-only), meeting-tool integrations (need company consent), operating-system recognisers (none usable on Windows; macOS's is for live dictation). Status: decided by the user, January 2026.
- **H-2 `02-architecture.md`** — three components: Transcription Service (in the dictation module), OBS Recorder Adapter, Minutes Service, with their arrows (Minutes → Transcription, OBS, Journal; Minutes → Ordin Host through the program).
- **H-3 `00-overview.md` glossary** — *Minutes*, *Transcript*, *Speech model*, *Recording indicator* (definitions as Phase 1C).
- **H-4 `05-tech-stack.md`** — `symphonia` (audio decoding, MPL-2.0), whisper.cpp through `whisper-rs` (speech recognition, MIT; binding licence per C-108).
- **H-5 `01-context.md`** — OBS (a recording program the person installs, reached on this computer only) and the speech-model download address as neighbouring systems.
- **H-6 `03-sequences.md`** — "Record a meeting": Record → reminder → OBS StartRecord → indicator → Stop → StopRecord → move file → transcribe → notice (first time) → Ordin writes → minutes under **Minutes**.
- **H-7 `06-regulatory.md`** — recording consent: the person is responsible for telling participants; Ordin reminds on every Record and shows a constant indicator; recordings stay on the computer until deleted.
- **Changelog line (`99-changelog.md`):** "January 2026 — user decision: meeting minutes from transcripts and recordings, made on the computer with a downloaded speech model after consent; recording through the person's own OBS; Ordin writes the minutes after a one-time notice (ADR-122; 00, 01, 02, 03, 05, 06, 09)."

## What happens next

`/breakdown` has planned §7.15 in a working plan kept outside the repository; `/apply` runs it one phase at a time.

## Decisions needed from you

None outstanding for the spec set. The plan's own open assumptions (confirmation items C-108 to C-111) are settled by its phases.

## Earlier audits

- **Twenty-fifth audit (January 2026): Passed** for the set before this amendment (icons, ADR-117, decision 99); the user promoted it to *Approved* on 2026-01-28.
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

## Amendment audit, 2026-11-21 — Ordin's minutes and transcription tools (approved)

> **Scope:** amended — `components/OrdinHost.md` (OH-MIN-4 to OH-MIN-11; OH-MIN-1 step 4), `components/MinutesService.md` (MIN-TR-1 to MIN-TR-5), `components/TranscriptionService.md` (TRX-ORD-1, TRX-ORD-2), `components/ServiceApi.md` (SVC-MIN-2 amended, SVC-MIN-5), `components/ScreensModule.md` (SM-MIN-11), `components/OrdinDesktop.md` (DSK-MIN-6), `components/McpFront.md` (SA-MCP-SRV-01), `components/SessionSupervisor.md` (SUP-SRV-1), `components/ClaudeLocalAdapter.md` and `components/CopilotLocalAdapter.md` (the copy into `SessionGrant`), `cross-cutting/ErrorHandling.md` (6 codes), `phase-2c-shared-types-wire.md` (WIRE-MIN-7, WIRE-RUN-SRV-1), `phase-5-playbook.md` (PB-MIN-4). HLD: `09-decisions.md` ADR-122 amended, revision 0.70.0. **Status:** self-audit passed; Approved by the user, 2026-11-21 ("approve as written").

| # | Item | Result |
|---|---|---|
| 1 | Complete data models | Pass — `MinutesJobPurpose`, `TranscriptReady`, `LocalFile`, `MinutesExportFormat`, `only_servers`, every tool's argument type with bounds |
| 2 | Every action has trigger, inputs, outputs, side effects, errors | Pass — eleven tools (OH-MIN-7 answers 1–9), `transcribe`, `transcription` |
| 3 | Error table per changed component | Pass — OrdinHost (5 rows + owners' codes), MinutesService (1 row) |
| 4 | Codes registered | Pass — ErrorHandling.md rows and counts (`OH_` 58, `MIN_` 13, total 2,167) |
| 5 | Gherkin per behaviour | Pass — OrdinHost 9 scenarios, MinutesService 4, McpFront 1 |
| 6 | No unverified identifier | Pass — ports, types, crates and ids read in this run (Evidence lines) |
| 7 | No banned phrase | Pass — after one fix in OH-MIN-8 |
| 8 | Security for every entry point | Pass — tools offered to Ordin only; person-only set widened (SVC-MIN-2); paths confined to the folder after link resolution; export never replaces a file; a minutes run limited to server `ordin` in the MCP Front, closing the 2026-11-21 finding |
| 9 | Long work never holds a tool call | Pass — transcription is a job (MIN-TR-1); tool calls stay within McpFront.md's 300 s |
| 10 | Build switch | Pass — every tool and type behind `minutes`; `only_servers` in every build, unused without it |
| 11 | Playbook | Pass — PB-MIN-4 names the steps and the gate |
| 12 | HLD consistency | Pass — the ADR-122 delta approved by the user |
| 13 | Spec anchors | Pass — `make doc-ref-check`: 0 dangling |

### Assumptions chosen here (the user may change any)

- **A-1** Transcription for Ordin runs as a minutes job the person sees and can cancel, not inside the tool call (a call is stopped at 300 s).
- **A-2** Ordin needs no folder grant for the minutes file tools: grants limit agents, and these tools are offered only to Ordin, acting for the person.
- **A-3** Deleting a recording and removing the speech model join the person-only commands.
- **A-4** The minutes-run fix sits in the MCP Front (one rule for every provider, present and future), with the host's own offer kept as a second check.
- **A-5** A recording turned into text for Ordin passes the one-time notice, because Ordin sends the text to the AI service when it reads it.
- **A-6** Ordin gets no tool to stop a recording.

## Amendment audit, 2026-11-22 — The speech model chosen by the machine (approved)

> **Scope:** amended — `components/TranscriptionService.md` (TRX-MACH-1 to TRX-MACH-4), `components/MinutesService.md` (MIN-MACH-1 to MIN-MACH-4), `components/ScreensModule.md` (SM-MIN-12), `phase-2c-shared-types-wire.md` (WIRE-MIN-8), `phase-1-analysis.md` (SA-MIN-01, SA-MIN-03, C-110), `phase-5-playbook.md` (PB-MIN-5). HLD: `09-decisions.md` ADR-122 and the `00-overview.md` glossary amended, revision 0.71.0. **Status:** self-audit passed; Approved by the user, 2026-11-22 ("approve as written").

| # | Item | Result |
|---|---|---|
| 1 | Complete data models | Pass — `MachineFacts` (four fields with their sources and fallbacks), `SpeechModelSetting`, `MinutesStatus.recommended_model` |
| 2 | Every action has trigger, inputs, outputs, side effects, errors | Pass — `machine_facts`, `recommend`, `recommended_model`; the model in use at B3, C1, C4 and status |
| 3 | Error table per changed component | Pass — no new code; unreadable facts fall to Smaller; `MIN_SETTING_INVALID` covers a bad value |
| 4 | Codes registered | Pass — none added |
| 5 | Gherkin per behaviour | Pass — TranscriptionService 1 outline (8 rows) + 1, MinutesService 4, ScreensModule 2 |
| 6 | No unverified identifier | Pass — `sysctlbyname` (libc 0.2.191), `GlobalMemoryStatusEx` (windows-sys 0.61.2, `Win32/System/SystemInformation`), `available_parallelism` and `is_x86_feature_detected!` (std), `SpeechModelChoice`, `MinutesSettings`, `MinutesSettingsChange`, `MinutesStatus` (crates/ordin-types/src/wire/minutes.rs) |
| 7 | No banned phrase | Pass |
| 8 | Security for every entry point | Pass — no new entry point; the facts are read locally and logged without personal data |
| 9 | Long work never holds a tool call | Pass — the facts are read once per process, within 100 ms |
| 10 | Build switch | Pass — all behind `transcription` / `minutes` |
| 11 | Playbook | Pass — PB-MIN-5 names the steps and the gate |
| 12 | HLD consistency | Pass — ADR-122 and the glossary amended in the same draft |
| 13 | Spec anchors | Not run — `make doc-ref-check` runs on the server once the draft is approved and committed |

### Assumptions chosen here (the user may change any)

- **A-1** The thresholds: Apple silicon with 16 GB, or x86 with AVX2, 8 or more logical processors and 16 GB, gets the larger model; any other computer the smaller one. Basis: 45.7 s per 60 s on 8 x86 processors is at SA-MIN-03's x86 budget.
- **A-2** The recommendation is worked out again on every start, so moving to another computer changes it; the person's own choice is never overwritten.
- **A-3** A settings file written before this amendment keeps its value as the person's choice.
- **A-4** Choosing the larger model on a computer it does not suit is allowed, with a one-line warning, not refused.
