# ordin desktop — UI prototype

> **Mode:** draft
> **Generated from:** `docs/design/ordin-desktop/user-interface.html` (the clickable prototype, moved into this folder at revision 0.26.0; revised at 0.27.0, 0.34.0, 0.35.0, 0.44.0, 0.45.0, 0.46.0 and 0.56.0), `11-ux-guidelines.md` and `09-decisions.md`
> **Revision:** 0.58.0
> **Last updated:** January 2026
> **Status:** Draft

## 1. What the prototype is

[`user-interface.html`](./user-interface.html) is a single self-contained page — no network requests, no web fonts, no external scripts — that renders every screen, block and conversation pattern of the desktop interface and of the ordin server's chat channels from sample data. Its **Show** menu holds 354 entries in 34 groups (344 scenes, and the ten entries of the story), each group named after the guideline section it illustrates, and the nine story steps can also be typed into the conversation (`STEPS`). The page's own code draws every block from data, as ADR-028 requires of the real application.

It is part of this design and an input to `/spec`. `specgen` reads the `*.md` files of this folder; this document is how the prototype reaches it.

## 2. How `/spec` uses it

| Question the spec writer has | Where the answer comes from |
|---|---|
| What a screen, block or card must do, and what it must never do | `11-ux-guidelines.md` and the ADRs. These are normative. |
| Layout, order of controls, labels and sentences where the guidelines quote none | The prototype. Wording and layout shown in a scene are the reference for Component specs' acceptance criteria where the text is silent. |
| Which blocks exist and what data each takes | `11-ux-guidelines.md` §5.15 for the catalogue; the prototype's renderers (`R.<kind>`) show the fields each block reads. |
| How a flow runs from the person's side, step by step | `03-sequences.md` for the system side; the prototype scene for the person's side. |

**Precedence (UX-A55).** Where the prototype and the written design disagree, the written design wins and the disagreement is a defect, recorded in §4 until one side is changed. A Component spec never cites a scene for behaviour that §4 lists as open; it cites the guideline row and names the divergence id in its *Gaps*.

**What the prototype does not fix.** Component names, the interface framework (OQ-1, UX-OQ-2), colour values beyond the palette roles, timings, and sample data — names, numbers, companies and documents are illustrative.

**Citing a scene.** A spec cites a scene by its id as it appears in `SCENES`, for example `user-interface.html#scene=agentWithdrawn`. Scene ids are kept stable across revisions in the same way assertion ids are.

## 3. Scene index

| Show-menu group | Guideline section | Principal decisions |
|---|---|---|
| §5.1 Getting ready | §5.1, §6 | ADR-038, ADR-044, ADR-072 |
| §5.2 Adding an agent | §5.2 | ADR-012, ADR-013, ADR-082, ADR-086, ADR-087, ADR-089, ADR-090; from revision 0.34.0 also `libraryUses`, `libraryUnavailable`, `agentAsks`, `agentPartWithdrawn`, `agentProhibited` |
| §5.26 Making an agent | §5.26 | ADR-081 to ADR-090; from revision 0.34.0 also `makeLibOld`, `makeClash`, `makeChoose`, `makeRehFailed`, `makeFailed`, `makeEmptyRepo`, `makeClosed` |
| §5.27 Managing an agent repository | §5.27 | ADR-091, UX-A60; from revision 0.37.3; `partLifecycle` from revision 0.38.0 |
| §5.28 Handing work over | §5.28 | ADR-092, UX-A61; from revision 0.38.0; from revision 0.56.0 also Slack on the handover (ADR-102, UX-A72: `hoSlack`, `hoSlackCard`, `hoSlackNotFound`, `hoSlackRule`, `hoSlackRemind`, `hoSlackOpen`) and Jira handovers (ADR-106, UX-A75: `hoJira`, `hoJiraWhich`, `hoJiraNew`, `hoJiraCard`, `hoJiraIn`, `hoJiraBack`, `hoJiraDone`, `hoJiraRecord`) |
| §5.29 What an agent is made of | §5.29 | ADR-093, UX-A62; from revision 0.39.0 |
| §5.30 Importing, checking and fixing | §5.30 | ADR-094, UX-A63; from revision 0.39.0 |
| §5.31 Your ordin server on the desktop | §5.31 | ADR-096, UX-A66; from revision 0.42.0 |
| §5.31a Team knowledge | §5.31a | ADR-103, UX-A73; from revision 0.56.0: `tkOn`, `tkNew`, `tkShare`, `tkReview`, `tkReadOnly`, `tkNotTaken`, `tkPages`, `tkWords`, `tkOff` |
| §5.3 Starting a task | §5.3 | ADR-028, UX-A8, UX-A35 |
| §5.4 Showing progress, §5.7 Crews | §5.4, §5.7, §8 | ADR-027, ADR-068 |
| §5.20 Workflows | §5.20, §5.7 | ADR-064 to ADR-066 |
| §5.21 Reports and dashboards | §5.21 | ADR-067 |
| §5.22 Email | §5.22 | ADR-070, ADR-071 |
| §5.23–5.24 The ordin server, through Telegram and Slack | §5.23, §5.24 | ADR-073, ADR-074, ADR-076 |
| §5.25 Telegram for agents, on the desktop | §5.25 | ADR-075, ADR-077 |
| §5.25a Slack for agents and handovers | §5.25a | ADR-102, UX-A72; from revision 0.56.0: `skLink`, `skChannel`, `skPost`, `skGrant`, `skRule`, `skWords`, `settingsSlack` |
| §5.5 Approvals | §5.5 | ADR-046, ADR-080, UX-A57 |
| §5.6 Documents and changes | §5.6 | ADR-029, ADR-079 |
| §5.6 Markdown documents | §5.6 | ADR-069 |
| §5.8 Agents that run elsewhere | §5.8 | ADR-008, ADR-009 |
| §5.8 Hosted agents | §5.8, §5.31 | ADR-097, ADR-098, UX-A67; from revision 0.43.2 |
| §5.9 Recovery after a restart | §5.9 | sequence *Recovery after restart* |
| §5.10 Memory and knowledge | §5.10 | ADR-030, ADR-057; from revision 0.56.0 also knowledge page links (ADR-104, UX-A74: `knLinks`, `knRename`, `knTags`, `knSuggest`, `knOtherApp`) |
| §5.11 Engineering mode | §5.11 | ADR-050 to ADR-056 |
| §5.12 Conversation | §5.12 | ADR-046, ADR-078, UX-A58 |
| §5.13 Workspaces | §5.13 | ADR-025, ADR-029, ADR-051, UX-A33; from revision 0.56.0 also Jira issues (ADR-106, UX-A75: `jiraAdd`, `jiraCoApp`, `jiraBlocked`, `jiraTree`, `jiraChanged`, `jiraRemove`, `settingsJira`) |
| §5.14 Confluence | §5.14 | ADR-026, ADR-029 |
| §5.15 What Ordin shows, §5.16 Ordin as go-between | §5.15, §5.16 | ADR-028 |
| §5.17 Keeping track across workspaces | §5.17 | ADR-058, ADR-079; from revision 0.56.0 also ADR-105 (`every` changed, `everyServer`) |
| §5.18 Using your plan | §5.18 | ADR-062 |
| §5.19 How Ordin talks | §5.19 | ADR-063 |
| §6 Errors and repairs, §7 Notifications | §6, §7, §8 | ADR-072, UX-A4 |

ADR-107 (revision 0.55.0) changes what ordin does on every run of a hosted agent and adds no screen, block or sentence to `11-ux-guidelines.md`, so it has no scene.

## 4. Divergence register

The alignment review of January 2026 (revision 0.26.0) recorded UI-D1 to UI-D76. Revision 0.27.0 closed them all, except as listed below: the user's decisions 54 to 58 settled UI-D1 to UI-D5; 11 — UX guidelines took the guideline-side fixes; 09 — Decisions took the amendment notes (UI-D20); the prototype took the rest, with 36 new scenes, and every Show-menu entry runs headless with no errors. Ids are not reused.

Sentences the prototype shows and the guidelines do not quote are the reference under UX-A55; they are not divergences.

| ID | Divergence | Behind | Recommendation |
|---|---|---|---|
| UI-D109 | The prototype does not yet show the conversation picker or **Continue this conversation** (UX-A76). | Prototype | Add scenes for the picker, the confirmation from the tab and from the tree node, the continued conversation's fold, a moved item, and the crew wait. |
| UI-D110 | The prototype does not yet show **Settings › Language**, the Language screen, **Make a new language…**, import and export, or Ordin's offer to draft a missing language (UX-A77). | Prototype | Add scenes for the section, the Language screen on *Needs checking*, a refused edit, the make dialog with its estimate, the conversation offer, and an import report. |
| UI-D111 | The prototype does not yet show read folders, folders set by the company, **Adopt**, the marks, **Describe this folder…** or the recall check (UX-A78). | Prototype | Add scenes for "Also reads" with a company folder, a folder the person cannot open, the adopt dialog and a refusal, a fact marked as changed at its source, and the recall check line. |
| UI-D112 | The prototype does not yet show the **Things** tab, a thing with its links, fading with **Keep**, a fact's **History** and **Made from**, **Forget everything about this…** with its numbers, **Same as…**, **What Ordin used** under an answer, **Remembered during this run** on progress, or a hosted agent's memory on the operator screen (UX-A79). | Prototype | Add scenes for the Things list with "Explained in", a thing's detail with a link being corrected, a fading fact kept, a corrected fact's history, the forget-everything confirmation and its changed-numbers state, **What Ordin used** opened from an answer, and the run's remembered list. |
| UI-D113 | The prototype does not yet show Settings › Knowledge agents (add with the company sign-in and the sentence on what is sent and what is recorded, details, remove), knowledge-agent rows in **What Ordin used** with their answer reference, **More found** and "did not answer in time" under an answer, a page's **Type** and **Description** with the **Draft** and **Deprecated** marks, a glossary as a read folder, or the knowledge status on a knowledge agent's operator screen (UX-A80). | Prototype | Add scenes for adding a knowledge agent, an answer with knowledge-agent facts and **More found**, a page with its type, description and a deprecated mark, and a knowledge agent's operator status. |

## 5. Keeping the prototype and the design together

- A revision that changes a screen, block, label or sentence in `11-ux-guidelines.md` changes the prototype in the same revision, or adds a row to §4.
- The changelog line names the scenes added or changed by id.
- A divergence closes when both sides agree; its row is then removed and the changelog line names its id.
