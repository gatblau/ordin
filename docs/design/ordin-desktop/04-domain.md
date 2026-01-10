# ordin desktop — Domain Model and Data Lifecycle

> **Mode:** draft
> **Generated from:** inline context — the user's decisions of January 2026
> **Revision:** 0.58.0
> **Last updated:** January 2026

Key fields are named without types; `/spec` fixes types and shapes.

## Domain Model

- **Brand pack** (revision 0.10.0; ADR-041). The identity the product shows.
  - **Fields:** brand identifier, version, brand strings per locale (product name, coordinator name, About and legal text), logos and icon set (light and dark), design tokens, font files, help, support and privacy links.
  - **Relationships:** the build's own pack is fixed in the installer; a run-layer pack arrives in the organisation profile.
  - **Lifecycle:** received with a profile → contrast-checked → active, or refused with the build's pack kept.

- **Locale preference** (revision 0.10.0; ADR-042). The person's interface and dictation language.
  - **Fields:** locale, source (operating system or the person's choice).
  - **Lifecycle:** follows the operating system until the person chooses; limited to the locales the profile offers. *Amended in revision 0.58.0 (ADR-109): or to a person's language the person has confirmed.*

- **Person's language** (revision 0.58.0; ADR-109). An interface language the person made with Ordin or imported, or the person's edits to a language that comes with ordin.
  - **Fields:** language tag, kind (shipped with edits, made, imported), state, name given when made, the ordin version it was made for, and its messages — each with its wording, how it came (edited, drafted, imported), whether the person has checked it, and whether it passes the check against the current en-GB wording.
  - **Lifecycle:** a made language is *drafting*, then *needs checking*, then *in use* once the person confirms it; an imported one starts at *needs checking*; edits to a shipped language are in use at once, except imported safety wording, which waits for the person's check. After an update, messages that no longer pass the check show in en-GB; messages removed from en-GB are deleted. The person can remove a made or imported language that is not the active one.

- **Harness attachment** (revision 0.10.0; ADR-044). The application's record of one vendor harness on the computer.
  - **Fields:** harness (Claude Code or Copilot CLI), location, version, tested (yes or no), signed in (yes or no), sign-in method as the harness reports it, last checked.
  - **Relationships:** used by every local session of that harness.
  - **Lifecycle:** found → checked → ready, or needs sign-in, or needs update. Holds no credential.

- **Organisation profile.** The signed settings IT publishes (ADR-013).
  - **Fields:** profile version, signature, marketplace, executable-trust policy reference, remote agent catalogue, Slack workspace and dedicated channel, identity-provider settings, default and maximum limits, update feed, help-request destination.
  - **Relationships:** configures the marketplace, the remote agents and the guardrail ceilings.
  - **Lifecycle:** fetched → verified → active → replaced by a newer verified version. A profile that fails verification never becomes active.
  - *Amended in revision 0.48.0 (ADR-082): the profile has no marketplace field. In place of the marketplace it names the company's agent repositories (Agent repository, below), and it configures those repositories, not a marketplace.*

- **Marketplace.** A registered APM marketplace. In the first release there is exactly one, the company's. *From revision 0.25.0 replaced by **Catalogue repository** below (ADR-082).*
  - **Fields:** name, source address, last refreshed.
  - **Relationships:** offers many package versions.
  - **Lifecycle:** registered by the Setup Assistant from the organisation profile → refreshed in the background → replaced when the profile names a different one. The person never registers or removes it.

- **Package version.** One version of an AgentWright package, as the marketplace lists it. *From revision 0.25.0 an agent version is a WorkspaceProfile at a commit of an agent repository; an imported APM package becomes an agent draft (ADR-082).*
  - **Fields:** package name, version, description, kind (agent package or standalone skill package).
  - **Relationships:** installed as zero or more agent instances.
  - **Lifecycle:** read-only to the application.

- **Agent instance.** One installed package version in one agent folder. *From revision 0.25.0 it also records its agent repository, commit and lock file with a digest per part, and whether it is a trial of the person's own draft (ADR-082, ADR-083).*
  - **Fields:** instance identifier, display name, package name and version, agent folder path, lock file reference, audit result, authority (document-only, system-changing or routing-only), parsed declaration reference, executable-part decisions, status.
  - **Relationships:**
    - belongs to one package version and owns one agent folder;
    - may be allowed in many workspaces, with folder grants per workspace;
    - has many runs and folder grants;
    - may be a member or the coordinator of many crew charters.
  - **Lifecycle:** *installing* → *ready* or *refused* (audit failed) → *updating* → *ready* → *removed*. A *ready* instance whose required part was denied stays *ready* and reports the part as unavailable. *Amended in revision 0.36.0: an update that does not change what the agent may do or read moves it through* updating *between runs without a question, and its line says it was updated between runs; an update that changes what it may do or read waits for the person's choice (A-16).*
  - *Amended in revision 0.48.0: each instance's install record is owned by the Installers; the list of agent instances is owned by the Package Catalogue.*
  - *Amended in revision 0.49.0 (`/spec` Phase 3): the install record is a file `meta/install-record.json` beside the agent folder, outside it, written by the Installers; the Package Inspector's parsed model is a file per instance. Neither module has a journal table (2B-3).*

- **Executable-part decision.** Whether one hook, MCP server or bundled program of an instance is trusted. It is decided by the organisation's recommended executable set, never by the business user.
  - **Fields:** part name, kind, decision, source (organisation policy), when.
  - **Lifecycle:** created at install. Re-evaluated when an update changes the part or the organisation policy changes. A part that loses trust disables its instance until IT approves it again.

- **Place.** A folder the person gave the application, a workspace's own folder, or an agent folder, shown by a friendly name (agent folders are never listed to the person).
  - **Fields:** place identifier, friendly name, path, kind (the person's folder, the workspace's own folder, an agent folder, *the working copy of a SharePoint or Confluence link*, or *a coding task's worktree*; the last three are never listed; revision 0.49.0), added at.
  - **Relationships:** has many folder grants and many files.
  - **Lifecycle:** added through the system folder picker → renamed by the person → removed from the application. Removing a place never deletes the folder.

- **Folder grant.** Permission for one instance or crew to read, or read and write, one place, within one workspace (ADR-025).
  - **Fields:** workspace, instance or crew, place, access, granted at.
  - **Lifecycle:** granted by the person → revoked by the person, or when the instance is removed, or when the place leaves the workspace. *Amended in revision 0.36.0: the person sets it with a choice of three per agent, "can't use", "can read" or "can read and change"; "can't use" means no grant.*
  - *Amended in revision 0.48.0: folder grants are recorded by the Workspace Service, which owns them per workspace; the Agent Folder Manager applies them, and the File Service reads them. Neither of those two records a grant.*

- **Workspace.** The person's named home for one piece of work (ADR-025). Not the agent folder of an instance, which revisions before 0.7.0 called "workspace".
  - **Fields:** workspace identifier, name, created at, its own folder (for loose files, A-23), archived or not, last seen (when the person last had it open; revision 0.10.13, ADR-058).
  - **Relationships:** has many places (a place may belong to several workspaces), Confluence links, folder grants, conversations, task runs, crew runs and approvals.
  - **Lifecycle:** created by the person with a name → places and links added and removed → archived (hidden from navigation, everything kept) → removed. Removing a workspace never deletes a place's folder or a Confluence page; the workspace's own folder goes to the system's bin. *From revision 0.44.0 (UX-A68) removing first moves the workspace, with its links, conversation, history and unsent changes, to ordin's Bin, from which it can be restored; it is deleted only when the person deletes it there or empties the Bin.*
  - *Amended in revision 0.48.0: the workspace's own folder no longer cites A-23. A-23 is superseded by ADR-029: loose dropped files are linked where they are, not copied into this folder (10 — Risks). The own folder stays, as a place of the workspace (Place, above).*

- **Home marks** (revision 0.10.13; ADR-058). What the application keeps to tell the person what is new.
  - **Fields:** Home last opened, and per workspace the documents changed elsewhere that the person has not opened since (the changed marks, ADR-079).
  - **Relationships:** one per installation; read with each workspace's *last seen*.
  - **Lifecycle:** updated when the person opens Home or a workspace; kept on the computer only; removed with the installation. A workspace's entries go when the workspace is removed.

- **Source link** (revision 0.8.0). A workspace's pointer to where content lives (ADR-029).
  - **Fields:** workspace, kind (local folder, git repository folder, SharePoint location, Confluence location), location at the origin, display name, added at, last read at, *the place (local and repository folders; for SharePoint and Confluence links, their working copy; revision 0.49.0)*.
  - **Relationships:** has many working-copy items; a local or repository folder is also a place.
  - **Lifecycle:** proposed by Ordin in a file block → confirmed by the person → active → removed. Removing a link never deletes anything at the origin. *Amended in revision 0.36.0: removing it asks first, in a confirmation ("Remove “…” from this workspace?"), because it changes only this computer (UX-A57).*
  - *Amended in revision 0.54.0 (ADR-106):* **kind** also *Jira issue*, on the desktop only, with the issue's site and key as its location. Its working-copy items are read-only: the summary and description, each comment except ordin's record comments, each attachment, and one list of the issue's sub-tasks and linked issues, whose content is not read. Removing the link changes nothing in Jira.

- **Working-copy item** (revision 0.8.0). The local copy of one file or page read from an origin.
  - **Fields:** source link, path or page at the origin, origin version or change time when read (A-25), local copy's version.
  - **Lifecycle:** read → changed by an agent → *awaiting push* → pushed (new origin version recorded), or *origin changed* (push stopped; both versions shown) → re-read. *Amended in revision 0.49.0 (`/spec` Phase 3): also* awaiting push → changed *(push rejected, changes kept), and* pushed → read → changed *(a change after a push).*

- **Email link** (revision 0.15.0; ADR-070). A workspace's pointer to part of the person's mailbox.
  - **Fields:** workspace, kind (mail folder or label, or people and domains), the folder or label, or the list of addresses and domains, display name, added at, last checked at, count of messages new since the person last looked. *With the company's mail server (ADR-071):* for each folder read, its validity value and the highest message number seen (IMAP `UIDVALIDITY`, `UID`).
  - **Relationships:** belongs to one workspace; has many email grants; matches many messages, which stay in the mailbox and are never copied whole into ordin.
  - **Lifecycle:** proposed by Ordin or chosen on **Add to this workspace** → confirmed by the person → active → removed. Removing a link never deletes or moves a message. *Amended in revision 0.36.0: removing it asks first, in a confirmation ("Remove “…” from this workspace?"), because it changes only this computer (UX-A57).*

- **Email grant** (revision 0.15.0; ADR-070). Permission for one agent instance to use one workspace's email.
  - **Fields:** workspace, instance, access (**Read email**, or **Read and draft email**), granted at.
  - **Lifecycle:** granted by the person, usually when Ordin first asks → changed or revoked by the person, or when the instance or the workspace is removed. Ordin's read access needs no grant.

- **Email draft** (revision 0.15.0; ADR-070). An email written in ordin, kept on this computer until it is sent or discarded.
  - **Fields:** workspace, kind (new, reply, forward), the message it answers, From (the person), To, Cc, Bcc, subject, body, attachments (each: file and the place it comes from), drafted by (agent instance, Ordin's crew member, or the person), edited by the person (yes or no), state, created at.
  - **States:** *draft*; *waiting for you* (an approval card is open); *sending*; *sent*; *not sure it was sent* (after a crash, until the Sent folder is checked; with the company's mail server, until the message is found in Sent or the person chooses **Send again** or **Mark as sent**, ADR-071); *not sent* (failed, with the reason and **Try again**; with SMTP, including the refused address); *discarded*.
  - **Rules:** recipients and attachments are checked against the organisation profile's email rules when written and again before sending; only the Approval Service's card moves a draft to *sending*.
  - **Lifecycle:** drafted → shown to the person → edited (optional) → *waiting for you* → approved → *sending* → *sent*, or rejected → *draft*, or discarded. *Amended in revision 0.36.0: discarding asks first, in a confirmation ("Discard this draft?"; UX-A57).*

- **Sent email** (revision 0.15.0; ADR-070). The record of one email sent from ordin.
  - **Fields:** the draft, the approval, the external action's unique key, the mail system's message identifier (with the company's mail server, the Message-ID ordin gave it, recorded before sending), sent at, recipients, attachment names, saved in Sent (yes or no; ADR-071).
  - **Lifecycle:** written when the mail system accepts the message; kept with the journal's retention. The message itself stays in the person's Sent folder.

- **Pending push** (revision 0.8.0). The changes of one request, waiting for the person's approval (ADR-029).
  - **Fields:** workspace, request, working-copy items with a one-line summary each, origins, created at, state.
  - **Relationships:** one approval; one external action per item once pushed.
  - **Lifecycle:** proposed by Ordin → approved (pushed item by item) or rejected (changes kept locally) → closed; or *held* when the person changes a document locally meanwhile, until they choose a version (ADR-035).

- **Memory fact** (revision 0.8.0). One fact in the memory component's store (ADR-030); its fields are the memory component's.
  - **Relationships:** belongs to the installation and, unless the person moves it, to one workspace's scope.
  - **Lifecycle:** written by Ordin or a crew member → recalled → corrected or deleted by the person, or faded by the memory component.
  - *Amended in revision 0.48.0 (ADR-101; the user's decision of January 2026): the memory component's store sits behind a storage interface of its own, implemented on SQLite (in the journal's file, for the desktop, the personal server and the agent profile's single shape) and on PostgreSQL (for the agent profile's scaled shape). SurrealDB is not shipped with ordin. The fact's fields and behaviour are unchanged.*
  - *Amended in revision 0.60.0 (ADR-111):* written by any agent session whose agent may remember (Ordin, crew members, agent task sessions with memory rules) or, for a **summary**, by Ordin with the facts it was made from; shown by its kind (something that happened, something known, a summary), when it was true and what it replaced; a fact that happened and has not been used for most of its fading time is marked fading until it is used or the person chooses **Keep**, which makes it something known. Forgetting a fact also forgets every summary made from it.

- **Thing** (revision 0.60.0; ADR-111). Someone or something memory knows about; the memory component's *entity*.
  - **Fields:** name, other names (up to 32), the workspace scope it belongs to (and, for a read folder's facts, that folder's collection).
  - **Relationships:** named by one or more memory facts; joined to other things by links; may be explained in a knowledge page whose title or other name matches its name or an other name.
  - **Lifecycle:** created or found when a fact names it → other names added by later facts → made one with another thing by the person's **Same as…** → deleted with everything about it by **Forget everything about this…**, or when its last fact goes.

- **Link** (revision 0.60.0; ADR-111). A stated relation between two things; the memory component's *relationship*.
  - **Fields:** from thing, words of the relation (free text, lower-cased, merged with a near-identical kind), to thing, true from, true until (empty while it holds), confidence, the facts that stated or ended it, and the link it replaces when the person corrected one.
  - **Relationships:** belongs to the scope of the facts that state it; followed in recall only when the reader may read a current fact that states it.
  - **Lifecycle:** stated with a fact → stated again by other facts (one link) → closed when a fact says it no longer holds, or when every fact that stated it is replaced or retired → corrected (closed and replaced) or forgotten (deleted with proof) by the person.

- **Recall record** (revision 0.60.0; ADR-111). The ids of the facts and links one recall returned, with the run and who asked, so **What Ordin used** can show them under an answer.
  - **Fields:** recall id, run, workspace or agent scope, who asked (Ordin, crew member, agent, hosted agent), when; items by position (fact id or link id). Never a statement, a name or a query.
  - **Lifecycle:** written on every recall → read when the person opens **What Ordin used** → removed after the person's history setting (90 days by default) or, item by item, when the fact or link is forgotten.
  - *Amended in revision 0.61.0 (ADR-113):* in a workspace connected to a front door, an item may also be a knowledge-agent fact: its agent, repository, path, section, commit, "as of" and, when the agent keeps a sources record, the answer id. Still never a statement or the question.

- **Knowledge page** (revision 0.10.12). One Markdown page of a workspace's knowledge (ADR-057).
  - **Fields:** workspace, title, Markdown body, written by (Ordin or the person), edited by the person (yes or no), last changed, sources (each: origin, location, version read), state, published copies (each a *Published copy*, ADR-069).
  - **States:** current; may be out of date (a cited source has a newer version); removed (in the system's bin).
  - **Relationships:** belongs to one workspace; cites zero or more sources; may be cited by memory facts; versions kept by the File Service.
  - **Lifecycle:** written by Ordin when work finishes, or on the person's request → read by Ordin and agents → marked "may be out of date" when a source changes → brought up to date (the person uses the update) or left → edited by the person → published as a copy on approval → removed, with Undo.
  - An agent package's `knowledge/` pages appear alongside, read-only, with the package's name and version; they are not knowledge pages and have no lifecycle in ordin.
  - *Amended in revision 0.52.0 (ADR-104):* **Fields** also: other names, tags, header keys of other apps (kept as written); **Relationships** also: links to and from other pages of the same workspace, by page id; suggestions (link or possible duplicate) awaiting the person. **Lifecycle** also: created as a file in `knowledge/` by another app → adopted; renamed (in ordin or another app) → the pages that link to it rewritten; deleted in another app → removed, with Undo.
  - *Amended in revision 0.61.0 (ADR-112):* each page file is an Open Knowledge Format 0.2 concept. **Fields** also: type (Explanation, How-to, Decision, Reference or Glossary; another value read from a bundle is kept as written), description (one sentence), generated (by `ordin/<version>`, at), verified (each a verifier — `human:<Git host login>` for the person — and when), status (draft, stable or deprecated, read), stale after (read); each source also: resource, id, last modified. **States** also: draft, deprecated (recalled only when nothing else matches), may be out of date when its stale-after date has passed. **Trust** is derived: human-reviewed (a `human:` verifier), machine-confirmed (verifiers that are programs or processes only), unverified (none). **Lifecycle** also: a person's write, edit or **Use the update** adds a verified entry; a page written by another producer is adopted as a page with its keys kept. `index.md` and `log.md` are never pages.

- **Confluence link.** A folder of a workspace connected to a Confluence location (ADR-026).
  - **Fields:** workspace, place and folder within it, Confluence space, parent page, audience shown at approval, approved at, state (active or paused).
  - **Relationships:** has many published copies; created through one approval.
  - **Lifecycle:** proposed → approved by the person → active → paused by the person → active, or removed. Removing a link never deletes pages in Confluence.

- **Published copy** (revision 0.14.0; ADR-069; called *Published item* before, and limited to Confluence). One copy of an original at a destination where its readers are (ADR-026, ADR-048, ADR-069).
  - **Fields:** original (a place and file path, or a knowledge page); destination kind (Confluence page, Confluence attachment, or SharePoint file); destination location (Confluence space and parent page, or SharePoint site, library and folder) and the copy's identifier there; format (page, Word, PDF, slides, or the exact file for an attachment); maker (ordin, WordCraft, PdfCraft, DeckCraft, or none for an exact file); how it was published (a Confluence link, or **Publish a copy…**); followed branch, for an original in a repository; keep up to date (yes or no); audience shown at approval; version of the original last published (commit or file version); destination version last pushed; last updated at; state.
  - **States:** *ready to publish* (new file in a linked folder, awaiting the person's check); *awaiting approval* (first publish or an update waiting for its card); *current*; *out of date* (the original has a newer version than the one last published); *updating* (being remade and pushed); *changed at destination* (a newer version there than the one pushed; automatic updates of this copy stop); *failed* (with the reason and **Try again**); *not kept up to date* (the person turned it off); *removed*.
  - **Relationships:** belongs to one original and, when published through one, to one Confluence link; one external action in the journal per push.
  - **Lifecycle:** *awaiting approval* → approved → *current* → the original changes → *out of date* → remade and pushed, automatically when keep up to date is on and the change reached the original's origin, otherwise after the person's approval → *current*. Or *changed at destination* → the person brings the change into the original, or leaves it to be replaced at the next update → *current*. Or the person turns updates off → *not kept up to date*; or removes the copy, with a check before it is deleted at the destination → *removed*. A copy is never deleted at its destination without the person's check.

- **File version.** One state of a file in a place, as the File Service recorded it.
  - **Fields:** file path within its place, version number, hash, origin (person, run with tool, or ArtCraft app edit), created at.
  - **Lifecycle:** created on each change the application observes → pruned with the journal's retention, keeping the latest version always. A deleted file goes to the system's bin.

- **Project.** A place opened for software engineering.
  - **Fields:** place, display name, git repository or not, current branch, language servers in use, check commands available.
  - **Relationships:** belongs to one place; has many change sets.
  - **Lifecycle:** opened by the person → indexed → closed. Closing changes no file.

- **GitHub operation setting.** Whether a kind of `gh` operation needs the engineer's approval in one project.
  - **Fields:** project, operation kind, approval required (yes or no), set by, set at.
  - **Lifecycle:** defaults from the organisation profile (every write kind requires approval; reads do not) → changed by the engineer within the profile's ceiling → reset when the profile tightens the ceiling.

- **Language plug-in.** The language intelligence for one language.
  - **Fields:** language, approved servers and the default one, the engineer's chosen server and its pinned version, source (APM package or profile), on or off, the engineer's server settings (JSON), status.
  - **Lifecycle:** available (listed in the profile or a package) → added by the engineer in Settings, or on first use in a project of that language → installed on first use → updated when the pin changes → turned off or on by the engineer → removed. Turning a server off leaves highlighting (ADR-055).

- **Coding task.** One agent's work on one request that changes a linked repository, in its own git worktree (ADR-053).
  - **Fields:** run or crew step, project, agent instance, request, worktree, branch (`ordin/<task name>`), base branch and commit, attempt group (optional), test outcome, new problems, state.
  - **Relationships:** belongs to one project; has one change set; an attempt group holds two or more coding tasks on the same request.
  - **Lifecycle:** *working* → *ready for review* → *settled* when every change is accepted or rejected; or *set aside* when another attempt is kept. Its worktree and branch are removed when it is settled or set aside.

- **Change set.** The edits one coding task made to a project (before revision 0.10.9, one agent run).
  - **Fields:** coding task, run, project, entries (file, hunk, state: pending, accepted or rejected), created at.
  - **Lifecycle:** *open* while the run edits → *in review* → *settled* when every entry is accepted or rejected. Rejected entries are reverted in the working tree.

- **Craft installation.** One ArtCraft app and its CLI installed by the application.
  - **Fields:** app name, version pinned by the profile, installed version, verification result, CLI modes available (MCP, run, batch), status.
  - **Lifecycle:** approved in the profile → downloaded and verified → installed → updated when the pin changes → removed when the profile withdraws it.

- **File-type map entry.** One kind of file and the ArtCraft app that opens and edits it.
  - **Fields:** file kind (extensions and media types), app, the Craft tools offered for it.
  - **Lifecycle:** published in the organisation profile; changed only by a new profile.
  - *Amended in revision 0.36.0: the app that opens a kind for the person and the Craft tool agents change it with can differ. Word, Excel and PowerPoint open in the person's own Microsoft app (the system's default app); agents change Word through WordCraft and PowerPoint through DeckCraft, and do not change spreadsheets yet (GridCraft makes new ones when a report is saved). PDF opens in PdfCraft and images in PhotoCraft, which agents also use to change them. A kind with no entry offers "No editor for this kind of file — open it in your usual app?". The person edits by hand only Markdown documents, knowledge pages, agents' own memory pages and email drafts (ADR-029, ADR-048).*

- **Remote agent.** A wrightrun-deployed agent listed in the organisation profile's remote agent catalogue. The person picks it by name; addresses, routes and identity settings come from the profile.
  - **Fields:** display name, route (HTTP or Slack), the deployment's address (HTTP), the agent's Slack account and the dedicated channel (Slack), identity provider settings (HTTP), status.
  - **Relationships:** has many runs; may be a member of many crew charters.
  - **Lifecycle:** listed by the profile → enabled when the person first uses it (sign-in to the route's account) → verified (one successful read of the deployment or the channel) → withdrawn when the profile drops it.
  - *Amended in revision 0.48.0 (ADR-098): ordin replaces wrightrun, so a remote agent is a **hosted ordin agent**, an ordin server in the agent profile, not a wrightrun-deployed agent.*

- **Task.** What a person asked for.
  - **Fields:** target (agent instance, remote agent or crew), command or free text, arguments, attached artefacts, requested at, run again of (the earlier task, when started with Run again; revision 0.10.13, ADR-060).
  - **Relationships:** produces one run, or one crew run.
  - **Lifecycle:** immutable once submitted.

- **Run.** One execution by one agent.
  - **Fields:** run identifier, task or crew step, agent, adapter, runtime session or remote run identifier, remote thread identifier, state, limits, usage counters (model turns, tool calls, model level, and tokens and the share served from cache where the harness reports them; revision 0.10.14, ADR-062), started at, ended at, end reason, inputs used (each: name, where it lives, version read; revision 0.10.13, ADR-060, A-29).
  - **Relationships:**
    - belongs to a task or to a crew round;
    - has many tool requests, approvals and artefacts.
  - **Lifecycle:** *queued* → *running* → *paused* (awaiting approval, sign-in or the person) → *running* → ended as one of *completed*, *failed*, *stopped at limit*, *cancelled*, *interrupted*, *abandoned* (remote run the application stopped waiting for) or *no reply* (Slack).

- **Usage settings** (revision 0.10.14; ADR-062). The person's choices of how Ordin uses their plan.
  - **Fields:** choice (*Save my plan*, *Balanced*, *Best results* or *Custom*), model level per kind of work, thinking, answer simple requests without the model, reuse answers, give agents only the parts they need, summarise long conversations, continue from a summary after a break, ask before work estimated as, agents at the same time, show usage on each run, daily pause; each field marked as set by the person or fixed by the company.
  - **Relationships:** one per installation; bounded by the organisation profile's usage section.

- **Voice settings** (revision 0.10.14; ADR-063). How Ordin talks to the person.
  - **Fields:** tone (*Friendly*, *Neutral*, *Warm*, *Direct*), register (*Everyday*, *Formal*), length (*Brief*, *Shortest*), name to use (empty by default), notes (up to 300 characters); each marked as set by the person or by the company's default.
  - **Relationships:** one per installation; bounded by the organisation profile's voice defaults and company terms.

- **Reused answer** (revision 0.10.14; ADR-062). A journal entry recording that an earlier answer was shown again: the request, agent, workspace, input versions, the earlier run it came from and when. Expires after 7 days or when any input has a newer version. *Amended in revision 0.48.0: reused answers are indexed by the Usage Planner, which owns the answer-reuse index, and each reuse is also journalled.*

- **Tool request.** One tool call a local session asked for.
  - **Fields:** run, tool, arguments digest, the decision (allow, deny or ask), the rule that decided, the approval if any, outcome.
  - **Lifecycle:** created on request → decided → executed or refused. Immutable after that.

- **Approval.** A person's decision on one proposed action.
  - **Fields:** run or crew round, action, destination, affected records, consequence text, state (pending, approved, rejected, expired), decided by, decided at.
  - **Lifecycle:** *pending* → *approved*, *rejected* or *expired* (its run ended first). Survives a restart while pending.

- **External action.** An action with effect outside the application: a Slack post as the person, a write through a declared connection, a wrightrun run started. *Amended in revision 0.49.0 (`/spec` Phase 3): kinds add* pull request opened *and* handover written.
  - **Fields:** unique action key, the run that took it, *when one did — publishing, handovers, **Publish a copy…**, a person's own email send, automatic updates of copies and linked-folder publishing have none (revision 0.49.0)*, kind, destination, state (sent, confirmed, unknown outcome).
  - **Lifecycle:** *sent* → *confirmed*, or *unknown outcome* after a crash. Never resent automatically. *Amended in revision 0.49.0 (`/spec` Phase 3):* unknown outcome → confirmed when the person chooses *It arrived — mark as done* (03 — Recovery after restart); *Send it again* creates a new action with a new key after a new approval.
  - *Amended in revision 0.48.0 (ADR-098): "a wrightrun run started" reads "a hosted ordin agent's run started".*

- **Artefact.** A file a person supplied or a run produced.
  - **Fields:** artefact identifier, path in its agent folder or place, version, hash, producing run, media type.
  - **Lifecycle:** created → new versions on edit → exported → deleted with its agent folder or by the person (an artefact in a person's place is moved to the system's bin, never deleted).

- **Crew charter.** The definition of a crew.
  - **Fields:**
    - name, purpose text, charter version;
    - the coordinator (Ordin from revision 0.8.0), and the members (agent instances and remote agents) with their roles;
    - the allowed hand-offs;
    - the guardrails: maximum rounds, wall-clock limit, request budget, maximum members running at once, members allowed to take externally visible actions, approval points, and failure-repeat limit;
    - the input fields of the purpose;
    - *from revision 0.11.0*, the workflow and version it was written from, and that workflow's steps, when it has them (ADR-064).
  - **Relationships:** has many crew runs.
  - **Lifecycle:** from revision 0.8.0, written by the Ordin Host when Ordin forms a crew (ADR-027) → valid (checked against installed instances and the remote agent catalogue) → used once and discarded, or *kept* under a name on the person's request → reused by Ordin → archived. *Amended in revision 0.36.0: Ordin shows a charter it writes as a crew plan, and nothing starts until the person presses **Start** (UX-A35). From revision 0.11.0 a crew is kept as a Workflow (below), not as a named charter (ADR-064).* Before revision 0.8.0: built in the guided crew builder → valid (checked against installed instances and the remote agent catalogue) → edited in the builder (new charter version) → archived. The file form exists for export and import; the person never edits it.
  - *Amended in revision 0.48.0 (ADR-064): a charter is no longer* kept *under a name on the person's request. Crews are kept and shared as workflows (Workflow, below), and a charter is used once and discarded or archived.*

- **Crew run.** One execution of a charter.
  - **Fields:** crew run identifier, charter and version, workflow and version (from revision 0.11.0; empty for a crew Ordin coordinates as it goes), the step from which Ordin took over (empty when the steps fitted), inputs (each with where it lives and the version read, from revision 0.10.13; ADR-060, A-29), state, round counter, budget counters, end reason.
  - **Relationships:** has many rounds; each round has one coordinator turn and the member runs it delegated.
  - **Lifecycle:** *starting* → *running* → *paused* (approval, restart, escalation) → ended as one of *finished*, *escalated*, *stopped at limit*, *killed* or *failed*.

- **Workflow** (revision 0.11.0; ADR-064). A named, versioned list of steps a crew follows, owned by one workspace.
  - **Fields:**
    - name, purpose text, version number, a one-line note of what changed in each version;
    - inputs: label, kind (document or text), required or optional;
    - agents: each agent instance or remote agent, with its marketplace package and the range of versions it was saved with, and its role in one line; *Amended in revision 0.48.0: named by agent repository, identifier and version (ADR-082; workflow file format 3).*
    - steps (Workflow step, below), in order;
    - limits: the guardrails of the Crew charter, each at or below the company's maximum;
    - origin: saved from a crew run (with the run), drafted by Ordin, duplicated (from which workflow and version), or imported (from which file name, on which date);
    - state: *not saved yet*, *saved*, or *needs agents* (an agent it uses is not in the workspace);
    - *from revision 0.12.0 (ADR-066)*, where it is shared: GitHub or GitLab, repository and folder, file name, the version followed (date, by whom, commit), whether it has changed here and not yet been sent back, and whether a newer version is in the repository.
  - **Relationships:** belongs to one workspace; has many versions; each crew run of it records the version used.
  - **Lifecycle:** drafted (from a run, by Ordin, by duplicating or by importing) → *not saved yet* → saved as version 1 → changed (each **Save** a new version; **Restore** a new version equal to an earlier one) → removed. A version that fails the core's check is never saved. Exporting writes a file and changes nothing. *Amended in revision 0.36.0: the person can rename a workflow. The name is required, at most 80 characters, and not already used by a workflow in the workspace; its versions, runs and shared location are kept.*

- **Workflow step** (revision 0.11.0; ADR-064). One entry in a workflow's list.
  - **Fields:** position; group (steps sharing a group start at the same time); agent; line (what it does, in one sentence); uses (named inputs, or the results of earlier steps by position); *asks first* (true or false); *Ordin checks the result* (true or false).
  - **Rules:** a step uses only inputs and results of steps before it; no condition, branch or loop; a group holds at most the company's maximum of agents running at once. *From revision 0.12.0 (ADR-065):* a step is either an agent step, with **if it fails** (*Ordin takes over*, *ask me*, *skip it*), or a **control block** of one kind — *if* (condition, two to four named paths), *each* (the list: a list input or an earlier step's list result; a maximum of at most 50), *until* (condition; a maximum of at most 10), *call* (another workflow of the workspace). A condition has who decides (*application* with a kind and the step it looks at, *Ordin* with words, or *person* with words). Blocks hold paths of steps, nested at most 4 deep, at most 50 steps and blocks in all; a step uses a result only where that result exists on its path; calls go at most 3 levels deep, with no cycle.

- **Report** (revision 0.13.0; ADR-067). Data an agent or Ordin shows as tables, charts and key figures.
  - **Fields:** title; "as of" time; parts (one to twelve), each a kind (table, chart, key figures), a heading, the data (typed columns and rows; or chart kind, axes and series; or figures), and its source (documents with the version read, a company system and the time of the read, or an earlier result, with the agent that worked the numbers out); the run that produced it; *as a dashboard:* name, the task or workflow and the inputs that made it, and each refresh's run.
  - **Rules:** data only — no markup, script, style, formulas or expressions; every part has a source; within A-34's limits.
  - **Lifecycle:** produced by a run → shown → kept as a dashboard (optional) → refreshed (each refresh a new run and a new "as of") → removed with the dashboard or pruned with its run. *Amended in revision 0.36.0: a dashboard's name is unique in its workspace. Each refresh is summarised with its new "as of" and what changed, and Home shows "Refreshed [name]". The person removes a dashboard after a confirmation ("Remove the dashboard “…”?"; UX-A57).*

- **Run step** (revision 0.13.0; ADR-068). One step of the plan shown on a progress screen.
  - **Fields:** position, line (at most 80 characters), where it came from (workflow, crew plan, agent's plan, harness to-do list, added by Ordin), mark (*next*, *now*, *done*, *skipped*), when each mark was set, the events seen while it was *now*, and the plan version it belongs to.
  - **Rules:** three to seven steps per plan; a changed plan is a new plan version, recorded and announced.
  - **Lifecycle:** stated → marked *now* → *done* or *skipped*; immutable once its run ends.

- **Round.** One coordinator turn and its delegated steps.
  - **Fields:** number, coordinator decision, guardrail decisions, member runs.
  - **Lifecycle:** immutable once closed.

- **Start-up report.** What a session or crew carries across and enforces.
  - **Fields:** enforced rules, rules not enforced with the reason, unavailable parts, available tools, adapter verbs not offered.
  - **Lifecycle:** generated at each start; stored with the run or crew run.

- **Credential.** A token of the person: GitHub, *GitLab (revision 0.49.0, ADR-051)*, OIDC, Slack, Confluence, Microsoft (SharePoint and, from revision 0.15.0, Outlook email) or Google (Gmail, ADR-070), or the company mail server's sign-in: a single sign-on token, or the person's mail password or app password (revision 0.16.0, ADR-071).
  - **Fields:** kind, account label, expiry.
  - **Lifecycle:** created at sign-in → refreshed → revoked at sign-out. The secret itself lives only in the OS keychain.

- **Channel pairing** (revision 0.18.0; ADR-073). The one chat account an ordin server answers.
  - **Fields:** channel kind (Telegram first), the chat service's user identifier, private chat identifier, paired at, paired by (the setup command run), state.
  - **Relationships:** one per server installation; open card references and channel messages belong to it.
  - **Lifecycle:** pending (code shown, valid 10 minutes) → paired → ended, only through the setup command. A new pairing ends the previous one.
  - *Amended in revision 0.48.0 (ADR-076): one chat account per channel kind, not one per server installation. Telegram and Slack may both be paired at once, and a card then goes to both; a new pairing ends only the previous pairing of the same channel kind.*

- **Desktop pairing** (revision 0.42.0; ADR-096). An ordin desktop that an ordin server lets use its Remote API.
  - **Fields:** the computer's name, the hash of its desktop key, paired at, last connected at, last sequence number sent, state.
  - **Relationships:** many per server installation, all of its one person; open event streams belong to it.
  - **Lifecycle:** pending (code shown, valid 10 minutes) → paired → ended, by **Disconnect this computer** on the desktop or through the setup command. Ending closes its event stream at once.

- **Server connection** (revision 0.42.0; ADR-096). The desktop's record of one ordin server it shows.
  - **Fields:** the server's display name, its address and port, the Service API version it last reported, last sequence number received, state (connected, not connected, disconnected, refused for its version). The desktop key itself lives only in the keychain.
  - **Relationships:** one per paired server on this desktop; the server's workspaces, conversation and *Needs you* items shown on the desktop belong to the server, never to the desktop's own workspaces (OQ-39).
  - **Lifecycle:** created when pairing succeeds → connected ⇄ not connected → disconnected when the person ends it or the server refuses the key; removed with **Remove this server** in Settings › Servers.

- **Card reference** (revision 0.18.0; ADR-073). The random, single-use reference a chat button carries for an approval or question card.
  - **Fields:** reference (fits the channel's button data; Telegram's 64 bytes), card, button (approve, not now, show details), created at, used at.
  - **Lifecycle:** created when the card is sent → used once → closed; closed as well when the card is decided elsewhere or the run ends.

- **Trigger** (revision 0.19.0; ADR-074). A saved rule on the ordin server that starts one workflow.
  - **Fields:** name; kind (chat file, NATS event); listens for (file kind and name pattern, or a subject within the profile's list); delivery (JetStream consumer name, or core NATS); filter conditions; input mapping (input → payload field or fixed value); identifier source (`Nats-Msg-Id`, CloudEvents `id`, or a named field); workflow and pinned version; limits; state (on, off, paused by the daily limit); turned on by, at.
  - **Relationships:** belongs to one workspace; starts runs of one workflow; has many trigger events.
  - **Lifecycle:** proposed by Ordin as a card → on (the person's **Turn on**) → off (**Turn off**, the workflow removed, or its subject removed from the profile) → removed.

- **Trigger event** (revision 0.19.0; ADR-074). One event or chat item a trigger received.
  - **Fields:** trigger, identifier, received at, outcome (started, skipped by the filter, duplicate, refused by a limit, unreadable), run started (if any). Not the payload.
  - **Lifecycle:** recorded before acknowledgement; identifiers kept 7 days; outcomes kept as the journal.

- **Telegram link** (revision 0.20.0; ADR-075). A group, forum topic or channel linked to a workspace.
  - **Fields:** chat identifier, topic (if any), title, kind, linked at, bot state (member, administrator, removed), member count when last read.
  - **Lifecycle:** offered once the bot is added → linked → removed by the person (kept messages deleted) or marked when the bot is removed. *Amended in revision 0.36.0: removing it asks first, in a confirmation ("Remove “…” from this workspace?"), because it changes only this computer (UX-A57).*

- **Kept Telegram message** (revision 0.20.0). A message the bot received in a linked chat: chat, message identifier, sender name, time, text, reply to, file list. Deleted with the link. *From revision 0.36.0 the person sees kept messages on a Telegram message list screen, and a file from one is saved to the workspace with* Save file *only after it is checked.*

- **Telegram draft and post record** (revision 0.20.0). A draft by an agent or Ordin (chat, reply to, text, files, drafted by), then the post's external-action key and Telegram's message identifier once posted.

- **Slack link** (revision 0.50.0; ADR-102). A Slack channel the person belongs to, linked to a workspace.
  - **Fields:** Slack workspace identifier, channel identifier, channel name, private or public, whether Ordin looks at new messages, state (linked, no access), linked at, read up to.
  - **Relationships:** belongs to one workspace; a channel is linked to at most one workspace of the person.
- **Kept Slack message** (revision 0.50.0). A message read from a linked channel: channel, time stamp, thread, sender's member identifier and name, time, text, whether the person wrote it. Deleted with the link.
- **Slack draft and post record** (revision 0.50.0). A draft by an agent or Ordin (channel, thread, text, drafted by); a post record for every Slack post (destination — a channel or one person's direct message — text, what caused it — a handover notice or a draft — what allowed it — a card or a rule — external-action key, state, Slack's time stamp and the message's link).
- **Slack rule** (revision 0.50.0). A rule the person set once: kind (handover notices of one stage, or Ordin's answers in one linked channel), channel, daily cap of 1 to 20, on or off, when set and when turned off.

- **Team knowledge binding** (revision 0.51.0; ADR-103). A workspace in Team Knowledge mode and the repository folder it shares through.
  - **Fields:** workspace, the repository folder link, host, repository, branch, folder, access (read and write, or read only), turned on at, last checked, last got, last shared, what is new from the team and what waits to be shared (counts), facts held for the person's check, and, per shared file, its version as last seen on the branch and the digest of the local item it matched.
  - **Relationships:** one per workspace; several people's workspaces may bind the same folder. *Amended in revision 0.59.0 (ADR-110): now called a **Team folder** — a workspace has at most one with the role *own* and up to 20 with the role *read*; a read folder records whether the person added it, the company named it, or both, and may be part of another team's folder.*
  - **Lifecycle:** turned on with **Share knowledge with a team…** → checked, got and shared → turned off with **Stop sharing with the team**, or when its link is removed or its workspace deleted; turning off deletes the binding and its records and leaves every local item and every shared file.
- **Team share** (revision 0.51.0). One share: its approval, the commit it was built on and the commit it made, the review request when the branch requires one, the counts, and its state (proposed, approved, sending, waiting for review, done, rejected, failed, abandoned, superseded).
- **Adoption record** (revision 0.59.0; ADR-110). The record of one adopted fact or page: the read folder it came from, the source item and the version of its file when adopted, who adopted it and when, whether the text was changed, the copy's version, and any mark (changed at its source, removed at its source).
  - **Relationships:** one per adopted item of the own folder; shared as its own file beside the item.
  - **Lifecycle:** made by **Adopt** → marked when a get of the source folder brings a new version or none → **Adopt again** (new version) or **Keep as it is** → removed with the adopted item.

- **Agent repository** (revision 0.25.0 as *Catalogue repository*, ADR-082; revision 0.27.0, ADR-086, ADR-087). A GitHub repository holding agents and their parts in one agent format, named in the organisation profile or added by the person.
  - **Fields:** display name, host and repository, protected branch, format (ordin or APM) and format version, source (the profile, the person, *or ordin itself for the catalogue bundled with ordin (shown "From: ordin", revision 0.49.0)*), signed merges required (yes or no; profile repositories only), the person can publish (yes or no, from the last check), last fetched commit, withdrawn list. *Amended in revision 0.37.0 (ADR-091): the withdrawn list is ordin's own file (Withdrawn list entry below), and the repository also holds the parts it offers (Part below); the person can curate it (yes or no) when they can open a review in it.*
  - **Relationships:** offers many agent versions; holds the person's drafts on branches of their clone.
  - **Lifecycle:** named by the profile, or added by the person on **Add an agent repository…** (unless the profile turns adding off) → cloned → fetched in the background → dropped when the profile no longer names it, or removed by the person (their own only) with a confirmation. Removing one does not remove agents already added from it; they stop receiving updates, and the library says so.

- **Agent draft** (revision 0.25.0; ADR-083). An agent being made, from a conversation with the agent maker or from an imported APM package.
  - **Fields:** name, purpose, jobs (each with where it came from), tasks, will, won't, needs, reused parts, origin (made or imported), branch, check results, wording changes, rehearsal, trial (yes or no), pull request and its state.
  - **Relationships:** belongs to one person and one agent repository, the one chosen in **Publish to**; has at most one open pull request.
  - **Lifecycle:** *draft* → *waiting for review* → *changes asked for* → *waiting for review* → *published* (merged), or *discarded*. A closed pull request returns it to *draft*. *Amended in revision 0.36.0: where the agent repository requires signed merges, an approved change that is not signed is* approved, not signed yet *until it is signed, then* published *(ADR-084).*

- **Rehearsal** (revision 0.25.0; ADR-083, ADR-085). One run of a draft on a set of situations.
  - **Fields:** draft and its version, situations (each: description, the documents used, fixed expectations, outcome per expectation, the reviewing agent's opinion), when.
  - **Relationships:** belongs to one draft; its runs are in the journal.
  - **Lifecycle:** written once. A changed draft needs a new rehearsal before trying or publishing.

- **Part** (revision 0.37.0; ADR-082, ADR-091). One part of one of the 25 kinds of the ordin format, as the Catalogue Service ingested it from an agent repository's protected branch.
  - **Fields:** agent repository, kind, identifier (`metadata.id`), name, version, status (`draft`, `approved` or `deprecated`; absent reads as `approved`, `crucible/pkg/types/status.go`), the part it replaces (`metadata.supersedes`, optional), owner, file path, commit, content digest, the vocabulary terms it cites, the parts it uses (dependency edges), withdrawn (yes or no, from the withdrawn list).
  - **Relationships:** belongs to one agent repository; uses many parts and is used by many (the reverse closure gives **Used by**); replaces at most one part and is replaced by at most one; has at most one open part draft per person.
  - **Lifecycle:** *approved* (merged) → *deprecated* (still works, not reused in new agents; refused as an agent of its own) → *withdrawn* (an entry on the withdrawn list; agents that use it stop or move to the replacement). A part also changes version by a merged part draft. Withdrawal is final: a withdrawn part returns only as a new part that names it in `metadata.supersedes`.

- **Part draft** (revision 0.37.0; ADR-091). One part being created or changed on its own, outside any agent draft, or one change to the repository's catalogue: a status change, a withdrawn-list entry or a vocabulary term.
  - **Fields:** agent repository, kind of change (new part, changed part, deprecation, withdrawal, vocabulary term), the part's fields, the version ordin computed and the person's answer to "Does this change what the part does?", **Used by** at the time of the check, branch, check results (part and whole repository), wording changes, who wrote each field (the person, a model then the person, or a model; ADR-090), pull request and its state.
  - **Relationships:** belongs to one person and one agent repository the person can open a review in; concerns one part (none for a new part until it is merged); has at most one open pull request.
  - **Lifecycle:** *draft* → *waiting for review* → *changes asked for* → *waiting for review* → *published* (merged), or *discarded*. A closed pull request returns it to *draft*. Where the repository requires signed merges, an approved change that is not signed is *approved, not signed yet* until it is signed (ADR-084).

- **Withdrawn list entry** (revision 0.37.0; ADR-090, ADR-091). One entry of an agent repository's withdrawn list, in the file format ordin defines (proposed as `withdrawn.yaml` at the repository root, with `listSchema: ordin.withdrawn/v1`; `/spec` confirms both). Its fields come from crucible's `RevocationEntry` (`crucible/pkg/types/revocation.go`), without the signing fields.
  - **Fields:** part (kind, identifier, and optionally one version with its content digest; with neither, every version), reason, withdrawn at (RFC 3339), replacement (identifier and version, optional).
  - **Relationships:** belongs to one agent repository's withdrawn list; names one part and at most one replacement part.
  - **Lifecycle:** written once, by a merged pull request; never changed or removed. The Catalogue Service keeps every entry it has read, so an entry removed by hand stays in force and the removal is reported to Diagnostics.

- **Lifecycle** (revision 0.38.0; ADR-092). A part in an agent repository naming the stages of a piece of work handed between people; the `Journey` kind, in ordin's format or read from crucible's seams.
  - **Fields:** agent repository, identifier, name, the stages in order (each: name, role in plain words, agent, what it hands over, handover place, how many days the receiver has to answer (optional; *absent means no reminder*; revision 0.49.0); *from revision 0.45.0, ADR-099,* what comes next — the stage after it, one of several with when each is chosen, several side by side, or nothing), status, version. *Amended in revision 0.50.0 (ADR-102): each stage may also name a Slack channel (identifier and name), where its handovers are posted.*
  - **Invariants (revision 0.45.0, ADR-099):** every stage can be reached from the first; no stage leads to an earlier one; a stage several stages lead to starts when every one under way has handed over; *from revision 0.46.0* a stage may declare send-backs to earlier stages, each with when and at most 1 to 10 rounds, counted per handover.
  - **Relationships:** a Part of kind `Journey`; each stage names at most one agent (WorkspaceProfile), one OutputContract and one handover place; a workspace follows at most one stage.
  - **Lifecycle:** as a Part (written as a part draft, published by a merged pull request, deprecated or withdrawn).

- **Handover** (revision 0.38.0; ADR-092). One person's results and note, handed to one other person.
  - **Fields:** identifier, title, lifecycle and stage (optional), sender, receiver (name, address, whether they use ordin, *and, for a Git handover place, their GitHub login (revision 0.49.0)*), results (links to where each lives), note (four headings, drafted-by-Ordin marks), checks passed, handover place, round, events (sent, seen, accepted, sent back with reason, taken back; each with who and when). *Amended in revision 0.50.0 (ADR-102): and the links of the Slack messages posted about it (which notice, link, by whom, when), appended by the installation that posted.*
  - **Relationships:** sent from one workspace; read by the receiver's installation from the handover place; accepted into one workspace of the receiver's; journalled on both sides.
  - **Lifecycle:** *draft* (only the sender) → *sent* → *seen* → *accepted*, or *sent back* → *draft* (next round) → *sent*; *taken back* by the sender at any point before *accepted*. Accepted and taken back are final.
  - *Amended in revision 0.54.0 (ADR-106):* **Fields** also, for a Jira handover place: the issue it travels on (site and key), each person's Jira account id, and an identifier on each event so a retried write is never recorded twice. **Relationships** also: on Jira, the record is the issue's comments — one with the note and the record without its events, then one per event — and accepting it links the issue to the receiving workspace.
  - *Amended in revision 0.48.0: the stored state for a handover sent back to its sender is `returned` (in place of* sent back*, in the lifecycle and the events), and the person sees it as "Return to [name]…"; "Send back" is not used for handovers (11 — UX guidelines, from revision 0.44.0).*

- **Handover place** (revision 0.38.0; ADR-092). The Git repository or SharePoint folder a handover travels through. *Amended in revision 0.54.0 (ADR-106): or a Jira project, on the company's Jira Cloud site.*
  - **Fields:** type (Git or SharePoint; *Jira from revision 0.54.0*), address, display name.
  - **Relationships:** named by a lifecycle stage, or chosen from the workspace's links outside a lifecycle; holds the handover record files (08 — Interfaces).

- **Vocabulary term** (revision 0.37.0; ADR-091). One term of one of the eight vocabulary axes of crucible (`VocabularyAxis`, `crucible/pkg/types/vocabulary_set.go`), held in an agent repository's `vocabulary` part for that axis.
  - **Fields:** axis, code, label, description, the parts that cite it (from the index).
  - **Relationships:** belongs to one vocabulary part, so to one agent repository; cited by many parts of that repository.
  - **Lifecycle:** added by a merged part draft → cited by parts → retired by a merged part draft, which the Checker allows only when no part of the same repository cites it. The status terms `draft`, `approved` and `deprecated` are never retired. The set of axes is fixed in ordin's code and is not changed by a repository.

- **Server profile** (revision 0.43.0; ADR-097). Which way an ordin server runs.
  - **Fields:** profile (personal or agent), shape (single or scaled, agent profile only), chosen at setup.
  - **Relationships:** one per server installation.
  - **Lifecycle:** set by the setup command → fixed for the installation's life.

- **Agent configuration** (revision 0.43.0; ADR-098). Every per-agent choice of a hosted agent; wrightrun's configuration format with ADR-097's optional keys.
  - **Fields:** as `agent-wright-runtime/docs/design/agent-runtime/08-interfaces.md` lists for the configuration file, plus who may reach the agent (channels, users, groups), approvers, whether the asker may approve actions that stay inside the company, and the connections marked external. No secret: credentials are named, and their values are in the server's secret store.
  - **Relationships:** one per agent release; checked by the Agent Configuration module.
  - **Lifecycle:** written by a team → checked → published in an agent release; changed only by publishing a new release.
  - *Amended in revision 0.61.0 (ADR-113):* **Fields** also, optional: the **knowledge-agent section** — the agent's name; repositories and glossaries (each an address, an optional sub-path and branch, the name of the secret variable whose credential reads it, and the operator's statement that the whole audience may read it), audience groups and the claim that carries them, front doors allowed to ask, proxies to ask with the service identity used to ask them, the most proxies asked for one question, the daily reading budget, the fetch interval, the hot-copy threshold, the MCP endpoint switch, and the sources record switch with its retention period. An agent whose configuration has this section is a **knowledge agent**.

- **Agent release** (revision 0.43.0; ADR-098). The immutable artefact a hosted agent runs.
  - **Fields:** reference, digest, bundle, agent configuration and the files it names, record (package name and version, the ordin version and contract version it was prepared with, the check's start-up report).
  - **Relationships:** held by the container registry; named by one deployment at a time; obtained by every instance at start.
  - **Lifecycle:** published (check passed) → obtained → running; never changed. A changed package or configuration is a new release.

- **Run record** (revision 0.43.0; ADR-098). One run of a hosted agent.
  - **Fields:** run and thread identifiers, who started it (caller identifier, schedule entry, intake, or chat connection with the asker), scope, command and arguments or message, gathered context, state, lease holder and expiry, result, summaries per stretch.
  - **Relationships:** belongs to one thread; one thread takes one run at a time; owned by the Run Queue.
  - **Lifecycle:** *queued* → *executing* (lease) → *paused* (approval; session record stored) → *queued* (decision) → … → *ended* (completed, failed, run limit) or *cancelled*; *interrupted* when a lease lapses, never started again.

- **Scope** (revision 0.43.0; ADR-097). Where a hosted agent keeps content.
  - **Fields:** kind (channel, direct chat, agent-wide), **the chat connection it belongs to** (revision 0.49.0), the chat system's identifier, enabled or disabled, erased mark.
  - **Relationships:** holds memory pages, notes, knowledge items, setting values and conversation of that scope; a direct chat's content never moves to another scope.
  - **Lifecycle:** enabled when the agent's account is added to the channel (or the first direct message from a listed user) → disabled when removed → erased by an operator, after which it stays silent until removed and added again.
  - *Amended in revision 0.61.0 (ADR-113):* **kind** also *knowledge*: exactly one per knowledge agent, created at its first start, holding the facts, things and links read from its repositories. It belongs to no chat connection, holds no conversation, and is answered from only through the knowledge query route group.

- **Approver list** (revision 0.43.0; ADR-097). Who decides a hosted agent's approvals.
  - **Fields:** users and identity-provider groups; whether the asker may approve actions that stay inside the company.
  - **Relationships:** part of the agent configuration; read by the Approval Service.
  - **Lifecycle:** as the agent configuration.

- **Knowledge connection** (revision 0.61.0; ADR-113). A front door a workspace asks.
  - **Fields:** workspace, front door address (HTTPS), display name, added by (the person or the company), the profile entry it came from, whether the person removed a company entry, when added.
  - **Relationships:** belongs to one workspace (at most 8); asked by the Memory Service's recall through the Knowledge Query Client; one table of contents cached per connection.
  - **Lifecycle:** added by the person, with their company sign-in, or from the organisation profile → asked on recalls that are not confident → removed by the person (a company entry stays removed and is not added again). Adding and removing are journalled without any question text.

- **Table of contents** (revision 0.61.0; ADR-113). What one knowledge agent covers.
  - **Fields:** agent name, repositories with their last commits and fact counts, the 500 most-connected things with their other names and, where a glossary names them, their shared identity, when built. No statement.
  - **Relationships:** built by the Knowledge Reader, published by the Knowledge Query Server, cached by front doors and desktops; also published as an Open Knowledge Format `index.md`.
  - **Lifecycle:** rebuilt after each fetch → replaces the previous one.

- **Knowledge answer** (revision 0.61.0; ADR-113). One result a knowledge agent returns.
  - **Fields:** ranked facts (each: statement, agent, repository, path, section, commit, page title where page-backed, trust, status, "as of", shared identity), the agents that did not answer in time, answer id (when the sources record is on).
  - **Relationships:** cached by the front door per access group with the commits it relied on.
  - **Lifecycle:** built → cached → dropped when a commit it relied on moves, or when the cache is full.

- **Sources record entry** (revision 0.61.0; ADR-113). The optional record of one answer a knowledge agent served.
  - **Fields:** answer id, time, access group, and each returned fact's agent, repository, path and commit. Never the question, the asker's identity or a statement.
  - **Relationships:** kept by the Knowledge Query Server; joined by an auditor with the answer id kept in the person's own record to show which sources an answer used.
  - **Lifecycle:** written for each answer while the record is on (off by default) → deleted after the retention period (365 days by default).

- **Glossary concept** (revision 0.61.0; ADR-112, ADR-113). One thing an organisation names, as an Open Knowledge Format concept of type `Glossary` in a Git repository.
  - **Fields:** preferred name (title), other names (`aliases`), description, links to related concepts; the repository and path, which together are its **shared identity**.
  - **Relationships:** read by knowledge agents that list its glossary, and by a person's ordin as a read folder; gives the things it names their preferred name.
  - **Lifecycle:** owned and changed by a team through Git review; when two glossaries an agent lists give one other name to different things, the first listed wins and the agent's status names the conflict.

- **Schedule firing, Job record, Setting value, Knowledge entry, Note** (revision 0.43.0; ADR-098). Kept as wrightrun's design defines them (`agent-wright-runtime/docs/design/agent-runtime/04-domain.md`), owned by the Duty Timer, the Job Runner, the Settings Store and the Knowledge Service respectively. *Amended in revision 0.48.0: five entities have four owners; the Knowledge Service owns both the Knowledge entry and the Note.*

## Data Lifecycle

| Data | Origin | Stored in | Retention | Who can read or change it |
|---|---|---|---|---|
| Package files | Internal marketplace via `apm install` *(amended in revision 0.48.0: agent repositories, written by the Installers, ADR-082)* | The instance's agent folder | Until the instance is removed | Read by the person and the instance's sessions. Never edited by the application. |
| Memory pages | The package's `memory` skill | `memory/` in the agent folder | Until the person deletes them or removes the instance | The person (Memory view: view, edit, delete) and the instance's sessions |
| Recall records (revision 0.60.0; ADR-111) | Every recall by an agent session | The Memory Service's tables in the local database (or PostgreSQL in the scaled shape) | As the person's history setting (90 days by default); an item goes at once when its fact or link is forgotten | The person, through **What Ordin used**; ids only, no text |
| Knowledge connections (revision 0.61.0; ADR-113) | The person, or the organisation profile | The Knowledge Query Client's tables in the local database | Until the person removes the connection or the workspace | The person, in Settings › Knowledge agents |
| A knowledge agent's graph (revision 0.61.0; ADR-113) | Documents in the Git repositories the agent reads | The Memory Service's knowledge scope in the agent's datastore | As long as the source is current; a changed file's facts are replaced; everything can be rebuilt from Git | The agent's audience, through answers; the agent's operators |
| Knowledge answer cache and tables of contents (revision 0.61.0; ADR-113) | Answers and fetches | The Knowledge Query Server's tables, apart from memory | Until a relied-on commit moves, or the cache is full (10,000 entries); can be cleared alone | Kept per access group; never shared across groups |
| Sources record (revision 0.61.0; ADR-113) | Each answer, when the operator turns it on | The Knowledge Query Server's tables | 365 days by default, as the configuration sets | The agent's operators and auditors; no question text and no asker identity |
| Input and result artefacts | The person; runs | The agent folder, or the workspace place a run wrote to | Until the person deletes them or removes the instance | The person and the instance's sessions; crew members receive a copy of the artefacts the orchestrator passes |
| Journal (runs, rounds, tool requests, decisions, approvals, external actions) | The core | The local database (ADR-010) | 90 days by default, configurable by the person (assumption A-08). Pending approvals and in-progress runs are never pruned. | The person (Diagnostics), read-only |
| Copilot session state | Copilot CLI server | The CLI's own session storage under the application data root | As long as the run it belongs to is resumable, and pruned with the journal | Not shown directly |
| Remote conversation content | wrightrun / Slack *(amended in revision 0.48.0: hosted ordin agents / Slack, ADR-098)* | The journal stores the result text and the thread link; Slack and wrightrun keep their own copies under their own retention | As the journal | The person. In Slack, everyone in the dedicated channel. |
| A server's state shown on the desktop (revision 0.42.0; ADR-096) | The ordin server's Remote API | The desktop's memory while it runs; the last state received is kept in the local database so a closed desktop reopens showing it, marked **Not connected** until it catches up | Until the server is removed from the desktop | The person. The server's own journal stays the record; the desktop's copy is never sent back or merged into its own workspaces |
| Workflows and their versions (revision 0.11.0; ADR-064) | Ordin, the person, a duplicate or an imported file | The local database (ADR-010), per workspace | Until the person removes the workflow or the workspace; an exported file is the person's, wherever they saved it | The person (workflow screen); Ordin and the Crew Orchestrator read them |
| Reports and dashboards (revision 0.13.0; ADR-067) | Agents and Ordin, through the MCP Front's report tool | The local database (ADR-010), per workspace | A report with its run (A-08); a dashboard until the person removes it or the workspace | The person (report and dashboard screens); Ordin and the workspace's agents read them as earlier results |
| Files in a person's place | The person; runs; ArtCraft edits | The person's own folder | The person's; the application never deletes them, only moves to the system's bin on the person's action | The person; agents and crews only by grant |
| File versions | File Service | Copies under the application data root, referenced by hash | With the journal's retention; the latest version is always kept | The person |
| Catalogue clones and the search index (revision 0.25.0; ADR-082) | Catalogue repositories, with the person's GitHub sign-in | Under the application data root | While the profile names the repository | The core only; the person sees agents in the library *Amended in revision 0.48.0: the catalogue index is a set of tables in the journal's database, the local database of ADR-010, not a separate index database; the clones stay under the application data root.* |
| Agent drafts and rehearsals (revision 0.25.0; ADR-083) | The person, the agent maker, the Authoring Service | A branch of the person's clone; rehearsals in the journal | Until published or discarded; rehearsals with the journal's retention | The person; reviewers see what the pull request holds *Amended in revision 0.48.0: rehearsal records are owned by the Authoring Service and each rehearsal is also journalled.* |
| Part drafts (revision 0.37.0; ADR-091) | The person, with suggested wording from a model; the Authoring Service | A branch of the person's clone | Until published or discarded | The person; reviewers see what the pull request holds |
| Withdrawn list (revision 0.37.0; ADR-091) | Merged pull requests, written by ordin or by hand | The agent repository (proposed `withdrawn.yaml`); the entries read are also kept with the index | Append-only, never pruned while the repository is read | Everyone who can read the repository; changed only through a reviewed pull request |
| Handover note and record (revision 0.38.0; ADR-092) | The sender (note, drafted by Ordin); both installations (events) | The handover place: a pull request's branch in Git, or a SharePoint folder; each side's journal keeps the events it wrote and read | As long as the place keeps them; ordin deletes nothing there | Whoever can read the place; changed only by appending events, each installation its own |
| Credentials (GitHub, Okta, Slack, Confluence, Microsoft, Google) *Amended in revision 0.48.0: the kinds are those of the Credential entity above: GitHub, *GitLab* (ADR-051; revision 0.49.0), an OIDC provider (in place of Okta), Slack, Confluence, Microsoft, Google and the company mail server's sign-in.* | Sign-in flows | OS keychain | Until sign-out | The core only |
| Vendor harness sign-ins (Claude Code, Copilot CLI) | Each harness's own sign-in flow | The harness's own store (OS keychain or its credentials file) | The harness's own | The harness only; the application never reads them (ADR-044) |
| Dictation audio | The microphone, while the person dictates | Memory only, during recognition | Discarded when recognition ends | The Dictation Service only (ADR-043) |
| Speech model | Update feed, on first use of dictation | Under the application data root | Until replaced by a newer version | The Dictation Service *Withdrawn in revision 0.22.0 (ADR-078): no speech model is kept.* |
| Sandbox session folder | Sandbox runs of one agent session (ADR-040) | A temporary folder of the sandbox | Deleted at session end or after the idle limit | The sandbox; contents returned as change records |
| Workspaces, Confluence links and published copies (ADR-069) | The person; publishing; automatic updates of copies | The local database (ADR-010) | Until the person removes the workspace, the link or the copy; published copies' history with the journal | The person; agents read only their workspace's links |
| Email messages read for a workspace (ADR-070) | The person's mailbox, through the Mail Service | Stay in the mailbox; the parts a session read are in that session's context and journal entries as the tool results | The mailbox's own; journal entries with the journal's retention | The person; Ordin and agents granted email in that workspace, for messages matching its email links |
| Email drafts and sent-email records (ADR-070) | Agents, Ordin's crew members and the person; the Mail Service on sending | The local database (ADR-010) | Drafts until sent or discarded, then with the journal's retention; sent email in the person's Sent folder | The person; the drafting agent for its own drafts |
| Attachments saved from email | *save attachment* | The workspace's own folder, as files | As files in a place | As the place's grants |
| Published content in Confluence | Publishing from linked folders and **Publish a copy…** | Confluence | Confluence's own retention; never deleted by the application without the person's check | Whoever the Confluence space allows — stated on the approval card |
| Published copies in SharePoint (ADR-069) | **Publish a copy…** and automatic updates of copies | SharePoint, as Word, PDF or slides files | SharePoint's own retention and versions; never deleted by the application without the person's check | Whoever the library allows — stated on the approval card (C-42) |
| Last seen and Home marks (ADR-058, ADR-079) | The core, as the person opens Home and workspaces | The local database (ADR-010) | Until the workspace or the installation is removed | The core only; shown to the person as "new" marks on Home and changed marks in the tree |
| Search index | File Service, over a workspace's places (A-22) | Under the application data root | Rebuilt when needed; removed with the workspace | The core only |
| Organisation profile | IT | Cached under the application data root | Until replaced by a newer verified version | Read by the core; shown in Diagnostics |
| Help requests | The Setup Assistant or any failed step, on the person's "Send to IT" | Sent to the profile's help destination; a copy in the journal | As the journal | The person and IT; redacted as support exports are |
| Chat messages on an ordin server (ADR-073) | The person and Ordin, through the Channel Gateway | The chat service, under its own retention; the server's journal keeps conversation entries as on the desktop, and counts unanswered messages from others without content | The chat service's own; the journal's as the journal | The person in their chat app; the chat service (R-63) |
| NATS event payloads (ADR-074) | The company's NATS servers | Not stored: only the mapped input values, in the run, and the trigger event's outcome and identifier | Inputs as the run; identifiers 7 days | The core; the agents of the run, as inputs only |
| Messages kept from linked Telegram chats (ADR-075) | The Telegram Service, from what the bot receives | The server's local database (ADR-010) | Until the link is removed | The core; Ordin and agents granted Telegram access in that workspace |
| A hosted agent's runs, threads, conversation and memory by scope (revision 0.43.0; ADR-097, ADR-098) | People in enabled channels and listed direct chats; callers; intakes; the Duty Timer | The datastore of the hosted agent (SQLite in the single shape, PostgreSQL in the scaled shape) | The agent configuration's retention period for each kind; erasure of one person or one scope on an operator's request | The agent, within the run's scope; callers for their own runs; operators through the operator routes |
| A hosted agent's work folder (revision 0.43.0; ADR-098) | The agent's file tools | The datastore, one namespace per agent | Until removed by the agent within its allowed paths, or by an operator | The agent and its sub-agents (reviewer read-only); operators through the work-folder routes |
| Agent releases (revision 0.43.0; ADR-098) | The publish command | The organisation's container registry | The registry's own | Whoever the registry allows; every instance of the deployment reads its release |
| Logs, metrics and traces of a hosted agent (revision 0.43.0; ADR-098) | Every instance | Standard output, the metrics route, and the organisation's trace collector where switched on | The organisation's log and metrics systems | The organisation's operators; no content in metrics or spans |
| Team knowledge records (revision 0.51.0; ADR-103) | The Team Knowledge Service | The local database (ADR-010), plus its own copy of the repository under the application data root | Until the person stops sharing, the link is removed or the workspace is deleted | The core only |
| Shared facts and pages in a team folder (revision 0.51.0; ADR-103) | **Share what we've learned**, after the card | The GitHub or GitLab repository | The repository's own retention; a deleted item's file is removed by an ordinary commit and stays in the repository's history (D9) | Whoever the repository allows |
| Logs | The core | Rotating local files | 14 days (assumption A-09) | The person; exported only by the person, with redaction (07 — Cross-cutting) |

**Privacy constraints.**
- Slack questions and replies are visible to the whole dedicated channel by design (ADR-009). The application warns before the first post to each remote agent through Slack.
- Content sent to Copilot leaves the computer under GitHub's terms for the person's subscription.
- Files published to Confluence are visible to everyone the Confluence location allows, and copies in SharePoint to everyone the library allows. The approval card states that audience before the first push (ADR-026, ADR-069). An automatic update reaches the same audience without a new card, so a copy whose audience has grown since approval is held until the person approves again (ADR-069).
- Email holds personal data of people inside and outside the company. Agents read only messages matching the workspace's email links; nothing is sent without the person's approval on a card that marks recipients outside the company (ADR-070, 06 — Regulatory).
- The application sends no telemetry of its own in the first release (assumption A-10).
- *Revision 0.43.0 (ADR-097, ADR-098).* A hosted agent keeps messages and files from many people, by scope. What is said in a channel can reappear in that channel to anyone there; a direct chat's content stays in that direct chat's scope. The agent's first reply in each scope says so. Retention periods by kind of content and erasure for one person or one scope are set in its agent configuration and carried out by operators. Spans and metrics carry no content.
- Facts and pages shared to a team folder are readable by everyone the repository allows and stay in its history after they are deleted; the first-use screen of Team Knowledge mode says so, and facts flagged as possibly holding personal data are not shared until the person has checked them (ADR-103).

**Multi-tenancy.** None. One installation serves one signed-in person. *Amended in revision 0.43.0 (ADR-097): an installation in the agent profile serves the people its organisation allows, as one agent; content is kept by scope, which separates channels inside one organisation and is not a tenant boundary.*
