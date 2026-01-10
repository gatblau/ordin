# ordin desktop — Architectural Decisions

> **Mode:** draft
> **Generated from:** inline context — the user's decisions of January 2026
> **Revision:** 0.58.0
> **Last updated:** January 2026

This file is append-only. A decision is amended by a new entry, never by overwriting. Entries marked *decided by the user* record a decision of January 2026. The others are proposed by this draft and await the user's review.

**Approval of January 2026.** The user approved ADR-013 ("yes to ADR-013") and stated that the marketplace is hosted on GitHub. ADR-013's Status line below still reads "Proposed" because entries are append-only; this paragraph is the record that it is decided. ADR-014 records how the GitHub hosting is used.

**Terminology from revision 0.7.0.** In ADR-001 to ADR-024, "workspace" means the hidden per-instance folder now called the **agent folder**, and "Workspace Manager" means the component now called the **Agent Folder Manager**. From ADR-025 on, "workspace" means the person's home for one piece of work. The earlier entries are not reworded, because this file is append-only; the rest of the design uses the new terms. "Slack workspace" and Slack's workspace administrators keep their Slack meaning throughout.

**Confluence deployment, January 2026.** The user stated that the company uses **Confluence Cloud**. This settles the deployment left open in ADR-026's Status line (OQ-23): each person signs in with their own Atlassian account through Atlassian's OAuth 2.0 authorisation-code flow, with the site and the OAuth app's client identifier in the organisation profile; whether a desktop client can do this without a stored client secret is confirmation item C-12.

### ADR-001: Local sessions run on the GitHub Copilot SDK for Rust, with the Copilot CLI bundled

- **Status.** Decided by the user, January 2026 ("Copilot mostly, yes local SDKs now"). The bundling detail is proposed. *Amended by ADR-038 (Claude is the first harness, Copilot the second) and ADR-044 (each harness signs the person in its own way; ordin attaches to the signed-in harness).*
- **Decision.** The first local runtime adapter uses the official Copilot SDK for Rust. It drives a bundled, pinned Copilot CLI in server mode and authenticates as the signed-in GitHub user.
- **Context.** Most users hold Copilot seats. The Copilot SDK runs on that subscription; a third-party product may not offer claude.ai login without Anthropic's approval. The Rust SDK keeps the application single-language in its core. It does not bundle the CLI by default, so the application must.
- **Consequences.**
  - No API key is asked of business users (G6).
  - Usage counts against each person's Copilot allowance, so budgets are measured in requests.
  - The installer carries the Copilot CLI, and its version is tested with each release.
  - Behaviour depends on the Copilot CLI loading what `apm install --target copilot` deploys (OQ-2, R-01).
- **Alternatives considered.**
  - *Claude Agent SDK as the first adapter.* Rejected: needs an organisation API key and a TypeScript or Python worker, and most users hold Copilot rather than Claude seats.
  - *Copilot SDK in TypeScript or Python.* Rejected: it would add a second language runtime, where the Rust SDK is official.
  - *A custom Rust agent loop.* Rejected: most implementation responsibility, and no subscription route.

### ADR-002: wrightrun is not the local harness in the first release

- **Status.** Decided by the user, January 2026 ("local SDKs now"). To be reconsidered once wrightrun has code. *Superseded in revision 0.43.0 by ADR-098: wrightrun is not built; hosted agents run in the ordin server's agent profile.*
- **Decision.** Local sessions do not run in a desktop-hosted wrightrun. wrightrun is reached only as a remote runtime.
- **Context.** The analysis of January 2026 found the following:
  - wrightrun's models are limited to `anthropic` and `google_genai` with API keys (wrightrun SA-04), so it cannot use a Copilot subscription.
  - Durable runs and the work folder need PostgreSQL.
  - The work folder lives in the database, not on disk.
  - Every request needs an identity-provider token.
  - It has no live output and no cancel (A-33), and serves one agent per deployment.
  - It has no code yet, and its spec audit is Failed.
- **Consequences.**
  - The rules of a declaration are enforced by two implementations, wrightrun's and this application's. ADR-007 limits the drift between them.
  - The desktop keeps direct access to local files and uses Copilot seats.
- **Alternatives considered.**
  - *wrightrun local with an embedded database and a desktop profile.* Rejected for now: it needs several wrightrun RFCs that reverse its non-goals, and it gives no Copilot subscription.
  - *wrightrun "run once".* Rejected: kept for checks, with nothing persisted.

### ADR-003: One runtime adapter interface, in wrightrun's caller vocabulary

- **Status.** Proposed. *Amended in revision 0.43.0 (ADR-098): the caller vocabulary is ordin's own from now, kept compatible with wrightrun's caller interface; the wrightrun HTTP Adapter becomes the Hosted Agent Adapter.*
- **Decision.** Every agent route — local Copilot, wrightrun over HTTP, wrightrun through Slack, and a later Claude adapter — implements one interface:
  - start (command and arguments, or free text);
  - progress events;
  - pause for a decision;
  - resume with approve or reject;
  - read state and result;
  - cancel.

  An adapter declares the verbs it cannot offer.
- **Context.** Crews mix local and remote members (decision of January 2026). wrightrun's caller interface already fixes the remote verbs. The Copilot SDK's permission handler and events map onto the same set.
- **Consequences.**
  - The Crew Orchestrator and Session Supervisor do not depend on a route.
  - Missing verbs (cancel and progress for wrightrun; structured approval for Slack) show in the start-up report.
  - A Claude adapter can be added later without touching crews.
- **Alternatives considered.**
  - *Route-specific code paths in the orchestrator.* Rejected: every new route would change crew logic.
  - *Adopting the Copilot SDK's session model as the interface.* Rejected: remote routes cannot honour it.

### ADR-004: Packages are installed only through the `apm` CLI from the internal marketplace

- **Status.** Decided by the user, January 2026 ("use apm cli to install"). The audit and approval detail is proposed. *Superseded in revision 0.25.0 by ADR-082: agents come from catalogue repositories in crucible's format, APM packages are imported, and `apm` leaves the bundle.*
- **Decision.** The Package Catalogue drives a bundled, pinned `apm` binary for registering, browsing, searching, installing, updating, auditing, approving and denying. It installs each instance into its own workspace with `--target copilot`. It never edits package files.
- **Context.**
  - AgentWright publishes APM packages.
  - The marketplace is company-internal.
  - APM 0.31.0 offers `marketplace`, `search`, `install --target`, `audit`, `approve`, `deny` and `policy`, which cover install-time review of executable content.
- **Consequences.**
  - The application inherits APM's resolution, lock files and governance.
  - The `apm` version is tested with each release.
  - Git credentials for the marketplace are the person's.
- **Alternatives considered.**
  - *An application-level package manifest and installer* (`agent-package/manifest.json`). Rejected: it duplicates APM and AgentWright's package, and the builder already targets Copilot.
  - *Calling the marketplace's git hosting directly.* Rejected: it bypasses APM's audit and policy.

### ADR-005: Every local tool request is decided by the Rules Engine before it runs

- **Status.** Proposed.
- **Decision.** The Copilot Local Adapter registers the SDK's permission handler and tool hooks. Each tool request is passed to the Rules Engine, which returns allow, deny or ask a person, with no model call. Approval is the default for externally visible actions.
- **Context.**
  - Policy is enforced outside prompts.
  - The Copilot SDK exposes the CLI's first-party tools much as `--allow-all` would, unless restricted, and execution is governed by the permission handler.
- **Consequences.**
  - Safety rests on the handler and hooks seeing every tool request (R-02, confirmation item C-02).
  - The application's tool allowlist per instance comes from the package's tool access and the reviewer's decisions.
- **Alternatives considered.**
  - *Relying on the package's guardrail text.* Rejected: the model may not follow it.
  - *Operating-system sandboxing of the CLI.* Deferred: stronger, but platform-specific, and kept for where the threat model requires stronger process or remote isolation (OQ-11).

### ADR-006: The application owns crews, and members never message each other directly

- **Status.** Decided by the user, January 2026 ("Coordinating agent for crews with clearly defined guardrails"; "mostly desktop based but could also integrate remote agents"). The structure is proposed. *Amended by ADR-027 (Ordin coordinates every crew; no coordinator package) and ADR-064 (workflows).*
- **Decision.**
  - A crew is a charter run by the Crew Orchestrator.
  - The coordinator is an AgentWright package with routing-only authority. Each round it is invoked once and acts only through the crew tools.
  - The orchestrator checks every crew-tool call against the charter's guardrails in fixed code, then dispatches it.
  - Every message between members passes through the orchestrator.
- **Context.**
  - Both the builder and wrightrun keep crews as a non-goal, so neither can own them.
  - The builder already builds routing-only agents with a single-pass loop (observe and escalate), which fits "one coordinator turn per round".
  - Guardrails that hold whatever the model says follow wrightrun's principle (wrightrun ADR-023).
- **Consequences.**
  - Coordinators are ordinary packages; no builder change is needed to start.
  - The crew-tool contract becomes public towards AgentWright.
  - Every hand-off is journalled and can be stopped.
  - Crews cost a coordinator turn per round.
- **Alternatives considered.**
  - *Fixed-rule routing without a coordinating agent.* Rejected by the user in favour of a coordinator.
  - *Members talking directly.* Rejected: unobservable, and impossible to bound.
  - *A coordinator inside one Copilot session using the SDK's custom sub-agents.* Rejected: remote members cannot be sub-agents, and guardrails would sit inside the model's session.

### ADR-007: The Rules Engine conforms to wrightrun's declaration semantics, checked by shared scenarios

- **Status.** Decided in direction by the user, January 2026 (accepted "rules engine exists twice"). The method is proposed. *Superseded in revision 0.43.0 by ADR-098: there is one Rules Engine, with wrightrun's ControlEvaluator semantics in full; its scenarios are ordin's own tests.*
- **Decision.**
  - The Rules Engine implements the declaration semantics that apply to a local session, as wrightrun's ControlEvaluator spec states them.
  - A conformance suite runs the applicable scenarios of wrightrun's spec set against the Rust engine.
  - Rules that do not apply to a local session are reported as not enforced, by name: chat triggers, recurring-duty gates, settings that only a chat system edits.
- **Context.** Two implementations of one contract drift unless something checks them against each other. wrightrun's specs carry scenarios in a form a test can replay.
- **Consequences.**
  - A wrightrun spec change can break the application's suite; the suite pins the wrightrun spec revision it was derived from.
  - The declaration format is Draft on both sides (R-03).
- **Alternatives considered.**
  - *Ignoring the declaration locally.* Rejected: G7.
  - *Embedding wrightrun's Python evaluator.* Rejected: it brings a Python runtime into the installer for one component.

### ADR-008: Slack route posts as the person in a dedicated channel

- **Status.** Decided by the user, January 2026 ("posts as the person (with their Slack sign-in)"; "thread delivery in a dedicated channel"). *Amended in revision 0.43.0 (ADR-098): the Slack route reaches hosted ordin agents, which replace wrightrun agents.*
- **Decision.**
  - The Slack Thread Adapter signs in to Slack as the person.
  - It posts mentions of the remote agent in a thread of one dedicated channel per workspace, and reads the agent's replies from that thread.
  - The application receives no Slack events and holds no bot token.
- **Context.** Some wrightrun agents are reachable only through Slack. Posting as the person means wrightrun treats the message as an ordinary person asking, so how it treats messages from programs does not matter.
- **Consequences.**
  - The application acts with the person's Slack permissions, so posts are approval-gated.
  - Workspace administrators must approve the app's user scopes.
  - Polling adds delay.
  - Approvals a remote agent asks for in Slack are decided in Slack.
- **Alternatives considered.**
  - *Posting as a bot.* Rejected: wrightrun may treat program messages differently, and replies always go visible for programs (wrightrun A-16).
  - *Only the HTTP route.* Rejected: some agents are only in Slack.

### ADR-009: Remote agents used through Slack reply in the thread

- **Status.** Decided by the user, January 2026. *Amended in revision 0.43.0 (ADR-098): applies to hosted ordin agents reached through Slack.*
- **Decision.** The wrightrun configuration of every agent the application reaches through Slack sets `delivery: thread` for the trigger the application uses. The adapter treats a missing reply as that configuration's absence and reports it.
- **Context.** wrightrun's default reply is visible to the asker only (`chat_postEphemeral`, wrightrun A-16). Such replies are not stored by Slack for later reading. This is unverified and recorded as confirmation item C-04.
- **Consequences.**
  - Questions and answers are visible to the dedicated channel.
  - A wrightrun configuration change is needed per agent; no wrightrun code change.
- **Alternatives considered.**
  - *Private replies, with the application reading them.* Rejected: not expected to be readable.
  - *A wrightrun change to copy private replies elsewhere.* Rejected: unnecessary given thread delivery.

### ADR-010: The journal is an embedded SQLite database

- **Status.** Proposed.
- **Decision.** One SQLite file under the application data root holds the journal, approvals, crew state and indexes. Artefact content stays as files in workspaces.
- **Context.** There is one writer, and the data must survive a restart. No service may be required on a business user's computer.
- **Consequences.**
  - Recovery reads one file.
  - Schema migrations run on start (07 — Cross-cutting).
  - The file is not encrypted by the application; it relies on the operating system's disk encryption (A-07).
- **Alternatives considered.**
  - *JSON-lines files.* Rejected: no transactions across journal and approval state.
  - *Embedded PostgreSQL.* Rejected: heavy, for a single user.

### ADR-011: Recurring duties and unattended work stay out of the desktop

- **Status.** Proposed. *Amended by ADR-073 and ADR-076: the ordin server runs unattended work started from the chat and from events; the desktop still runs none.* *Amended in revision 0.43.0 (ADR-097): an ordin server in the agent profile runs a package's recurring duties on the Duty Timer; the desktop and the personal profile still schedule nothing.*
- **Decision.** A package's recurring duty is not scheduled by the application. It is listed as not carried across. Crews run only while the application is open, and pause on sleep or quit.
- **Context.**
  - Sleep, offline and closure need explicit behaviour, and unattended work is a remote concern.
  - wrightrun has a duty timer.
- **Consequences.**
  - Business users who need standing duties are pointed at a wrightrun deployment.
  - No background service is installed.
- **Alternatives considered.**
  - *A background service that runs duties.* Rejected: sleep, battery and offline behaviour, and a second resident program to support.

### ADR-012: The application performs every technical step; the business user never does

- **Status.** Decided by the user, January 2026: "the process of installing packages along with all other technical actions should be friendly done by the desktop, that is one of its main purposes: enable non tech people". The mechanisms are proposed.
- **Decision.** No screen outside Diagnostics asks the business user to type, choose or judge anything technical. That covers tools, versions, targets, addresses, channels, hooks, MCP servers, permissions expressed as tool names, and errors from command-line tools. The application performs those steps itself:
  - the Setup Assistant on first start and in the background;
  - the Package Catalogue for adding, updating and removing agents;
  - the adapters for connecting accounts.

  The person's actions are signing in with existing accounts, choosing agents and tasks, supplying inputs, and approving consequences stated in plain language. When a step cannot be completed, the person sees one plain sentence with one action, and can send the details to IT.
- **Context.** Enabling non-technical people is a main purpose of the application. The first draft asked the person to approve executable parts, read APM's messages and register remote agents with addresses. Each of those recreates the friction the application exists to remove: an application that immediately requests an API key keeps much of the original friction.
- **Consequences.**
  - Technical decisions move to the organisation (ADR-013) or to fixed defaults.
  - The application needs automatic repair and plain-language messages for every failure path, which `/spec` turns into an error table per component.
  - Diagnostics remains for support staff.
- **Alternatives considered.**
  - *An "advanced settings" mode for business users.* Rejected: it puts technical choices back in front of them.
  - *Asking the person to approve executable parts with an explanation.* Rejected: it is a security judgement a non-technical person cannot make well.
- **Amends.** ADR-004, in its detail: per-part `apm approve` and `apm deny` decisions by the person are replaced by the organisation's recommended set (ADR-013).

### ADR-013: Technical configuration comes from a signed organisation profile published by IT

- **Status.** Proposed. *From revision 0.25.0 (ADR-082, ADR-084) the profile names one or more catalogue repositories instead of the marketplace, expresses the executable-trust policy over native kinds (tool bindings and hooks), and can require signed merges per repository.*
- **Decision.** IT publishes one signed organisation profile. It names:
  - the marketplace;
  - the executable-trust policy (APM's organisation-recommended executable set);
  - the remote agent catalogue, with each agent's route, address and identity settings;
  - the Slack workspace and dedicated channel;
  - identity-provider settings;
  - default and maximum limits;
  - the update feed; *Amended in revision 0.48.0: the update feed is carried by the branded build's installer (ADR-041, build layer), not by the profile.* *Amended in revision 0.49.0 (decision 80): once the profile is verified, an update feed it names (`updates.feed_url`) replaces the installer's address (08 — Organisation profile); a redirect therefore needs IT's signing key.* *Amended in revision 0.62.0 (ADR-114): the unbranded open-source build's installer carries the project's release feed, so updates need no profile; a verified profile's redirect is unchanged.*
  - where help requests go.

  The application reads it from an address the installer carries, verifies it, and configures itself. An agent whose executable parts fall outside the recommended set is not installed; it is shown as needing IT approval.
- **Context.**
  - Every technical setting must come from somewhere other than the business user (ADR-012).
  - APM already supports an organisation-recommended executable set (`apm approve --recommended`, `executables.recommend`) and policy discovery (`apm policy status`).
  - The wrightrun configuration change for thread delivery (ADR-009) is IT's action, so the remote agent catalogue is the natural place to list the agents ready for it.
- **Consequences.**
  - The application depends on IT publishing and maintaining a profile; without one, setup cannot finish (R-11).
  - The first-draft assumption that the person reviews executable parts (A-14) is withdrawn.
  - OQ-9 is settled: IT decides trust.
- **Alternatives considered.**
  - *Settings typed by the person.* Rejected by ADR-012.
  - *Settings baked into the installer.* Rejected: every change would need a new installer.
  - *Device management (MDM) only.* Not rejected outright: it may be the way the profile's address reaches each computer (OQ-14).

### ADR-014: The marketplace is reached with the person's GitHub sign-in

- **Status.** The hosting was stated by the user on January 2026 ("marketplace is on GitHub"). The mechanism is proposed. *From revision 0.25.0 (ADR-082) the same GitHub sign-in reads the catalogue repositories and opens publishing pull requests through `gh`; `GITHUB_APM_PAT` is no longer set, because `apm` is gone.*
- **Decision.**
  - The one GitHub sign-in that enables Copilot also gives `apm` read access to the internal marketplace.
  - The Package Catalogue passes the person's token to each `apm` invocation through the `GITHUB_APM_PAT` environment variable of that invocation only.
  - The token is never written to a file, a git configuration or the organisation profile.
  - The organisation profile names the GitHub organisation (and the host, for GitHub Enterprise Server, through `GITHUB_HOST`) where the marketplace lives.
- **Context.**
  - APM resolves GitHub credentials in this order: `GITHUB_APM_PAT`, `GITHUB_TOKEN`, `GH_TOKEN`, `gh auth token`, then a git credential helper.
  - APM stores no token itself.
  - Its documentation names the `repo` scope for private repositories (APM CLI reference, *auth*, read on January 2026).
  - A second sign-in, or a git account, would break ADR-012.
- **Consequences.**
  - The person's GitHub account needs read access to the marketplace repositories. IT grants this through organisation membership, as part of the onboarding the profile assumes.
  - The token's scope must cover both Copilot and private-repository reads, which favours an organisation-owned OAuth GitHub App for sign-in (OQ-6).
  - Whether APM accepts a GitHub App user token as it accepts a personal token is confirmation item C-06.
  - OQ-13 is settled.
- **Alternatives considered.**
  - *A shared read-only token in the organisation profile.* Rejected: the profile holds no secrets, and a shared token on every computer cannot be revoked per person.
  - *An internal mirror of the marketplace outside GitHub.* Rejected: an extra service for IT, and the marketplace is already on GitHub.
  - *The person's own git credential helper.* Rejected: business users have no git set-up (ADR-012).

### ADR-015: Friendly file access through places, pickers and versions

- **Status.** Decided in capability by the user, January 2026 ("the desktop needs to have friendly access to the file system"). The model is proposed.
- **Decision.**
  - The person works with **places**: the workspaces, plus folders they add with the system's folder picker. Places are shown by friendly names, never as paths.
  - A Files view gives each file a preview, its versions, its origin, and the actions *open*, *edit*, *give to an agent* and *save a copy elsewhere*. Files can be dragged in and out.
  - Agents and crews reach a place only by the person's grant, at read or read-and-change access.
  - Every change a run makes is kept as a version.
  - Nothing is deleted permanently: removal moves a file to the system's bin.
- **Context.**
  - Enabling non-technical people (ADR-012) rules out paths, permissions dialogs and file-system jargon.
  - The desktop's main advantage over a server is local documents.
  - Agents changing files a person cares about need a safety net.
- **Consequences.**
  - The File Service and the Rules Engine share one notion of place and grant.
  - Versions take disk space, bounded by retention (04 — Domain).
  - The Workspace Manager's folder grants become place grants.
- **Alternatives considered.**
  - *Full disk access for agents, with approvals per write.* Rejected: too many approvals, and a non-technical person cannot judge a path.
  - *Workspaces only, with copies in and out.* Rejected: people work in their own folders and would lose track of copies.

### ADR-016: Files of many kinds are created and edited with ArtCraft apps, installed and pinned by the application

- **Status.** Decided by the user, January 2026 ("creation and manipulation of different types of files (facilitated by ArtCraft applications accessed via cli)"). The installation detail is proposed.
- **Decision.**
  - The application uses the ArtCraft Crafting Apps as its editors and file tools.
  - The organisation profile approves which apps may be used, pins their versions and publishes the file-type map.
  - The Setup Assistant downloads each approved app and its CLI from the app's GitHub releases, verifies the download, installs it under the application data root and updates it when the pin changes.
  - The ArtCraft Launcher is not used.
  - Where no ArtCraft app is approved for a kind of file, the person can open it with the system's default app.
- **Context.**
  - The ArtCraft apps are open-source desktop editors, native on macOS, Windows and Linux, each "built for automation and AI agents" with a CLI, a JSON control channel and an MCP server (getartcraft.com/apps, read on January 2026).
  - Installing them must be the application's work, not the person's (ADR-012).
  - The apps' release pace is fast, so versions are pinned.
- **Consequences.**
  - The application depends on apps that are early alpha (PhotoCraft, PdfCraft, FigCraft) or in development (the other ten) (R-14).
  - Office-format support (Word, Excel, PowerPoint files) by WordCraft, GridCraft and DeckCraft is unconfirmed (C-09).
  - A CLI download is listed for macOS on the app pages read; Windows and Linux CLI availability is unconfirmed (C-07, R-15).
  - The installer does not carry the apps; they are fetched on demand, per approved app.
- **Alternatives considered.**
  - *The ArtCraft Launcher.* Rejected: a second updater the organisation does not control, and pins would not be enforced.
  - *Linking the ArtCraft Rust code into the application.* Rejected for now: the user named the CLI. It is a later option, given the shared language.
  - *Each person installs the apps.* Rejected by ADR-012.

### ADR-017: Agents reach ArtCraft through Craft tools checked by the Rules Engine

- **Status.** Proposed.
- **Decision.**
  - A local session receives the Craft tools its package and the profile allow.
  - Where an app's CLI serves MCP, the Craft Toolkit starts that MCP mode for the session as an MCP server.
  - Otherwise it offers custom tools that call the CLI's run and batch commands.
  - Every Craft tool call is decided by the Rules Engine. Input and output paths must lie in the session's granted places, and the command must be on the app's allowed list in the profile.
  - Outputs become new file versions.
  - The JSON control channel, which drives an open window, is not used in the first release.
- **Context.**
  - A Craft MCP server is an executable part like any other, so the same trust and rules apply (ADR-005, ADR-013).
  - The run and batch modes are headless, which suits agents.
  - Driving a window would act on what the person is looking at.
- **Consequences.**
  - The Rules Engine gains path rules over Craft tool arguments.
  - The Craft Toolkit must learn each app's command list to restrict it (C-07).
  - Packages need not ship their own ArtCraft integration; AgentWright may later declare which Craft tools a skill needs (OQ-16).
- **Alternatives considered.**
  - *Giving agents a shell to call the CLIs.* Rejected: a shell cannot be restricted to paths and commands reliably.
  - *Driving the apps' windows through the JSON control channel.* Deferred: useful for showing work live, but it acts in the person's window.

### ADR-018: Office documents, spreadsheets, slides and PDFs come first, and office formats wait for ArtCraft

- **Status.** Decided by the user, January 2026 ("word, spreadsheets, slides, pdfs"; for office formats the ArtCraft editors cannot yet read and write: "wait for artcraft").
- **Decision.**
  - The first file kinds are Word documents, spreadsheets, slides and PDFs, served by WordCraft, GridCraft, DeckCraft and PdfCraft.
  - Until an app reads and writes its office format, as confirmed by C-09 at a pinned version, agents get no Craft tool for that format, and no interim library is added.
  - Opening such a file with the system's default app stays available to the person (ADR-016), because that is opening, not editing by an agent.
  - PDFs are served by PdfCraft as soon as its test passes.
- **Context.**
  - On January 2026 WordCraft, GridCraft and DeckCraft are listed as "In development", and PdfCraft as "Early alpha".
  - The user chose to wait rather than add a second route.
- **Consequences.**
  - Agent work on office files is gated on ArtCraft's progress (R-17).
  - The business pilot's workflows are chosen among those whose formats pass C-09 when the pilot starts.
  - OQ-15 and OQ-17 are settled.
- **Alternatives considered.**
  - *The person's usual Office apps for hand edits, with no agent editing.* Not chosen by the user.
  - *An interim document library for agents.* Not chosen by the user.

### ADR-019: An engineering mode for people who build software, enabled by the organisation profile

- **Status.** The capability was decided by the user, January 2026 ("the desktop should allow software engineering by providing code editors for files and projects"). Its placement is proposed. *From revision 0.10.4 its placement is settled by ADR-047: inside workspaces.* *From revision 0.10.7 it is no longer enabled per person: ADR-050 ties it to a workspace linking a code repository.* *From revision 0.10.9 code is not an exception mode but a first-class kind of work in a workspace (ADR-052); technical detail stays limited to workspaces with code.*
- **Decision.**
  - Projects, change review and git views form an engineering mode.
  - The organisation profile enables it for named people or groups. It is hidden from everyone else.
  - Inside engineering mode, technical detail — paths, diffs, branches, command output — is shown, because the people using it build software.
  - The code editor itself is available to everyone for opening any text file.
- **Context.**
  - The application's main purpose is to enable non-technical people (ADR-012).
  - The same application now also serves engineers, who need the detail that ADR-012 hides.
  - Gating by profile keeps the business user's screens unchanged.
- **Consequences.**
  - Engineering mode is the documented exception to G10.
  - Software-engineering agents (system-changing authority, check commands) are mostly used here.
- **Alternatives considered.**
  - *Show engineering features to everyone.* Rejected: it puts technical choices in front of business users.
  - *A separate engineering application.* Rejected: crews, packages, rules and files would be duplicated.

### ADR-020: The code editor is Monaco Editor, embedded in the WebView, with language servers relayed by the core

- **Status.** Proposed. *From revision 0.10.9 the editor stays a viewer and light editor for review and quick edits, and engineers keep their own IDE beside ordin (ADR-052); this amends the alternative "Launching an external IDE" below. Its language servers also serve agents and change review (ADR-054). From revision 0.10.10 it highlights every language it has a tokeniser for, with or without a language server (ADR-055).*
- **Decision.**
  - The code editor and the change-review diff use Monaco Editor in the WebView.
  - Language intelligence comes from language servers that the Project Service starts and relays over the Language Server Protocol.
  - Language servers come from packages through APM (which installs MCP and LSP dependencies) or are pinned in the organisation profile.
  - The WebView never starts a program itself.
- **Context.**
  - Engineers expect editing behaviour close to VS Code, whose editor Monaco is.
  - A diff editor is needed for change review.
  - Tauri's trust boundary keeps program execution in the core (01 — Context, trust boundary 1).
- **Consequences.**
  - The interface bundle grows by the editor's size.
  - Each supported language needs a pinned language server (OQ-21).
- **Alternatives considered.**
  - *CodeMirror 6.* A lighter editor; not chosen because the diff editor and VS Code familiarity would need more work. It remains the fallback if Monaco's size or behaviour in a WebView fails the prototype.
  - *Launching an external IDE.* Rejected: the change review, rules and journal would be outside the application.

### ADR-021: Agents propose changes to projects; only the engineer commits and pushes

- **Status.** Proposed. *From revision 0.10.9 an agent edits in its coding task's own git worktree, never in the engineer's working copy, and each change set belongs to one coding task (ADR-053).* *Amended by ADR-023 and ADR-047.*
- **Decision.**
  - An agent's edits to a project are recorded as a change set and reviewed entry by entry.
  - Agents never stage, commit, push, merge or rewrite history.
  - The engineer commits, using a message the agent may propose, and pushes with their own GitHub sign-in.
  - Check commands an agent runs (build, tests, lint) pass through the Rules Engine; anything that would commit, push, deploy, publish or change a shared database is refused. This follows the AgentWright rule that check commands exclude those.
- **Context.**
  - AgentWright agents carry check commands but leave out any that commit, push, deploy, publish or change a shared database (`features.md` §9).
  - Shared repositories need a person's decision.
- **Consequences.**
  - Pushing needs write access with the engineer's GitHub sign-in, which the read-only marketplace access of ADR-014 does not give (OQ-20).
  - Every change in a shared repository has a person's commit behind it.
- **Alternatives considered.**
  - *Agents commit to a local branch, and the engineer pushes.* Rejected for the first release: history the engineer did not write is harder to review than a change set.
  - *Agents open pull requests.* Deferred: an externally visible action for later, behind approval.

### ADR-022: A terminal for the engineer only

- **Status.** Decided by the user, January 2026 ("terminal for engineer only"). Settles OQ-18.
- **Decision.**
  - Engineering mode offers the engineer a terminal in the project folder.
  - Agents cannot use it: no tool reads from it or writes to it.
  - Its content is not journalled or logged.
- **Context.** Engineers expect a terminal. Commands typed in it run outside the application's rules and approvals, so it must never become an agent's route around them.
- **Consequences.**
  - Engineering mode can run anything the engineer types (R-18).
  - Agents' commands still go only through check commands and the GitHub tool, under the Rules Engine.
- **Alternatives considered.**
  - *No terminal.* Not chosen by the user.
  - *A terminal agents may also use.* Rejected: it would bypass the Rules Engine.

### ADR-023: The application manages `gh`; engineers choose which GitHub operations need their approval

- **Status.** Decided by the user, January 2026 ("app manages gh; engineers can configure whether gh operations require human approval or not"). The operation kinds and the profile ceiling are proposed. **Amends ADR-021.**
- **Decision.**
  - The Setup Assistant installs, pins and updates the GitHub CLI and signs it in with the engineer's GitHub sign-in.
  - Agents reach `gh` only through a GitHub tool. The Rules Engine classifies each call by operation kind — read, comment, open pull request, push branch, merge, release (the full list is fixed in `/spec`).
  - Each kind needs the engineer's approval unless the engineer has switched approval off for that kind in the project.
  - Reads never need approval. Write kinds need approval by default.
  - The organisation profile can mark kinds whose approval may never be switched off (OQ-22).
  - ADR-021 is amended: agents may now publish their accepted changes through GitHub operations (for example a branch and a pull request), where before only the engineer committed and pushed. The rest of ADR-021 stands: agents edit only through the change set, and check commands exclude commit, push, deploy and publish.
- **Context.**
  - Engineers want agents to take part in GitHub work, and want to decide per project how much trust to give.
  - `gh` covers pull requests, issues, reviews and workflow runs in one tool, and the engineer already knows it from the terminal.
- **Consequences.**
  - An engineer can let an agent change a shared repository with no per-action decision (R-19).
  - Every operation is journalled and shown in the project's activity.
  - The engineer's token needs write access to repositories, which the read-only marketplace access of ADR-014 lacks. Engineers' sign-in therefore asks for write access (OQ-20 settled through `gh`; scope breadth stays R-13).
- **Alternatives considered.**
  - *Every GitHub write needs approval, always.* Not chosen by the user.
  - *Agents call the GitHub API directly.* Rejected: a second client to maintain, where `gh` is already present for the engineer.

### ADR-024: Language intelligence comes from language plug-ins; the first four are Go, Rust, Java and TypeScript

- **Status.** Decided by the user, January 2026 ("if a plugin infrastructure of language servers exists, new can be added later, but in principle, golang, rust, java, typescript"). The plug-in mechanism is proposed. Settles OQ-21. *From revision 0.10.9 the plug-ins also serve agents and change review (ADR-054). From revision 0.10.10 the engineer chooses among approved servers, turns them off and edits their settings in Settings (ADR-055). From revision 0.10.11 the Go server is gopls and the Rust server is rust-analyzer, and they serve completion in the editor (ADR-056).*
- **Decision.**
  - A language plug-in is a pinned language server with its settings.
  - Plug-ins are listed in the organisation profile, or carried by APM packages as LSP dependencies, and installed on first use in a project of that language.
  - The first four are Go, Rust, Java and TypeScript; the servers are chosen and pinned in `/spec`.
  - A new language needs a plug-in, not a new release of the application.
- **Context.**
  - APM installs "APM, MCP, and LSP dependencies" (`apm install --help`, 0.31.0).
  - The Project Service already relays the Language Server Protocol (ADR-020).
- **Consequences.**
  - Each plug-in is an executable part, so it comes under the same trust policy as package parts (ADR-013).
  - Some servers need a language toolchain on the computer — Java's needs a Java runtime — which the plug-in must bring or the Setup Assistant must install (R-20).
- **Alternatives considered.**
  - *Fixed built-in languages.* Rejected: every new language would need a release.
  - *Editor-only syntax highlighting.* Rejected: engineers need completion, diagnostics and navigation.


### ADR-025: Workspaces are the person's unit of work

- **Status.** Decided by the user, January 2026 ("There is no concept of workspace?"; chosen meaning: "a piece of work"). The model below is proposed. **Amends ADR-015.**
- **Decision.**
  - A **workspace** is a named home for one piece of work. It holds:
    - one or more places (a place may belong to several workspaces);
    - zero or more Confluence links (ADR-026);
    - the agents and crews allowed in it, each with its access to each place (*can read* or *can read and change*);
    - its conversations, approvals, results and history.
  - The person creates a workspace by naming it, then drags in files or folders, or picks them with the system's pickers. A dropped folder becomes a place; dropped files are copied into the workspace's own folder, which the application creates under the person's Documents and shows by the workspace's name.
  - Every task, crew run and conversation starts in one workspace. Its agents reach only that workspace's places and Confluence links, at the access granted there. Access granted in one workspace grants nothing in another.
  - The person switches workspace from the navigation. The Today view can show one workspace or all of them.
  - The per-instance agent folders of ADR-004 stay hidden. They are an implementation detail, never called "workspace" on screen.
  - The Files view is built for size: a folder tree, a list with sortable columns (name, kind, changed, changed by), search by name and content across the workspace, filters by kind and by who changed a file, and a breadcrumb trail. Lists load in pages and render only the visible rows.
- **Context.**
  - ADR-015 gave people places but no way to group the folders, agents and history of one piece of work, so grants were global and the Files view was flat.
  - Business users think in pieces of work, not in folders.
  - Real document folders hold thousands of files in nested folders (A-21).
- **Consequences.**
  - Folder grants become per workspace: the File Service and the Rules Engine resolve access from the workspace a run started in.
  - The journal records the workspace of every run, approval and conversation.
  - The term "workspace" in sections written before revision 0.7.0 meant the agent folder; those sections are reworded in revision 0.7.0, except earlier entries of this file (see the terminology note above).
  - Search across a workspace needs a local index of file names and text (A-22).
- **Alternatives considered.**
  - *A workspace is one folder tree* (as in a code editor). Rejected by the user: business work spans several folders and Confluence.
  - *A shared team workspace.* Rejected by the user for now: the application serves one person, and sharing goes through Confluence.

### ADR-026: Confluence is read both ways and published out through linked folders

- **Status.** Decided by the user, January 2026 (choices "Read both, publish out", "Pages for text, attachments otherwise", "Linked folders, approve the first time"). The mechanism is proposed; the deployment and sign-in are open (OQ-23).
- **Decision.**
  - **Reading.** A workspace can show Confluence locations beside its local places. The person browses spaces and page trees, reads pages with their attachments, and gives a page to an agent as an input. Agents read only the Confluence locations linked to their workspace.
  - **Publishing.** A local file is published to a Confluence location:
    - a text document (Markdown, plain text, and Word once it can be converted — OQ-24) becomes a Confluence page, with the file's folder structure becoming the page tree;
    - every other kind of file (spreadsheets, slides, PDFs, images and the rest) is attached, as the exact file, to the page of its folder.
  - **Linked folders.** The person links a folder to a Confluence location once, through an approval card that states the space, the parent page and who can see it there. After that:
    - a change to an already-published file is pushed automatically as a new version of its page or attachment;
    - a new file, a removed file and a renamed file wait in a *Ready to publish* list for the person's one-click check.
  - **Edits made in Confluence.** When a published page has a newer version in Confluence than the one last pushed, the file shows "Changed in Confluence" with **Bring a copy back**, which saves the page as a new local version. The local file is never overwritten without that action, and the next automatic push of that file waits until the person chooses which side to keep.
  - **Sign-in.** The person signs in with their own Confluence account the first time a workspace needs it (the same pattern as Slack, A-17). The Confluence site comes from the organisation profile.
  - **Agents publishing.** An agent may ask to publish a file to a linked location; that request uses the approval card of the UX guidelines §5.5 unless the folder is already linked, in which case it follows the linked-folder rules above.
- **Context.**
  - Confluence is where the company shares documents; people want local work to reach it without copying by hand.
  - Documents and Confluence pages are different formats, so a true two-way mirror would lose formatting and create silent conflicts (rejected below).
- **Consequences.**
  - A new external system, a new adapter and a new credential kind (Confluence) in the keychain.
  - Every push is an external action in the journal, with its unique key, so a restart never pushes twice.
  - Converting a document to a page can change its layout (R-22).
  - A draft placed in a linked folder could be published unintentionally; the *Ready to publish* check for new files is the safeguard (R-21).
- **Alternatives considered.**
  - *Full two-way synchronisation.* Rejected by the user: conflicts and lost formatting.
  - *Read only.* Rejected by the user: publishing is required.
  - *Every publish approved separately.* Rejected by the user: too many approvals for busy folders.
  - *Every file as an attachment.* Rejected: readers would have to download text documents.

### ADR-027: Ordin is the person's one conversation and the coordinator of every crew

- **Status.** Decided by the user, January 2026 (choice "Ordin builds, can save"). The mechanism is proposed. **Amends ADR-006** (a crew's coordinator is no longer a package named in its charter) **and the guided crew builder** of 11 — UX guidelines §5.7. *Amended by ADR-038 and ADR-044: Ordin runs on the person's own plan with the harness they signed in to, Claude first; ADR-062 sets how that plan is spent.* *Amended in revision 0.36.0 (ADR-064): a kept crew becomes a workflow, which repeats its steps with no coordinator turn per round and can be changed on its own screen.*
- **Decision.**
  - **Ordin** is an agent package built with AgentWright, bundled with the application at a pinned version, and always present. The person does not install, update or remove it.
  - The primary interface is one conversation between the person and Ordin, per workspace (ADR-025). Every task can start there; forms and the other views are things Ordin shows inside or beside that conversation (ADR-028).
  - Ordin knows every installed agent instance: its commands, skills, arguments, lifecycle, authority and declared rules. The core gives it this as an **agent directory** built by the Package Inspector, so Ordin never reads package internals at run time and the person never has to learn them.
  - For each request Ordin either answers itself, delegates to one agent, or forms a crew. It is the coordinator of every crew. Its crew tools are those of ADR-006 (delegate, finish, escalate) plus the tools of ADR-028, ADR-030, ADR-032 and *create crew*. It has no file, shell, network or Craft tool of its own.
  - A crew Ordin forms is written as a crew charter by the core, within the guardrail defaults and maxima of 07 — Cross-cutting. The Crew Orchestrator enforces it unchanged (G4). The person can say "keep this crew"; the charter is then saved under a name and Ordin can reuse it. The guided crew builder screen is removed. *Amended in revision 0.36.0 (ADR-064): the person saves the crew with **Save as workflow…**; "keep this crew" is a retired phrase (11 — UX guidelines §3, UX-A38).*
  - Ordin answers a member's first questions itself, from the request, the workspace and memory (ADR-030), and passes to the person only what it cannot answer. Each such question reaches the person in plain, brief language, with Ordin's recommendation.
  - Ordin runs on the person's Copilot subscription like any local session (G6), and its requests count towards the crew's request budget.
- **Context.** The person should not need to know which agent does what, how its commands work or how a crew is put together. A per-crew coordinator package and a crew builder both asked the person to make technical choices that G10 rules out.
- **Consequences.**
  - Ordin is a new package for AgentWright to build and for the company to maintain; its quality bounds the whole experience (R-24).
  - Ordin sees every request, so it is the main target for prompt injection through documents and replies. Its lack of action tools and the fixed-code guardrails are the mitigation (07 — Cross-cutting, threat model).
  - Charters still exist as files, so a saved crew is repeatable and exportable as before.
- **Alternatives considered.**
  - *Ordin with no saved crews.* Rejected by the user: the same work could not be repeated the same way.
  - *Ordin plus the crew builder.* Rejected by the user: a second, manual route to the same result.
  - *A coordinator package per crew (ADR-006 as written).* Superseded: the person would choose coordinators.

### ADR-028: Ordin assembles screens from a fixed catalogue of blocks

- **Status.** Decided by the user, January 2026 (choice "Ordin picks from fixed blocks"). The catalogue below is proposed; block schemas are fixed in `/spec`. *Amended by ADR-046 (screens in the centre), ADR-050 (no project tree), ADR-058, ADR-067 (reports and dashboards) and ADR-085 (fixed-code checks).*
- **Decision.**
  - The interface is minimal: the workspace switcher, the conversation with Ordin, a *Needs you* list, and Settings. Everything else appears when Ordin shows it.
  - Ordin shows things by calling a **show** tool with a block from a fixed catalogue. The first catalogue is: message; question with choices; form (from a command's declared arguments, or composed by Ordin from typed fields); file list and folder tree; preview; change view (text differences, or before-and-after pages for documents, slides and images); plan and progress; result; agent proposal (from the marketplace); crew plan; memory facts. *From revision 0.10.14 an agent proposal is shown as the agent library screen with the proposed agent chosen, and a file list is never used to confirm adding a link (UX-A33).*
  - The core validates every block against its schema and renders it with the application's own code. Ordin supplies data, never markup, script or style. A block that fails validation is dropped and Ordin is told why.
  - **Approval cards are not in Ordin's catalogue.** The Approval Service builds them from the action itself (what, where, what it affects), so no agent can draw an approval or word one misleadingly.
  - A block can be pinned beside the conversation (for example a file list or a crew's progress), so the person keeps it in view while talking.
  - Engineering mode (ADR-019) keeps its fixed views — project tree, code editor, change review, terminal — because engineers need them in a stable place.
- **Context.** The person should not have to learn the interface; the interface should fit the task. Free-form screens written by a model could carry injected controls and would look different every time.
- **Consequences.**
  - Settles the shorter-menu question of revision 0.6.0: the fixed views of 11 — UX guidelines §4 become blocks, except engineering mode and Settings.
  - A new public contract towards AgentWright: the block catalogue Ordin is built against.
  - A need the catalogue does not cover cannot be shown until the catalogue grows; Ordin falls back to a message.
- **Alternatives considered.**
  - *Ordin writes its own screens.* Rejected by the user: injected controls and inconsistent screens.
  - *Keep the fixed screens.* Rejected by the user: the person would still have to learn them.

### ADR-029: A workspace is a set of links to documents where they live; Ordin edits them and every push back is approved

- *Amended in revision 0.54.0 by ADR-106: a workspace may also link a Jira issue, on the desktop; its summary and description, comments, attachments and related-issue list are read-only working-copy items, and nothing is pushed back to Jira from a link.*
- **Status.** Decided by the user, January 2026 (choice "You approve each push"). The mechanism is proposed. **Amends ADR-025** (a workspace holds links, not only local places), **ADR-026** (Ordin's changes are never pushed automatically) **and A-23** (dropped loose files are no longer copied into Documents by default). *Amended by UX-A33 (adding is done on a screen in the centre, not in a file block in the conversation) and by §5.10 and §5.15 of 11 — UX guidelines (the person edits knowledge pages and documents with **Edit**).* *Amended in revision 0.36.0 (ADR-048, ADR-057, ADR-070): ADR-048 amends this decision; the person edits Markdown documents, knowledge pages, agents' own memory pages and email drafts by hand.*
- **Decision.**
  - A workspace is a set of **source links**. Each points to where the content actually lives: a local folder; a folder of a git repository on the computer (typically Markdown documentation kept on GitHub); a SharePoint site, library, folder or file; a Confluence space, page tree or page.
  - The application reads linked content to show it and to give it to agents. It keeps a local working copy of what has been read, for previews, search and editing; the origin remains the source of truth.
  - The person asks Ordin to add a link in the conversation ("add the supplier folder from SharePoint"), or drags a file or folder in. Ordin finds the location, shows it in a file block, and the link is added once the person confirms.
  - The person does not edit documents by hand in ordin. Ordin has an agent make the change, through the Craft tools (ADR-017): WordCraft for Word, DeckCraft for PowerPoint, GridCraft for spreadsheets, PdfCraft for PDFs, PhotoCraft for images, and the code editor's text operations for Markdown. The person can still open any file in its own app. *Amended in revision 0.36.0 (ADR-048, ADR-057, ADR-070): by hand the person edits Markdown documents (with a formatting toolbar), knowledge pages, agents' own memory pages (with the same toolbar from 0.36.1) and email drafts (with the same toolbar); everything else is changed by agents or opened in its own app. Word, Excel and PowerPoint files open in the person's own Microsoft app, and agents do not change spreadsheets yet.*
  - *Added in revision 0.36.0:* what the person can view in ordin, which app opens each kind, what they edit by hand, through which app agents change it and how the change is shown, is set out kind by kind in 11 — UX guidelines §5.6, *Changing documents*.
  - **Every push to an origin is approved.** Ordin groups the changes of one request into one approval card: each file, its origin, a one-line summary and the change view. Approval pushes: a new version to SharePoint, a new page version or attachment to Confluence, or a commit and push to the repository's remote through `gh` and git under ADR-023. A change Ordin made is never pushed automatically, even in a folder linked under ADR-026; the automatic push of ADR-026 applies only to changes the person made outside ordin.
  - Before a push, the core checks that the origin has not changed since the working copy was read. If it has, the push stops and Ordin shows both versions; nothing at the origin is overwritten.
  - SharePoint is reached through Microsoft Graph with the person's own Microsoft account (delegated permissions); the tenant and the application's registration come from the organisation profile (C-14).
- **Context.** Corporate documents live in SharePoint, Confluence and git repositories; copying them into a local folder creates stale duplicates. The person wants Ordin to keep them current where they are. A document can carry instructions that try to steer an agent, so a change reaching colleagues needs a person's look first.
- **Consequences.**
  - A new external system (SharePoint), adapter and credential kind.
  - Editing Word and PowerPoint depends on WordCraft and DeckCraft command-line tools (ADR-018, C-13); until confirmed, Ordin can read and summarise those files but not change them (R-25).
  - Every push is an external action in the journal with its unique key, so a restart never pushes twice.
- **Alternatives considered.**
  - *Approve once per workspace.* Rejected by the user: a bad edit could reach colleagues unseen.
  - *Fully automatic pushes.* Rejected by the user: drops the rule that anything others can see needs approval (G8).
  - *Copy everything into a local folder (A-23).* Superseded: stale duplicates.

### ADR-030: Ordin's memory is the memory component, embedded in the core

- **Status.** Proposed, January 2026, so that Ordin has agentic memory supported by `~/IdeaProjects/recall`. **Amends the non-goal "Elaborate memory automation"** of 00 — Overview. *From revision 0.10.12 memory sits beside knowledge pages: facts stay short and fade, and explanations with their sources are kept as pages (ADR-057).* *Amended in revision 0.51.0 (ADR-103): in a workspace in Team Knowledge mode, the workspace's facts also leave the computer, as files in a GitHub or GitLab repository folder, after the person approves each share; a colleague's facts arrive through the write gate as data.* *Amended in revision 0.60.0 (ADR-111): every agent session with memory rules remembers through the memory, agent task sessions included; facts carry the things they are about and the links between them, which recall uses; Ordin writes summaries; the person sees things, links, history, fading and what each answer used.*
- **Decision.**
  - The memory component (`memory/`, a Rust crate with a library target, copied from recall under ADR-034) is linked into the core and used in-process, with its embedded store in the application's data folder. There is no memory server and no network port.
  - Scope: one tenant per installation, one user (the person), and one team scope per workspace, so facts learned in one workspace are not recalled in another unless the person moves them.
  - Ordin, and members of a crew Ordin runs, write and recall facts through crew tools that the core forwards to the memory component. Package `memory/` folders continue unchanged for each package's own skill.
  - The person sees, corrects and deletes facts through the memory block (11 — UX guidelines §5.10).
  - Content read from documents passes the memory component's write gate as data, never as instructions.
- **Context.** Ordin must remember the person's preferences, earlier decisions and what it has learned about their documents across sessions, and crew members should share what they learn.
- **Consequences.**
  - The memory component today requires an embedding service and a reranking service at configured addresses (`MEMORY_EMBED_URL`, `MEMORY_RERANK_URL`) and an OIDC issuer (`MEMORY_OIDC_ISSUER`) (`memory/src/config.rs`). In-process use needs either a mode without OIDC or a local issuer, and needs a decision on where embeddings come from (OQ-25, C-15).
  - recall belongs to the user, who has confirmed it may ship inside ordin; it will be copied into this repository under a new name (ADR-034). Its third-party dependencies keep their own licences when distributed (C-16).
- **Alternatives considered.**
  - *Package `memory/` files only.* Rejected: no memory across agents or for Ordin itself.
  - *recall as a separate local service.* Rejected for now: a second process and port for one person's data.

### ADR-031: Code written by agents runs in the sandbox, embedded in the core

- **Status.** Decided by the user, January 2026 (choice "Agent-written code only"). The mechanism is proposed.
- **Decision.**
  - Any code an agent writes and wants to run — data transformation, analysis, calculation — runs in the sandbox (`sandboxd` in `sandbox/sandbox-host/`, a Rust crate with a library target, using wasmtime, copied from faraday). The agent reaches it through the sandbox's one tool, `python_sandbox`.
  - The sandbox has no network and no files of its own. Inputs the agent names from granted places are passed in by the core; outputs come back as artefacts. Calls to company systems, where allowed, go through the sandbox's broker with the person's tokens from the keychain; the code never holds a token.
  - Trusted, pinned programs run outside the sandbox, under the Rules Engine and limited to granted places: the Craft CLIs, git, `gh`, `apm`, language servers and the Copilot CLI.
  - The sandbox's off-laptop broker service is not used in the first release; only the local broker is.
- **Context.** Code execution happens in a sandbox built into the desktop. The sandbox runs Python only, in a WebAssembly room, so native programs cannot run inside it.
- **Consequences.**
  - Replaces A-06's approved shell commands for agent-written code; shell commands remain only for system-changing packages working on a project, under approval (OQ-26).
  - Only Python code can run; an agent that needs another language cannot run its code (R-26).
- **Alternatives considered.**
  - *Every program in faraday.* Rejected by the user: faraday would have to sandbox native programs.

### ADR-032: Ordin finds and adds agents from the marketplace

- **Status.** Proposed, January 2026, so that Ordin can search the marketplace for agents and install them. **Builds on ADR-012 and ADR-013.** *From revision 0.25.0 Ordin searches the catalogue index, not `apm search` (ADR-082), and when nothing covers the request it says what is covered and offers to make an agent (ADR-083).*
- **Decision.**
  - When no installed agent can do what the person asks, Ordin searches the internal marketplace through a crew tool that runs `apm search` in the Package Catalogue, and shows the best match as an agent proposal block: what it does, in plain language, and who publishes it. *From revision 0.10.14 the proposal opens the agent library as a screen with that agent chosen and marked "Ordin suggests this one", and **Add it** is there; the conversation carries one line (UX-A33).*
  - On the person's "Add it", the Package Catalogue installs it exactly as ADR-012 describes. The organisation's trust policy alone decides whether its executable parts are enabled (ADR-013); a package the policy does not cover cannot be added, and Ordin says so.
  - Ordin never installs without the person's "Add it", and never removes an agent.
- **Context.** The person should not have to browse a marketplace or know agent names.
- **Consequences.** The *Add an agent* screen becomes a block Ordin shows; browsing remains possible by asking Ordin.
- **Alternatives considered.**
  - *Ordin installs silently.* Rejected: the person would find agents they did not choose acting in their workspaces.

### ADR-033: Memory's embedding and reranking run on small models bundled with the application

- **Status.** Decided by the user, January 2026 (OQ-25: "A small model bundled with the app"). The models are chosen in `/spec`. **Amends the non-goal "Local model inference"** of 00 — Overview.
- **Decision.**
  - The application bundles a small embedding model and a small reranking model, and runs them on the person's computer for the Memory Service (ADR-030). No text leaves the computer to make memory searchable.
  - These are the only local models. Conversation, delegation and every agent's work still run on Copilot's models in GitHub's service.
  - The memory component today reaches embedding and reranking over HTTP (`memory/src/providers/mod.rs`); running local models needs either an in-process provider in the memory component or a loopback endpoint the core serves (C-18).
- **Context.** A company-hosted service would have needed IT to run a server; the user chose to keep memory entirely on the computer.
- **Consequences.**
  - The installer grows by the size of both models, and indexing uses the computer's processor (R-28).
  - Model versions are pinned with the application and updated with it.
- **Alternatives considered.**
  - *A company-hosted service named in the organisation profile.* Rejected by the user.
  - *Keyword-only memory.* Rejected: recall's retrieval is built on embeddings.

### ADR-034: The memory component is copied into this repository under a new name

- **Status.** Decided by the user, January 2026 ("I own the code … we will copy it into this workspace and rename it"). The new name is not yet chosen (OQ-27).
- **Decision.** recall's source is copied into this repository and renamed, and the core links that copy. The design calls it *the memory component* and keeps "recall" only for the source it comes from.
- **Consequences.** Changes for the desktop — single-user scope without OIDC (C-15) and local models (ADR-033) — are made in the copy, not upstream.
- **Alternatives considered.** *Depend on recall's own repository.* Rejected by the user.

### ADR-035: A local edit to a document with an unsent change from Ordin holds that change

- **Status.** Decided by the user, January 2026 (UX-OQ-3: "hold Ordin's change and show both versions"). **Extends ADR-029.**
- **Decision.** When the person changes a document on the computer while Ordin has an unsent change to it, the Push Service holds Ordin's change, marks the pending push *held*, and shows both versions in a change view. The person keeps their version (Ordin's change is dropped, and Ordin can redo it on the new version), keeps Ordin's, or asks Ordin to apply its change to their version. Nothing is pushed until the person has chosen and then approved the push.
- **Context.** The same rule as a change at the origin (ADR-029): nothing is overwritten without the person seeing both sides.
- **Consequences.** The working copy is watched for changes while a push is pending.

Project check commands and system-changing packages' shell commands stay outside the sandbox, under the engineer's approval, as ADR-031 states (OQ-26, settled by the user on January 2026: "keep it for now").

The memory component is named `memory` (OQ-27, settled by the user on January 2026).

### ADR-036: ordin is an exo-harness over a replaceable vendor harness

- **Status.** Decided by the user, January 2026 (decision 13: "ordin … can be considered an exo-harness that sits on top of a vendor harness"; "yes" to writing it into the design). The boundary detail is proposed. *From revision 0.25.0 the APM marketplace service is replaced by the agent catalogue: catalogue repositories, the Checker, the Installers and the agent maker's authoring tools (ADR-081 to ADR-083).*
- **Decision.**
  - A **vendor harness** is the agent runtime a model vendor ships: Claude Code (ADR-038) or the Copilot CLI (ADR-001). It owns the inner loop of one session: model calls, the tool loop, context and session persistence.
  - **ordin** owns the outer loop and adds two things the vendor harness does not provide:
    1. **A coordinating entity.** Ordin mediates between the person and every agent that works on a job, at all times (ADR-027). Before Ordin reports a delegated step or a crew run as done, it compares the result with the person's request and with the package's acceptance checks where the package declares them. It reports any shortfall to the person with its recommendation; it never reports a shortfall as done.
    2. **Services:** agentic memory (ADR-030), a sandbox for agent-written code that brokers the person's corporate credentials (ADR-031), the ArtCraft apps (ADR-016, ADR-039), and the internal APM marketplace, through which agents are found, installed, updated and removed (ADR-004, ADR-012, ADR-032).
  - The guardrails, the Rules Engine, approvals and the journal are ordin's own fixed code, outside every harness (G4, G8, ADR-005).
  - No ordin service depends on a specific harness. Harness-specific code lives only in a runtime adapter (ADR-003). Services reach a harness session through MCP (ADR-037).
- **Context.** The design described a desktop application with one local runtime. The user's framing names what the application is for: its value lies in the coordinator and the services, and it should survive a change of vendor harness.
- **Consequences.**
  - A second harness costs one adapter, not a change to crews, memory, the sandbox, the Craft tools or the marketplace.
  - The check-before-done duty is a new requirement on Ordin's package and its acceptance checks (R-24).
  - Sections written for the Copilot SDK describe the Copilot adapter; they are not rewritten (00 — Overview, revision 0.9.0 note).
- **Alternatives considered.**
  - *A desktop front end for one harness.* Rejected: it ties every service to one vendor's extension points.
  - *ordin's own agent loop in Rust.* Rejected, as in ADR-001: the most implementation responsibility, and no vendor's model access comes with it.

### ADR-037: MCP is the one way ordin's services reach a harness; the sandbox's MCP tool is the only route for code execution

- **Status.** Decided by the user, January 2026 ("MCP as the common way in: yes and the MCP interface should be the one exposed by the sandbox for code execution"). The mechanism is proposed. **Amends ADR-017, ADR-027 and ADR-031** where they offer tools through the Copilot SDK's custom tools.
- **Decision.**
  - Every ordin service is offered to a harness session as an MCP server, registered for that session by the runtime adapter. No service is offered as a harness-specific custom tool. The servers are:
    - **Sandbox:** the sandbox's own MCP server and its one tool, `python_sandbox` (`sandbox/README.md`).
    - **Memory:** the memory component's MCP server (`memory-mcp` in `memory/Cargo.toml`).
    - **Craft tools:** each ArtCraft app's MCP mode, fronted by ordin (ADR-039).
    - **Ordin tools:** the crew tools and the tools of ADR-027, ADR-028, ADR-030 and ADR-032, served by the core as one MCP server, offered only to Ordin's session.
  - **Code execution.** The sandbox's MCP tool is the only way an agent runs code it wrote. Each harness's own code-running and shell tools are switched off in every session. The one exception is unchanged: system-changing packages working on a project keep their approved shell commands outside the sandbox (ADR-031, OQ-26).
  - **Enforcement at the server.** Every call to an ordin-served MCP server is decided by the Rules Engine inside that server, before it acts. Safety for ordin's services therefore does not rest on a harness's permission hooks. Hooks remain necessary for the harness's remaining built-in tools and for MCP servers a package declares (ADR-005).
  - The sandbox and memory stay linked in the core (ADR-030, ADR-031). The core serves their MCP servers to the harness process over a local transport the harness can reach, either stdio through a small launcher or a loopback address; the choice is made in `/spec` (C-17, C-22).
- **Context.**
  - Both harnesses accept MCP servers per session: Claude Code with `--mcp-config` and `--strict-mcp-config`, and the SDK's `mcpServers` option (code.claude.com/docs/en/cli-reference, …/agent-sdk/mcp, read on January 2026); the Copilot SDK lists MCP among its features (05 — Tech stack).
  - The sandbox is already an MCP server built to be "independent of the editor" (`sandbox/README.md`).
- **Consequences.**
  - One tool contract per service for every harness.
  - The sandbox's credential broker is the only place agent-written code reaches company systems, whatever the harness.
  - An agent that needs a language other than Python cannot run its code (R-26, unchanged).
- **Alternatives considered.**
  - *Custom tools per harness SDK.* Rejected: each service would be written once per harness, and enforcement would depend on each SDK's hooks.
  - *Leaving the harness's shell tool on under approval.* Rejected by the user: code execution goes through the sandbox.

### ADR-038: Claude is the first vendor harness, Copilot the second, both in the first release

- **Status.** Decided by the user, January 2026 ("yes, suggest claude first, then copilot"). The mechanism is proposed. **Amends ADR-001** (Copilot is no longer the first adapter) **and G6**. It withdraws the non-goal "A Claude Agent SDK runtime adapter in the first release".
- **Decision.**
  - The **Claude Local Adapter** is built first. It drives a bundled, pinned Claude Code CLI as a subprocess, one per local session, with the agent folder as its working folder. It uses the headless mode (`-p`) with streamed JSON in and out (`--input-format stream-json`, `--output-format stream-json`) and `--bare`. The Agent SDK has no Rust edition; the documentation names the CLI as a subprocess as the route for other languages (code.claude.com/docs/en/agent-sdk/overview, read on January 2026). Whether to drive the CLI directly or through a TypeScript Agent SDK sidecar is OQ-29.
  - Each session is started with only the MCP servers ordin registers and the package declares (`--mcp-config`, `--strict-mcp-config`). The CLI's own shell, code-running and web tools are switched off (`--tools`, `--disallowedTools`). The file tools that remain are decided by the Rules Engine through a hook or a permission-prompt tool ordin serves (`--permission-prompt-tool`), as C-20 confirms.
  - Packages are installed for the session's harness: `apm install … --target claude` for the Claude adapter, `--target copilot` for the Copilot adapter. APM 0.31.0 lists both targets (`apm install --help`, observed on the user's Mac on January 2026).
  - The **Copilot Local Adapter** of ADR-001 follows as the second adapter, unchanged in mechanism.
  - The organisation profile names which harness, or harnesses, the organisation uses. The person never chooses a harness (G10).
  - **Sign-in.** A business user is still never asked for a model key (G6), and the application still holds no shared secret of the organisation (01 — Context, trust boundary 3). Anthropic does not allow a third-party product to offer claude.ai login without prior approval: "Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK" (code.claude.com/docs/en/agent-sdk/overview, read on January 2026). How each person reaches Claude with their own identity is therefore OQ-28, which blocks the Claude adapter's spec.
- **Context.** ADR-001 chose Copilot because most users hold Copilot seats and Claude needed an organisation key and a second language runtime. The user now puts Claude first. The exo-harness framing (ADR-036) makes the order a matter of adapters, not of architecture.
- **Consequences.**
  - The first release carries two harness sidecars, and its tests run each package on both (R-30).
  - Claude usage is billed to the organisation through the route OQ-28 chooses, not to a person's subscription. Budgets are measured in tokens or cost for Claude and in requests for Copilot (R-29, OQ-3).
  - A-11 and A-24 are read per harness: one CLI process per local session, and Ordin's turns measured against whichever harness runs it.
  - C-01 to C-03 and C-05 are repeated for Claude Code as C-19 and C-20.
- **Alternatives considered.**
  - *Copilot first, Claude later* (ADR-001). Superseded by the user's decision.
  - *A TypeScript Agent SDK sidecar.* Open as OQ-29: it gives programmatic `canUseTool` and hooks, but adds a second language runtime to the installer.
  - *An organisation API key in the organisation profile.* Rejected: a shared secret of the organisation on every laptop breaks trust boundary 3.

### ADR-039: Agents reach ArtCraft only through each app's MCP mode, fronted by ordin

- **Status.** Decided by the user, January 2026 ("Should ArtCraft be driven through MCP? yes"). The mechanism is proposed. **Amends ADR-017** (the custom-tool route over the CLIs' run and batch commands is withdrawn) **and G12** ("Agents drive the apps' command-line tools" now reads "through the apps' MCP mode").
- **Decision.**
  - A Craft tool is a tool of an ArtCraft app's own MCP server (`<app>-cli mcp`, as WordCraft's app text names it; C-13).
  - The Craft Toolkit starts that server and serves its tools to the session through ordin's MCP front, so the Rules Engine decides every call before it reaches the app (ADR-037). The checks of ADR-017 are unchanged: input and output paths lie in the session's granted places, and the tool is on the app's allowed list in the profile. Outputs become new file versions.
  - An app whose CLI has no MCP mode gives agents no Craft tool for its kinds of file; the start-up report says so (G9). The person can still open the file in the app's window.
  - The JSON control channel stays unused in the first release (ADR-017).
- **Context.** ArtCraft describes every app as drivable "from a CLI, a JSON control channel or an MCP server" (getartcraft.com/apps, read on January 2026). MCP is the common way in (ADR-037).
- **Consequences.**
  - One integration route per app, the same for every harness.
  - Each app's MCP tool list must be confirmed before approval (C-07, C-13). `wordcraft-cli` is not yet installed on the test Mac, so no Craft MCP tool list has been read.
  - The person's own uses of the CLIs (convert, export, preview) by the File Service are not agent tools and keep calling the CLI directly.
- **Alternatives considered.**
  - *The CLIs' run and batch commands as custom tools* (ADR-017). Rejected by the user.
  - *Harness sessions connecting to the app's MCP server directly.* Rejected: enforcement would then rest on each harness's hooks.

### ADR-040: The sandbox runs shell scripts as well as Python, and gains the rest of agent-sandbox's working features

- **Status.** Decided by the user, January 2026 ("the sandbox should also be able to run bash code in line with https://github.com/Parassharmaa/agent-sandbox, including any other features that might make the exo-harness more proficient and all rounded"). The mechanism is proposed. **Amends ADR-031** (Python only) **and R-26.** Settles F3 and F4 of the sandbox RFC (`sandbox/docs/wip/rfc/01-agent-sandbox-features.md`) and adds F5 to F8 to it. The earlier rule stands: the code is written for this repository, nothing is ported (RFC decision of January 2026).
- **Decision.**
  - **Shell (RFC F3).** A shell guest runs agent-written shell scripts in the sandbox's WebAssembly room. It is a shell interpreter that answers to `sh` and `bash`, with pipes, redirections, variables, loops, functions and command substitution, and a toolbox of text, file, data and archive commands. Its `git` works on local repositories only. It cannot start native programs. The shell is a subset of GNU bash (R-32).
  - **JavaScript (RFC F4, moved from deferred).** A JavaScript guest runs agent-written scripts without Node's built-in modules.
  - **Sessions (F5).** The working folder of sandboxed runs (sandbox ADR-041) can live for one agent session: several calls share it, and the change record is cumulative. The folder is deleted when the session ends, or after an idle limit set by the profile.
  - **Brokered HTTP from every guest (F6).** The shell's `curl`-compatible command and JavaScript's `fetch()` reach the network only through the broker, under the same capability allowlist and the same token injection as Python's broker calls. No guest has a direct network path, and code still never holds a token.
  - **Environment (F7).** The caller may pass non-secret environment variables into a run. Secrets are refused at the interface; credentials reach company systems only through the broker.
  - **Fast start (F8).** Guest modules are compiled ahead of time and cached per process, where the runtime does not already do so (C-23).
  - **One MCP server.** All guests are tools of the sandbox's one MCP server, beside `python_sandbox` (ADR-037). Tool names are fixed in the sandbox's spec.
- **Not adopted from agent-sandbox, as before:** direct guest networking under a fetch policy, mounting the person's folders into the room (inputs are still passed in and outputs returned, ADR-031), Node bindings, and prebuilt WebAssembly from that project.
- **Context.** Agents work in shell as often as in Python: text chores, file sorting, archive handling and local git. agent-sandbox (MIT, Rust, Wasmtime) shows the feature set in a WebAssembly room (`https://github.com/Parassharmaa/agent-sandbox`, read on January 2026). Its networking model would break the sandbox's central property, which is why only its broker-compatible features are taken.
- **Consequences.**
  - Agent packages are told the sandbox runs Python, shell and JavaScript. Native programs, such as compilers and project build tools, still cannot run in it; project check commands stay outside under the engineer's approval (OQ-26).
  - The sandbox's own HLD (`sandbox/docs/design/sandbox-host/`) needs an HLD-impact pass for F3 to F8 before implementation.
  - Shell scripts written for full GNU bash may fail on unsupported features; the failure returns to the agent as an error it can act on.
- **Alternatives considered.**
  - *Depending on the agent-sandbox crate.* Rejected, as in the RFC decision of January 2026.
  - *A native shell under approval instead of a sandboxed one.* Rejected: agent-written code runs only in the sandbox (ADR-031, ADR-037).

### ADR-041: The interface is white-label, through a brand pack in two layers

- **Status.** Decided by the user, January 2026 ("there should be white label capability in the exo-harness UI"). The mechanism is proposed.
- **Decision.**
  - Everything that identifies the product to the person comes from a **brand pack**, never from code. The default brand pack is ordin's own. A brand pack holds:
    - the product name, and the coordinator's display name (default "Ordin"), each per locale;
    - logos and the application icon set, in light and dark variants;
    - colour and typography design tokens, with font files;
    - help, support and privacy links, and the "About" and legal text, per locale.
  - **Build layer.** What the operating system shows before the application runs — the application's name, icon, bundle identifier, installer, code-signing identity and update feed — is set by a branded build produced from the brand pack by the build pipeline. A rebrand of this layer is a new installer. *Fixed in revision 0.49.0 (`/spec`): ordin signs in to GitHub as a GitHub App (OQ-6's recommendation) with the OAuth device grant on the desktop and the server; the App's client identifier is compiled into the branded build (`GITHUB_APP_CLIENT_ID`), and its device flow must be enabled (C-49).* *Amended in revision 0.62.0 (ADR-114): the unbranded open-source build carries the project's release feed address; a branded build may carry another.*
  - **Run layer.** What the application shows inside its window — logo, colours, typography, coordinator name, links and text — comes from the brand pack carried in the signed organisation profile (ADR-013) and changes without a reinstall.
  - Every colour, font and spacing in the App Shell and the Block Renderer is drawn from design tokens. No component carries a literal colour or product name.
  - **Fixed by the application, not the brand:** the structure and wording of approval cards, the meaning of the run-state labels and their colours for danger and waiting (11 — UX guidelines §8), the statement of who acts and where it goes, and the accessibility baseline. A brand pack whose tokens fail WCAG 2.2 AA contrast for text and controls is refused, and the default brand is used, with the reason sent to IT. *Amended in revision 0.49.0 (`/spec`): the reason is kept with the refused pack and shown in Settings › Diagnostics, where the person's **Send to IT** sends it; ordin sends no help request without the person's press (08 — Help request).*
- **Context.** The application is to be offered under other names and identities. G10 rules out the person choosing appearance settings beyond their own preferences, so the brand is IT's decision, through the profile.
- **Consequences.**
  - The organisation profile grows a brand section, and the build pipeline takes a brand pack as an input.
  - The coordinator's display name is a brand string; "Ordin" in this design is the default brand's name for it. Ordin's package instructions take the display name as a setting, so the agent names itself as the brand does.
  - The screen review checklist gains a brand check: every screen is reviewed with the default brand and with a test brand of different name length, colours and fonts (11 — UX guidelines §12).
- **Alternatives considered.**
  - *Run layer only.* Rejected: the operating system would still show ordin's name and icon.
  - *Build layer only.* Rejected: every change of logo or colour would need a new signed installer.
  - *Free CSS in the profile.* Rejected: a style sheet can hide or restyle approval cards (ADR-028).

### ADR-042: Internationalisation is built in from the first release

- **Status.** Decided by the user, January 2026 ("internationalisation should be considered from the outset, not an after thought"). The mechanism is proposed; the first locales are OQ-30. *Amended in revision 0.58.0 (ADR-109): catalogues are no longer only compiled in. A person-owned layer, kept in ordin's own storage, holds the person's edits to a shipped language and whole languages the person made with Ordin or imported, and one fixed check guards every way text enters it. Model translation is allowed only as a draft the person asks for: the safety wording — approval cards and the statements of who acts and where it goes — is used only after the person has checked it, and no model is called to show any message. Ordin is told the person's language as data in its seed.*
- **Decision.**
  - **No literal interface text.** Every string the person can see — in the App Shell, the Block Renderer, approval cards, notifications, errors, the Setup Assistant and the installer — is a message in a catalogue, by identifier, in Project Fluent format. The core (approval cards, errors, notifications) and the WebView use the same catalogues.
  - **Locale.** The person's locale follows the operating system by default. The person can change it in Settings, among the locales the organisation profile offers; the profile also sets the fallback. en-GB is the source locale.
  - **Formatting.** Dates, times, numbers, plurals, lists, sorting and time zones use the locale's Unicode CLDR rules, in the core and the WebView. Nothing is formatted by string concatenation.
  - **Layout.** Right-to-left scripts are supported from the start: the interface uses logical layout directions, and direction-bearing icons mirror. Layout allows text 40% longer than en-GB.
  - **Ordin's language.** Ordin converses, summarises and fills blocks in the person's locale. An agent's own output and documents stay in the language they were written in; Ordin translates when the person asks, and shows the original beside the translation.
  - **Approval cards are never translated by a model.** Their fixed text comes from the catalogue. Content being approved — a message to post, a change to a document — is shown verbatim, in its own language.
  - **Search and memory are multilingual.** The workspace search index tokenises by the content's script (A-22), and memory's bundled embedding and reranking models are chosen multilingual (ADR-033, in `/spec`).
  - **Tests.** Every build runs a pseudo-locale (accented, 40% longer, and a right-to-left variant) through the screen tests. A string with no catalogue entry fails the build. *Fixed in `/spec` (revision 0.49.0):* the pseudo-locales are `en-XA` (accented, 40% longer) and `ar-XB` (right-to-left), present only in test builds; message identifiers are compiled constants, so a missing `en-GB` message fails the build.
- **Context.** Retrofitting catalogues, layout direction and formatting after screens exist touches every screen. White-labelling (ADR-041) needs per-locale brand strings, and dictation (ADR-043) needs the person's language.
- **Consequences.**
  - The interface framework (OQ-1) must support Fluent catalogues and right-to-left layouts.
  - Each locale added needs a reviewed translation of the catalogues; a missing message falls back to the profile's fallback locale, never to an identifier.
  - The design documents stay in en-GB (`cspa/layer/output-language`); that rule governs these artefacts, not the product's interface.
- **Alternatives considered.**
  - *English first, translation later.* Rejected by the user.
  - *Model translation of interface text at run time.* Rejected: wording of approvals and errors must be reviewed and stable.
  - *ICU MessageFormat or gettext catalogues.* Considered; Fluent is proposed because it has maintained Rust and JavaScript runtimes and handles grammatical cases without code. The final choice is made in `/spec`.

### ADR-043: Dictation uses the operating system's on-device speech service where one exists, and a bundled local model elsewhere

- **Status.** Decided by the user, January 2026 ("voice enable dictation should be implemented using OSX and Windows local services (where possible)"). The mechanism is proposed. *Amended in revision 0.36.0 (ADR-078): there is no bundled local model. Dictation works only where the operating system provides it, macOS 26 and later; on Windows and older macOS the microphone button is hidden and Settings says dictation isn't available.*
- **Decision.**
  - The person can dictate into the conversation and into every text field of a form. Dictation fills the field; the person reviews the text and sends it. Dictated text is never sent on its own, and a spoken "yes" is never an approval (11 — UX guidelines §5.12).
  - Audio never leaves the computer and is never stored. Only the text the person sends enters the conversation and the journal.
  - **macOS 26 and later:** Apple's `SpeechAnalyzer` with `SpeechTranscriber`, or `DictationTranscriber` where the locale needs it. Apple describes transcription as entirely on-device, with models the system installs. These APIs are Swift-only, so the core reaches them through a small Swift bridge.
  - **Windows, and macOS before 26:** a small speech-to-text model bundled with the application and run on the computer (C-24). Windows has no production-supported on-device dictation service today: its dictation grammar and voice typing use Microsoft's online service, and the on-device Windows AI speech API is experimental and not for production. When Microsoft supports that API for production, it replaces the bundled model on Windows.
  - `SFSpeechRecognizer` is not used. Apple documents it as a network-based service with a one-minute limit, and on-device mode only for some languages.
  - The browser engine's Web Speech API is not used, because it is not local in either web view.
  - The dictation language follows the person's locale (ADR-042). A locale no local recogniser supports shows the microphone as unavailable, with the reason in plain words.
  - The operating system's own dictation, such as macOS's Dictation shortcut, is neither blocked nor relied on.
- **Context.** Sources read on January 2026: developer.apple.com/documentation/speech (SpeechAnalyzer, SpeechTranscriber, DictationTranscriber, SFSpeechRecognizer); WWDC25 session 277; learn.microsoft.com/en-us/windows/apps/design/input/speech-recognition; learn.microsoft.com/en-us/windows/ai/apis/speech-recognition; Microsoft's speech privacy statement.
- **Consequences.**
  - The application asks for microphone access, and on macOS for speech recognition, the first time the person uses dictation, with the purpose stated in plain words. The macOS build carries the usage descriptions and the audio-input entitlement.
  - The bundled model adds to disk use. It is downloaded on first use of dictation, from the update feed, not carried in the installer (A-26).
  - The model is not a streaming one, so partial text appears in short phrases split at pauses.
- **Alternatives considered.**
  - *Cloud dictation through the operating system* (Windows voice typing, `SFSpeechRecognizer` with servers). Rejected: audio would leave the computer.
  - *The bundled model everywhere.* Rejected where macOS offers an on-device service, because the user asked for the operating system's own services where possible.

### ADR-044: Each vendor harness signs the person in its own way; ordin attaches to the signed-in harness

- **Status.** Decided by the user, January 2026 ("I have claude code in this machine, start by reusing the existing setup (login via /login command in claude shell) then attaching at exo-harness level (similar approach for copilot) login is performed in the individual vendor harness in its own way"). The mechanism is proposed. **Settles OQ-28. Amends ADR-001 and ADR-038** (bundling and sign-in) **and G6.** *Amended in revision 0.43.0 (ADR-097): in the ordin server's agent profile no person signs in; Claude Code is given an organisation key, and the rule that removes credential variables from a session applies to the desktop and the personal profile only.*
- **Decision.**
  - ordin never signs anyone in to a model vendor and never reads, stores or relays a harness's credentials. Each harness signs the person in through its own flow and keeps its own credentials: Claude Code's `/login` or `claude auth login`, and the Copilot CLI's `/login` or `copilot login`.
  - **First step: attach to the harness already on the computer.** ordin uses the person's installed, unmodified Claude Code. Before starting a session it checks that the harness is at a tested version and signed in: `claude auth status --json` for Claude Code (exit status 0 when signed in, with the sign-in method). For Copilot it uses the SDK's `auth.getStatus` call. On the test Mac on January 2026, Claude Code 2.1.300 reported `loggedIn: true` with a claude.ai Max sign-in.
  - **Not signed in:** ordin starts the harness's own sign-in flow, which opens the vendor's page in the browser, and waits for the status check to pass. The person never types a credential into ordin. Whether `claude auth login` completes when started by ordin, with no terminal, is C-25. Until it is confirmed, the person signs in from the harness itself. *C-25 confirmed for Claude Code in the spike of January 2026 on Claude Code 2.1.302: started with no terminal and no input, it opened the browser, printed a fallback link, and exited 0 with "Login successful" once the person signed in. The Copilot half is open.*
  - **Later step, for business users:** the Setup Assistant installs a missing harness from the vendor's official installer, at a version the organisation profile pins, and then starts its sign-in as above (G10).
  - **Sessions use the person's sign-in.** The Claude adapter does not pass `--bare`, because that mode never reads the sign-in. It removes `ANTHROPIC_API_KEY` and the other credential variables from the session's environment, because in `-p` mode a key in the environment is always used ahead of the sign-in.
  - **The person's own customisations do not enter ordin sessions.** The Claude adapter starts sessions with `--restricted` and `--strict-mcp-config`. `--restricted` ignores the user, project and local settings files while `--settings` still applies, removes the built-in tools that run commands or code and web fetching unless `--tools` names them, confines the file tools to the working folders, and refuses `bypassPermissions`. ordin's own hooks and MCP servers are added per session (ADR-045, ADR-037). *From revision 0.28.0 (C-26): the design first named `--safe-mode`. The spike of January 2026 on Claude Code 2.1.302 found that `--safe-mode` also drops the hook passed through `--settings` and the servers passed through `--mcp-config`, so ordin's hook never ran and no tool call was guarded. Under `--restricted` the hook ran for every call and the person's CLAUDE.md, hooks, MCP servers, installed plugins, skills, auto-memory and extra working folders did not load.* `CLAUDE_CONFIG_DIR` is not used, because the sign-in is stored per configuration folder. Both points are C-26.
  - **Who pays.** Each person's own subscription to each vendor. Organisation-paid routes (an API key behind a gateway, or a cloud provider) are kept as the fallback should Anthropic object to the subscription route (R-31).
  - ordin's product name and brand never include "Claude Code" or its logo. The interface may say in plain text that a session runs on Claude Code (ADR-041).
- **Context.** Anthropic's legal and compliance page (code.claude.com/docs/en/legal-and-compliance, read on January 2026) says Anthropic does not permit third-party developers "to offer Claude.ai login into their own applications, or to route requests through Free, Pro, or Max plan credentials on behalf of their users", and that developers "may not collect, store, or intermediate Claude.ai credentials or session tokens". It also says this does not "prevent an end user from signing in to the unmodified Claude Code binary with their own Claude subscription, including where a platform hosts Claude Code". This design follows the second case: the person signs in to the unmodified binary through Anthropic's flow, and ordin never touches the credentials. The same page says advertised plan limits "assume ordinary, individual usage", which crews of parallel sessions may exceed (R-31).
- **Consequences.**
  - ordin holds no model credential of any vendor; trust boundary 3 is unchanged.
  - The organisation profile names the harnesses in use and their tested versions, not keys.
  - Budgets for subscription sessions are counted in turns and tool calls, because ordin cannot read a subscription's remaining allowance (OQ-3).
  - The GitHub sign-in that `apm` and `gh` use stays ordin's (ADR-014, ADR-023); it is separate from the Copilot CLI's own sign-in.
- **Alternatives considered.**
  - *An organisation-run gateway with Okta sign-in* (OQ-28 options a to c). Kept as the fallback.
  - *Reading the harness's stored token and passing it on.* Rejected: Anthropic forbids intermediating credentials, and it would put a vendor credential in ordin's hands.

### ADR-045: Approval hooks are ordin's own Rust code, shared by every harness

- **Status.** Decided by the user, January 2026 ("Talk directly, implement the hooks for enforcing approvals at the exo-harness level written in rust. this would also mean the hooks are portable to other child harnesses"). The mechanism is proposed. **Settles OQ-29** (the CLI is driven directly, with no TypeScript sidecar). **Extends ADR-005.**
- **Decision.**
  - **One hook, one protocol.** ordin ships one small Rust program, the **ordin hook**, whose name is fixed in `/spec`. A harness runs it before every tool call. It forwards a harness-neutral tool request to the core over an authenticated local channel (a per-session secret in its environment): session, agent instance, tool, arguments, and the tool's origin (built-in, ordin's MCP Front, or a package's MCP server). The Rules Engine decides allow, deny or ask a person. The hook returns the decision in the harness's own format.
  - **Per-harness mapping, in the same program:**
    - *Claude Code:* a `PreToolUse` command hook registered per session through `--settings` (C-26). It reads `tool_name`, `tool_input` and `mcp_server.source`, and answers with `hookSpecificOutput.permissionDecision`. It is registered with `"onFailure": "block"`, so a failed or timed-out hook blocks the call. Sessions run with `--permission-prompts none`, so nothing else can prompt. *Confirmed by C-26 (spike of January 2026 on Claude Code 2.1.302, with `--restricted`): the hook received every Bash, Write and MCP call; a JSON deny and an exit-2 deny each blocked all three in the default, accept-edits, don't-ask, plan and auto modes, even for calls `--allowedTools` approved; a hook that outlived its 5-second timeout blocked with "timed out; blocking because onFailure is \"block\"". An MCP call's payload carries `mcp_server` as `{name, source}`, `source` being `dynamic` for a server from `--mcp-config`.*
    - *Copilot CLI:* a `preToolUse` command hook, which answers with `permissionDecision`. The Copilot CLI documents a Claude-compatible payload as well, so one mapping may serve both (C-27). The Copilot adapter registers it in the agent folder's hook file, or calls the same decision code in process through the Copilot SDK's `onPreToolUse` when the SDK route is used.
    - *Any later harness* with a pre-tool hook gets one more mapping in the same program. A harness without one is run with no built-in tools, so that only ordin's MCP Front can act.
  - **Fail closed.** If the core does not answer, or the request cannot be parsed, the hook denies. Where a harness treats a hook timeout as allow (the Copilot CLI documents this), the hook's own deadline is shorter than the harness's timeout, and it denies at that deadline.
  - **Waiting for a person.** When the decision is *ask*, the hook waits for the person's answer on the approval card up to its deadline. If the person has not decided by then, it denies with the reason "waiting for the person's approval". The core resumes the session with the decision once it is made, so a slow approval never runs a tool unapproved.
  - **Other events** use the same program and protocol: after-tool events feed the journal, and the stop event lets Ordin check a result before the agent's turn ends (ADR-036), on harnesses that offer these events.
  - **Defence in depth.** The MCP Front still decides every call to ordin's services itself (ADR-037). The hook also covers the harness's remaining built-in tools and package-declared MCP servers.
- **Context.** Claude Code and the Copilot CLI both run command hooks before tool calls, with allow, deny and ask decisions in near-identical JSON (code.claude.com/docs/en/hooks; docs.github.com/en/copilot/reference/copilot-cli-reference/cli-hooks-reference; read on January 2026). Claude Code combines hook decisions as deny over ask over allow, so a person's own hooks or rules cannot overturn ordin's deny.
- **Consequences.**
  - The Rules Engine and approval logic exist once, in Rust, for every harness (G7, ADR-005).
  - The hook program is a signed, pinned part of the installer.
  - Each harness's hook payload is tested by a conformance case per tool kind (C-26, C-27).
- **Alternatives considered.**
  - *A TypeScript Agent SDK sidecar with `canUseTool`.* Rejected by the user. It is also not called for tools an allow rule approves.
  - *The permission-prompt tool only.* Rejected: it sees only calls that would prompt, not every call.

### ADR-046: The tree and Ordin's cards open screens in the centre; the conversation keeps its own panel

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", accepting the three recommended options for where chosen items open, how several workspaces' conversations are shown, and what stays inside the conversation). Found in the UI concept prototype review. **Amends ADR-028** (the pinning bullet, and "everything else appears when Ordin shows it") **and ADR-027** (forms and other views appear "inside or beside" the conversation).
- **Decision.**
  - **Looking never writes to the conversation.** Choosing something in the side-rail tree (UX-A14) opens it as a **screen** in the centre of the window. It adds no message and does not move the conversation's scroll.
  - **Screens are tabs.** Each workspace has its own row of screen tabs. Opening something already open brings its tab to the front. Up to six tabs stay open; opening a seventh closes the oldest one that is neither in front nor showing running work. Closing a tab never deletes anything.
  - **The conversation has its own panel**, beside the screens, always visible on a wide window. It has one tab per workspace the person has opened, with a mark for new messages and a count of what waits for the person. Choosing a conversation tab switches the whole window to that workspace, so the screens and the conversation in view always belong to the same workspace. Each conversation and each screen keeps its own scroll position.
  - *From revision 0.24.0 approval cards open as screens too, with a one-line card in the conversation (ADR-080).*
  - **Decisions stay in the conversation; everything else lives on a screen.** Questions, forms, agent proposals, crew plans, documents waiting to be confirmed, offers that wait for an answer, and approval cards appear in the conversation in full. *From revision 0.10.14 adding documents, code repositories and agents is done on a screen, whoever asks (UX-A33).* *Agent proposals and documents waiting to be confirmed therefore leave the conversation. From the same revision forms and crew plans also open as screens, with a waiting card in the conversation and an entry in Needs you (UX-A35); questions with choices and approval cards stay.* Documents, previews, changes, plan and progress, results and memory appear in the conversation as a one-line card with **Open**, and in full on their screen.
  - **Ordin opens a screen at once only when it answers what the person has just said or done.** Results that arrive later open as a new tab marked *new*, without coming to the front.
  - Screens are drawn by the Block Renderer from the same catalogue (ADR-028). The tree and the cards choose which block to show; neither Ordin nor an agent supplies layout. Approval cards remain the Approval Service's alone.
  - The pinned area beside the conversation is removed; screens replace it.
- **Context.** In the prototype the tree added full blocks to the conversation and scrolled to them, so the conversation filled with material the person had only looked at, and their reading position jumped. A list-and-detail layout, with the conversation in a docked panel, lets the person read and talk at once and matches desktop software they already use.
- **Consequences.**
  - The App Shell gains a screen area with tabs and a conversation panel with tabs. Pinning is no longer a Block Renderer responsibility.
  - On narrow windows the conversation becomes a panel that slides over from the side, opened from the header.
  - 11 — UX guidelines §3, §4, §5.6, §5.7, §5.12, §5.15, §10 and §12 change (UX-A15); UX-A9 is superseded.
- **Alternatives considered.**
  - *The conversation as the first of the centre tabs.* Rejected: the person cannot read a screen and talk about it at the same time.
  - *A screen replacing the conversation, with a way back.* Rejected: it hides Ordin and anything waiting in the conversation.
  - *Two conversations side by side.* Not chosen: it needs a very wide window once a screen is open. It can be added later as an option.
  - *Only messages in the conversation.* Rejected: questions and approvals would leave the conversation that asked for them.

### ADR-047: Engineering mode lives inside workspaces; a file in the linked repository goes back only with the engineer's commit

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", accepting option B for where engineering sits and the recommended rule for how a repository file goes back). Found in the UI concept prototype review. **Settles the placement left proposed in ADR-019. Amends ADR-021** (scope: it now also covers repository files that business users see as documents) **and the Send back flow of §5.6.** *Amended in revision 0.36.0 (ADR-050): nothing is enabled per person. The Code group and the engineering screens appear for whoever works in a workspace that links a code repository, so the wording "a person the organisation profile enables for engineering mode" and the consequence that a business user sees nothing of the Code group no longer hold.*
- **Decision.**
  - **Code is part of a workspace.** A repository is something a workspace links to. For a person the organisation profile enables for engineering mode (ADR-019), the workspace's side-rail tree shows a **Code** group holding that repository, with its branch and the number of changes waiting. There is no separate project view.
  - **The engineering views are screens.** The code editor, **Changes waiting** (change review with commit and push) and the terminal open as tabs in the workspace's screen area (ADR-046). They remain fixed views, not generated (ADR-028).
  - **Changes waiting covers documents and code.** Document changes go back through the approval card (§5.5). Code changes are accepted or rejected entry by entry, then committed and pushed by the engineer (ADR-021).
  - **Each code change names its basis.** An agent's change entry carries the document it is based on, or the agent's own memory page.
  - **One way back for each file.** When engineering mode is on and a document lives in the repository the workspace's Code group holds, the document is left out of the approval card and goes back only with the engineer's commit. Otherwise it goes back through the approval card, as before.
  - Agents' check commands keep their approval card; their output returns in the conversation, never in the engineer's terminal (ADR-022).
- **Context.** In the prototype the project was a separate view opened from its own button. It drew a second tree beside the side rail, stacked the editor, change review, terminal and commit down one long page, and belonged to no workspace, so it had no agents, crews, memory or *Needs you*. A business user could also send a repository file back through the approval card while an engineer sent the same file back by commit, with no rule between the two.
- **Consequences.**
  - The App Shell no longer has a Projects view; the Code group and the engineering screens replace it. The Project Service is unchanged.
  - Approval cards for sending back leave out files the linked repository holds when engineering mode is on.
  - A workspace that links a repository is still usable by a business user: the repository's documentation appears under *Documents*, and nothing of the Code group appears.
  - 11 — UX guidelines §4, §5.6, §5.11 and §5.15 change (UX-A18).
- **Alternatives considered.**
  - *Keep engineering mode as a separate view.* Rejected: two trees, one long page, and a project outside any workspace.
  - *Treat the repository only as another place documents live.* Rejected: entry-by-entry review and the engineer's own commit do not fit the document flow.
  - *One approval that publishes the documents and commits the code together.* Rejected: it breaks ADR-021's rule that only the engineer commits and pushes, and joins two decisions that different people may own.

### ADR-048: Markdown documents are first-class originals, published as copies made on this computer by ordin, WordCraft, PdfCraft and DeckCraft

- **Status.** *Amended by ADR-069 (first publish, keeping copies up to date, changes made outside ordin).* Decided by the user, January 2026 ("proceed with recommendations, the word conversion should be done by wordcraft using local MCP interface, the same with PDFs (Pdfcraft) and powerpoint pesentations (deckcraft)"), accepting the recommended options for how Markdown is read and edited, how originals and copies relate, and the format of new documents. **Amends ADR-026** (Markdown originals and their copies; edits made in a copy), **ADR-029** (Markdown documents may be edited by hand in ordin), **ADR-039** (ordin's own conversions use the apps' MCP mode, not their command-line commands) **and ADR-047** (a coder's own edit to a repository document goes in the commit, and copies are updated after the push).
- **Decision.**
  - **Markdown is a document.** ordin shows a Markdown file as a formatted page, wherever it lives: a repository, a folder on this computer, SharePoint or Confluence.
  - **Editing by hand.** The person can edit a Markdown document in ordin with a formatting toolbar; people who code can also edit its text. On saving, every block the person did not change keeps its exact original text. A saved edit is held on this computer until it goes back, like a change by Ordin (ADR-029, ADR-035).
  - **Originals and copies.** A Markdown document can be published as copies elsewhere, called its projections. ordin records, for each original, its copies: where each lives and its format. The copy is never treated as the original.
  - **Making copies.** Copies are made on this computer, from the original as it is at that moment:
    - a Confluence page, by ordin's Confluence Adapter (ADR-026);
    - a Word document, by WordCraft;
    - a PDF, by PdfCraft;
    - slides, by DeckCraft, one slide per heading.
    ordin reaches WordCraft, PdfCraft and DeckCraft through each app's local MCP mode (`<app>-cli mcp`), the route agents already use (ADR-039). No document leaves the computer to be converted.
  - **One approval.** Sending a changed original back updates its copies in the same approval card, which names every copy and the app that remakes it. For a repository original sent back by a coder's commit (ADR-047), the push is followed by one approval card that updates the copies.
  - **Changes made in a copy.** When a copy has a newer version at its origin than the one ordin published, the original shows the change as a suggestion: bring it into the original, or leave it. A copy whose change was left is replaced at its next update, and that update's card says so.
  - **New text documents.** Text documents that Ordin or an agent creates are written as Markdown, and published as copies where their readers are.
  - Documents that are not Markdown are unchanged: a Word file kept in SharePoint stays the original, and is changed by asking Ordin (ADR-029).
- **Context.** Much documentation lives in Markdown and is published to SharePoint and Confluence; some lives only there. Treating Markdown as a file to attach or as the input to a one-off publish leaves business users unable to read or change it, and leaves the published copies drifting from it. ADR-026's rejection of two-way synchronisation still holds: one side is the original.
- **Consequences.**
  - The File Service records copies per original; the Push Service updates copies in the same approval as the original.
  - The Craft Toolkit serves ordin's own conversions through the apps' MCP mode, under the same path checks as agents' Craft tools.
  - Making Word, PDF and slides copies depends on each app's MCP tools converting from Markdown (C-28). Until confirmed, ordin offers only Confluence copies and says so.
  - Converting to Word, PDF or slides can change a document's look (R-35).
  - 11 — UX guidelines §3, §4, §5.6, §5.11 and §5.15 change (UX-A19, UX-A20).
- **Alternatives considered.**
  - *Show Markdown as text beside a preview.* Rejected: business users would work in Markdown symbols.
  - *Open Markdown in another app.* Rejected: ordin's change review and approval would be outside the application.
  - *Keep every copy in step both ways.* Rejected, as in ADR-026: conflicts and lost formatting.
  - *Write new documents in the destination's format.* Rejected: originals would spread across formats and be harder to review.
  - *Convert through the apps' command-line commands.* Rejected by the user: conversion goes through each app's local MCP interface.

### ADR-049: Ordin's conversation can be summarised or started fresh; ordin does both itself, the same on every harness

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", accepting the recommended options for what clearing does, when summarising happens, what is kept before starting fresh, and what happens while a crew runs). The mechanism is proposed.
- **Decision.**
  - **Summarise** (shown as **Summarise**). On the person's request, or when the conversation nears the harness's context limit, Ordin writes a summary of the conversation. The Ordin Host then starts a new Ordin session for the workspace, seeded with that summary, the items still waiting for the person and the agent directory. The person's view keeps the earlier messages, folded.
  - **Start fresh** (shown as **Clear**; the cleared conversation is kept). On the person's confirmation, the Ordin Host starts a new Ordin session for the workspace with no conversation history, seeded with the items still waiting and the agent directory. Before it does, Ordin proposes key points, and the ones the person keeps are written to memory (ADR-030).
  - **Nothing is deleted.** A conversation that was started over is kept, read-only, as an earlier conversation of the workspace. Approvals and other external actions keep their journal entries (ADR-010). *Amended in revision 0.36.0 (11 — UX guidelines §5.12): an earlier conversation says it is "kept to read; Ordin no longer works from it", and offers **Read it** and **Ask Ordin about it**. Ordin reads an earlier conversation only when the person asks.* *Amended in revision 0.57.0 (ADR-108): an earlier conversation can be continued; continuing opens a new conversation that refers to it, so nothing is appended to the earlier one.*
  - **Running crews.** Starting fresh is not offered while a crew Ordin coordinates is running in the workspace (ADR-006, ADR-027). Summarising is, and its seed includes the crew's state.
  - **Harness-independent.** ordin does both through session start and seeding, not through a harness's own commands, so they work the same on Claude Code and on the Copilot CLI (ADR-036, ADR-038). A harness's own automatic compaction stays as a fallback if ordin's summary comes too late.
- **Context.** Each message makes the harness resend the conversation, and sessions run on the person's own plan (ADR-044, R-31). Folding items in the window (UX-A12) saves nothing. A long conversation costs more of the plan and can reach the context limit.
- **Consequences.**
  - The Ordin Host owns a workspace's current session and its earlier conversations; it records each switch in the journal.
  - The seed for a new session must carry every waiting item, so an approval or a question is never lost by summarising or starting fresh.
  - A summary can drop a detail the person later needs; the folded messages and earlier conversations stay readable, and *What Ordin remembers* holds what was kept.
  - The point at which Ordin summarises by itself depends on reading each harness's context use (C-29).
  - 11 — UX guidelines §3 and §5.12 change (UX-A21).
- **Alternatives considered.**
  - *Clear the window only.* Rejected: it saves nothing and duplicates **Close finished items**.
  - *Delete the conversation.* Rejected: it cannot be undone, and the record of decisions must stay.
  - *Use each harness's own compact and clear commands.* Not chosen: their behaviour differs between harnesses, and ordin could not guarantee that waiting items survive.
  - *Hand a running crew to a new session.* Rejected for now: the crew could stall.

### ADR-050: Code appears in a workspace when it links a code repository; there is no personal coding mode

- **Status.** Decided by the user, January 2026 ("coding bow is not needed, if you add a code repo from github/gitlab then it shows in the treeview"). **Amends ADR-019** (engineering mode is no longer a mode enabled per person: it is what a workspace shows once it links a code repository) **and ADR-047** (the Code group appears for whoever works in that workspace).
- **Decision.**
  - A workspace can link a code repository, as it links SharePoint, Confluence or a folder. Linking one gives the workspace a **Code** group in its tree, the engineering screens (code editor, *Changes waiting*, terminal), technical detail in its replies and approval cards, and agents for code in its agent library.
  - Removing the repository from the workspace removes all of these from it and closes their screens. Other workspaces are not affected.
  - Nothing is turned on per person. The organisation profile keeps one control from ADR-019: whether a person may link code repositories at all.
  - ADR-021 is unchanged: agents propose changes, and only the person commits and pushes, with their own GitHub sign-in.
- **Context.** A per-person switch made a person choose a mode before they had code to work on, and showed code features in workspaces that had none. Tying them to the link puts them where the code is.
- **Consequences.**
  - A business user who links a code repository sees code features in that workspace. Workspaces without one are unchanged for everyone.
  - Repositories on GitLab are covered by ADR-051 (OQ-32 settled).
  - 11 — UX guidelines §1, §4, §5.2, §5.6 and §5.11 change (UX-A22).
- **Alternatives considered.**
  - *Keep a personal coding mode, enabled by the profile* (ADR-019). Rejected by the user.

### ADR-051: GitLab is supported alongside GitHub, for documents and code; the application manages `glab` as it manages `gh`

- **Status.** Decided by the user, January 2026 ("yes, support gitlab too"). Settles OQ-32. The mechanism is proposed.
- **Decision.**
  - A workspace can link a folder of documents, or a code repository (ADR-050), on GitLab: gitlab.com or the company's own GitLab, whose address comes from the organisation profile.
  - The person signs in with their own GitLab account the first time a workspace needs it, in the system browser. The token is kept in the OS keychain.
  - The Setup Assistant installs, pins and updates the GitLab CLI (`glab`) and signs it in, as for `gh` (ADR-023).
  - Agents reach `glab` only through a GitLab tool. The Rules Engine classifies each call by the same operation kinds as for GitHub — read, comment, open merge request, push branch, merge, release — with the same approval defaults and the same profile ceiling.
  - Git fetch and push to GitLab use HTTPS with the person's GitLab token. Commit and push stay the person's own (ADR-021).
  - In the interface GitLab is its own origin, with its own generic icon (ADR-041). Business users see the same words as for GitHub; people working with code see "merge request" where GitHub says "pull request".
- **Context.** Some teams keep their code and documentation on GitLab. Leaving it out would leave them without the Code group and without their Markdown documents.
- **Consequences.**
  - A second CLI to pin, sign in and test, and a second set of operation mappings (R-36).
  - The organisation profile gains the GitLab address and whether people may link GitLab repositories.
  - Whether `glab` signs in through the browser with no terminal, for gitlab.com and for a company's own GitLab, must be confirmed (C-30).
  - 11 — UX guidelines §5.13 changes.
- **Alternatives considered.**
  - *GitHub only.* Rejected by the user.
  - *GitLab through its API directly.* Not chosen: a second client to maintain, where `glab` covers merge requests, issues and pipelines and engineers already know it.

### ADR-052: Code is a first-class kind of work in a workspace; ordin coordinates agents on code alongside the engineer's own IDE

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", decision 21), after a review of the design against JetBrains' announcement of Air. The mechanism is proposed. **Amends G10, G13, ADR-019 and ADR-020.**
- **Decision.**
  - Code is a first-class kind of work in a workspace, not an exception mode. G13 and ADR-019 are read that way; technical detail is still shown only in workspaces that link a code repository (ADR-050).
  - For code, ordin owns the outer loop: which agents work on which request, with which documents, under which rules and approvals, recorded in the journal, and checked by Ordin before it is reported done (ADR-036). ordin is not a full IDE.
  - The code editor (ADR-020) is for change review, reading files and editing them with the language server's completion (ADR-056). It has no debugger, no refactoring tools and no run configurations.
  - The Code group, its repository and each file offer **Open in [IDE]**, one action for each IDE found: for example **Open in IntelliJ IDEA** and **Open in VS Code**. ordin detects the IDEs installed on the computer — JetBrains IDEs and Visual Studio Code — at their standard install locations, and starts the chosen one with the repository or file through the command-line launcher the IDE installs (C-32). In Settings, *Code › Open code in* lists the IDEs found; unticking one removes its action. *Revised in revision 0.10.10 by the user's request ("should also be able to open in VS Code, and in settings it should be possible to configure different language servers, the code editor must render each language syntax").*
  - ordin watches the engineer's working copy for changes on disk, so edits made in the IDE are seen. When the engineer and an agent change the same file, change review shows both and never applies the agent's change over the engineer's (§5.11, unchanged).
  - ordin writes no IDE setting and installs no IDE plug-in.
- **Context.**
  - JetBrains IDEs ship Air from release 2026.3 as "a new window for the agents and subscriptions you already use", running multi-agent workflows and reviewing their work with IDE intelligence. Engineers will keep working in their IDE.
  - ordin's strengths lie outside the editor: Ordin as coordinator, crews, fixed-code guardrails, approvals, the journal, memory, the sandbox and the company's documents beside the code. Competing as an IDE would need a much richer editor and would not use those strengths.
- **Consequences.**
  - G10's exception now reads "a workspace that links a code repository", not "engineering mode".
  - IDE detection and start-up must be confirmed per platform (C-32).
  - A file watcher runs per linked repository while its workspace is open.
  - Reaching ordin from inside an IDE is open (OQ-33).
- **Alternatives considered.**
  - *ordin as a full IDE.* Rejected: a much richer editor for less value than the coordination ordin adds.
  - *Only launching an external IDE, with no editor in ordin.* Rejected: change review needs a diff inside ordin, next to the approval cards and the documents each change is based on (ADR-020 stands).
  - *An ordin plug-in inside each IDE.* Deferred to OQ-33.

### ADR-053: Each coding task works in its own git worktree on its own branch; the engineer can ask for several attempts and keep one

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", decision 21). The mechanism is proposed. **Amends ADR-021** (where an agent edits) **and G3.**
- **Decision.**
  - A **coding task** is one agent's work on one request that changes a linked repository, alone or as a crew step.
  - When a coding task starts, the Project Service creates a git worktree for it under the application data root, on a new local branch `ordin/<task name>`, from the engineer's current branch. The task's session gets that worktree as its granted place (read and change). It never gets the engineer's working copy.
  - The task's check commands run in its worktree. A check command the package declares, and its declared rules allow, runs without asking (ADR-021, unchanged); any other command asks the engineer first through the approval card.
  - Each coding task has its own change set. *Changes waiting* groups changes by task, showing the agent, the request, the branch, the test outcome and the new problems the language server found (ADR-054).
  - Accepting a change applies it to the engineer's working copy, uncommitted. Commit and push stay the engineer's (ADR-021). A rejected change is dropped from the task's change set; the engineer's working copy is untouched.
  - **Attempts.** When the person asks for it ("try two ways"), Ordin starts several coding tasks on the same request, each in its own worktree, with different agents or different instructions. The organisation profile caps the number of attempts per request (default 3). *Changes waiting* compares the attempts side by side: request, test outcome, new problems and the changes. **Keep this one** brings that attempt's changes in for review; the others are set aside, and their worktrees and branches are removed. Ordin may recommend one, with its reason.
  - When every change of a task is accepted, rejected or set aside, its worktree and local branch are removed. Each creation and removal is journalled.
  - `ordin/` branches stay local. They reach a remote only through a GitHub or GitLab operation under ADR-023's approvals.
- **Context.** G3 isolates each session in an agent folder, but an agent's code edits went to the one working copy of the repository. Two agents on one repository would overwrite each other, and an agent's half-finished edits sat in the files the engineer was editing in their IDE. Comparing two approaches, a common pattern in agentic development tools, had no place.
- **Consequences.**
  - Disk use grows by one checkout per open task; worktrees share the repository's object store, so history is not copied.
  - Each open task with a language needs its own language-server instance and build cache (R-37).
  - Attempts multiply the tokens a request uses; they count against the crew's budget (R-29, R-31).
  - The engineer's working copy may have moved on when a change is accepted; the change is then applied with a three-way merge, and a conflict shows both versions (R-38).
  - 04 — Domain gains the coding task; the change set belongs to one task.
- **Alternatives considered.**
  - *One shared working copy* (before revision 0.10.9). Rejected: agents collide with each other and with the engineer.
  - *Agents commit to their branch and the engineer merges.* Rejected, as in ADR-021: history the engineer did not write is harder to review than a change set.
  - *A full clone per task.* Rejected: slower and larger than a worktree, for no gain.
  - *A container per task.* Out of scope for the first release; check commands stay outside the sandbox under approval (OQ-26).

### ADR-054: Language intelligence serves agents and change review, through an ordin MCP server

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", decision 21). The mechanism is proposed. **Amends ADR-020, ADR-024 and ADR-037** (one more ordin-served MCP server).
- **Decision.**
  - The Project Service offers a **code intelligence** MCP server to each session working on a coding task (ADR-053). Its tools read: go to definition, find references, list a file's or the workspace's symbols, hover information and diagnostics. They are answered by the language servers of ADR-024, started for the task's worktree, and limited to that worktree.
  - Every call is decided by the Rules Engine inside the server, as for every ordin-served MCP server (ADR-037). The tools only read; none edits a file.
  - **In change review**, each change shows the new problems the language server reports in the task's worktree that it did not report on the base branch, or "No new problems"; each task shows its test outcome.
  - A coding task that ends with new problems or failing tests is a shortfall. Ordin reports it with the task, and never as done (ADR-036).
- **Context.**
  - Language servers fed only the code editor (ADR-020). Agents found their way through code by reading files, and change review gave the engineer no quality signal beside the diff.
  - Air presents IDE intelligence as the way to review agents' work; ordin already relays the Language Server Protocol and can offer the same signal through MCP to every harness.
- **Consequences.**
  - One language-server instance per open coding task, besides the engineer's editor (R-37).
  - "New problems" needs diagnostics for the base branch as well as for the task's worktree; the comparison is fixed in `/spec`.
  - Whether the pinned servers work in a worktree without extra set-up must be confirmed (C-33).
  - 08 — Interfaces: the MCP Front gains the code-intelligence tools.
- **Alternatives considered.**
  - *Agents run language tools through a shell.* Rejected: the harnesses' shell tools are off (ADR-037).
  - *The IDE's own inspections, through an IDE plug-in.* Deferred to OQ-33.
  - *No quality signal in change review.* Rejected: the engineer would have to build and inspect each change themselves.

### ADR-055: The code editor highlights every language it knows; engineers configure language servers in Settings, within the company's list

- **Status.** Decided by the user, January 2026 ("should also be able to open in VS Code, and in settings it should be possible to configure different language servers, the code editor must render each language syntax"). The mechanism is proposed. **Amends ADR-020 and ADR-024.**
- **Decision.**
  - **Highlighting for every language.** The code editor and the diffs in change review colour each file by its language with the editor's own tokenisers, whether or not a language server runs for it: at least Go, Go modules, Rust, Java, TypeScript, JavaScript, Python, shell scripts, YAML, JSON, TOML, SQL, HTML, CSS and Markdown. When a language server runs, its semantic tokens refine the colours. A file of an unknown kind shows as plain text.
  - **The file's language line.** Above each file the editor shows its language and its language server with the server's state ("Go · gopls · Running"), or "highlighted · no language server" with a link to **Language servers…** in Settings.
  - **Language servers in Settings.** Settings has a **Code** section, shown once any workspace links a code repository. *Amended in revision 0.36.0 (11 — UX guidelines §4): the Code section is always shown, whether or not a workspace links code.* For each language it lists the language server, its pinned version, where it comes from (the company's list, or an agent package), and where it is running. The engineer can:
    - choose among the servers the organisation profile approves for that language (for example `typescript-language-server` or `vtsls`);
    - turn a language's server off and on, and restart it;
    - edit the server's settings, as JSON passed to the server as its configuration; invalid JSON is refused and nothing is saved;
    - add a language from the approved list that is not yet in use; its server is installed the first time a file in that language is opened.
  - Only servers the organisation profile approves, or an installed agent package carries as an LSP dependency, are offered (ADR-013, ADR-024). An engineer cannot enter an arbitrary program or address.
  - The settings are the engineer's own, kept on the computer; the profile's pin and approval still apply.
- **Context.** ADR-024 made language servers plug-ins chosen by the profile or packages, with no place for the engineer to choose between approved servers or set their options. Engineers expect to tune their servers, and to read every file of a repository — configuration, scripts, documentation — with highlighting, not only the four languages with a server.
- **Consequences.**
  - The editor bundle includes the tokenisers for the listed languages (C-34).
  - The profile's language list gains, per language, the approved servers and the default one.
  - A server turned off still leaves highlighting; agents lose the code-intelligence tools for that language (ADR-054), and change review shows no "new problems" for it.
  - 04 — Domain: the language plug-in gains the engineer's choice, on or off, and settings.
- **Alternatives considered.**
  - *Highlighting only where a language server runs.* Rejected by the user: "the code editor must render each language syntax".
  - *Free choice of any language server.* Rejected: a language server is a program run on the computer, under the trust policy (ADR-013).
  - *Server settings as a form per server.* Not chosen: each server has its own options; JSON is what engineers already know from their IDEs.

### ADR-056: The engineer edits code in ordin's editor with the language server's completion; the Go server is gopls and the Rust server is rust-analyzer

- **Status.** Decided by the user, January 2026 ("the code editor should also allow editing with autocomplete, add golang and rust language servers"). The mechanism is proposed. **Amends ADR-024** (names the Go and Rust servers) **and ADR-052** (the editor edits, not only reviews).
- **Decision.**
  - **Editing.** Every code file that is not also one of the workspace's documents opens editable in the code editor. A file business users read as a document is edited on its document screen, which also edits the Markdown itself (ADR-048).
  - **Completion.** As the engineer types an identifier, or after `.` or `::`, and on Ctrl+Space, the editor asks the file's language server for completions (`textDocument/completion`, relayed by the core, ADR-020). The list shows each item's name, kind and signature, and the server's name. Arrow keys choose; Enter or Tab inserts; Escape closes. An item with arguments places the caret inside the brackets. With the language's server off, or no server for the language, there is no completion and the editor says so.
  - **Keyboard.** Tab inserts a tab. Escape, then Tab, moves focus out of the editor, so the editor never traps keyboard focus. ⌘S or Ctrl+S saves.
  - **Saving.** **Save** writes the file to the engineer's working copy, through the File Service, as a new version (G11). The file then shows in *Changes waiting* as the engineer's own change and goes in their next commit (ADR-021). **Discard my edits** returns to the saved file. Unsaved edits stay with the file's tab until saved or discarded.
  - **Go and Rust.** The Go language plug-in is **gopls**, and the Rust plug-in is **rust-analyzer**, each pinned by the organisation profile and installed on first use (ADR-024). Both serve the editor, the agents' code-intelligence tools (ADR-054) and change review.
  - The editor still has no debugger, refactoring tools or run configurations (ADR-052).
- **Context.** ADR-052 kept the editor to review and quick edits. Engineers expect a quick edit to come with the same completion as their IDE, and the Go and Rust servers are the ones the user named.
- **Consequences.**
  - The editor needs the Language Server Protocol's completion, signature and resolve requests from the relay, not only diagnostics.
  - An engineer's saved edit to a file with an agent change waiting shows both versions in change review (§5.11, unchanged).
  - gopls needs a Go toolchain and rust-analyzer a Rust toolchain on the computer for full results; each plug-in declares it, and the Setup Assistant installs it or says plainly what is missing (R-20).
- **Alternatives considered.**
  - *Read-only editor, editing only in the engineer's IDE.* Rejected by the user.
  - *Completion from words in the file, without a language server.* Rejected: suggestions would not know types, packages or signatures.

### ADR-057: Each workspace keeps knowledge pages, written by Ordin with their sources, separate from memory and from the search index

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", on the recommendation to keep a wiki per workspace, on the computer only, with agents' built-in reference shown read-only). The mechanism is proposed. **Builds on ADR-030** (memory) **and ADR-048** (Markdown originals); **amends the loop table of 02 — Architecture**, whose "Knowledge and memory lifecycle" row still said the application adds no store of its own. *Amended by decision 56 (January 2026): agents' reference pages are labelled with the agent only, never its version (11 — §5.10).* *Amended in revision 0.51.0 (ADR-103): pages also leave the computer as files in a team folder, after the person approves each share; a shared page that cites a place another person cannot read is withheld from their agents by the same rule.* *Amended in revision 0.52.0 (ADR-104): the knowledge folder may be edited in another Markdown app; pages link to each other, carry other names and tags, and a rename rewrites the links that name the page.* *Amended in revision 0.61.0 (ADR-112, ADR-113): each page file is an Open Knowledge Format 0.2 concept with `type`, `description`, `generated` and, when the person writes, edits or accepts an update, `verified`; trust is derived from who verified a page. Knowledge agents read pages as pages and write none; a document still never becomes a page, and becomes facts at document trust only on a knowledge agent.*
- **Decision.**
  - **Three layers, kept apart.**
    - *Sources:* documents and code where they live, with the working copy and the workspace search index (A-22) as copies for speed. These are not knowledge, and are never turned into pages wholesale.
    - *Knowledge:* pages that explain what things are, how they work and why, each citing its sources.
    - *Memory:* small facts and preferences that fade on purpose (ADR-030).
  - **Pages.**
    - A knowledge page is a Markdown file in the workspace's own folder on the computer, in a `knowledge/` folder (A-28).
    - A short header block records the title, who wrote it (Ordin or the person), when it was last changed, and each source: its origin, location and the version read.
    - The File Service keeps every version of a page (G11). Removing a page moves it to the system's bin, with Undo.
  - **Who writes.**
    - Only Ordin and the person write pages. Crew members and other agents read pages; what they learn reaches a page through Ordin, when their work finishes.
    - Ordin writes or updates pages when a task or crew run finishes, never while it runs, and says so in one line in the conversation, naming the page. The person can also ask ("write down what we know about Acme").
    - The person edits a page on its screen, as any Markdown document.
  - **Freshness.**
    - When a cited source has a newer version than the one the page records, the page says "may be out of date" and names the change: who changed which source, and when.
    - Ordin checks this before it relies on a page, and when the page is opened.
    - **Bring up to date** has Ordin read the changed source again and show the lines of the page that would change. The page changes only when the person chooses **Use the update**. **Leave it** keeps the page and clears the mark.
    - A figure that goes into a change sent back to an origin is taken from its source, not from a page.
  - **How agents reach it.**
    - The MCP Front serves knowledge tools to sessions in the workspace: *search pages* and *read page*. Ordin's session also has *write page* (08 — Interfaces).
    - A page is withheld from an agent that cannot read one of the places the page cites, under the workspace's grants (ADR-025).
  - **With memory and search.**
    - The memory component indexes pages by section, so *recall* can return a fact with the page that explains it. A fact may cite a page; a page never cites a fact alone.
    - The workspace search index also covers pages. Documents search lists matching pages above the documents, marked as Ordin's summaries.
  - **Agents' reference material.** The `knowledge/` folder that an agent's package ships is shown under Knowledge, in a group of its own, read-only, labelled with the agent (decision 56: never its version). It changes when the agent is updated.
  - **Leaving the computer.** Pages stay on the computer. **Publish to Confluence** sends a page as a copy through an approval card the application builds, as any push (ADR-026, ADR-029). The page on the computer stays the original, as Markdown originals do (ADR-048).
  - **Scope.** Pages belong to their workspace. Ordin copies a page to another workspace only when the person asks.
- **Context.**
  - Memory holds separate facts built for quick recall; nothing turned many facts and documents into pages a person or an agent can read, review and build on. So every conversation rebuilt that understanding, at the cost of tokens on the person's plan.
  - This is the pattern sometimes called an LLM wiki: a model keeps a set of linked Markdown pages, built from raw sources and kept current as the sources change.
- **Consequences.**
  - New: a Knowledge Service in the core (02 — Architecture), a knowledge page entity (04 — Domain), the knowledge tools and the page file format (08 — Interfaces), and a Knowledge screen and tree item (11 — UX guidelines §5.10, UX-A28).
  - Keeping pages current uses tokens on the person's plan, which is why Ordin writes only when work finishes (R-39).
  - Pages can be wrong or go stale; sources, freshness checks and the rule on figures that are sent back limit the harm (R-39).
  - Pages are a new place where document content lands. Ordin writes them as its own summaries, treating document content as data, and agents read them as reference only (R-27).
- **Alternatives considered.**
  - *Memory and search only.* Rejected: understanding is rebuilt every conversation and the person cannot read or review it.
  - *Show memory facts grouped as pages.* Rejected: facts make poor explanations and fade by design.
  - *Index every document for retrieval (RAG).* Rejected: a second copy of every source to keep in step; the memory component reads sources when a question needs them.
  - *Keep the wiki in Confluence.* Rejected: work in progress would be visible to others and leave the computer; publishing a page stays possible, with approval.
  - *Let every agent write pages.* Rejected: a document read by one agent could rewrite what every agent relies on.

OQ-31 settled by the user, January 2026, for ADR-044: no written confirmation is sought from Anthropic. In the user's words, opening several Claude Code sessions "all of them contribute to the same token usage … the only thing that matters is that you are effectively paying for all of them". Sessions ordin starts are the person's own sessions on their own plan; R-31 now records only that crews reach the plan's limits sooner.

### ADR-058: Home shows the person's work across all workspaces; opening a workspace shows what happened since the person was last there

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", on the recommendation to make ordin fit heavy use across many workspaces with a Home view, a catch-up on opening a workspace, one search box and Run again). The mechanism is proposed. **Amends ADR-028 and G17**: Home is a fixed part of the shell beside the workspace switcher, the conversation, *Needs you* and Settings. *Amended in revision 0.36.0 (ADR-079): the second half of this decision's title no longer holds. Opening a workspace writes nothing into the conversation; changed documents are marked in the tree, and the rest is on Home. The* Catch-up on return *bullets below are kept as history.*
- **Decision.**
  - **Home.** A fixed view, opened from **Home** at the top of the side rail, under the brand, with a count of new items. Like Settings, it is not a block from the catalogue. The application's own code builds it from the journal (G8), the Knowledge Service and the source adapters' change marks, with no model call, so it costs nothing on the person's plan.
  - **Sections.**
    - *Waiting for you*: the same items as *Needs you*, grouped by workspace. *Needs you* stays in the rail. *From revision 0.10.14 *Needs you* is the fixed first tab of the conversation panel, no longer in the rail (UX-A37).*
    - *Running now*: every task and crew run in every workspace, with its state (11 — UX guidelines §8) and **Show progress**.
    - *Since you last looked*: finished tasks and crew runs; documents changed where they live by someone else; knowledge pages that may be out of date (ADR-057); replies from agents that answer in Slack. Newest first, each with its workspace, its time, **Open** and, for finished work, **Run again…** (ADR-060).
  - **Filter.** **All workspaces**, or one workspace. Items new since the person last opened Home are marked "new".
  - **Home decides nothing.** Every row opens the item in its own workspace, where its card or screen is, switching to that workspace first, as *Needs you* does. Approvals are decided in the workspace that caused them (on their own screen from revision 0.24.0, ADR-080). Home has no button that approves, sends, posts or publishes.
  - *From revision 0.23.0 there is no catch-up note: changed documents are marked in the tree and the rest is on Home (ADR-079).*
  - **Catch-up on return.** When the person opens a workspace in which something happened since they last had it open, the conversation shows an application note, **Since you were last here**. It is not a message from Ordin, so no model call is made.
    - At most five lines, with alike items grouped ("3 documents changed in SharePoint"), each with **Open**, and **Everything on Home**, which opens Home filtered to that workspace.
    - Shown once per return. It folds to one line when the person sends a message or presses **Close**.
    - Not shown when nothing happened, nor for what happened while the person had that workspace open.
    - Asking "what's new?" or "what did I miss?" shows it again.
  - **Marks kept.** The application keeps, per workspace, when the person last had it open (*last seen*), and when they last opened Home. Both stay on the computer.
- **Context.**
  - People who work in many workspaces had no single place that showed what ran, finished or changed across them. *Needs you* lists only what waits for a decision, and each conversation shows only its own workspace's work, so finding out what happened meant opening every workspace, or asking Ordin in each at the cost of a model turn.
  - Everything Home and the catch-up show is already in the journal, the knowledge pages and the adapters' change marks; listing it needs no reasoning.
- **Consequences.**
  - The shell gains a fixed view and a fixed note, served by a new Home and Search component (02 — Architecture).
  - 04 — Domain: a workspace keeps *last seen*, and the person's Home marks are kept.
  - In a busy workspace the catch-up could become noise; five grouped lines that fold on the first message limit it (R-41).
  - 11 — UX guidelines §4 and §5.17 (UX-A29).
- **Alternatives considered.**
  - *Ordin summarises what changed on return.* Rejected: a model turn on the person's plan each time a workspace opens, for a list the journal already holds.
  - *Home as a block Ordin shows.* Rejected: a block lives in one workspace's conversation, and Home spans all of them.
  - *Fold Needs you into Home.* Not chosen: *Needs you* is the quick count of decisions and stays in the rail; Home shows the same items beside the rest. *From revision 0.10.14 *Needs you* is the fixed first tab of the conversation panel, no longer in the rail (UX-A37).*
  - *A system notification for every change.* Rejected: notifications stay limited to approvals, finished work and Ordin's questions (UX-A4).

### ADR-059: One search box covers the person's own workspaces, goes to things and starts actions; agents keep searching only their own workspace

- **Status.** Decided by the user, January 2026 (same decision as ADR-058). The mechanism is proposed. **Builds on ADR-025** (a workspace limits an agent's access) **and A-22** (the workspace search index); **ADR-057's knowledge tools are unchanged.**
- **Decision.**
  - **Where.** **Search or go to…** at the top of the side rail, also opened with Ctrl+K (⌘K on a Mac). It opens a dialog over the window.
  - **Results**, in groups:
    - *Go to*: workspaces, agents, workflows, knowledge pages, earlier conversations, and documents by name. *Amended in revision 0.36.0 (ADR-064): "kept crews" now reads "workflows"; Go to also lists dashboards, each with the time it is as of (ADR-067).*
    - *Found in pages and documents*: text matches in knowledge pages and in each workspace's local search index (A-22).
    - *Actions*: Home, New workspace, Add documents, Summarise, Clear and Settings, named as the conversation's own buttons (§5.12). *Amended in revision 0.36.0 (11 — UX guidelines §5.17): Earlier conversations is also an action.*
    - Each result from another workspace names that workspace; choosing it switches to that workspace first, as *Needs you* does.
  - **For the person only.** The box searches the person's own workspaces for the person to see. Agents keep searching only the workspace they work in, through the MCP Front, unchanged (ADR-025, ADR-057); no MCP tool is added. To use something from another workspace, the person adds it to this one with the existing Add flow (11 — UX guidelines §5.13), at the access granted there.
  - **Decides nothing, sends nothing.** An action opens the same screen, form or card as asking Ordin would (P2). The box reads only what is on the computer — the journal, the knowledge pages and the per-workspace indexes — and makes no model call and no request to an origin.
  - **Keys.** Ctrl+K or ⌘K opens it; the arrow keys move through the results; Enter opens the chosen one; Escape closes it and returns focus to where it was. The dialog's footer names the keys. The box is reachable from the keyboard like any rail control, and works with screen readers as a combobox with a listbox.
  - **Words.** "search everywhere for …" opens it with those words filled in.
- **Context.**
  - With many workspaces, the person often knows a document, page or conversation exists but not in which workspace. The tree shows names only, and documents search covers one workspace.
  - Widening agents' reach in the same way would undo ADR-025: an agent granted one workspace would read another's documents.
- **Consequences.**
  - The core answers the box from the journal, the Knowledge Service and every workspace's File Service index, with no model call, within A-21's 1 s for first results (02 — Architecture, Home and Search; C-36).
  - Results from several workspaces in one list tempt the person to copy content between workspaces; results are shown to the person only, and using one elsewhere goes through the Add flow (R-40).
  - While the code editor has focus, Ctrl+K and ⌘K may already belong to the editor's own key chords; `/spec` fixes which applies there.
  - 11 — UX guidelines §4, §5.17 and §10 (UX-A30).
- **Alternatives considered.**
  - *Search one workspace at a time.* Rejected: the person would have to guess the workspace first.
  - *Let agents search across workspaces.* Rejected: it breaks the workspace as the limit of an agent's access (ADR-025), and lets one workspace's documents steer work in another (R-27).
  - *Ask Ordin to find it.* Kept as a route, not the only one: it costs a model turn for a lookup the index answers in under a second.
  - *A separate quick-command box beside the search box.* Rejected: two boxes in the rail for one habit; one box with groups does both.

### ADR-060: Finished work can be run again in one step; repeating work on a schedule stays off the desktop and is asked of IT

- **Status.** Decided by the user, January 2026 (same decision as ADR-058). The mechanism is proposed. **Keeps ADR-011.** *Amended in revision 0.43.0 (ADR-098): the request to IT asks for a hosted ordin agent with a recurring duty, in place of wrightrun.* *Amended in revision 0.53.0 (ADR-105): where the person has an ordin server that reads NATS events, the request asks IT to have the company's own scheduler start the work by sending an event; ordin keeps no timetable.*
- **Decision.**
  - **Run again.** A finished task or crew run shows **Run again…** on its progress screen and on its Home row. It opens the same task form (11 — UX guidelines §5.3) or crew plan (§5.7), filled with the previous inputs.
  - **Newer inputs.** When an input has a newer version than the one read last time, the form uses the newer one and says so ("Quote — Kestrel.pdf: newer version since last time"). Nothing starts until the person presses the form's button or **Start**.
  - **Words.** Asking "run that again" does the same.
  - **What the journal keeps.** Each finished run's inputs by name, with where each lives and the version read (A-29). Content is not copied; a run again reads each input from its source.
  - **Repeating on a schedule.** **Every week…**, offered beside **Run again…** on a finished run's progress screen, or asking to repeat work on a schedule, has Ordin explain in one line that this computer may be asleep or closed, so repeating work is set up on the company's agent service (wrightrun), and offer **Ask IT to set it up…**. It opens an approval card that sends a request to IT naming the agent, the task, the inputs by name only, and how often. Nothing is scheduled on the computer.
- **Context.**
  - People repeat the same work, such as a weekly supplier summary. Starting it again from the conversation costs a model turn to rebuild the request, and a changed input can go unnoticed.
  - A laptop sleeps and closes, and ADR-011 keeps unattended work off the desktop; wrightrun has a duty timer.
- **Consequences.**
  - 04 — Domain: a run keeps its inputs with the version read.
  - 08 — Interfaces: a repeating-work request to IT.
  - A run again could use a changed input without the person seeing it; naming each newer version before Start prevents that (R-42).
  - 11 — UX guidelines §5.4 and §5.17 (UX-A31).
- **Alternatives considered.**
  - *Run again starts at once.* Rejected: inputs may have changed, and a task can end in a push; the form shows what will be used.
  - *Schedule on the computer while ordin is open.* Rejected: ADR-011; the work would silently not happen while the laptop is closed.
  - *Ordin sets up the schedule in wrightrun itself.* Rejected: ordin is a client of one person and deploys nothing in wrightrun; IT owns deployments there.

### ADR-061: ordin is open source under the Apache License 2.0, published by gatblau

- **Status.** Decided by the user, January 2026 ("license should be apache 2 this will be released open source to gatblau").
- **Decision.**
  - ordin's own code and documents are licensed under the Apache License, Version 2.0. The full text is `LICENSE` at the repository root; `NOTICE` names the copyright holder, gatblau.org.
  - Source files carry no per-file licence header; the root `LICENSE` covers every file (the user's decision, January 2026).
  - Every crate in the repository declares `license = "Apache-2.0"`: `memory`, `sandboxd`, `sandboxd-ipc` and `pysandbox-guest`.
  - Third-party code keeps its own licence. The patched RustPython crates under `sandbox/sandbox-host/guest/patches/` keep their upstream licence and are not relicensed.
  - The installer lists every bundled program and library with its licence at build time, as before; for an open-source release the list also goes in the release notes.
- **Context.**
  - The repository had no licence file. `memory/` and `sandbox/` were copied in without their earlier licence files (changelog, January 2026).
  - Apache 2.0 is permissive and carries an explicit patent grant, which suits a desktop application that companies install and may brand (ADR-041).
- **Consequences.**
  - 06 — Regulatory: a row for ordin's own licence.
  - 10 — Risks: the memory component depends on SurrealDB under the Business Source License 1.1, which is source-available, not open source; an Apache-2.0 release must say so plainly (C-16, R-43).
  - cargo-deny keeps checking third-party licences against its allow list; the workspace's own crates stay excluded from that check because they are not published to crates.io.
- **Alternatives considered.**
  - *MIT.* Rejected: no explicit patent grant.
  - *A copyleft licence (GPL or AGPL).* Rejected: companies that brand and bundle ordin would have to publish their changes, which works against white-label builds.

### ADR-062: ordin spends the person's plan sparingly by design, and Settings offers one choice of how, with details for those who want them

- **Status.** Decided by the user, January 2026 ("ordin should introduce measures to manage token consumption efficiently, from carefully drafting prompts to selecting which models are most appropriate for the specific task … Add token saving options to settings"; and, on Ordin's tools, "because ordin implements an mcp service, the mcp interface could be let known deterministically to the ordin agent avoiding having to carry all mcp bloating in the context"). The user then accepted *Balanced* as the default for everyone, with each company free to change it in the organisation profile ("proceed with recommendations", January 2026). The mechanism and the other setting values are proposed. **Extends ADR-049** (summarising) **and ADR-037** (the MCP Front).
- **Decision.**
  - **What ordin does always, with no setting.**
    - *Ordin's tools are known when ordin is built.* The Ordin tools, the crew tools and the knowledge tools are ordin's own, served by the MCP Front, and Ordin is bundled with the application. Their names, arguments and a one-line purpose each are written once, as a compact tool catalogue, into Ordin's fixed instructions and versioned with the application. The harness therefore does not need every tool's full description in each request: the MCP Front keeps each description to one line, and where a harness defers tool definitions until a tool is used (Claude Code does so for MCP tools by default), they stay deferred. Calls still go through the MCP Front and the Rules Engine. A call with wrong arguments is answered by the core's own check, with no model call, by a short error that names the correct form.
    - *Each session is offered only its own tools.* The MCP Front answers each session's tool list with the tools its role, package and workspace grants allow, and no others. Ordin's session is not offered the Craft tools, because it delegates to agents that use them. A member is offered only the Craft tools its package declares. The code-intelligence tools stay limited to coding tasks (ADR-054). Fewer tools mean fewer tokens in every request and fewer wrong choices.
    - *Bulk work runs as code.* In the sandbox, the tools an agent may call are also offered as a generated module for Python and JavaScript, named in `/spec`, whose functions mirror the MCP tools and go through the same Rules Engine decision. An agent comparing 42 suppliers' rates writes one script. The data it reads stays in the sandbox, and only the result returns to the conversation (ADR-031, ADR-040).
    - *A stable order, so the cache is reused.* Ordin's instructions are assembled in a fixed order: the fixed instructions and the tool catalogue first, then the agent directory, then the workspace's summary, with what changes from turn to turn last. The harness's prompt cache then serves most of each request. A change of model starts a new cache, so a session's model changes only when ordin starts that session again: on **Summarise**, on **Clear**, or when a step goes to another session.
    - *Focused briefs, short results.* Ordin's brief to a member is built from a fixed shape: the purpose, the inputs by reference (name, where it lives, version), the expected result and the step's limits. It never includes the conversation. A member returns a short summary and references to what it made, not its transcript.
    - *Specific requests before heavy work.* When a request would start a crew, or a task over many documents, and does not say what to work on, Ordin asks one question or shows the task's form first (§5.3), rather than starting a broad search.
    - *Knowledge before documents.* Agents read the workspace's knowledge pages and the search index before whole documents (ADR-057).
    - *Application-built views stay free.* Home, the search box, the tree and its changed marks, approval cards and guardrail checks are drawn or decided by the application's own code, with no model call (ADR-058, ADR-059, G4).
  - **What the person chooses, in Settings › Using your plan.**
    - **How Ordin uses your plan:** *Save my plan*, *Balanced* (the default) or *Best results*. It sets the model level for each kind of work, how deeply models think, when long conversations are summarised and when Ordin asks before heavy work:

      | | Save my plan | Balanced | Best results |
      |---|---|---|---|
      | Talking with you (Ordin) | Smaller | Standard | Most capable |
      | Planning and checking crews (Ordin as coordinator) | Standard | Standard | Most capable |
      | Routine steps: reading, sorting, summarising | Smaller | Smaller | Standard |
      | Writing and changing documents | Standard | Standard | Most capable |
      | Code | Standard | Standard | Most capable |
      | Thinking | Low | Automatic | High |
      | Summarise long conversations | Early | When nearly full | When nearly full |
      | Ask before work estimated as | Medium or heavy | Heavy | Heavy |

    - **Answer simple requests without the model** (on). Opening, finding, going to, "what's new" and running the conversation's own buttons by words are done by the application, as the search box does (ADR-059); Ordin is asked only when the words match none of them.
    - **Reuse an answer when nothing has changed** (on). The same question to the same agent in the same workspace, over the same versions of the same inputs, within 7 days, shows the earlier answer, labelled with when it was made, with **Ask again**. Only answers are reused: never an action, a change, a push or an approval.
    - **Give agents only the parts they need** (on). Tools ordin serves (documents, knowledge, search, the sandbox and the Craft tools) return the relevant parts of long documents and long tool output, with a reference the agent can use to read the rest. Turned off, they return the whole.
    - **Summarise long conversations:** *Early* (saves most), *When nearly full* or *Only when I ask*. Summarising is itself one large request, so *Early* pays off only in a conversation that continues.
    - **After a break of more than an hour, continue from a summary** (on). After an hour the harness's cache has expired on a subscription, and the next message re-reads the whole conversation; continuing from a summary (ADR-049) costs less. The person sees one line, "Continuing from a summary of this conversation", with **Use the whole conversation**.
    - **Ask before work estimated as:** *Heavy*, *Medium or heavy* or *Never ask*.
    - **Agents working at the same time:** 1, 2 (the default) or 3, up to the company's maximum.
    - **More choices** (folded): the model level for each kind of work (*Automatic*, *Smaller and quicker*, *Standard*, *Most capable*), which turns the choice above into *Custom*; **Show usage on each run** (on in workspaces with code); and **Pause work Ordin starts after** *No daily limit*, 100, 250 or 500 model turns a day.
  - **Levels, not model names.** A business user sees the four levels. Each harness adapter maps them to that harness's models, from the organisation profile: for Claude, Haiku, Sonnet and Opus; for Copilot, *Automatic* uses Copilot's own automatic model selection, and the other levels name Copilot models, remembering that a model can count as more than one premium request. Model names appear only for people who code and in Diagnostics.
  - **One step up when a cheaper model falls short.** A step on a *Smaller* model whose result fails its check (a form's validation, the coordinator's check of a member's result, or the same tool error twice) is tried once more on the next level up, and the run records it. A result offers **Try again with a more capable model**; that is the person's choice and is never automatic beyond the one step.
  - **An estimate before heavy work.** Before a crew starts, and before a task over more than 20 documents, the plan shows **Estimated use**: *Light*, *Medium* or *Heavy*, with "about N to M model turns" and what it is based on (the charter's limits, the members, the documents' size, and the last runs of the same work in the journal). When the estimate reaches the level the person chose, a task without a plan asks first in one line, with **Start** and **Not now**.
  - **What is counted.** Each run and crew round records its model turns, tool calls, level used and, where the harness reports it, tokens and the share served from cache. Settings shows **Today**: model turns, the share on smaller models, and how many requests the application answered with no model call. ordin cannot read how much of a subscription is left (OQ-3), so it never shows a remaining allowance; a harness's own limit message is shown in plain words (R-29).
  - **The company sets the frame.** The organisation profile sets the default for every one of these settings, the levels allowed for each kind of work, the maximum agents at once, and which settings a person may change. A setting the company fixes shows "Set by your company".
- **Context.**
  - Ordin coordinates on top of a vendor harness, so each crew round adds a coordinator turn to the members' own (ADR-006, ADR-027). On a person's own plan (ADR-044) that is a reason not to adopt it (R-29, R-31).
  - Vendors and research point the same way, as read on January 2026. Claude Code's guidance (code.claude.com/docs/en/costs): match the model to the job and give simple sub-agents a smaller model; lower the effort level for simple tasks; keep tool definitions deferred and unused servers off; preprocess output before the model sees it; delegate verbose work so only a summary returns; clear between unrelated tasks; and note that compaction is itself a large request, that the cache lasts an hour on a subscription, and that agent teams use about 7 times the tokens. GitHub reports that Copilot's automatic model selection routes among models that count as up to one premium request, at a 10% discount (github.blog changelog, 2026-04-17). Routing research reports more than 2 times lower cost at the same quality by sending easy requests to smaller models (RouteLLM, arXiv 2406.18665), and cascades try a cheaper model first (FrugalGPT). Anthropic describes agents calling tools from code so that intermediate data stays out of the context, with one workflow falling from about 150,000 to about 2,000 tokens, and on-demand tool search with about 85% fewer tokens; both figures are second-hand and depend on the workload. Studies of context compression for long-running agents report dropped constraints and invented file paths (arXiv 2608.06503), and task-conditioned pruning of tool output is an active topic (arXiv 2604.04979).
  - ordin controls its own tools, its own instructions and the order it assembles them in, but not the harness's internals; the saving comes from what ordin sends and how often, not from changing the harness.
- **Consequences.**
  - 02 — Architecture: a Usage Planner in the core; the MCP Front gains per-session tool lists, one-line descriptions, trimming and the sandbox module; the Ordin Host assembles Ordin's instructions in the fixed order.
  - 04 — Domain: usage preferences; usage counters per run gain level, tokens and cache share; a reused answer is recorded.
  - 08 — Interfaces: the organisation profile's usage section; the tool catalogue in Ordin's instructions; the sandbox module.
  - 10 — Risks: A-30, C-37, C-38, R-44 to R-46.
  - 11 — UX guidelines §3, §5.7, §5.18 and §12 (UX-A32).
  - Whether ordin is cheaper or dearer than using the harness alone for the same work is measured in the pilot, not assumed (A-30).
- **Alternatives considered.**
  - *A learned router choosing a model for each message.* Not chosen for the first release: it needs training data from ordin's own work. Levels by kind of work, plus one step up on failure, give the larger part of the saving; the journal's counters supply the data to revisit it (A-30).
  - *Compressing the conversation word by word.* Rejected: the studies above report lost constraints and invented paths. Summarising with the original messages kept folded (ADR-049) is used instead.
  - *Reusing answers to similar questions.* Rejected: a similar question can need a different answer. Only an exact match over the same input versions is reused.
  - *Leaving every choice to the person.* Rejected: a business user should make one choice, not eight. One choice with **More choices** folded serves both.
  - *Choosing everything for the person with no setting.* Rejected: people on a tight plan, and engineers, want control, and the company needs to set the frame.
  - *Showing cost in money.* Not possible on a subscription, where ordin sees turns and tokens but not price; levels, bands and turns are used instead.
  - *Sending the full MCP tool descriptions to Ordin each turn.* Rejected: ordin knows them when it is built, and they would cost tokens in every request.

### ADR-063: Ordin always speaks plainly and briefly; the person chooses its tone, register and length in Settings

- **Status.** Decided by the user, January 2026 ("Add tone of voice and register to settings so that when ordin talk to uses always uses plain language, it brief, and use a tone of voice the user can customise"). The mechanism and the option values are proposed. *Amended in revision 0.58.0 (ADR-109): **Words** also change the interface language ("speak French"), and one Ordin tool carries every setting changed in words; notes for Ordin still change only in Settings.*
- **Decision.**
  - **Always, with no setting.** Ordin's fixed instructions carry the plain-language rules, and no setting can turn them off:
    - the answer or the result first, then at most one recommendation and one next step;
    - everyday words, in short sentences, one idea to a sentence;
    - none of the words in 11 — UX guidelines §3's right-hand column (paths, versions, package and tool names, model names, error codes), except for people who code, in a workspace with code (ADR-050);
    - one question at a time, and only when the answer changes what happens;
    - the person's language and spelling (ADR-042);
    - facts, numbers, warnings and what Ordin will and will not do are never softened or left out for the sake of tone or length.
  - **What the person chooses, in Settings › How Ordin talks to you.**
    - **Tone:** *Friendly* (the default), *Neutral*, *Warm* or *Direct*, each shown with one example sentence.
    - **Register**, shown as **Formality**: *Everyday* (the default) or *Formal*.
    - **Length:** *Brief* (the default) or *Shortest*. *Brief* keeps a message to about 80 words; *Shortest* to one or two sentences. Anything longer is folded behind **Show more**. "Tell me more" in words gives the full answer once.
    - **Call me:** no name (the default), or a name the person types; used in greetings only.
    - **Notes for Ordin:** up to 300 characters on how the person likes things explained ("I'm new to procurement; explain a term the first time you use it"). Notes change how Ordin explains, never what it may do.
    - **Preview.** One fixed example message, drawn by the application for the chosen tone, register and length, with no model call.
    - **Words.** "be more formal", "keep it shorter", "be warmer", "call me Sam" change the setting and say so in one line, as the setting would.
  - **What tone never changes.** Approval cards, run-state labels, errors and the statements of who acts and where it goes keep their fixed wording and structure (§5.5, §8, P7; ADR-041 already fixes them for brand packs).
  - **Who it applies to.** Ordin's messages, its summaries and the results it writes. Members of a crew get the same style as a one-line note in their brief (ADR-062), and Ordin reports their results in the person's style. An agent the person talks to directly keeps its own package's voice, with the same one-line note offered to it.
  - **Checked by the application.** The core folds a message longer than the chosen length behind **Show more**, with no model call. Whether Ordin follows tone and length on each harness is checked before release (C-39).
  - **The company sets the frame.** The organisation profile sets the default tone and register, and may add company terms to use or avoid; the brand pack still names the coordinator (ADR-041). A person's own choices apply within that frame.
  - **Fits the cache.** The style settings sit in the stable part of Ordin's instructions (ADR-062). A change applies from the next message and costs one cache rebuild; *Brief* and *Shortest* also cut the tokens each answer uses.
- **Context.** ordin is for people who are not technical (G1, G10). Plain, short answers were a principle already (P7, P9) but nowhere could the person adjust how Ordin sounds, and people differ: some want warmth, some want only the facts, and some work where formal language is expected.
- **Consequences.**
  - 04 — Domain: voice settings.
  - 08 — Interfaces: the profile's voice defaults; the style note in briefs.
  - 10 — Risks: A-31, C-39, R-47.
  - 11 — UX guidelines §3, §5.19, §12 item 23, UX-A34.
- **Alternatives considered.**
  - *A free-text "personality" box only.* Rejected: it invites instructions rather than preferences (R-47), and gives people nothing to choose from. Fixed choices with a short notes field serve both.
  - *Letting tone switch off plain language*, for example a "technical" tone. Rejected: people who code already see technical detail in workspaces with code (ADR-050); the plain-language rules stay for everyone else.
  - *Rewriting every message in the application to fit the tone.* Rejected: it would need a second model call per message. The style is given to Ordin once, and the application only folds length.
  - *Tone per workspace.* Not chosen: one voice is simpler to understand; it can be revisited if pilots ask for it.

### ADR-064: Workflows are first-class: Ordin works out a crew's steps the first time, a saved workflow repeats them, and workflows open as screens, change, duplicate, export and import

- **Status.** Decided by the user, January 2026 ("having crews poses the question of whether ordin should have the ability to define agent workflows that can be reuse within a crew or leave ordin to dynamically coordinate and create workflows or both", then "proceed with recommendations, but make workflows a first citizen so they can be rendered in the UI central pane, updated, duplicated, exported as files and improted into ordin"). The mechanism, the step kinds and the file format are proposed. **Amends ADR-006** (a crew can run saved steps with no coordinator turn per round) **and ADR-027** (a kept crew becomes a workflow; a workflow can be changed on its screen). *Extended by ADR-065, ADR-066 and ADR-074.*
- **Decision.**
  - **Both ways of coordinating.** A crew either follows a **workflow**, a saved list of steps, or is coordinated by Ordin as it goes, round by round, as before. The crew plan says which: "Same steps as last time" or "Ordin decides each step as it goes".
  - **Where a workflow comes from.** Nobody builds one from nothing.
    - **Saved from a run.** When a crew Ordin coordinated finishes, Ordin offers **Save as workflow** with a suggested name. It condenses the rounds that happened into steps; the workflow screen shows them before anything is saved. This replaces **Keep this crew**.
    - **Asked for in words.** "Make a workflow that compares the quotes and then updates the summary" has Ordin draft the steps, shown on the workflow screen as *Not saved yet* with **Save** and **Discard**.
    - **Duplicated** from another workflow, or **imported** from a workflow file (below).
  - **What a workflow holds.** A name; a purpose in one line; a version; the inputs it asks for at the start (label, kind, required or not); the agents it uses, each with its role in one line; the steps in order; the limits; and where it came from (*saved from the run of 16 Oct*, *duplicated from "Supplier rates refresh" version 3*, *imported from a file on 17 Oct*).
  - **Step kinds.** Four, and no others:
    - **An agent step:** one agent does one thing, described in one line, using named inputs or the result of an earlier step.
    - **Steps at the same time:** two or more agent steps that start together, within the limit of agents running at once (07 — Cross-cutting).
    - **Ask me first:** a mark on a step, so the crew pauses and asks the person before that step starts.
    - **Ordin checks the result:** a mark on a step, so Ordin compares that step's result with the purpose before the next step starts.
    - There are no conditions, branches or loops. When the work needs a decision the steps do not cover, that decision is Ordin's, as below.
  - **Running a workflow.** The Crew Orchestrator writes the run's charter from the workflow and runs the steps itself, in order, checking each against the limits, the Rules Engine and the Approval Service exactly as it checks a hand-off Ordin chose. Ordin is called:
    - at the start, to fill in the inputs from the request and the workspace;
    - at each *Ordin checks the result* mark;
    - when a member asks a question Ordin answers (§5.16);
    - when something the steps do not cover happens: a step fails, a check finds a shortfall, an agent in the workflow is missing from the workspace or has a version the workflow was not saved with, or an input is missing;
    - at the end, to compare the result with the person's request and report (ADR-036).
  - **When the steps do not fit.** Ordin takes over for the rest of that run, coordinating as it goes, and says so in one line ("Step 3 failed twice, so I've taken over from there"). The progress screen marks the steps it changed. When the run finishes, Ordin offers **Update the workflow**, showing the steps before and after. A workflow never changes on its own.
  - **A workflow is a screen.** Opening a workflow — from the tree, from **Search or go to…**, from the workspace's agent list or by asking — shows it in the centre (ADR-046): its purpose, inputs, agents, numbered steps with their marks, limits, where it came from, its versions and its last runs. Its actions are **Run…**, **Change**, **Duplicate…**, **Export…**, **Rename…** and **Remove…**.
  - **Changing a workflow.** **Change** turns the screen into an editor for one version:
    - reword a step's line; move a step up or down; remove a step; set or clear *Ask me first* and *Ordin checks the result*; join a step to the one before so they run at the same time, or separate them;
    - lower a limit, never past the company's maximum;
    - rename an input, mark it required or optional, or remove it;
    - **Ask Ordin to add a step…** for anything that needs an agent chosen: adding a step, or giving a step to another agent. Ordin proposes the change on the same screen.
    - **Save** checks the workflow in fixed code — every step's agent is in this workspace, every input or earlier result a step uses exists, at most the company's maximum of agents run at once, every limit within its maximum — and shows any problem on the step concerned. Saving makes a new version. **Cancel** discards the edit.
    - Asking in words ("check prices before the summary") does the same through Ordin, and ends in the same screen with **Save**.
  - **Versions.** Each save is a version with a one-line note of what changed. The screen lists versions; **Restore** makes a new version equal to an earlier one. Each run records the workflow and version it used (A-29), and **Run again…** (ADR-060) runs the same version unless the person chooses the latest.
  - **Duplicating.** **Duplicate…** asks for a name (default "Copy of …") and a workspace (default this one). The copy starts at version 1 and says where it came from. In another workspace, agents missing there are listed on the copy's screen with **Find in the library** (UX-A33); the copy cannot run until they are present.
  - **Exporting.** **Export…** writes the workflow, at its current version, to a file the person chooses through the system's save dialog: `<name>.workflow.yaml`, in the format of 08 — Interfaces. The file names agents by their marketplace package and the range of versions the workflow was saved with. *Amended in revision 0.48.0: from revision 0.25.0 agents are named by agent repository, identifier and version (ADR-082; workflow file format 3, 08 — Interfaces).* It holds no documents, places, links, folder access, memory, knowledge pages, run history, inputs used or the person's name; the export dialog says so in one line.
  - **Importing.** **Import workflow…**, on the tree's Workflows group and in the workspace's actions menu, or dropping a `.workflow.yaml` file on the tree or the conversation, reads a file into the workspace. The core checks it in fixed code before anything is shown:
    - the file is at most 256 KB, matches the format, and its format version is one this release reads; a newer format is refused with "This workflow was made by a newer ordin";
    - each agent is matched to the workspace's agents by package; a missing agent is listed with **Find in the library**;
    - a limit above the company's maximum is lowered to it and said so ("The file asked for 40 rounds; your company's maximum is 30");
    - a name already used in the workspace gets " (2)".
    - The imported workflow opens on its screen as *Imported — not saved yet*, every step's line shown in full, with **Save to this workspace** and **Discard**. It is never run before it is saved, and its first run shows its crew plan, as every run does.
    - A file grants nothing: no folder access, no agent, no approval and no raised limit. Its step lines are data given to agents inside each agent's own declared rules; they cannot change what the Rules Engine allows or remove an approval the application requires for a push, a post or a publish.
  - **Removing.** **Remove…** opens an approval card that says the workflow and its versions go, its agents stay, and its past runs stay in the journal. *Amended in revision 0.49.0 (decision 80):* **Remove…** asks for a confirmation on the workflow's screen; it is not an approval card, because removing changes nothing outside ordin and the Ordin Host does not reach the Approval Service (ADR-100).
- **Context.**
  - ADR-027 rejected "Ordin with no saved crews" because "the same work could not be repeated the same way". A kept crew as written saved its members, roles, hand-offs and limits, but not its steps, so each run was planned afresh, cost a coordinator turn every round (ADR-006, R-29, R-31) and could take a different route on the same inputs.
  - Agent frameworks separate the two: fixed workflows where the path is known, and an orchestrating agent where it is not. Mature ones offer both (from what is known of CrewAI's processes and Flows, LangGraph's graphs, AutoGen-style orchestrators and Anthropic's published advice to prefer fixed workflows where they fit; not re-read for this decision).
  - The user asked for workflows to be first-class: shown in the centre pane, changed, duplicated, exported as files and imported.
- **Consequences.**
  - A run of a saved workflow costs about two Ordin turns, plus one per check mark, member question or step that does not fit, instead of one per round; the estimate before Start (ADR-062) is counted from the steps.
  - 02 — Architecture: the Crew Orchestrator runs saved steps; the Ordin Host owns workflows, their versions, export and import.
  - 03 — Sequences: *Run a saved workflow* and *Import a workflow*.
  - 04 — Domain: **Workflow** and **Workflow step**; a crew charter may carry steps; a crew run records its workflow and version.
  - 08 — Interfaces: the workflow file.
  - 10 — Risks: A-32, A-33, R-48 to R-50; OQ-8 narrowed.
  - 11 — UX guidelines §3, §4, §5.7, §5.20 and UX-A38; the tree's *Kept crews* becomes *Workflows*.
- **Alternatives considered.**
  - *Ordin always coordinates as it goes.* Rejected: repeated work would not repeat, and every round costs a coordinator turn.
  - *Workflows are only defined in advance.* Rejected: someone would have to author each one, which brings back the crew builder ADR-027 removed and asks business users for technical choices (G10).
  - *A drawing canvas of boxes and arrows.* Rejected: it invites branches and loops the steps deliberately leave to Ordin, and is hard to use with a keyboard or a screen reader (§10). A numbered list carries the four step kinds.
  - *A blank "New workflow" form.* Not chosen: a workflow saved from work that ran, drafted by Ordin, duplicated or imported starts from something that worked or was checked; a blank form is the crew builder again.
  - *Signed workflow files, shared by IT through the marketplace.* Deferred (OQ-8): files can be exported and imported now; company-wide sharing needs a signing and publishing route.
  - *Import runs a workflow at once.* Rejected: the file comes from outside the workspace; it is shown in full and saved first.

### ADR-065: Workflows are fully capable: control blocks for branches, loops and running other workflows, in the same outline, with a diagram to read

- **Status.** Decided by the user, January 2026 ("why the workflows are only linear? no branches or loops? there should be fully capable", then "proceed with recommendations": the outline with control blocks and a read-only diagram; conditions in words decided by Ordin; workflows that run other workflows, up to 3 levels). The block kinds, maxima and condition kinds are proposed. **Amends ADR-064** (which had four step kinds and no conditions, branches or loops). *Extended by ADR-085.*
- **Decision.**
  - **A workflow is an outline of steps and blocks.** Agent steps and their marks stay as in ADR-064 (*Asks you first*, *Ordin checks the result*, *at the same time as the step before*). Control blocks hold steps, and can hold other blocks:
    - **If / otherwise**: two to four named paths; one is taken. Paths that are empty mean "carry on".
    - **Ask me**: an If decided by the person's answer; the path names are the choices.
    - **For each**: goes through a list (a list input, or a step's list result), repeating its steps for each item, at most the company's limit of agents at once; at most 50 items, or fewer as the workflow says.
    - **Repeat until**: repeats its steps until its condition holds; a maximum is required, at most 10.
    - **Run another workflow**: runs a workflow saved in the same workspace, with its own steps.
  - **If it fails.** Each agent step says what happens when it fails twice: *Ordin takes over* (the default, as in ADR-064), *ask me*, or *skip it and carry on*.
  - **Who decides a condition.**
    - **The application, in fixed code**, for what it can know for certain: a step's list has any items; a step failed; Ordin's check of a step found a shortfall; a document changed since the last run. The person's answer to **Ask me** and to a person-decided **Repeat until** is also read by the application.
    - **Ordin, from words**, for anything else ("any notice period is longer than 90 days"). Each decision costs one Ordin turn and is recorded with its reason.
    - The progress screen names each path taken, why, and who decided ("Took *Yes*: Kestrel's notice period is 120 days — one Ordin turn").
  - **Numbering.** Steps inside a block are numbered under it (2.1, 2.1.1), across its paths; steps at the same time keep letters (3a, 3b).
  - **Ending is guaranteed.** Every **For each** and **Repeat until** has a maximum; a **Repeat until** that reaches its maximum asks the person whether to carry on with the last result or stop. The crew's limits on rounds, time and repeated failures still apply to the whole run, counting every step a block repeats.
  - **The check before saving and on import** (fixed code), in addition to ADR-064's: every If has at least two named paths and a step on one of them; every condition has words or an application kind, and an application condition looks at an earlier step; every loop has a list and a maximum within the company's; blocks nest at most 4 deep; a workflow has at most 50 steps and blocks; a step uses only results that exist on its path (not a result made only on another path of an If, or only on one path when it comes after the If); **Run another workflow** names a workflow in this workspace, at most 3 levels deep, and never one that would run the first again ("This would run *Contract review* again inside itself").
  - **Changing.** The editor of ADR-064 gains **Add…** at the end of every list and every path: *An agent step — ask Ordin*, *If… / otherwise*, *Ask me…*, *For each…*, *Repeat until…*, *Run another workflow…*. A block's fields are changed in place: who decides, the words, the list, the maximum, the workflow, the path names, **Add a choice**. **Remove block** removes the steps inside it, and the change is listed before saving.
  - **Show as diagram.** The workflow screen offers **Show as diagram**: a flowchart drawn on the computer from the outline — boxes for steps, a diamond for each If with its paths side by side, a dashed frame with a return arrow for each loop, a heavy box for another workflow. It is for reading; the outline is the one place it is changed, so editing stays usable with a keyboard and a screen reader (§10).
  - **The workflow file** moves to format 2, which carries blocks, conditions, paths, maxima and *if it fails*; format 1 files are still read. A file whose loop has no maximum, or whose blocks fail the check, opens with the problems shown and cannot be saved until they are fixed.
- **Context.** ADR-064 kept steps linear so that business users could read and change them, branching was left to Ordin, and agents return prose that fixed code cannot test. The user asked for fully capable workflows. Real processes need them: review until approved, the same check for every contract, a different path when prices rose.
- **Consequences.**
  - Conditions in words cost Ordin turns; the estimate before Start (ADR-062) counts one per decision and per loop round that has one, at its maximum.
  - 02 — the Crew Orchestrator walks the outline; 04 — Workflow step gains kinds and fields; 07 — the extra checks; 08 — file format 2; 10 — A-32 revised, R-51; 11 — §5.20 and UX-A39.
- **Alternatives considered.**
  - *An editable canvas of boxes and arrows.* Rejected: hard for people who are not technical, poor with keyboards and screen readers, and a tangled drawing is hard to review before saving. The diagram is offered for reading.
  - *Conditions only in fixed code.* Rejected: agents return prose, so few conditions could be tested; Ordin decides the rest and says why.
  - *Unbounded loops, ended by Ordin.* Rejected: a run must end by fixed code, not by a model's judgement.
  - *Go-to steps.* Rejected: jumps make the outline unreadable; If, loops and *if it fails* cover the same needs.

### ADR-066: Workflows are shared through GitHub and GitLab repositories; the company can publish its own

- **Status.** Decided by the user, January 2026 ("workflows can be shared on github/gitlab"). The mechanism is proposed. **Settles OQ-8** (crews are not shared as marketplace packages) and **amends ADR-064** (sharing beyond a file the person exports). *Extended by ADR-084 and, in revision 0.51.0, by ADR-103: the same repository folders, sign-in and approval path carry a team's knowledge.*
- **Decision.**
  - **Where shared workflows live.** As `.workflow.yaml` files (format of ADR-065) in a folder of a GitHub or GitLab repository: a repository folder linked to the workspace (ADR-029, ADR-051), or the company's workflow repository named in the organisation profile (*Company workflows*, read-only to most people, set by IT).
  - **Finding them.** **Workflows in linked repositories…**, on the tree's Workflows group, lists the workflow files in the workspace's linked repository folders and in the company's workflow repository, each with where it lives and who changed it last. **Add…** reads the file and checks it exactly as an import does (ADR-064, ADR-065); it opens *Imported — not saved yet* and is never run before it is saved.
  - **Sharing one.** **Share…** on a workflow's screen chooses a repository folder linked to the workspace, then opens an approval card naming the repository, the folder, the file and what it holds. Approving sends it through the Push Service: a commit and push, or a pull request (GitHub) or merge request (GitLab) where the folder's branch needs a review, using the person's own sign-in through `gh` or `glab` (ADR-023, ADR-051). For an engineer, the GitHub operation settings of ADR-023 apply.
  - **A shared workflow in a workspace** says so on its screen and in the tree: "Shared in GitHub — acme/supplier-portal › workflows, version from 12 Oct by Priya Shah".
    - **Newer versions.** When the file in the repository is newer than the version in the workspace, the screen says who changed it and what, in one line, with **See what changed** (the outline before and after) and **Use the newer version**, which saves it as a new local version. Nothing changes until the person chooses; a run in progress keeps the version it started with, and a crew plan says when a newer version exists.
    - **Changing it here.** Saving a change to a shared workflow marks it *Changed here — not yet sent back*, as a changed document is, with **Send back…** (an approval card, as above) and **Keep it as my own** (stops following the repository; the shared file is unchanged). When both the repository and the workspace changed, both are shown and the person chooses; nothing is merged silently.
  - **What a repository can and cannot do.** A file from a repository grants nothing: no folder access, no agent, no approval, no raised limit (ADR-064). Who may share is decided by the repository's own access and branch protection; ordin adds no permission of its own. Each person's ordin checks each version before it is used.
- **Context.** People and teams need the same workflow; the company wants to give everyone tested ones. Workspaces already link GitHub and GitLab folders, people already sign in to both, and repositories already keep versions, reviews and access control. OQ-8 had asked whether crews should be shared through the marketplace as packages.
- **Consequences.**
  - No new service: sharing reuses the Push Service, the Project Service's `gh` and `glab`, and the organisation profile.
  - Workflow files are in plain text in repositories; reviews of a shared workflow happen where the company reviews other changes.
  - 02 — Ordin Host sharing; 03 — sequence *Share a workflow in a repository*; 04 — Workflow gains where it is shared; 08 — the company workflow repository in the organisation profile, workflow files in repository folders; 10 — R-52, OQ-8 settled; 11 — §5.20 and UX-A39.
- **Alternatives considered.**
  - *Marketplace packages for workflows (OQ-8).* Rejected: packages are for agents and go through IT's trust policy; a workflow is a short text file that teams change often, and repositories already review and version such files.
  - *A sharing service of ordin's own.* Rejected: a new cloud service to run, secure and pay for, where GitHub and GitLab already do it.
  - *Follow the repository automatically.* Rejected: a colleague's change would alter what runs without the person seeing it (R-52).

### ADR-067: Reports and dashboards are data blocks in the fixed catalogue, filled through ordin's MCP server

- **Status.** Decided by the user, January 2026 ("I would like to have a native report / dashboarding capability for agents to display data probably supported by the MCP interface Ordin would use", then "proceed with recommendations"). The mechanism is proposed; block schemas, limits and the tool's name are fixed in `/spec`. **Extends ADR-028** (four new kinds in the catalogue) **and ADR-037** (one new tool on the MCP Front).
- **Decision.**
  - **Four new kinds in the catalogue.**
    - **Table**: named, typed columns (text, number, money with its currency, percentage, date, document link) and rows. Sortable by any column, filtered by a search box, drawn in pages, with only the visible rows rendered.
    - **Chart**: one of bar (vertical or horizontal, grouped or stacked), line, area, scatter, or pie with at most 6 slices. It is described by its kind, its data rows, which column goes on each axis, the series and the axis titles. Every chart has **Show as table**, which shows the same rows as a Table.
    - **Key figures**: one to eight figures, each with a label, a value, a unit, and optionally the change against a named earlier value ("Spend €412k — up 8% on Q2").
    - **Report**: a screen with a title, an "as of" date and time, and one to twelve parts, each a Table, Chart or Key figures with a heading of its own, in the order given.
  - **Every part names its source.** A part carries where its numbers came from: one or more documents with the version read, a company system reached through the sandbox's broker (ADR-040) with the time of the read, or an earlier result. When an agent computed the numbers, the source line says so ("Worked out by Supplier analyst from Q3 rates.xlsx"). A part with no source fails the check.
  - **Data only.** A block carries values, never markup, script, style, formulas or expressions. Text is shown as text. A link may only point to a document or place in the workspace. Colours come from the application's theme and brand pack (ADR-041), never from the data. The Block Renderer checks every block against its schema and the limits of A-34 in fixed code; a block that fails is dropped, and the sender is told why (ADR-028).
  - **How data reaches the screen: MCP.** The MCP Front serves a **report** tool (name fixed in `/spec`) to every agent session in a workspace and to Ordin. An agent calls it with a Report, or with one Table, Chart or Key figures, as the result of its step. The Rules Engine decides the call like any other (ADR-037); the core keeps the report as a result of the run, in the journal and the workspace (04 — Domain, *Report*), and shows it on a screen with a one-line card in the conversation (ADR-046). Ordin's *show* tool accepts the same kinds. Because the route is a plain MCP tool, it is the same on every harness (ADR-036).
  - **Dashboards are saved reports.** **Keep as dashboard** on a report's screen saves it in the workspace under a name, with the task or workflow and the inputs that produced it. A dashboard opens from the tree's **Dashboards** group, Home and search. **Refresh** is **Run again** with the same inputs (ADR-060). It runs only when the person asks and shows "as of" the new time. A dashboard never refreshes on its own. Asking for a regular refresh goes to IT (ADR-011, ADR-060).
  - **Sending a report elsewhere is a document.** **Save as…** writes a spreadsheet (one sheet per Table, through GridCraft), a PDF or slides (through PdfCraft or DeckCraft) or a Markdown document with tables, as a new file in the workspace (ADR-048). Sending it anywhere goes through an approval card, as for any document (ADR-029).
  - **MCP Apps are not shown in this release.** A tool result from a package's MCP server that points to an MCP Apps interface (`ui://` resources, the `io.modelcontextprotocol/ui` extension) is not drawn. The start-up report names it: "Shows its own screens, which ordin does not display" (G9). Whether IT-approved packages may show them later is OQ-34.
  - **The chart drawing** is done by the application's own code, with an open-source charting library under a licence compatible with Apache-2.0 (ADR-061), chosen in `/spec` after C-40.
- **Context.** Agents produce numbers — rates compared, spend by supplier, tickets by week — that people need to see at a glance, not as a paragraph or a file to open. The catalogue had no table or chart. The Model Context Protocol's official MCP Apps extension (stable since 2026-01-26) lets a server send its own HTML page, drawn in a sealed frame; Google's A2UI (v0.9, 1.0 not final) describes interfaces as JSON from a catalogue of known parts. Read on January 2026: thoughtworks.com/radar/platforms/mcp-apps, a2ui.org/introduction/what-is-a2ui, a2ui.org/concepts/catalogs.
- **Consequences.**
  - A new public contract towards AgentWright: agent packages can report data with one tool, on both harnesses (11 — UX guidelines §11.2).
  - Reports look the same every time, and no report can carry a button or a control (ADR-028).
  - A need beyond the six chart kinds cannot be drawn; the agent sends a Table instead, or the catalogue grows.
  - 02 — Block Renderer, MCP Front; 04 — Report entity, data lifecycle; 08 — Block catalogue, MCP Front; 10 — A-34, R-53, OQ-34, C-40; 11 — §5.15, new §5.21, §11.2, checklist item 26, UX-A41.
- **Alternatives considered.**
  - *MCP Apps (an agent's own HTML page in a sealed frame).* Rejected for this release: it is the "Ordin writes its own screens" option ADR-028 rejected — each report would look different and could draw controls that look like ordin's. Package MCP servers also connect to the harness directly, not through ordin (ADR-037), so ordin would have to sit in the middle of that traffic. Kept as OQ-34.
  - *A2UI as the catalogue.* Not chosen: still a draft (v0.9), and its basic parts lay out general screens (rows, columns, cards) rather than business data. Its idea — the agent names known parts and the client draws them — is the one ADR-028 already follows.
  - *Vega-Lite chart descriptions from agents.* Rejected: the format carries a formula language that runs inside the application, which reopens the injection risk the catalogue closes.
  - *Reports only as files (spreadsheets, slides).* Not chosen as the only route: a file must be opened in another app and cannot be glanced at beside the conversation. Kept as **Save as…**.

### ADR-068: Progress shows a short list of plain steps over what was seen to happen

- **Status.** Decided by the user, January 2026 ("a native ability to display progress in a friendly way for non-technical users", then "proceed with recommendations"). The mechanism is proposed. **Amends P4** of 11 — UX guidelines and §5.4 (a count of steps from a stated plan is allowed) **and extends ADR-037** (one new tool on the MCP Front; progress notifications relayed).
- **Decision.**
  - **Three layers on every progress screen**, top to bottom:
    1. **Now**: one plain sentence — what is happening and which step it is ("Comparing the Q3 rates — step 2 of 4"). When the person is needed, it says so first ("Waiting for you: approve sending the summary") and **Review** takes them to the item. Beside it: how long the work has run, and, for remote agents, when they were last checked (§5.4).
    2. **Steps**: three to seven steps in plain words, each marked *done*, *now*, *next* or *skipped*, with a symbol and a word, never colour alone (§10).
    3. **What happened**: the observable events of today's §5.4, folded under each step, opened with **Show what happened**. Counts from tools appear here ("Read 12 of 40 quotes").
  - **Where the steps come from**, first match wins:
    1. A saved workflow's own steps and blocks (ADR-064, ADR-065).
    2. A crew Ordin coordinates as it goes: the steps of the crew plan the person started (§5.7). A round Ordin adds outside them appears as a step marked *added by Ordin*.
    3. A single agent task: the steps the agent states through the MCP Front's **plan** tool (name fixed in `/spec`): three to seven steps of at most 80 characters, stated before its first other tool call, and each marked *now* or *done* through the same tool.
    4. The harness's own to-do list, where the runtime adapter can read it from the session's event stream (C-41), turned into steps by the same rules.
    5. Nothing: the screen shows Now and What happened only, as before this decision.
  - **A changed plan is shown, never rewritten silently.** When the steps change during a run, the screen says so in one line ("Plan changed: one step added") and the count follows the new plan.
  - **Honest marks.** A step an agent marks *done* is shown done, with the events seen during it underneath. A step marked *done* with no tool call, file read or result recorded during it reads **Done — the agent reports no activity for this step**, so the person sees what is claimed and what was seen (R-54).
  - **Counts only from real totals.** MCP progress notifications from ordin's own servers — the sandbox and the ArtCraft apps through the MCP Front — appear as "12 of 40" when the notification carries a total and as "12 so far" when it does not (MCP specification 2025-11-25, *Progress*). Progress from a package's own MCP servers reaches the harness, not ordin, and is not shown. The experimental MCP Tasks feature is not used.
  - **Still no percentage bar and no time remaining** (P4). "Step 2 of 4" and "12 of 40" are allowed because their totals are known.
  - **The same Now line everywhere.** Home's *Running now* row, the one-line card in the conversation, the screen tab's name for screen readers and notifications use the Now sentence and the step count, so the person reads the same words wherever they look (ADR-058).
  - Run states and their labels (§8) are unchanged.
- **Context.** A list of observable events is accurate but does not tell a non-technical person how far along the work is or what comes next; the prototype's progress lists rounds and events only. Saved workflows already give named steps (ADR-064). For other work the steps must come from Ordin's crew plan or from the agent itself, and a plan an agent states is a claim, not an observation, which P4 did not allow before this decision.
- **Consequences.**
  - Agent packages gain one convention: state the plan, then mark steps (11 — §11.2). A package that does not still shows progress, with events only.
  - The journal records each stated plan, each change to it and each mark, with the events seen during each step.
  - 02 — MCP Front, Crew Orchestrator, Journal; 04 — Run step; 08 — MCP Front; 10 — A-35, R-54, C-41; 11 — P4, §5.4, §11.2, checklist item 27, UX-A42.
- **Alternatives considered.**
  - *Events only (P4 as written).* Not chosen: accurate, but the person cannot see how far along the work is.
  - *A percentage bar estimated by a model.* Rejected: an invented number (P4).
  - *MCP Tasks for long work.* Not chosen: experimental in the 2025-11-25 specification, and it reports a tool call's status rather than a task's steps.
  - *Steps written by Ordin for every task.* Not chosen: one more Ordin turn per task (ADR-062), and Ordin does not know the agent's own way of working as well as the agent does.

### ADR-069: A Markdown original in git is the source of its published copies, which follow it once the person says "keep copies up to date"

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", on the question "the case where a markdown document in git is projected as confluence page or word document/pdf file in sharepoint should be first citizen, has this been done?"). The mechanism is proposed. **Amends ADR-048** (first publish of a copy, keeping copies up to date, changes made to the original outside ordin, copies that cannot be brought back) **and ADR-026** (a Confluence page made from a Markdown original is a published copy under this decision; the published item of a linked folder becomes one kind of published copy).
- **Decision.**
  - **One record per copy.** Every copy of an original is a **published copy** (04 — Domain): a Confluence page or attachment, or a Word, PDF or slides file in a SharePoint library. It records the original, the destination, the format, the app that makes it, the version of the original it was made from and the version it has at the destination. A file published through a Confluence linked folder (ADR-026) is a published copy of that file.
  - **First publish.** **Publish a copy…** on a Markdown document, or Ordin on the person's request, opens one approval card the Approval Service builds. The card names:
    - the destination: a Confluence space and parent page, or a SharePoint site, library and folder;
    - who can see it there, as the destination reports it (C-42);
    - the format and the app that makes it (Confluence page by ordin, Word by WordCraft, PDF by PdfCraft, slides by DeckCraft);
    - for an original in a repository, the branch the copy follows: the repository's default branch unless the person picks another;
    - the choice **Keep this copy up to date when [original] changes**, ticked by default, with its consequence in one sentence;
    - when a file of that name already exists at the destination, that it would be replaced, with **Use another name** beside it.
  - **Keeping copies up to date.** For a copy whose *keep up to date* is on, a new version of the original that has **reached its origin** remakes the copy and pushes it without another approval:
    - for an original in a repository, a commit on the followed branch, whoever made it and wherever it was made: pushed by the engineer from ordin (ADR-047), merged on GitHub or GitLab, or pushed from another computer;
    - for an original in a local folder, a new version saved there by the person, by hand in ordin or in another app;
    - for an original in SharePoint or Confluence, a new version there.
  - **The audience is checked again.** Before each automatic update, the application reads who can see the destination (C-42). If more people can see it than the approval card stated, that update is held and the person approves again on a card that names the change in audience.
  - **A coder's push.** A push to the followed branch updates the copies kept up to date, as any commit there; an approval card offers to update the others. A push to another branch updates no copy, and the conversation says the copies follow when the change is merged. This replaces, for copies kept up to date, the approval card ADR-048 shows after a coder's push.
  - **Changes still on this computer never update a copy by themselves.** A change by Ordin or an agent, and the person's own edit not yet sent back, wait for the approval card of ADR-029 and ADR-048 as before. That card updates the copies with the original.
  - **Noticing changes made elsewhere.** The Project Service fetches the followed branch of each repository holding an original with copies when its workspace opens, when ordin starts, and every 15 minutes while ordin runs (A-36). The File Service watches local folders. The SharePoint and Confluence adapters compare versions when a workspace opens (A-25). A change on another branch never updates a copy.
  - **The person is told.** Every automatic update is an external action in the journal with its unique key. It appears on Home ("3 copies of rates.md updated from GitHub") and on the original's **Published as** list with the time and the commit or version it came from.
  - **A copy changed at its destination is held.** When a copy has a newer version at its destination than the one ordin pushed, automatic updates of that copy stop until the person chooses, as in ADR-026:
    - a Confluence page or Word copy shows the change as a suggestion for the original: **Bring it in** or **Leave it** (ADR-048);
    - a PDF or slides copy cannot be read back into Markdown. The original shows "[person] replaced the PDF copy in SharePoint. ordin cannot bring changes back from a PDF." with **Open** and **Replace it at the next update**.
  - **Each copy names its original.** The application adds one line at the top of every copy (on slides, a footer on the first slide): "Published copy of rates.md, from [repository or place], branch [name]. Last updated [date, time]. Make changes in the original." Readers see where to change it and how fresh it is, even when no installation of ordin is running.
  - **Turning it off.** **Stop keeping up to date** on a copy turns automatic updates off; later changes then wait for the person's **Update copies**. **Remove copy** asks before deleting the copy at its destination, and never deletes it without that check.
  - **Before the apps are confirmed.** Until C-28 confirms WordCraft's, PdfCraft's and DeckCraft's conversions from Markdown, **Publish a copy…** offers only a Confluence page and says the other formats are not ready yet (ADR-048).
- **Context.** Most documentation lives in Markdown in git and is read in Confluence and SharePoint. ADR-048 made the Markdown file the original but covered only changes sent back through ordin. A pull request merged on GitHub left every copy out of date, and nothing in the design noticed. A copy that falls behind its original is worse than no copy, because readers trust it. Merged changes have already passed the repository's own review, so a second approval in ordin adds a click without adding a check. Changes still on this computer have passed no review, so they keep their approval.
- **Consequences.**
  - 04 — Domain: the entity *Published copy* replaces *Published item*; data lifecycle row updated.
  - The Project Service fetches repositories holding originals with copies; the SharePoint Adapter can create a file in a folder, not only add a version; the Push Service sends automatic updates of copies and holds a copy changed at its destination.
  - A new sequence in 03 — Sequences; 08 — Interfaces rows for Microsoft Graph, the Confluence REST interface, git remotes and the ArtCraft conversion tools.
  - A merged mistake reaches the copies' readers within 15 minutes, unless ordin is closed (R-55). Copies of an original update only while some installation of ordin that published them is running (R-56).
  - 11 — UX guidelines §3, §5.6 and §5.14 change (UX-A43); checklist item 28.
- **Alternatives considered.**
  - *Every update approved (ADR-048 as written).* Not chosen: busy documents fall behind, and the approval repeats a review the repository already did.
  - *Update automatically from any change, including Ordin's.* Rejected: an agent's change would reach colleagues unseen (ADR-029, R-27).
  - *A server or CI job that publishes on merge.* Not chosen for this release: ordin has no server (ADR-025), and a CI job would publish with a shared account instead of the person's own. Teams can still run one beside ordin; ordin then shows the copies as documents that live in SharePoint or Confluence.
  - *Webhooks from GitHub and GitLab.* Rejected: a desktop application cannot receive them without a server.

### ADR-070: Email is an ordin service: the person's own mailbox, reached through the MCP Front, scoped to the workspace, and every message sent only after the person approves it

- **Status.** Decided by the user, January 2026 ("Add native email integration to exoharness as a service to agents via existing exoharness interface, update settings scene other scenes as required"). The mailbox systems, scope, tools and sending rule below are this draft's recommendations, taken as decided under that instruction; the mechanism is proposed. **Builds on ADR-037** (a new MCP server on the MCP Front) **and ADR-025** (a new kind of workspace link). **Amends ADR-027** (Ordin may read the workspace's email itself) **and ADR-028** (two block kinds). *Amended by ADR-071: the company's own mail server (IMAP and SMTP) comes first, Microsoft 365 second, Google Workspace last; the rejected alternative IMAP and SMTP with a password is reversed.*
- **Decision.**
  - **The person's own mailbox.** ordin reaches the mailbox of the signed-in person, with their own account, through the company's mail system named in the organisation profile:
    - **Microsoft 365 (Outlook)** through Microsoft Graph, with the same Microsoft sign-in and Entra ID registration as SharePoint, adding the delegated permissions to read mail, keep drafts and send as the person (C-43);
    - **Google Workspace (Gmail)** through the Gmail API with the person's Google account, registered by IT as an internal application (C-44).
    - Mail servers reached with a password (IMAP and SMTP) are not offered. Shared and delegated mailboxes are not offered in this release (OQ-36).
  - **Email links scope it.** A workspace links email as it links documents (ADR-025): an **email link** is a mail folder or label, or a set of people and domains ("everything from @acme.com and @kestrel.co.uk"). Agents and Ordin read only messages that match the workspace's email links. Nothing reads the whole mailbox; search across the mailbox stays in the person's own mail app.
  - **A service on the MCP Front.** The core's new **Mail Service** is offered to sessions as one MCP server, the **mail tools**, with every call decided by the Rules Engine before it acts (ADR-037). The tools, named in `/spec`:
    - *search email* and *read email*: messages matching the workspace's email links, with sender, recipients, date, subject, text and the list of attachments;
    - *save attachment*: copies one attachment into the workspace's own folder, as a new file through the File Service, after the safety check that every incoming file gets;
    - *draft email*: writes a new email, a reply or a forward, with attachments taken only from places the session may read. The draft is kept by ordin on this computer, not in the mailbox;
    - *ask to send*: hands a draft to the Approval Service. There is no tool that sends.
  - **Who gets the tools.** Ordin gets *search email* and *read email*, so it can answer "has Kestrel replied?" without starting an agent (ADR-062). An agent gets them in a workspace only after the person grants it email access there, as they grant a folder: **Read email**, or **Read and draft email**. Ordin asks the first time an agent needs it ("Let Supplier analyst read the Acme and Kestrel email in this workspace?"). A grant in one workspace gives nothing in another.
  - **Sending is always the person's approval.** Every email leaves only from an approval card the Approval Service builds from the draft itself, never from an agent's words. The card shows:
    - From (the person), To, Cc and Bcc, each recipient outside the company marked **Outside the company**;
    - the subject and the whole message as it will be sent, with the person's signature;
    - each attachment by name, with where it comes from;
    - which agent drafted it, and for a reply, the message it answers.
    - **Send**, **Edit**, **Not now**. Edit opens the draft on its own screen with the same toolbar as Markdown documents (ADR-048).
    - This cannot be turned off by the person, by a package or by a workflow. A workflow step that sends is always an *Asks you first* step (ADR-065).
  - **Sent as the person.** An approved email is sent from the person's mailbox and appears in their Sent folder. Each send is an external action in the journal with a unique key. After a crash, the Mail Service looks in the Sent folder for that message before offering to send again, and shows "Not sure this was sent" until it knows (§8).
  - **Rules from the organisation profile.** IT can turn email off; allow recipients inside the company only; set the most recipients for one email (default 20); keep attachments from named places out of emails; and require the line "Drafted with an agent in ordin" at the foot of every email an agent drafted.
  - **New email.** ordin checks each workspace's email links when the workspace opens, when ordin starts and every 15 minutes while it runs (A-37). New messages appear in the workspace's **Email** group with a count and on Home. They start no work by themselves in this release (OQ-35).
  - **Email is untrusted input.** Messages are read as content, never as instructions (R-27). Senders outside the company are marked; hidden text in a message is reported as it is in documents; agents do not open links in messages; attachments are files like any other, checked before they are saved.
  - **What ordin shows.** Two new block kinds in the fixed catalogue (ADR-028): **Email list** (messages with sender, subject, date, a marked sender outside the company, and Open) and **Email** (one message or one draft, with its attachments). An Email block from an agent is a draft, and only the Approval Service's card can send it.
- **Context.** Much business work arrives and leaves by email: quotes, replies from suppliers, follow-ups. Without email, agents work from attachments the person saves by hand, and the person copies drafts into their mail app. Email is also the easiest way for an agent's mistake to reach someone outside the company, and a common route for instructions hidden in content. One MCP server on the MCP Front gives every harness the same tools under the same Rules Engine checks (ADR-037); keeping drafts in ordin until approval means nothing reaches the mailbox or a recipient without the person seeing it.
- **Consequences.**
  - New: Mail Service, Mail Adapter (Graph, Gmail), the mail tools on the MCP Front, email links and email grants, drafts and sent-email records, two block kinds, a new credential scope.
  - 01 — the company's mail system in context; 02, 03 (a new sequence), 04, 06, 07, 08; 10 — A-37, C-43, C-44, OQ-35, OQ-36, R-57, R-58; 11 — §3, §4, §5.5, §5.15, a new §5.22 and Settings › Email (UX-A44); checklist item 29.
  - Personal data of people outside the company enters ordin and the agents' context (06 — Regulatory).
- **Alternatives considered.**
  - *Agents send without approval, within rules.* Rejected: an email cannot be taken back, and recipients outside the company make a mistake public.
  - *Drafts written into the mailbox's Drafts folder.* Not chosen: the draft would exist outside ordin before the person sees it, and could be sent from another device unseen. The person can still choose **Save to Outlook drafts** on a draft.
  - *IMAP and SMTP with a password.* Rejected: a stored password, and most company mail systems turn password access off.
  - *Email through the sandbox's credential broker.* Rejected: the broker serves code an agent wrote; email needs a fixed tool contract and an approval card built in fixed code.
  - *Each package brings its own email MCP server.* Rejected: each would need its own credential and its own approval, and its calls would be checked only by the harness's hooks (ADR-005, ADR-037).
  - *Whole-mailbox access.* Rejected: agents would see mail that has nothing to do with the work, against the workspace scope of ADR-025.

### ADR-071: Email through the company's own mail server comes first (IMAP to read, SMTP to send); Microsoft 365 second, Google Workspace last; POP3 is not offered

- **Status.** Decided by the user, January 2026 ("there is no pilot team IMAP is my company interface so should not be delayed, Microsoft second, google last"). Leaving out POP3, and the sign-in, connection and sending rules below, are this draft's recommendations, taken as decided under that instruction; the mechanism is proposed. *Password sign-in confirmed by the user, January 2026 ("proceed with recommendations", decision 37): where the servers do not accept single sign-on, a mail password in the keychain is accepted, app passwords preferred.* **Amends ADR-070**: its mailbox systems, and its rejected alternative *IMAP and SMTP with a password*, which this decision reverses. Everything else in ADR-070 stands unchanged: email links, grants, the mail tools, no send tool, the approval card, the profile rules and email as untrusted content.
- **Decision.**
  - **Order.** The Mail Service has three adapters, built and released in this order:
    1. **The company's mail server**, reading through IMAP and sending through SMTP submission, in the first release;
    2. **Microsoft 365 (Outlook)** through Microsoft Graph (ADR-070, C-43), second;
    3. **Google Workspace (Gmail)** through the Gmail API (ADR-070, C-44), last.

    The organisation profile names which one a company uses. The mail tools, the email links, the cards and the screens are the same for all three.
  - **IT names the servers; the person only signs in.** The organisation profile's email section names the IMAP server and port, the SMTP server and port, how the person signs in, and the company's email domains. The person never types a server name, so an agent's draft can only leave through the company's own server.
  - **Signing in.** Two methods, in this order of preference:
    - **the company's single sign-on** (OAuth 2.0, presented to the servers with SASL XOAUTH2 or OAUTHBEARER), where the profile says the servers accept it. ordin then holds a token, as for SharePoint, and no password;
    - otherwise **an app password**, where IT issues them for email, or else **the person's mail password**. The profile says which, and the sign-in screen names it ("App password" or "Password"). App passwords are preferred because each can be cancelled on its own without changing the person's main password (R-59). The person types it once in Settings › Email or on the add-email screen; the Identity and Credentials component keeps it in the OS keychain and passes it only to the Mail Adapter. It never reaches the WebView, a session, a log line or the journal.
    - After one refused sign-in, ordin stops trying and asks the person to sign in again in Settings › Email. It never retries a refused password, so it cannot lock the person's account. A password the person changes elsewhere is asked for again the same way.
  - **Encrypted connections only.** IMAP over TLS on port 993 and SMTP submission over TLS on port 465, or STARTTLS on 143 and 587 where the profile says so. The server's certificate is checked against the computer's trusted certificates, or a company certificate the profile names. If TLS cannot be set up, ordin does not connect, and tells the person "Your company's mail server can't be reached securely. Ask IT." It never falls back to an unencrypted connection.
  - **Reading.**
    - A folder link is an IMAP folder. A people-and-domains link is answered with an IMAP search on the sender and recipient addresses in the folders the profile names (default: Inbox and Sent).
    - Messages are fetched without changing them: reading in ordin does not mark a message as read in the person's mail app (IMAP `BODY.PEEK`).
    - Each message is identified by its folder, the folder's validity value and the message's number in it (IMAP `UIDVALIDITY` and `UID`), and by its Message-ID header. When the validity value changes, ordin reads the folder's list again.
    - New email is checked at workspace open, at start and every 15 minutes, by asking each linked folder for messages above the last number seen (A-38). ordin does not hold a connection open between checks.
  - **Sending.**
    - On approval, ordin gives the message its own Message-ID and records it, with the external action's unique key, in the journal **before** sending.
    - It then sends the message through SMTP submission, signed in as the person, with the person's address as the sender. If the server refuses any recipient, ordin sends to nobody, and shows which address was refused and the server's reason ("dan.moss@kestrel.co.uk was refused: no such mailbox"). The draft stays as it was.
    - A message larger than the server's announced limit is reported before anything is sent: "This email is 31 MB; your mail server takes up to 25 MB. Remove an attachment or share a link instead."
    - When SMTP has accepted the message, ordin saves a copy in the person's Sent folder through IMAP, unless the profile says the server saves sent mail itself. If saving that copy fails, the email is recorded as sent, and the Sent email shows "Sent, but not saved in your Sent folder", with **Save it there**.
    - **After a crash.** If the journal shows a send that started without SMTP's acceptance, ordin searches the Sent folder for the message's Message-ID. If it is there, the email is marked sent. If it is not, the draft shows "Not sure this was sent. It isn't in your Sent folder, but it may still have gone. Check with [recipient] or in your mail app." with **Send again** and **Mark as sent**. ordin never sends again on its own (ADR-010).
  - **Outside the company.** Recipients and senders outside the company are those whose domains are not in the profile's list of company email domains. With the company's mail server the list is required: email stays off until IT has set it.
  - **Drafts.** Drafts stay on this computer (ADR-070). **Save to mail drafts** saves a copy in the person's Drafts folder through IMAP on the person's choice. The button names the mailbox the profile uses: "Save to mail drafts", "Save to Outlook drafts" or "Save to Gmail drafts".
  - **POP3 is not offered.** It downloads email and can delete it from the server, which takes it away from the person's other mail apps; it has no folders for an email link to point at; and it cannot search on the server, so ordin would have to keep a copy of the whole mailbox, against ADR-070's workspace scope.
- **Context.** The user's company reaches email through its own mail server, by IMAP and SMTP, and there is no pilot team on Microsoft 365 or Google Workspace to wait for. ADR-070's rejection of IMAP rested on two points. The first, that most company mail systems turn password access off, holds for Microsoft 365 and Google but not for a company's own server. The second, a stored password, is met by single sign-on where the server accepts it, and otherwise by the keychain, the one-attempt rule and the profile naming the servers. The rules that make email safe do not depend on the mailbox system: no send tool, the approval card built from the draft, recipients outside the company marked, and email read as content.
- **Consequences.**
  - The Mail Service gains the IMAP and SMTP adapter, first; the Graph and Gmail adapters follow in that order.
  - 00 — G33, decision 36; 01 — the company mail system and its edge; 02 — Mail Service and Identity and Credentials; 03 — the email sequence's sign-in and sending steps; 04 — Email link, Sent email, Credential; 07 — authentication; 08 — new *Company mail server (IMAP)* and *Company mail server (SMTP submission)* rows, the profile's email section; 10 — A-38, C-45, R-59, R-60, OQ-35 and OQ-36 reworded; 11 — §5.22 sign-in, sending and Settings › Email (UX-A45); checklist item 29.
  - Where the company's servers accept only a password, ordin keeps one in the keychain (R-59).
  - With SMTP, "was it sent?" after a crash rests on the Sent folder and the Message-ID, which is less certain than Graph's and Gmail's own records (R-60).
- **Alternatives considered.**
  - *Microsoft 365 first, IMAP after (ADR-070's order with IMAP added).* Rejected by the user: the company's own interface is IMAP.
  - *POP3 for servers without IMAP.* Rejected, for the reasons above.
  - *The person types the server names.* Rejected: an agent's draft could then be sent through any server the person was talked into entering, and the company's domain list could not be trusted.
  - *JMAP.* Not chosen: few company mail servers offer it. It can be added later as another adapter behind the same tools.
  - *Holding the IMAP connection open (IMAP IDLE) to see new email at once.* Not chosen for this release: new email starts no work (OQ-35), so a 15-minute check is enough, and fewer open connections means fewer to drop and recover.
  - *Retrying a refused sign-in.* Rejected: a few retries with an old password can lock the person's account.

### ADR-072: ordin, the ArtCraft apps and everything ordin installs run in the person's own user account, with no administrator rights

- **Status.** Decided by the user, January 2026 ("Ordin should be able to run in the user space so no enterprise permissions to locked down areas prevent it from working (including art craft suite)"). The mechanism is proposed. **Amends ADR-016** (where the apps' windows open from) **and ADR-044** (the later step that installs a missing harness). **Extends ADR-011** (no background service). *Amended in revision 0.62.0 (ADR-114): every update of the desktop and of a personal server stays in the person's own account, with no administrator.*
  - *Machine-wide package deferred by the user, January 2026 ("proceed with recommendations", decision 39): the first release ships the per-user package only; companies with strict allow-listing add one rule per publisher. A machine-wide package for IT stays a later option.*
  - *Amended in revision 0.42.0 (ADR-096, decision 72), for the ordin server only: where the organisation profile allows it, the server's Remote API listens on one private address on the company network, still as an ordinary user and on a port above 1024. The desktop still opens no port.*
- **Decision.**
  - **No administrator rights, at any step.** Installing, starting, updating, repairing and removing ordin never asks for an administrator password: no `sudo`, no Windows User Account Control prompt, no macOS authorisation dialog. A person with a standard, locked-down account can do every one of these steps.
  - **ordin installs for the person only.**
    - *macOS:* the signed and notarised app goes in the person's own Applications folder (`~/Applications`), never in `/Applications`.
    - *Windows:* a per-user installer puts ordin in the person's own programs folder (`%LOCALAPPDATA%\Programs`) and writes only to the current user's part of the registry (`HKEY_CURRENT_USER`).
    - Tauri's updater replaces the per-user app in place on restart, with no prompt (C-46).
  - **Everything ordin brings or fetches lives under the application data root, in the person's account.** That is:
    - the ArtCraft apps and their CLIs (already so under ADR-016), started by ordin from there, for agents and for the person's own windows;
    - the bundled `apm`, the Copilot CLI and the ordin hook, inside the app itself;
    - `gh`, the language servers and the toolchains they need (R-20), the speech model and the local memory models;
    - the SQLite journal, file versions, the cached organisation profile and coding-task worktrees, as before.
    - Nothing is written to `/Applications`, `/usr/local`, `/Library`, `Program Files`, `ProgramData`, `HKEY_LOCAL_MACHINE`, the system-wide `PATH` or any other location shared by every user of the computer.
  - **Nothing runs outside the person's session.** No system service, daemon, launch daemon, driver, kernel or system extension, and no login item (ADR-011). Every ordin program — the core, the harness sessions, the hook, the ArtCraft CLIs' MCP modes and the sandbox — runs as the person, as a child of ordin, while ordin is open.
  - **Only what a standard user can do with their own account.**
    - Secrets go in the person's own keychain: the macOS login keychain and the Windows Credential Manager.
    - Local channels are standard input and output, named pipes or Unix sockets in the application data root, or loopback ports above 1024 bound to `127.0.0.1` only. No port is opened on the network, so no firewall exception is needed (C-46).
    - ordin registers no system-wide file types and makes no system setting change. Opening a file in an ArtCraft app is ordin starting that app with the file; the system's default app is opened through the operating system's own per-user call.
    - Operating-system permissions that the person grants for their own account — the microphone for dictation, and macOS's prompt before reading the Documents or Desktop folders — are asked for by the operating system in the ordinary way. Where device management has switched one off, the feature it serves is off, and ordin says so plainly (dictation is optional; ADR-043).
  - **Harnesses.** The first step is unchanged: ordin attaches to the harness the person already has (ADR-044). In the later step, the Setup Assistant installs a missing harness only through the vendor's own installer for the current user (C-46). Where a vendor offers only a machine-wide installer, the Setup Assistant does not run it; it says the harness must be installed by IT and offers **Send to IT**.
  - **Network.** Every outbound call uses the computer's own proxy settings and trusted certificates, including the certificate a company adds for inspecting traffic (A-39). The organisation profile can name a company mirror for ArtCraft releases and other downloads, in place of GitHub.
  - **Company controls are respected, not worked round.** Running in user space removes the need for administrator rights. It does not bypass a company's application allow-listing (Windows App Control for Business or AppLocker, or an allow-listing tool on macOS), device-management restrictions or traffic inspection, and ordin makes no attempt to. Instead:
    - every program ordin brings is signed by its publisher, so IT can allow all of them with one rule per publisher, not a rule per file or per path (R-61);
    - the profile pins each ArtCraft app by version and file hash, which IT can use for a hash rule where the app's own signature is missing (C-47);
    - when the operating system refuses to start a program, the Setup Assistant names the program and the reason in one sentence, turns off only what depends on it, and offers **Send to IT** with the program's name, publisher and hash (UX-A46).
- **Context.** The user's company locks down areas of the computer that need administrator rights, and the user wants ordin, including the ArtCraft suite, to work for every person regardless. ADR-016 already installs the ArtCraft apps under the application data root, ADR-011 installs no background service, and ADR-044 attaches to the person's own harness. What remained open was ordin's own installer and updater (05 — *Signed desktop installers*, with no statement of where they install), the harness installer of ADR-044's later step, and the language toolchains of R-20. Standard user accounts can install and run programs in their own folders on macOS and Windows; what can still stop them there is allow-listing that IT sets on purpose.
- **Consequences.**
  - ordin is installed once per person on a shared computer. Each person has their own copy, application data root and update.
  - The Setup Assistant gains a check that each program ordin brings has started, and a plain message when one was refused (UX-A46).
  - Language servers that need a toolchain install the toolchain from its per-user archive under the application data root, never through a system package manager.
  - 00 — G34, decision 38; 02 — Setup Assistant and Craft Toolkit; 05 — deployment, ArtCraft and signing rows; 07 — security posture; 10 — A-39, C-46, C-47, R-61; 11 — UX-A46 and checklist item 30.
- **Alternatives considered.**
  - *A machine-wide installer, deployed by IT through device management.* Not chosen as the only route: every person would wait on IT, and the ArtCraft apps, which ordin updates as the profile pins them, would need administrator rights at each update. It can be added later as a second package for IT, with the same program inside; deferred by the user, January 2026.
  - *Asking for administrator rights once, at installation.* Rejected: many company accounts cannot grant them, and the user asked for no dependence on them.
  - *Running ArtCraft through the ArtCraft Launcher.* Already rejected by ADR-016.
  - *Placing programs where allow-listing does not look.* Rejected: it would work round a control the company set on purpose.

### ADR-073: ordin can run headless on a remote server, as one person's ordin, reached through a chat channel; Telegram is the first channel

- **Status.** Decided by the user, January 2026 ("Add ability to deploy as headless to run on a remote server and be accessed via a suitable chat channel such as telegram"). The mechanism is proposed. **Extends ADR-072** (user space) to the server. **Amends ADR-011 and ADR-060** only in that the server does not sleep; whether the server runs repeating work on a schedule is OQ-38, and until it is decided nothing is scheduled there either. **Keeps ADR-044, ADR-045 and decision 10.** *Amended in revision 0.43.0 (ADR-097): what this decision describes is the server's* personal profile*; the setup command also offers the* agent profile*, a hosted agent with an identity of its own, which serves the people the organisation allows.*
  - *Open questions settled by the user, January 2026 ("proceed with recommendations", decision 41): Telegram is off unless the organisation profile allows it (OQ-37); Slack is the second channel and Matrix a later one for companies that refuse every outside chat service; the server schedules nothing (OQ-38); the desktop and the server keep separate workspaces, journals and memory, and a workspace whose documents live in SharePoint, GitHub or GitLab can be linked on both (OQ-39). Runs on the server start from the person's chat and from NATS events instead (ADR-074).*
  - *Checked against the publishers' documentation, January 2026 (decision 50; C-48 to C-52). Copilot on the server signs in with `copilot login --device-code` (docs.github.com, "Authenticating GitHub Copilot CLI"); without a keychain the CLI offers to keep its token in `~/.copilot/config.json`, which ordin never reads (ADR-044), and the token variables `COPILOT_GITHUB_TOKEN`, `GH_TOKEN` and `GITHUB_TOKEN` are rejected as `claude setup-token` is. GitHub's device flow must be enabled on ordin's app registration; Microsoft Entra ID needs public client flows allowed, and Conditional Access can block the device code flow, which Microsoft recommends blocking, so on such a tenant SharePoint and Outlook are not offered on the server; Atlassian's OAuth 2.0 (3LO) "currently supports the code grant flow only", so Confluence is not offered on the server, as the welcome already says. Encrypted credentials for a systemd user unit need `systemd-creds encrypt --user`, added in systemd 256; the increasing restart delay (`RestartSteps=`, `RestartMaxDelaySec=`) needs systemd 254. The archive therefore needs systemd 256 or later; an older server runs the container image, with the container runtime's secret (OQ-44). Lingering for one's own account is allowed without an administrator in systemd's own policy (`org.freedesktop.login1.set-self-linger`); distributions may override it (C-51). Telegram's limits hold, and messages are at most 4,096 characters and captions 1,024, so longer replies are split; bot chats are cloud chats, encrypted between client and Telegram's servers only (R-63 stands).*
  - *Which Linux servers settled by the user, January 2026 ("proceed with recommendations", decision 51): the archive needs systemd 256 or later; older servers run the container image with the container runtime's secret (OQ-44). No key is ever kept beside the data it protects.*
  - *Amended in revision 0.42.0 (ADR-096, decision 72): the server also serves the Remote API, a REST API with Server-Sent Events, to the person's own paired ordin desktops. It listens on one private address chosen at setup, which the organisation profile can forbid, never the internet (amended in revision 0.47.0, decision 78). The rejected alternative* a web interface on the server *stays rejected for browsers.*
  - *Container image first, decided by the user, January 2026 ("proceed with recommendations", decision 53): most current server releases ship systemd older than 256 (C-59), so the setup instructions lead with the container image and offer the archive for systemd 256 or later. In the archive, ordin decrypts its key with `systemd-creds decrypt --user` at start, because `LoadCredentialEncrypted=` fails in a user unit, and keeps the file at 0600; lingering needs polkit running or one `sudo loginctl enable-linger` by an administrator (C-50, C-51).*
- **Decision.**
  - **The same core, with no window.** The **ordin server** is a build of the same Rust core without the WebView. It carries Ordin, the Session Supervisor, the Rules Engine, the ordin hook, the MCP Front and every service the desktop has (memory, knowledge, sandbox, email, documents, workflows, reports). The Command Bridge is replaced by the **Channel Gateway**. The server's journal, workspaces and memory are its own; they are not shared with a desktop installation (OQ-39).
  - **One person per server installation.** A server installation serves one person, as the desktop does (non-goal *several people sharing one crew*; 07 — Multi-tenancy). Several people on one server each have their own installation in their own user account, with their own harness sign-in. This keeps Anthropic's terms: each person signs in to the unmodified Claude Code binary with their own subscription, "including where a platform hosts Claude Code" (ADR-044).
  - **Where it runs.** Linux on x86-64 and ARM64 first, as a signed archive and as a container image (A-01 keeps the desktop on macOS and Windows; Linux is first for the server only). It runs as an ordinary user account, with no root and no administrator rights, under the rules of ADR-072: everything under the application data root, and no port opened on the network. Keeping it running after that user logs out is the server owner's choice: a systemd user unit with lingering, or a rootless container (C-51).
  - **Who sets it up.** Deploying a server is a technical step, so it is for IT or an engineer, never asked of a business user (ADR-012). The person's own steps are three: sign in to the harness, pair their chat account, and talk.
  - **The server's setup command.** Over SSH, the server owner or the person runs the server's setup command (its name is fixed in `/spec`). It sets the organisation profile's address, shows the pairing code, and takes any sign-in that needs a secret typed in, such as a mail password (ADR-071). A secret is never asked for or accepted in the chat.
  - **Signing in to the harness.** The person signs in to Claude Code on the server themselves, over SSH, through Claude Code's own login: Claude Code shows a sign-in link and, because the browser cannot reach the server, a code to paste back (code.claude.com/docs/en/authentication, read on January 2026: "common in WSL2, SSH sessions, and containers"). On Linux, Claude Code keeps that sign-in in its own `~/.claude/.credentials.json` with file mode `0600` (same page). ordin checks `claude auth status --json` as on the desktop and never reads the file. `claude setup-token` and `CLAUDE_CODE_OAUTH_TOKEN` are not used: ordin would have to hold and pass on the token, which ADR-044 forbids. The Copilot CLI's own sign-in on a server without a browser is C-48.
  - **Other sign-ins.** Sources that offer a device sign-in (a short code entered on the provider's own page) are signed in from the chat: Ordin sends the provider's link and code, and the person finishes on their phone or computer. GitHub and Microsoft are the first (C-49). A source that needs a browser on the same computer, with no device sign-in, is not offered on the server until a route is confirmed; Confluence is the known case (C-49). Mail passwords go through the setup command.
  - **Secrets on the server.** The server has no login keychain. ordin uses the Secret Service of the user's session where one is running; otherwise a file under the application data root, readable only by that user (`0600`), encrypted with a key the server owner supplies at start through a systemd credential or the container runtime's secret (C-50). The chat channel's own token (for Telegram, the bot token) is one of these secrets.
  - **The Channel Gateway.** One channel-neutral interface between the core and a chat channel. In: the person's text, voice message, file and button press. Out: Ordin's text, an approval or question card with buttons, a file, an image, and an edit of an earlier card. It turns every block of the fixed catalogue into its text form (11 — §5.23): a table becomes a short list with the full table as a file, a chart becomes its key figures and values, a form becomes one question at a time with buttons for fixed choices and a summary ending in the task's own verb. Blocks with no text form say so in one line and save their content as a file in the workspace. The profile names the channels a company allows (A-40).
  - **Telegram, first.** The **Telegram Adapter** reaches the person through a Telegram bot the server owner creates for that installation.
    - It receives updates by long polling (`getUpdates`), so the server opens no port and needs no public address; it never sets a webhook, since "it's not possible to get updates via long polling while an outgoing Webhook is set" (core.telegram.org/bots/faq, read on January 2026).
    - **Pairing.** The setup command shows a one-time pairing code, valid for 10 minutes. The person sends it to the bot in a private chat; after 5 wrong codes in 10 minutes, pairing stops until the setup command starts it again. From then on the adapter accepts messages and button presses only from that Telegram user identifier, in that private chat. Messages from any other user, group or channel get no reply and start nothing; the journal counts them without their content. Pairing again, or ending it, is done only through the setup command.
    - **Limits it works within** (core.telegram.org, read on January 2026): a bot downloads files "of up to 20 MB" and sends files "of up to 50 MB"; button data is "1-64 bytes"; no more than about one message per second in one chat. A larger file is saved in the workspace and named in the reply; Ordin's long answers are split at paragraph ends (C-52).
    - Voice messages are turned into text on the server by the bundled local speech model (ADR-043), and Ordin answers the text. *Superseded by ADR-078 (revision 0.22.0), noted in revision 0.48.0: no speech model is bundled; a voice note is answered "I can't listen to voice notes here. Please type your message."*
  - **Approvals in the chat.** An approval card is a message with the same plain sentence as on the desktop, the exact action, destination and what will change, and buttons **Approve**, **Not now** and **Show details**. Each button carries a random, single-use reference to that card, within Telegram's 64 bytes. The Approval Service accepts a press only from the paired user, for a card still open, and only once; it then edits the message to show the result and removes the buttons. A typed "yes" is still never an approval (decision 10). The ordin hook's wait, deadline and deny are unchanged (ADR-045), so a slow answer from a phone never runs a tool unapproved. The profile can switch off chosen kinds of external action on the server altogether, such as sending email outside the company; Ordin then says that action is not available from the chat.
  - **Needs you, from a distance.** Everything that would appear in *Needs you* on the desktop — approvals, crew questions, finished work and failures — is sent to the chat as it happens, within UX-A4's limits on interruptions. **/stop** pauses every run at once and **/status** lists what is running, both in the chat.
  - **What the server does not offer in the first step.** Engineering mode's screens (editor, terminal, IDE launch; ADR-019 to ADR-022, ADR-052), the ArtCraft apps' windows for the person, local folders on the person's own computer, and dictation by microphone. Agents keep the ArtCraft CLIs where a Linux build is pinned (C-07). Each of these is named once in Ordin's welcome message and when asked for (G9).
- **Context.** The user wants ordin to keep working when the person is away from their computer, on a server that does not sleep, and to reach it from a chat app they already carry. Telegram was named as an example of a suitable channel. The desktop design already keeps every decision in the core (Rules Engine, Approval Service, hook), so a channel is one more way in and out; the WebView was the only part tied to a screen. Telegram's bot interface lets the server poll for messages, so no inbound connection reaches the server. Telegram's documents read do not say whether bot chats are end-to-end encrypted; the design treats every message, file and card as readable by Telegram (R-63, C-52).
- **Consequences.**
  - A new deployment target (Linux server) and a second build of the core without Tauri.
  - The Channel Gateway, the text form of every catalogue block, and the Telegram Adapter are new components (02).
  - A new trust boundary: the chat service sees what passes through the chat (01, 07). Companies decide in the profile whether Telegram is allowed (OQ-37).
  - Approvals from a phone are as binding as on the desktop, so a stolen chat account is a stolen ordin (R-62).
  - 00 — G35, decision 40, non-goals; 01 — trust boundary; 02 — ordin server, Channel Gateway, Telegram Adapter; 03 — sequence; 04 — channel pairing, card reference, chat messages; 05 — server and channel rows; 06 — chat service row; 07 — security posture and concerns; 08 — Telegram Bot API; 10 — A-40, OQ-37 to OQ-39, C-48 to C-52, R-62 to R-64, dependencies; 11 — §5.23, UX-A47 and checklist item 31.
- **Alternatives considered.**
  - *Reach the desktop ordin from the phone* (a relay to the person's own computer). Rejected for this step: the laptop still sleeps and closes, which is the reason the user gave, and it would open an inbound route to a business user's computer.
  - *A web interface on the server.* Rejected for this step: it needs an inbound port, a public address and certificates, and a sign-in of its own; a chat app needs none of these.
  - *Webhooks for Telegram.* Rejected: they need an inbound port reachable from Telegram.
  - *One server shared by several people through one bot.* Rejected: it breaks one person per installation and the harness terms of ADR-044.
  - *Slack, Microsoft Teams or Matrix as the first channel.* Telegram first as the user named it; the Channel Gateway takes others as further adapters (OQ-37). Matrix can run on the company's own server, which suits companies that refuse Telegram.
  - *A long-lived Claude token in the server's environment* (`claude setup-token`). Rejected: ordin would hold and pass on the person's Claude credential (ADR-044).

### ADR-074: On the ordin server, runs start from the person's chat and from NATS events, through triggers; nothing starts from a clock

- **Status.** Decided by the user, January 2026 ("even though there is not any schedule run, the runs should be triggered by channel interactions, add support for NATS events as a trigger", decision 42). The mechanism is proposed. **Extends ADR-073 and ADR-064. Keeps ADR-011 and ADR-060** (no schedule, on the desktop or the server). *Amended in revision 0.53.0 (ADR-105): a standard scheduler outside ordin, sending NATS events on a timetable, is the stated route for repeating work on the personal server; ordin stays the runtime and keeps no timetable.* *Amended in revision 0.43.0 (ADR-097, ADR-098): in the agent profile, runs also start from HTTP intakes and from the Duty Timer.* *Amended in revision 0.50.0 (ADR-102): on the personal server a trigger may also listen for a phrase in a Slack channel a workspace links, so messages from other people in that channel may start a saved workflow; the alternative rejected below for group chats stays rejected for Telegram groups and for every chat on the desktop.*
  - *Checked against the publishers' documentation, January 2026 (decision 50; C-53, C-54). `async-nats` 0.50.0 supports durable pull consumers, explicit acknowledgement, credentials files, user and password, tokens, WebSocket (`ws`, `wss`) and TLS with the operating system's trusted certificates by default; it has **no HTTP proxy support**. The server therefore reaches NATS directly (A-41), never through the company's web proxy (an exception to A-39); where NATS can be reached only through a proxy, event triggers are not offered and **/triggers** reads "Not connected". Reconnection is unlimited by default; ordin sets its own delay of 1 second doubling to 60 seconds through the client's reconnect delay setting. JetStream's own duplicate check applies when a message is published, within a 2-minute window, so ordin's 7-day record of handled event identifiers stays. The NATS server's default maximum payload is 1 MB and the client learns the server's value on connecting; ordin's limit is the smaller of 1 MB and that value.*
- **Decision.**
  - **A trigger** is a saved rule on the ordin server that starts one saved workflow, at a pinned version, with its inputs filled from what started it. It has: a name; its kind (chat or NATS event); what it listens for; a filter; how each workflow input is filled; its limits; on or off; and who turned it on and when. Triggers live on the server only: the desktop has no channel, and a laptop sleeps (ADR-011).
  - **Chat triggers** (the paired person only, in the private chat; ADR-073):
    - **A message to Ordin** starts work as it does on the desktop: Ordin answers, and a request to act comes back as a form, plan or card (decision 10).
    - **/run** followed by a workflow's name starts that saved workflow. Its inputs are asked one at a time, and the run starts when the person presses the workflow's verb on the summary (11 — §5.23). **/run** alone lists the workspace's workflows as buttons.
    - **A file sent to the chat** starts a workflow when it matches a chat trigger the person turned on: by file kind and, if given, a name pattern ("PDFs whose name starts with *invoice*"). The file becomes the input the trigger names; the reply says what started and offers **Stop**.
    - **Run again**, on a finished-work message, opens the run's inputs in the chat as ADR-060's form does, with newer inputs named.
    - Messages from other people, groups or channels never start anything (ADR-073).
  - **NATS event triggers.**
    - **Connection.** The organisation profile names the company's NATS servers, the TLS settings (using the computer's trusted certificates, A-39), and the subjects ordin may subscribe to, as a list with NATS wildcards (`*`, `>`). ordin subscribes to nothing outside that list. The credentials — a NATS credentials file (user JWT and NKey seed), or a user and password, or a token, as the company's server requires — are set through the server's setup command and kept in the server's secret store (ADR-073); they never pass through the chat. The connection is outbound only, so no port is opened. Reconnection uses exponential backoff with jitter, from 1 second up to 60 seconds.
    - **Delivery.** When the subject is held in a JetStream stream, each trigger has its own durable pull consumer with explicit acknowledgement. ordin acknowledges an event once the journal records it as started, skipped by the filter, or refused by a limit, so an event received just before a crash is delivered again, and events published while the server was down are read after it restarts. When the subject has no stream (core NATS), events published while ordin is disconnected are lost; the trigger says so where it is shown ("Events sent while the server is off are missed").
    - **Duplicates.** An event already recorded is not run again: its identifier is the `Nats-Msg-Id` header, or the CloudEvents `id`, or a payload field the trigger names. Identifiers are kept for 7 days.
    - **Payload.** JSON, including CloudEvents in JSON form (`id`, `type`, `source`, `time`, `data`), at most 1 MB (C-54). Any other payload, and JSON that lacks a field the trigger uses, starts nothing; it is counted, and the person is told once per trigger per day.
    - **Filter.** Conditions on payload fields, decided by the application in fixed code: equals, is one of, exists, and number comparisons. No model turn is spent deciding whether an event starts a run.
    - **Inputs.** Each workflow input is filled from one payload field, as text of at most 4,000 characters, or a fixed value. Event content is data, never instructions (01 — trust boundary 4): it reaches agents only as the named inputs.
  - **Limits, for every trigger.** At most one run at a time per trigger. Up to 10 events wait in order; with JetStream the consumer pulls no more than that, so the rest stay in the stream, and with core NATS the rest are refused and counted. At most 20 triggered runs per installation per day, which the profile can lower or raise up to 200. Reaching the daily limit turns event triggers off until midnight in the server's time zone and tells the person in the chat. Each triggered run has the workflow's own limits and the budgets of ADR-062.
  - **Approvals are unchanged.** A triggered run is the person's run. Every external action still waits for the person's approval in the chat; the card says what started the run ("Started by an event: invoice created, INV-2291"). Nothing is approved in advance because a trigger started the run.
  - **Turning a trigger on.** The person asks in words ("when an invoice-created event arrives, run Invoice check with its PDF"). Ordin returns a trigger card: what it listens for, the filter in plain words, how each input is filled, the limits, and the workflow. **Turn on** is the approval. **/triggers** lists every trigger with **Turn off**, the last event and today's count. A subject outside the profile's list cannot be chosen; Ordin says it must be added by IT.
  - **Not in this step:** publishing results back to NATS (it would be an external action with its own approval; OQ-41), and payloads in binary formats such as Protocol Buffers (OQ-40).
- **Context.** The user settled that the server schedules nothing (decision 41), and asked that runs start from the person's chat and from NATS events instead. Saved workflows already carry named inputs and limits (ADR-064, ADR-065), so a trigger only has to fill those inputs and start the run. NATS's JetStream keeps events for consumers that were away, which suits a server that restarts. An event from the company's own systems is published when those systems decide, so a regular event amounts to a schedule kept outside ordin; the daily limit and the person's approvals bound what that can cost (R-64).
- **Consequences.**
  - New components: the Trigger Service and the NATS Adapter (02).
  - Unattended runs now start without the person; the daily limit, the per-trigger queue and the chat notices are what keeps them within the person's plan (R-31, R-64).
  - Event payloads are a new route for text into agents (R-65).
  - 00 — G36, decision 41 and 42, glossary; 01 — company NATS servers; 02 — Trigger Service, NATS Adapter, server diagram; 03 — sequence; 04 — trigger and trigger event; 05 — NATS row; 07 — security posture, rate limiting; 08 — NATS; 10 — A-40, A-41, OQ-37 to OQ-41, C-53, C-54, R-64, R-65, NATS dependency; 11 — §5.24, UX-A48, checklist item 32.
- **Alternatives considered.**
  - *A model turn per event to decide whether it matters.* Rejected: it would spend the person's plan on every event and let event text steer the decision.
  - *Acknowledging an event on receipt.* Rejected: a crash before the journal records it would lose it.
  - *Acknowledging only when the run finishes.* Rejected: runs can wait hours for an approval, beyond the consumer's acknowledgement wait, and the event would be redelivered and run twice.
  - *Webhooks as a trigger.* Rejected: they need an inbound port (ADR-073).
  - *Messages from other people in a group chat as triggers.* Rejected: ordin would serve other people on the person's plan (00 — non-goals).
  - *Event triggers on the desktop.* Rejected: a laptop sleeps and closes (ADR-011).

### ADR-075: Telegram is an ordin service to agents: chats the workspace links, reached through the MCP Front, and every post sent only after the person approves it

- **Status.** Decided by the user, January 2026 ("Add native telegram integration as a service to agents via existing exo-harness interface", decision 43). The account, scope, tools and posting rule below are this draft's recommendations, taken under that instruction, as ADR-070 did for email; the mechanism is proposed. **Builds on ADR-037** (a new MCP server on the MCP Front), **ADR-025** (a new kind of workspace link) **and ADR-073** (the installation's bot). **Follows ADR-070's pattern.** *Amended in revision 0.50.0 (ADR-102), for Slack only: a Slack post may also leave under a rule the person set once (handover notices of one stage, or Ordin's answers in one linked channel, with a daily cap); Telegram posts still always need the card.*
  - *Checked against Telegram's documentation, January 2026 (decision 50; C-55). With privacy mode on, which is the default, a bot in a group receives only commands, replies to its own messages and service messages; with privacy mode off, or as a group administrator, it receives every message. ordin reads the bot's setting from `getMe` (`can_read_all_group_messages`) and, when it is on, **Add to this workspace › Telegram** says that the bot sees only messages addressed to it, with the step to turn the setting off in BotFather. A bot receives the posts of channels it is a member of. No Bot API method reads a group's or channel's earlier messages; the method added in Bot API 10.0 reads only the personal chat a user shows on their profile and is not used. Groups take at most 20 bot messages a minute, which posting from cards never approaches.*
- **Decision.**
  - **Through the installation's bot, on the ordin server first.** The Telegram Service uses the bot the ordin server already has (ADR-073). Posts appear from that bot, whose name the server owner sets ("Priya's ordin"), never as the person; the approval card says so. Acting as the person's own Telegram account is OQ-42; the service on the desktop is OQ-43. Telegram must be allowed by the organisation profile (A-40, decision 41).
  - **Telegram links scope it.** A workspace links a Telegram chat as it links documents (ADR-025): a group, a forum topic of a group, or a channel. The person adds the bot to that chat in Telegram; ordin then offers it under **Add to this workspace › Telegram**, listing the chats the bot has been added to. Agents and Ordin reach only the chats the workspace links.
  - **The person's own chat with ordin is never a link.** The paired private chat (ADR-073) carries approvals and commands; no tool reads it or writes to it, and nothing said in a linked chat controls ordin: commands and button presses count only in the paired private chat.
  - **What can be read.** A bot receives a chat's messages only from the time it is added, and cannot fetch older history. The Telegram Service keeps the messages the bot receives in linked chats, in the server's local database, from the time the link is made; earlier messages are not available, and the link says so ("Messages from 18 October, when ordin joined"). Whether the bot receives every message of a group depends on Telegram's privacy mode for bots, or the bot being an administrator (C-55).
  - **A service on the MCP Front.** The core's new **Telegram Service** is offered to sessions as one MCP server, the **Telegram tools**, every call decided by the Rules Engine before it acts (ADR-037). The tools, named in `/spec`:
    - *search Telegram* and *read Telegram*: messages of the workspace's linked chats, with sender name, time, text, the message it replies to and the list of files;
    - *save Telegram file*: copies one file from a message into the workspace's own folder, through the File Service and its safety check, up to Telegram's 20 MB for bots (C-52);
    - *draft Telegram post*: writes a new post or a reply to one message in a linked chat, with files only from places the session may read, up to 50 MB each. The draft is kept by ordin;
    - *ask to post*: hands a draft to the Approval Service. There is no tool that posts.
  - **Who gets the tools.** Ordin gets *search Telegram* and *read Telegram*. An agent gets them in a workspace only after the person grants **Read Telegram**, or **Read and draft Telegram**, there; Ordin asks the first time an agent needs it. A grant in one workspace gives nothing in another.
  - **Posting is always the person's approval.** Every post leaves only from an approval card the Approval Service builds from the draft, sent in the paired private chat. The card shows the chat by name with **Seen by about N people** (from Telegram's member count), the whole text as it will be posted, each file by name with where it comes from, the message it replies to, which agent drafted it, and that it will appear from the bot. Buttons: **Post**, **Not now**, **Show details**. This cannot be turned off by the person, a package or a workflow; a workflow step that posts is always an *Asks you first* step (ADR-065). Each post is an external action in the journal with a unique key, and Telegram's answer with the posted message's identifier closes it.
  - **Rules from the organisation profile.** IT can turn the Telegram tools off while keeping the chat channel; allow only channels and groups whose identifiers it lists; and require the line "Drafted with an agent in ordin" at the foot of every agent-drafted post. ordin keeps within Telegram's rate in groups (C-52).
  - **New messages start nothing by themselves.** A message in a linked chat is shown in the workspace's **Telegram** group and on Home; it is not a trigger, because the people writing in a group are not the person (ADR-074).
  - **Telegram content is untrusted input.** Messages and files are content, never instructions (R-27, R-66). Agents do not open links in messages; files are checked before they are saved.
- **Context.** Teams discuss work and post updates in Telegram groups and channels. With the server's bot already in place (ADR-073), one MCP server on the MCP Front gives every harness the same Telegram tools under the same Rules Engine checks (ADR-037). A post to a group cannot be taken back once read, so the approval rule of ADR-070 applies unchanged.
- **Consequences.**
  - New: Telegram Service, the Telegram tools on the MCP Front, Telegram links and grants, kept messages, drafts and post records.
  - Personal data of group members is kept on the server (06).
  - 00 — G37, decision 43; 01; 02 — Telegram Service; 03 — sequence; 04 — Telegram link, kept message, Telegram draft; 06; 07; 08; 10 — OQ-42, OQ-43, C-55, R-66; 11 — §5.25, checklist item 33, UX-A49.
- **Alternatives considered.**
  - *The person's own Telegram account* (Telegram's user API, through TDLib). Not chosen for the first step (OQ-42): it would reach every chat the person has, needs an application registration with Telegram whose terms for an open-source client are not confirmed, and would post as the person.
  - *Agents post without approval, within rules.* Rejected, as for email (ADR-070).
  - *The person's paired chat as a link.* Rejected: agents would see approvals and could steer the control channel.
  - *Each package brings its own Telegram MCP server.* Rejected: its own bot token, and its calls checked only by the harness's hooks (ADR-005, ADR-037).

### ADR-076: The ordin server runs all the time, listening to Telegram and Slack; Slack is a chat channel through Socket Mode

- **Status.** Decided by the user, January 2026 ("Add ability for ordin to run all the time when listening to telegram or slack in headless mode", decision 44). The mechanism is proposed. **Extends ADR-073** (brings Slack forward from second channel to this step, decision 41) **and ADR-074.** **Keeps ADR-008** (the Slack Thread Adapter that reaches wrightrun agents as the person is unchanged) **and ADR-072** (no root, no inbound port). The desktop is unchanged: it still runs only while open (ADR-011).
  - *Checked against the publishers' documentation, January 2026 (decision 50; C-56, C-57). Slack: Socket Mode works with an app-level token (`xapp-`, scope `connections:write`) and a bot token (`xoxb-`); events, button presses and direct messages (`message.im`, scope `im:history`, Messages tab on) arrive over it, each acknowledged; up to 10 connections; Socket Mode apps cannot be listed in the public Slack Marketplace, which does not affect an internal app; where app approval is on, the company's Slack owners approve it. Messages are kept to 4,000 characters of text and 50 blocks; button values carry the card reference within 2,000 characters; files are sent with `files.getUploadURLExternal` and `files.completeUploadExternal`, since `files.upload` stopped working on 12 November 2025. Claude Code: `claude auth status` reports JSON by default and exits with code 1 when signed out; version 2.1.302 reports no expiry time. The warning 3 days before expiry is withdrawn: ordin checks the sign-in every 6 hours and before each harness session starts, and tells the person at once when it has ended (R-67 amended). Whether systemd's watchdog works in a user unit is still to be tried (C-57).*
- **Decision.**
  - **Always on.** On the ordin server, ordin starts with the server and keeps running until the server owner stops it. It never quits because the person is away, a channel is quiet or a run has finished.
    - *Supervision.* The server archive ships a systemd user unit that restarts ordin after any exit other than a requested stop, waiting 5 seconds, then doubling up to 5 minutes after repeated failures; it keeps running after the user logs out through lingering (C-51). The container image ships with a restart policy of "unless stopped" and a health check. ordin reports itself alive to systemd's watchdog every 30 seconds where the unit asks for it, so a hung ordin is restarted as well as a crashed one (C-57).
    - *After a restart* ordin recovers as on the desktop (03 — *Recovery after restart*): runs resume or are marked, open approval cards stay open, and triggers continue from their JetStream position (ADR-074). It sends one message to the person: "ordin restarted at 03:12; 2 runs resumed, 1 approval still waiting."
    - *Connections are kept forever.* Every channel and the NATS connection reconnect with exponential backoff and jitter (Telegram 1 second to 5 minutes, ADR-073; Slack 1 second to 5 minutes; NATS 1 to 60 seconds, ADR-074) and never give up. A channel down for more than 10 minutes is reported once on the other channel, if one is paired. *Corrected in revision 0.48.0: ADR-073 states no backoff for Telegram; the figure of 1 second to 5 minutes is this entry's own.*
    - *Staying light over weeks.* A harness session with no turn for 30 minutes is stopped; the conversation resumes in a new session from ordin's summary (ADR-049) when the person writes again. Logs rotate as on the desktop (14 days, A-09). The journal's database is compacted once a week at 04:00 in the server's time zone when no run is going.
    - *Before something runs out.* ordin checks the harness sign-in (`claude auth status --json`, ADR-044) every 6 hours and tells the person in the chat when it fails, with the step to sign in again over SSH; when the harness shows its sign-in expiry, ordin warns 3 days before, as Claude Code itself does (code.claude.com/docs/en/authentication, read on January 2026; C-57). *Amended in revision 0.36.0 (11 — UX guidelines §5.23): ordin never names a technical way in, so there is no SSH step in the message. It says the sign-in has ended and new work waits, and asks the person to "Ask whoever runs your ordin server to sign in again". The 3-day warning is withdrawn (Status note above).* It does the same for each channel's and NATS's credentials when they are refused.
    - *Updates.* The server has no Tauri updater. ordin reads the same update feed and, once per new version, tells the person and writes it to the log; the server owner installs the new archive or image. ordin never replaces itself on the server. *Amended in revision 0.62.0 (ADR-114): a personal server replaces itself when the person asks — `ordin-server update`, or **Update server…** from a paired desktop — never unasked; a server in a container or not run by its user unit, and the agent profile, still only announce.*
  - **Slack as a chat channel.** The **Slack Channel Adapter** is a second adapter of the Channel Gateway (ADR-073), for a Slack app the company's Slack administrators create and approve for the server installation:
    - It connects with **Socket Mode**: an outbound WebSocket that Slack opens with the app-level token, so the server opens no port and needs no public address. Events and button presses arrive over that connection, and ordin acknowledges each envelope on receipt (C-56).
    - **Pairing.** As for Telegram: the setup command shows a one-time code, valid for 10 minutes; the person sends it to the app in a direct message. From then on ordin answers only that Slack user identifier, in that workspace, in that direct message. Messages from anyone else, and in any channel, start nothing.
    - **Cards** are Block Kit messages with the same sentence and buttons as in Telegram (**Approve**, **Not now**, **Show details**); each button's value is a single-use card reference. After a decision ordin updates the message to show the result and remove the buttons. Commands are the same words: **stop**, **status**, **run**, **triggers**, typed in the direct message (Slack's own slash commands are not used, so no command has to be registered per workspace).
    - **Tokens.** The bot token and the app-level token are set through the setup command and kept in the server's secret store (ADR-073). They are separate from the person's own Slack user token that the Slack Thread Adapter uses for wrightrun agents (ADR-008).
    - **Data** stays in the company's own Slack workspace, under its retention, which makes Slack the channel to prefer where the company has it (R-63).
  - **Telegram and Slack at once.** Both can be paired with the same installation, for the same person. Ordin replies on the channel the person wrote on. An approval card goes to every paired channel; the first press decides, and the card on the other channel is updated: "Decided in Slack, 10:44". What *Needs you* holds goes to the channel the person last wrote on, or to both when the person chose that in the setup command.
  - **The Telegram tools for agents (ADR-075) are unchanged.** Slack tools for agents are not added by this decision. *Amended in revision 0.50.0: ADR-102 adds them, through the Slack Service and the person's own user token, not through this channel's app.*
- **Context.** The user wants the headless ordin to stay up for as long as it listens, so that a message or event at any hour gets an answer, and wants Slack as well as Telegram. Slack's Socket Mode delivers events over a connection the app opens, which keeps ADR-073's rule of no inbound port. A process that runs for weeks needs a supervisor to restart it, a watchdog for hangs, and warnings before credentials expire, which the desktop never needed because the person restarts it.
- **Consequences.**
  - The server archive gains a systemd user unit and the image a health check; the core gains a watchdog signal, an idle-session stop and a weekly compaction.
  - A second channel adapter, a Slack app per server installation approved by the company's Slack administrators, and two more secrets.
  - 00 — G38, decision 44; 01 — Slack as a chat channel; 02 — always-on behaviour, Slack Channel Adapter, diagram; 03 — sequence; 05; 07 — health checks, graceful shutdown; 08 — Slack Socket Mode; 10 — C-56, C-57, R-67, dependency; 11 — §5.23 note, checklist item 34, UX-A50.
- **Alternatives considered.**
  - *Slack's Events API over HTTPS.* Rejected: it needs a public address and an inbound port.
  - *Reusing the person's Slack user token to read their direct messages.* Rejected: ordin would read the person's private Slack conversations by polling, and would answer as the person, not as an app. *Clarified in revision 0.50.0 (ADR-102): this rejection stands — direct messages and group direct messages are never read; reading Slack channels a workspace explicitly links is a different choice, made by ADR-102.*
  - *A self-restarting loop inside ordin with no outside supervisor.* Rejected: a crash of the process takes the loop with it.
  - *Always-on on the desktop as well.* Rejected: ADR-011 and ADR-072 (no login item, no background service).

### ADR-077: The Telegram tools also work on the desktop when Telegram is installed on the computer, through a bot of the desktop's own

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", and for OQ-43 "yes if Telegram is installed", decision 45). The mechanism is proposed. **Settles OQ-43; extends ADR-075 to the desktop.** OQ-40 (JSON events only), OQ-41 (nothing published back to NATS) and OQ-42 (the bot, not the person's own account) are settled on their recommendations by the same decision.
  - *Who connects the bot settled by the user, January 2026 ("proceed with recommendations", decision 47): the person connects it on the guided screen, as a named exception to ADR-012; IT can use the same screen for anyone who prefers that (R-68).*
  - *Checked against Telegram's documentation, January 2026 (decision 50; C-58). Unreceived updates are kept for at most 24 hours, which matches the note that messages sent while ordin was closed for more than a day may be missing. Long polling does not work while a webhook is set; that two receivers of one bot conflict is seen in practice but not stated by Telegram, and the desktop's own bot stays the rule. `tg://resolve?domain=BotFather` opens the chat with BotFather. On Windows the standard installer puts Telegram in `%APPDATA%\Telegram Desktop\Telegram.exe` for the current user, and the person may change the folder; macOS locations are not documented. ordin therefore finds Telegram by the `tg:` link handler registered for the user, and by the standard locations as a second check (C-58). The **Connect Telegram** screen gains a fourth step, turning the bot's privacy setting off in BotFather, so the bot can read the groups it is added to (ADR-075 note).*
- **Decision.**
  - **Only when Telegram is installed.** The desktop offers Telegram — under **Add to this workspace**, in Settings, and as Telegram tools to agents — only when the Telegram app is found for this user at its standard install locations, as ordin finds IDEs (ADR-052, C-58). When it is not found, **Add to this workspace › Telegram** says "Telegram isn't on this computer. Install it to link Telegram groups here," and nothing else changes. Telegram must also be allowed by the organisation profile (A-40).
  - **A bot of the desktop's own.** Telegram lets only one program at a time receive a bot's updates, so the desktop cannot share the ordin server's bot (C-58). Connecting Telegram on the desktop is a guided step on its own screen:
    - **Open Telegram** opens BotFather in the installed Telegram app through a `tg://` link;
    - the screen shows three plain steps (start a new bot, give it a name, copy the key BotFather replies with); *amended in revision 0.36.0 (decision 50): four steps, the fourth turning the bot's privacy setting off in BotFather (Status note above; 11 — UX guidelines §5.25); amended in revision 0.44.0 (user's decision of January 2026, UX-A69): three steps — make the bot, paste its key, add the bot to a group as an administrator, which reads every message without changing the privacy setting; the person does it, never IT;*
    - the person pastes the key into ordin's screen, never into a chat; ordin checks it with Telegram and keeps it in the computer's keychain (ADR-072).
    - This is the one technical step a business user takes for Telegram. IT can do it instead, using the same screen.
  - **Only while ordin is open.** The desktop receives Telegram updates by long polling while it runs, and pauses on sleep and quit (ADR-011). Telegram keeps a bot's unreceived updates for a limited time (C-58); messages that arrive while ordin is closed longer than that are missed, and the link says so ("Messages sent while ordin was closed for more than a day may be missing").
  - **Everything else is ADR-075's.** Telegram links, kept messages (in the desktop's local database), grants per workspace, the five tools, no tool that posts, the profile's rules, and messages that start nothing. The **post approval card** is the desktop's ordinary approval card, on its own screen (ADR-080), (11 — §5.5), with the chat by name, about how many people will see it, the whole text, the files and "from your ordin bot".
  - **The desktop is not a chat channel.** The person talks to Ordin in the window; the desktop's bot answers no one in a private chat (ADR-073 applies only to the server).
- **Context.** The user wants agents on the desktop to use Telegram too, where the person has Telegram. The Bot API needs a bot token; a business user's Telegram app is where BotFather is reached, so the installed app makes the step short.
- **Consequences.**
  - The desktop gains the Telegram Service, the Telegram tools and a Telegram section in Settings, shown only when Telegram is found.
  - A key pasted by the person is a secret in the keychain; removing Telegram in Settings deletes it.
  - 00 — decision 45; 02; 10 — OQ-40 to OQ-43 settled, C-58; 11 — §5.25, UX-A51.
- **Alternatives considered.**
  - *Sharing the server's bot.* Rejected: Telegram gives a bot's updates to one receiver.
  - *Reading the installed Telegram app's data.* Rejected: it would read the person's private chats, outside any API Telegram offers.
  - *Offering Telegram whether or not it is installed.* Not chosen: the user asked for it only where Telegram is installed.

### ADR-078: Dictation uses macOS's own on-device speech recognition only; it is off on Windows and needs no bundled model

- **Status.** Decided by the user, January 2026 ("Voice dictation using OSX native voice feature (disabled in windows)", decision 46). The mechanism is proposed. **Amends ADR-043** (no bundled speech model) **and ADR-073** (voice notes on the server).
- **Decision.**
  - **macOS 26 and later:** dictation as ADR-043 says, through Apple's `SpeechAnalyzer` with `SpeechTranscriber` (or `DictationTranscriber` where the locale needs it), on the device, through the Swift bridge.
  - **macOS before 26:** dictation is off. ADR-043's reason for not using `SFSpeechRecognizer` stands (a network service with a one-minute limit, on-device only for some languages), and there is no bundled model to fall back on.
  - **Windows:** dictation is off. The microphone button is not shown; Settings says "Dictation isn't available on Windows." Windows' own voice typing is neither blocked nor relied on, as for macOS's Dictation shortcut in ADR-043.
  - **The ordin server (Linux):** voice notes are not turned into text. A voice note gets the reply "I can't listen to voice notes here. Please type your message."
  - **No bundled speech model** is downloaded or kept on any platform.
  - Unchanged from ADR-043: dictated text is reviewed and sent by the person; a spoken "yes" is never an approval; audio never leaves the computer and is never stored; a locale `SpeechTranscriber` does not support shows the microphone as unavailable with the reason.
- **Context.** The user wants dictation from macOS's own speech feature only, and none on Windows, where no production on-device service exists (ADR-043). Dropping the bundled model removes its download, its disk use and C-24's spike.
- **Consequences.**
  - Dictation is a macOS 26 feature. People on Windows, older macOS and the server type.
  - C-24 and A-26 are withdrawn; R-33 narrows to `SpeechTranscriber`'s locales.
  - 00 — decision 46; 02 — Dictation Service, Channel Gateway; 04 — the speech model row withdrawn; 05 — dictation row; 10 — A-26, C-24, R-33; 11 — §5.12, §5.23.
- **Alternatives considered.**
  - *Keep the bundled model for Windows and older macOS* (ADR-043). Rejected by the user.
  - *`SFSpeechRecognizer` on older macOS.* Rejected, as in ADR-043.
  - *Windows' cloud voice typing driven by ordin.* Rejected: audio would leave the computer.

### ADR-079: Changed documents are marked in the tree, not listed in the conversation; a document opens on its own short page

- **Status.** Decided by the user, January 2026 ("go with the recommendations", decision 48). The mechanism is proposed. **Amends ADR-058** (the catch-up note is withdrawn) **and §5.6 of 11 — UX guidelines** (the documents view loses its detail panel).
- **Decision.**
  - **Changed marks.** A document someone changed where it lives, since the person last opened it, shows a blue dot beside its name in the tree. Each folder above it, up to the *Documents* group, shows how many changed documents it holds while it is folded. Pointing at the dot names the origin, who and when ("Changed in SharePoint by R. Ahmed, 10:40"). The mark goes when the person opens the document; the change's date and author stay on the document.
  - **Accent, not amber.** The mark uses the accent colour. Amber means "waiting for you" (11 — UX guidelines §8); a changed document asks nothing of the person. The mark is never colour alone: its row's accessible name ends "changed since you last looked".
  - **A filter.** *Changed since you looked*, at the top of the *Documents* group while any document is marked, opens the documents view showing only those documents, with **Show all documents**.
  - **Nothing in the conversation on return.** Opening a workspace writes nothing into its conversation. Finished work, knowledge pages that may be out of date, new email and Slack or Telegram replies are on Home (ADR-058), and as counts on their tree groups. Asking "what's new?" is answered in words, with **Show them** for the changed documents and **Everything on Home**.
  - **The document page.** Choosing a document in the tree, or its name in the documents view, opens a screen for that document only, in three levels:
    1. *At a glance*: its kind, where it lives and when and by whom it last changed, in one sentence; its state, when it has one ("Changed by Ordin — not yet sent back", with **Send back…**); **New since you last looked** with the change's note, while it is new; and two actions, **Open in [app]** and **Ask Ordin about this**.
    2. *On request*, behind headings: **Versions**, and **Where it lives** with **Show the folder**.
    3. *In a menu* (⋯): **Who can use [folder]** and **Remove from workspace**.
    A Markdown document still opens as a formatted page (UX-A20); a published copy's page leads with **Open the original**.
  - **The documents view** keeps the folder list and loses the detail panel: one search box, with where it lives and kind behind **Filter**; a *State* column only when a document in the list has a state; **Who can use [folder]** behind a heading. A click on a row selects it; a click on its name, double-click or Enter opens its page.
- **Context.** The note *Since you were last here* listed changed documents in the conversation, one card each, on every return. The person found it cluttered the conversation and made it harder to read, and asked for the change to be shown where the documents are. Clicking a document opened the whole documents view — search, filters, breadcrumb, a table, a selection bar, the document's details and versions, and its folder's access settings — when the person had asked about one document.
- **Consequences.**
  - 00 — G25, decision 48, glossary; 02 — Home and Search; 03 — publishing sequence; 04 — workspace marks; 10 — C-36, R-41; 11 — §5.6, §5.17, §10, §12, UX-A29, UX-A52.
  - A change is noticed when the person looks at the tree or Home, not on opening the workspace. Changes that need a decision still reach *Needs you*.
- **Alternatives considered.**
  - *Marks in the tree and one line in the conversation* ("3 documents changed since you were last here"). Not chosen: the conversation still gains an entry on every return.
  - *Marks only for changes that affect the person's work.* Not chosen: the tree could no longer be trusted to show every change.
  - *Keep the folder list and slide in a details panel with tabs.* Not chosen: the list repeats the tree, and the screen stays busy.
  - *Rows that expand in place.* Not chosen: crowded at narrow widths and harder with a keyboard.

### ADR-080: An approval card opens as a screen in the centre; the conversation keeps a one-line card

- **Status.** Decided by the user, January 2026 ("go with A", decision 49). The mechanism is proposed. **Amends ADR-046** (approval cards no longer appear in full in the conversation) **and extends UX-A35** (forms and crew plans) to approvals. *Amended by decision 58 (January 2026): "Clear this conversation?" and every other action that changes only this computer use a confirmation (11 — §5.5, UX-A57), not an approval card.*
- **Decision.**
  - **Where.** An approval card opens as a screen in the centre of the workspace whose work asked for it, at once and with focus on its heading, as a form does. In another workspace it opens there, without taking the person away from the one they are in.
  - **The one-line card.** The conversation that caused it keeps one line: the card's title, "Check it and press “[Verb]”" and **Waiting for you**, with **Open**. It cannot be closed while the approval waits, and it is kept when the conversation is summarised or cleared. Once decided, the line says what was decided ("Approved: Send back.") and the waiting mark goes.
  - **On the screen.** The card is the same card: built by ordin from the action, never by an agent; the same anatomy (Where, Affects, Changes, the items and what is held back); decided only by its buttons. Once decided, **Close** closes the tab; **Open** on the one-line card opens it again.
  - **Unchanged.** *Needs you* lists it until decided; a typed "yes" never approves; the channels' cards on Slack and Telegram (ADR-073, ADR-076) are not affected. "Clear this conversation?" is not an approval card and stays in the conversation.
- **Context.** A full approval card, with its documents, differences and held-back items, took most of the conversation's height while it waited, so the conversation was hard to read. Forms and crew plans had already moved to screens for the same reason (UX-A35).
- **Consequences.**
  - 00 — decision 49; 11 — §4, §5.5, §5.11, §5.12, §5.17, §5.20, UX-A53.
  - The decision is one click away from the request that caused it. The one-line card sits where the request is, so the link is not lost.
- **Alternatives considered.**
  - *A short card in the conversation, with the details on a screen.* Not chosen: it would let the person approve without Where, Affects and Changes in front of them, which is the point of the card.
  - *Leave the card in the conversation.* Not chosen: it is what cluttered the conversation.

### ADR-081: ordin absorbs crucible's agent machinery, rewritten in Rust in the one build; standalone crucible is frozen

- **Status.** Decided by the user, January 2026 ("rewrite is not an issue, I would rather have something that is coherent and drop standalone crucible, that way ordin would be able to create, publish and consume agents from a git repo, all as part of the same rust build as an integral monolith"; for the crucible repository, "freeze"; decision 52). The component split is proposed. Working paper: `docs/wip/agent-catalogue/proposal.md`. *Amended by ADR-087: APM Import becomes the APM format plug-in.* *Corrected in revision 0.37.0 (ADR-091): crucible's code has 25 kinds, not the 23 counted here; Memory (RFC-49) and ControlLoop (RFC-40) were added to `crucible/pkg/types/kinds.go` after the count was taken. The "23" below stays as written, because this file is append-only.*
- **Decision.**
  - The ordin core gains seven components, written in Rust and linked into the one build (02 — Architecture):
    - the **Agent Model**;
    - the **Catalogue Service**;
    - the **Checker**;
    - the **Resolver and Compiler**;
    - the **Installers**;
    - the **Authoring Service**;
    - **APM Import**.

    Together they replace the Package Catalogue's use of `apm`.
  - Ported from crucible (`../crucible`, commit `e270904`), about 24,000 lines of Go:
    - the component model and its canonical form (`crucible/pkg/types`, `crucible/pkg/canonical`);
    - the authoring and composition checks;
    - the resolver and compiler, the lock file;
    - the Claude Code, vscode and agents-md converters;
    - the Forge's drafts, the Foundry, the Assayer's deterministic checks, assisted drafting and the gap check (`crucible/internal/registry/buildplan`);
    - the MCP tool surface (`crucible/internal/registry/mcp`).
  - Not ported, about 47,000 lines. A git clone, a local index and ordin's own screens take their place:
    - the registry server, its HTTP API, PostgreSQL store, sync worker, tenancy, signing service and key storage;
    - the web portal;
    - the CLI;
    - the local runtime.
  - `../crucible` takes no new features. It is the reference the port is tested against: every WorkspaceProfile in its `crucible/models/@default/workspaces/` must build into the same bundle in ordin as in frozen crucible.
- **Context.** crucible, the user's own code, is an agent-composition registry. It holds a signed catalogue of 23 kinds of parts, a Forge for drafting with a steward's approval, an Assayer, and installers for Claude Code and other clients. ordin had treated building agents as a non-goal and relied on APM for installing them. The user wants one coherent product. Copying the user's own code into this repository has a precedent in the memory component (ADR-034).
- **Consequences.**
  - The non-goals *Building or editing agents* and *Publishing packages to the marketplace* are withdrawn (00 — Overview; ADR-083, ADR-084).
  - The port is large (R-69).
  - crucible's per-part registry signature does not survive, because there is no registry (ADR-084, R-71).
  - Crews remain ordin's own. crucible's `CrewProfile` and its LangGraph lowering are not ported.
- **Alternatives considered.**
  - *Bundle crucible as a pinned Go program and talk to it locally.* Not chosen: the user preferred one coherent Rust build to a second program with its own database.
  - *Keep crucible as a separate company registry.* Rejected by the user: it would keep two products and a server for IT to run.

### ADR-082: Agents come from catalogue repositories on GitHub, in crucible's format; APM packages are imported; `apm` leaves the bundle

- **Status.** Decided by the user, January 2026 ("ordin should use its native more sophisticated format but allow importing apm packages"; "Keep crucible's file format, so the existing 516 catalogue files carry over unchanged"; "consume first, then create, then publish"; decision 52). The mechanism is proposed. **Supersedes ADR-004.** **Amends ADR-013, ADR-014, ADR-032 and ADR-036.** *Amended by ADR-086 (catalogue repositories are agent repositories, several, the person sees and can add them) and ADR-087 (formats are plug-ins; ordin reads and writes APM packages).* *Amended by ADR-089 (decision 63): the ordin format gains one optional field, the packs an agent's maker fixed.* *Corrected in revision 0.37.0 (ADR-091): the format has 25 kinds, not 23 (see ADR-081).* *Amended by ADR-091: the withdrawn list is a file of ordin's own format, and a withdrawal can be prepared in ordin and sent for review.*
- **Decision.**
  - **The native format** is crucible's component model, unchanged except for the identifier prefix:
    - `apiVersion: ai.enterprise/v1`;
    - the 23 kinds and the `urn:ordin:…` identifiers (*amended January 2026, the user's decision: ordin's identifiers use the prefix `urn:ordin:` in place of crucible's own; the rest of the grammar is unchanged and no other prefix is read*);
    - the directory layout of `crucible/models/@default/`.
    
    The 516 files of that catalogue become the first catalogue repository, bundled with ordin. Renaming the identifiers waits (OQ-46).
  - **Catalogue repositories.** The organisation profile names one or more GitHub repositories holding parts in that format, each with its protected default branch.
    - The Catalogue Service clones them with the person's GitHub sign-in (ADR-014) and follows that branch.
    - It builds a local search index with facets: area, audience, publisher.
    - The agent library and Ordin's *find agent* (ADR-032) search that index, not `apm search`.
    - The person never sees which repository an agent comes from.
  - **Reading a part.** Every part read from a repository goes through the Checker before it is offered. A part that fails is not offered, and the failure goes to Diagnostics.
  - **Adding an agent.** The Resolver and Compiler build the WorkspaceProfile with its practice packs and overlays into a deterministic bundle. A lock file records, for each part, the repository, the commit and a content digest. The Installer for the session's harness writes the bundle into a new agent folder: Claude Code first, then Copilot (C-60, C-61).
  - **Trust over native kinds.** The profile's executable-trust policy lists the tool bindings (by identifier and the program each starts) and the hooks the company allows. An agent whose parts fall outside it is shown as needing IT approval, as before (ADR-013, UX §5.2).
  - **Updates and withdrawal.** A newer version on the protected branch is an update, under the existing rules (A-16). Withdrawal is a commit to the repository's withdrawn list. A withdrawn agent stops at once in every workspace, keeps what it did, and Ordin says why (UX §5.2).
  - **APM Import, one way.** An APM package can be imported into a draft in the native format. APM's primitives are mapped to native kinds; anything that does not map is reported and never guessed. The draft then goes through the same checks, rehearsal and publishing as any other (ADR-083, ADR-084). ordin never writes APM packages.
  - **`apm` leaves the bundle.** Agents installed through `apm` before this revision are imported once, before `apm` is removed (R-73).
- **Context.** ADR-004 installed packages only through `apm` from one marketplace. ADR-066 already shares workflows through repositories, with a repository's own access deciding who may share.
- **Consequences.**
  - The internal APM marketplace is no longer a dependency, and `GITHUB_APM_PAT` is no longer set (ADR-014).
  - `apm audit` and `apm approve --recommended` are replaced by the Checker and the trust policy above.
  - The person's GitHub account needs read access to the catalogue repositories (ADR-014).
  - 02 — Architecture: components; 04 — Domain: catalogue repository, agent version, agent instance; 08 — Interfaces: profile sections, catalogue repositories, APM package import; 10 — R-70, R-72, R-73, C-60, C-61.
- **Alternatives considered.**
  - *Keep `apm` for installing, beside the catalogue.* Not chosen: two install paths, and the user chose the native format.
  - *Read and write APM packages as ordin's format.* Rejected by the user: crucible's format is richer.

### ADR-083: Ordin offers to make an agent; the agent maker drafts it with the person; ordin writes, checks and rehearses it

- **Status.** Decided by the user, January 2026 ("ordin should be able to host a crucible agent, that uses the crucible tools to create agents"; then "proceed with recommendations", decision 52). The mechanism is proposed. **Amends ADR-032.**
- **Decision.**
  - **When.**
    - When *find agent* finds no agent that covers a request, Ordin first says what the library does cover, then offers **Make an agent**.
    - The person can also ask for one ("make an agent"), or choose **Make an agent…** on the library screen. *(Label amended by decision 66: one label everywhere, UI-D48.)*
    - There is no blank form for a new agent.
  - **The agent maker** is crucible's advisor workspace (`crucible/models/@default/workspaces/advisor`), bundled at a pinned version like Ordin (ADR-027). Everyone can use it, not only in workspaces with code. Ordin and the draft screen put its results in plain words.
    - The person talks to it as to any agent.
    - It brings in related requests other people made, and it suggests leaving out work that needs different access. It suggests the words of the agent: purpose, jobs, tasks, what it will and won't do, what it needs.
  - **Its tools.** The authoring tools are crucible's MCP tool surface, served by the MCP Front (ADR-037) to the agent maker's session only, under the server name its tool binding declares (`crucible`). They return structured fields, never files. Compiling, installing, the Foundry and publishing are not among them: they are ordin's own actions, each behind the person's approval where it acts on anything outside the draft.
  - **Making a whole agent.** The agent maker writes up the role from the conversation. ordin submits that write-up to the Foundry as its own action, and the draft is the result.
  - **The draft** opens as a screen (UX §5.26). It shows:
    - *Checked by ordin*: the Checker's results, each with a plain fix where one exists ("Leave Slack out", "Ask IT about Slack");
    - the **Rehearsal**;
    - what it is: jobs, will, won't, needs, and the parts reused from the library;
    - in a workspace that links code, the files ordin wrote.
  - **Rehearsal.** ordin runs the draft on situations made from the request and the workspace's documents. Each situation has fixed expectations, which ordin checks against the run's journal and result. A reviewing agent's opinion is shown, labelled as coming from a model, and decides nothing (ADR-085).
  - **Trying it.** Once every check and the whole rehearsal pass, **Try it here — only you** adds the draft to the workspace for the person only, on this computer. Its needs must already be within the trust policy.
  - **Publishing** follows ADR-084. Drafts live on a branch of the person's clone of a catalogue repository.
- **Context.** The user wants creating agents to be first-class. Decision 12 removed the guided crew builder because it was a second, manual route that asked technical questions. This route has no blank form and asks nothing technical.
- **Consequences.**
  - The non-goal *Building or editing agents* is withdrawn.
  - 03 — sequence *Make an agent with the agent maker*; 04 — Agent draft, Rehearsal; 08 — Authoring tools; 11 — §5.26 and UX-A54.
- **Alternatives considered.**
  - *Give the agent maker the Foundry and compile tools.* Rejected: crucible keeps them off its tool surface on purpose (`crucible/internal/registry/mcp/toolset/toolset.go`, `unprojected`), and an agent would then decide what is built.
  - *An agent-building form.* Rejected: the reason decision 12 removed the crew builder.
  - *Agents grown only from repeated work.* Not chosen as the only route: the user observed that thinking through what an agent needs can solve more than one problem.

### ADR-084: Publishing is a pull request to a catalogue repository; approval and merge happen on GitHub

- **Status.** Decided by the user, January 2026 ("approving sits outside of crucible and in github, so for example ordin will do a pull request, approval at git level will merge"; signed merges by "proceed with recommendations", decision 52). The mechanism is proposed. **Extends ADR-066.** *Amended by ADR-086 (the card names the agent repository by its display name) and ADR-087 (the pull request holds files in the target repository's format).* *Amended by ADR-091: a single part, a withdrawal and a vocabulary change are each published by the same pull request; a withdrawal is no longer only a commit made on GitHub.*
- **Decision.**
  - **Publish…** on a draft whose checks and rehearsal all pass opens an approval card built by ordin (ADR-080). The card names the catalogue repository, what the pull request holds and who reviews it.
  - Approving sends a pull request through the Push Service and `gh` (ADR-023, ADR-051). It holds the agent's files, the results of ordin's checks and the rehearsal.
  - **The review is GitHub's.** Reviewers, required approvals and who may merge are the repository's own settings, set by IT. ordin adds no approval role of its own. The merge onto the protected branch is the publication.
  - **Signed merges.** IT can require, per catalogue repository in the profile, that ordin accepts only signed merge commits. GitHub shows these as "Verified".
  - **Following the request.** The draft and *Needs you* follow the pull request:
    - *Waiting for review on GitHub*;
    - *Changes asked for*, with the reviewer's words and **Ask the agent maker to make this change**, then **Send the change…** with a new approval card;
    - *Published*.
  - **Withdrawal** is a commit to the repository's withdrawn list, made by its owners on GitHub (ADR-082).
- **Context.** crucible gated publishing with a steward's approval inside its registry. The user placed approval on GitHub. Shared workflows already follow the same pattern (ADR-066).
- **Consequences.**
  - The non-goal *Publishing packages to the marketplace* is withdrawn.
  - The person needs permission to open pull requests on the catalogue repository; without it, the card's result says so and the draft stays theirs.
  - 03 — sequence *Publish an agent through a pull request*; 10 — R-71.
- **Alternatives considered.**
  - *An approval card for a steward inside ordin.* Rejected by the user: approval belongs to GitHub.
  - *Merge from ordin once approved.* Not chosen: who merges is the repository's rule.

### ADR-085: Checks, validation, formatting and files are ordin's fixed Rust code; an agent only suggests content

- **Status.** Decided by the user, January 2026 ("ensure as much checks, validation, formating, etc is written deterministically in rust as opposed to leave it to the agent"; decision 52). **Extends ADR-028 and ADR-065**, which already check blocks and workflow files in fixed code.
- **Decision.**
  - An agent may **suggest** content: the words of a purpose, a job, a rule or a skill.
  - Everything that **decides, checks, formats, writes a file, compiles, gates or publishes** is ordin's Rust code. Its result for a given input is the same every time, with no model call in the decision.
  - For agents, that covers:
    - matching a request against the library;
    - generating every file from the draft's fields;
    - the wording rules: vocabulary, British spelling, date and number format, task names that start with a verb, length limits. These are applied and shown as "ordin tidied N things";
    - every check;
    - each rehearsal expectation;
    - the bundle, lock file and digests;
    - the pull request's contents;
    - reading, pinning and withdrawal;
    - the mapping of APM imports.
  - crucible's model-scored and narrative assay checks become **opinions**: shown, labelled as coming from a model, and never a gate.
- **Context.** The design already keeps decisions out of models where safety depends on them: the Rules Engine, the hook, approval cards, block and workflow checks. The user extends the rule to everything the agent machinery does.
- **Consequences.**
  - Every gate is a Rust function with table-driven tests over frozen crucible's fixtures.
  - An authoring tool call that carries file contents is refused.
- **Alternatives considered.**
  - *Let a model judge quality as a gate.* Rejected: the same draft could pass one day and fail the next.

### ADR-086: Agents come from several agent repositories, named in the profile or added by the person; the person sees which one

- **Status.** Decided by the user, January 2026 ("Use \"Agent Repository\" also you should be able to add more than one repository", decision 54). The mechanism is proposed. **Amends ADR-082** (a catalogue repository is now an *agent repository*; the person sees it, and can add one) **and ADR-084** (the publish card names the agent repository by its display name). Settles UI-D1 of 12 — UI prototype.
- **Decision.**
  - **The term.** What ADR-082 calls a catalogue repository is shown and written as an **agent repository**. Each has a display name ("Company agents"), a host (GitHub; GitLab waits on OQ-45), a protected default branch and a format (ADR-087). The repository's address is never shown outside Diagnostics.
  - **Several, from two sources.**
    - The organisation profile names zero or more agent repositories, each with its display name and the signed-merge rule of ADR-084. These are shown as *Set by your company* and cannot be removed by the person.
    - The person adds an agent repository on **Add an agent repository…** (Settings › Agent repositories, and the agent library). They paste the repository's web address; the Catalogue Service checks read access with the person's sign-in for that host, detects the format (ADR-087), counts the agents and says in one line what the person can do with it ("Found 12 agents in APM format. You can read it; publishing needs permission from its owners."); **Add** keeps it. The person names it, with the repository's own name as the default.
    - The profile can turn off adding repositories. The library and Settings then say "Your company manages the agent repositories. Ask IT to add one." with **Send to IT**.
  - **Same rules for every repository.** Every part read from any agent repository goes through the Checker; every agent's executable parts must fall inside the profile's trust policy or the agent needs IT approval (ADR-013, ADR-082). Adding a repository never widens what an agent may run.
  - **The person sees where an agent comes from.** The agent library shows "From: [display name]" on each agent and filters by repository. Two agents with the same name in two repositories are both listed, each with its source.
  - **Publishing.** The draft screen has **Publish to**, listing the agent repositories the person can open a review in, the profile's first; **Publish…** then opens the approval card, whose *Where* reads "Agent repository — [display name], on GitHub". A repository the person cannot publish to is not listed; if none is, the draft says so and stays the person's (ADR-084).
- **Context.** ADR-082 hid the repository behind one company catalogue. The user wants the person to work with more than one source of agents and to know which one an agent came from, and named the term.
- **Consequences.**
  - ADR-082's "The person never sees which repository an agent comes from" is withdrawn.
  - Adding a repository is a named exception to ADR-012, as the desktop Telegram bot is (decision 47): the person pastes one web address and ordin does every technical step.
  - 00 — glossary, decision 54; 02 — Catalogue Service; 04 — Agent repository; 07 — Configuration; 08 — profile section and agent repositories; 10 — R-74; 11 — §3, §4, §5.2, §5.26, UX-A56.
- **Alternatives considered.**
  - *Only the repositories the profile names.* Not chosen: the user asked that more can be added.
  - *Show the repository's address as its name.* Rejected: an address is an identifier (11 — §12 item 1).

### ADR-087: Agent formats are plug-ins in ordin's own code; ordin reads and writes the ordin format and the APM format

- **Status.** Decided by the user, January 2026 ("for adoption, it would be good if the tool could push agents in apm and ordin format and equally access agents from remote repositories that use both formats, using plugin architecture for formats", decision 55). The mechanism is proposed. **Amends ADR-081** (APM Import becomes the APM format plug-in), **ADR-082** (APM is no longer import-only, and ordin writes APM packages) **and ADR-084** (a pull request holds files in the target repository's format). **Keeps ADR-085:** every format plug-in is fixed Rust code.
- **Decision.**
  - **The ordin format** is crucible's component model of ADR-082, unchanged. It is the Agent Model's own form: everything ordin can express is expressed in it.
  - **A format plug-in** is a Rust module, compiled into the one build, that implements one interface of four operations:
    - *detect* — whether a repository tree is in this format;
    - *read* — turn the format's files into Agent Model parts, with a mapping report of anything that has no Agent Model equivalent;
    - *write* — turn Agent Model parts into the format's files, with a loss report of anything the format cannot hold;
    - *version* — the format versions it reads and writes.
  - **No plug-in is loaded at run time.** A new format is a new plug-in in a new release of ordin. This keeps every read and write deterministic and tested (ADR-085) and keeps code that a repository supplies from running in ordin.
  - **Two plug-ins in the first release.**
    - *ordin format*, lossless both ways.
    - *APM format*: reads and writes APM's package layout (08 — Interfaces), pinned to the APM format versions it was tested with. ordin still bundles no `apm` program (ADR-082).
  - **Reading.** Each agent repository has one format, detected when it is added or named in the profile. Agents from an APM-format repository are listed, checked, built and installed through the same Checker, Resolver and Compiler and Installers as any other. A part whose mapping report is not empty is not offered, and the report goes to Diagnostics.
  - **Writing.** Publishing writes the draft in the target repository's format. When the loss report is not empty, *Checked by ordin* shows one failing check naming what cannot be written, with the fixes "Leave it out" and "Publish to an ordin-format repository"; **Publish…** stays unavailable until it passes. Nothing is dropped without the person seeing it.
  - **Round trip.** For every fixture, *read* then *write* in the same format gives back the same files, and APM *read*, ordin *write*, ordin *read*, APM *write* gives back the same APM package where its loss report is empty. These are table-driven tests over frozen crucible's fixtures and a set of APM packages.
  - **Import stays.** Opening an agent from any repository as a draft (to change it) goes through *read*; the APM Import of ADR-082 is this path.
- **Context.** Adoption: companies that already keep agents as APM packages can use ordin without converting them, and agents made in ordin can be published where those companies already look. ADR-082 had made APM import-only because crucible's format is richer; the plug-in keeps the richer format as ordin's own and states each loss.
- **Consequences.**
  - The Agent Model is the only form the Checker, Resolver and Compiler, Installers and Authoring Service see; formats change only the plug-ins.
  - R-70 widens to writing: an APM format change can break publishing to APM repositories as well as reading them.
  - 02 — APM Import becomes the Format Plug-ins component; 04 — Agent repository's format; 05 — tech stack row; 08 — APM package format read and written; 10 — R-70, R-75, C-62; 11 — §5.26.
- **Alternatives considered.**
  - *Plug-ins loaded at run time (shared libraries or WebAssembly).* Not chosen: code from outside the build would decide what files are written, against ADR-085, and IT would need to approve each plug-in.
  - *Drive the `apm` program for APM repositories.* Not chosen: it brings `apm` back into the bundle and gives two install paths, which ADR-082 removed.
  - *Keep APM import-only.* Superseded by the user's decision.

### ADR-088: Each agent catalogue component names the crucible code it carries; a request becomes requirements before it is matched; drafting reaches a model through the person's harness

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", on folding the working paper's port map into the HLD and recording the crucible features with no replacement as open questions; decision 60). *Amended the same day by decision 61: OQ-49 and OQ-51 settled (see* Starting files *and* Conflicts *below); OQ-50 narrowed.* The mechanism is proposed. **Amends ADR-081** (the port list becomes a map per component) **and ADR-083** (the match before *Make an agent* works on requirements, not the raw request). Working paper: `docs/wip/agent-catalogue/proposal.md`, *Architecture*.
- **Decision.**
  - **The port map.** Each component of 02 — Architecture names what it carries from frozen crucible (`../crucible`, commit `e270904`). Line counts are Go, tests excluded, from the working paper.

    | Component | Carries from crucible | Kind of work |
    |---|---|---|
    | Agent Model | `crucible/pkg/types` (4,136), `crucible/pkg/canonical` (161) | Port |
    | Catalogue Service | The ingest steps of `crucible/internal/registry/syncworker` (1,785): walk the tree, group files into parts, parse, validate and canonicalise each kind, cross-check references, build dependency edges, all or nothing per commit. The search of `crucible/internal/registry/datastore` (6,033) becomes a local SQLite full-text index. | Rewrite against the same fixtures; signing and the PostgreSQL store are dropped |
    | Checker | `crucible/internal/registry/authoringcheck` (1,942), `compositioncheck` (341), `artefactbuild` (329) | Port |
    | Resolver and Compiler | `crucible/internal/registry/resolver` (1,674), `compiler` (1,598), `crucible/internal/cli/lockfile` (67) | Port |
    | Installers | `crucible/internal/cli/converters` (3,642: `claudecode`, `vscode`, `agentsmd`), `crucible/internal/cli/verify` (378) | Port; the `cursor` converter is not ported |
    | Authoring Service | `crucible/internal/registry/forge` (3,276), `foundry` (1,979), `assay` (919), `generate` (494), `assist` (421), `buildplan` (293), `dossier` (195) | Port; the model client is replaced (below) |
    | Authoring tools | `crucible/internal/registry/mcp` (2,318) | Port |
    | Format Plug-ins | ordin format: the Agent Model's own read and write. APM format: `crucible/internal/cli/converters/apm` is the starting point for *write*; *read* has no crucible source, because crucible never imported APM packages. | APM *read* is new code |

  - **Offline verify.** The Installers keep crucible's offline check of an installed agent folder against its lock file (`crucible verify`). When it runs, and what the person sees when a folder no longer matches, is left to `/spec`.
  - **From request to requirements.** crucible's gap check (`crucible/internal/registry/buildplan/buildplan.go`, `Plan`) takes a role write-up read against the `role-dossier` output contract, not free text. ordin keeps that:
    1. Ordin rewords the person's request into a short list of requirements and shows them ("To do this, an agent needs to: …"). This is content an agent suggests (ADR-085); the person can change the list.
    2. The Catalogue Service matches each requirement against the library in fixed code and returns, per requirement, *covered*, *partly covered* or *not covered* (crucible's met, partial and gap), each naming the parts that matched.
    3. When the agent maker hands over its role write-up, the dossier reader (`crucible/internal/registry/dossier`) reads it against the `role-dossier` contract and the gap check runs again on the write-up. The draft screen's *parts reused from the library* come from this second run.
  - **Ranking on SQLite.** crucible's met, partial and gap boundaries (`planMetRank` 0.30 and `planPartialRank` 0.02 in `buildplan.go`) are PostgreSQL `ts_rank` scores. ordin's index is SQLite (05 — Tech stack), whose ranking is on another scale, so the boundaries are set again for it. The use phase of R-69 adds a second parity test: over frozen crucible's build-plan fixtures, ordin gives each requirement the same classification as frozen crucible (C-63).
  - **How drafting reaches a model.** crucible's generation seam (`crucible/internal/registry/generate`) and the Assayer's scored and narrative checks (`crucible/internal/registry/assay`) call a model through OpenRouter. In ordin both run as a session on the person's own harness (ADR-038, ADR-044), with no separate model account. The seam keeps crucible's rule that source material reaches the model only as data, never inside the prompt template. What the model returns is content for a draft; the Checker decides on it (ADR-085).
  - **Not ported, and what takes their place.**
    - `crucible/internal/registry/seed` wrote the five controlled-vocabulary parts into an empty repository so the authoring checks pass. **Starting files** (decision 61, settling OQ-49): the agent repository always exists on GitHub before ordin is pointed at it; ordin never creates one. When it lacks what the target format needs, ordin's fixed code generates every starting file (for the ordin format, the vocabulary parts from the bundled catalogue; for the APM format, what the APM plug-in's *write* needs), and they travel with the first publish, under the same approval card and review. The checks then run against the starting files plus the draft, never against an empty tree.
    - The composer's pack choices (`crucible/internal/registry/service/composition.go`): a WorkspaceProfile can require the installer to pick packs (`requiredPracticePacks`) or offer optional ones (`selectablePracticePacks`), with no default in the format. Who picks is open (OQ-50); the organisation profile does not (decision 61).
    - `crucible pin` settled a conflict the Resolver reports when two parts contribute the same item. **Conflicts** (decision 61, settling OQ-51): no agent with conflicting parts is published. The Checker runs the Resolver's conflict detection on every draft, over each pair of parts that can be installed together, including every pack the agent allows. A conflict is a failing check in *Checked by ordin*, naming the two parts in plain words, with **Ask the agent maker to fix this**; **Try it here** and **Publish…** stay unavailable until it passes. Nothing is pinned, and ordin has no pin. An agent read from a repository that still has a conflict fails the Checker and is not offered (ADR-082).
- **Context.** ADR-081 listed what is ported and what is not. The working paper mapped each component to its crucible packages, but the map stayed in the paper, and the HLD no longer said where the dossier reader, the generation seam or the ingest steps went. A review of January 2026 found that the gap check needs a write-up the sequence did not produce, that its boundaries belong to PostgreSQL, and that three crucible features had no replacement.
- **Consequences.**
  - 02 — each agent catalogue component lists what it carries; 03 — *Make an agent* starts with the requirements; 05 — the agent catalogue row names the SQLite index; 10 — OQ-49 to OQ-51, R-76, C-63.
  - The port's size (R-69) includes rewriting the ingest steps and writing APM *read* from nothing.
- **Alternatives considered.**
  - *Match the raw request with the gap check.* Rejected: the gap check reads sections of a write-up; free text gives it nothing to read.
  - *Keep crucible's rank boundaries.* Rejected: they are PostgreSQL scores and mean nothing on another index.
  - *A model account for drafting, as crucible's OpenRouter client.* Rejected: ADR-044 gives ordin no model account of its own.

### ADR-089: Ordin picks open practice packs from the workspace's code; otherwise the agent's maker has fixed them before publishing

- **Status.** Decided by the user, January 2026 ("go with b, falling back to a", decision 62). Settles OQ-50. The mechanism is proposed. *Amended the same day by decision 63 ("go with a", settling OQ-52): the maker's fixed packs are one optional field added to the ordin format (see* Where the fixed packs are recorded *below). This amends ADR-082's unchanged format by that one field.* *Amended again by decision 64 ("have the agent asking"): a required choice can be left for the agent to ask the person (see* The agent asks *below).*
- **Decision.**
  - **Which choices.** A WorkspaceProfile can require the installer to pick packs (`requiredPracticePacks`: at least *n* of a class or of a named set) and can offer optional ones (`selectablePracticePacks`). crucible's format holds no default for either, and crucible's portal asked the installer.
  - **Ordin picks from the workspace (b).** When an agent is added to a workspace that links code, fixed code matches the languages of the linked projects (as the Project Service already finds them to install language plug-ins, ADR-024) against each pack's `metadata.audience.languages`. A pack whose language is in the workspace is picked: for a required rule, as many as the rule needs; for an optional pack, whenever it matches. No model call decides the pick (ADR-085).
  - **Otherwise, the maker's fixed packs (a).** For every required rule the workspace cannot settle, the packs the agent's maker fixed before publishing are used. Optional packs the workspace does not match are left out unless the maker fixed them.
  - **The person sees the pick.** Adding the agent shows what it will use, in plain words ("Uses: Go, Test pyramid"), with **Change**, which lists the packs the agent allows. Nothing is asked before **Add**.
  - **Checked before publishing.** A draft passes only if the maker's fixed packs alone satisfy every required rule (or, from decision 64, the rule is left for the agent to ask), so the agent can be added to any workspace, including one with no code. Otherwise *Checked by ordin* shows a failing check, "Choose which [class] this agent uses when the workspace doesn't say", with the choices; **Try it here** and **Publish…** wait for it. The conflict check of ADR-088 covers the fixed packs, every pack the workspace match could add, and every pack **Change** offers.
  - **The agent asks** (decision 64). For a required rule, the maker can, instead of fixing packs, leave the choice for the agent to ask. The same optional field records it.
    - The agent asks the person in its conversation the first time its work needs the choice, in its own words ("How should I organise the tests for this project?"). The choices are not the agent's: they are a *question with choices* block (ADR-028) whose options ordin's fixed code fills from the packs the rule allows, each described in plain words from the pack's own description. Nothing is asked before **Add it**.
    - ordin records the answer for this agent in this workspace, the Resolver and Compiler rebuild the bundle with that pack, and the Installer updates the agent folder before the agent goes on (C-64). The workspace's agent page shows the choice, with **Change**.
    - The check before publishing passes when every required rule is either satisfied by the maker's fixed packs or left for the agent to ask.
    - The bundled `cspa-431` leaves its testing pack for the agent to ask. Its stack pack comes from the workspace's code; where the workspace does not settle it, the agent is unavailable there, as above.
  - **Where the fixed packs are recorded** (decision 63). One optional field in the WorkspaceProfile, added to the ordin format, lists the packs the maker fixed; `/spec` names it. It is the only change to crucible's format (ADR-082):
    - A file without the field reads as before, and its digest is unchanged.
    - Frozen crucible's manifest reader ignores fields it does not know (only its tool arguments refuse them, `crucible/internal/registry/mcp/toolexec/args.go`), so a file carrying the field still reads there. The parity test of R-69 runs over frozen crucible's own fixtures, which do not carry it, with the same pack choices given to both.
    - The APM plug-in's *write* puts the fixed packs into the package where the APM format can hold them; where it cannot, the loss report names them (ADR-087).
    - The Agent Model writes the field from the draft's choices; the agent maker suggests the choice, and the Checker decides that it satisfies every required rule (ADR-085).
  - **An agent read from a repository** whose required rules the fixed packs do not satisfy is offered only in workspaces whose code settles every rule; elsewhere the library shows it as unavailable, saying which kind of pack is missing.
- **Context.** OQ-50. The organisation profile was withdrawn as an option by decision 61. In frozen crucible's catalogue only the stack packs (`go`, `java`, `python`, `rust`, `typescript`) and `database/postgres` (`sql`) declare languages; the testing, architecture, modelling and documentation packs declare none. So for `crucible/models/@default/workspaces/cspa-431`, the stack pack can come from the workspace and the testing pack always comes from the maker.
- **Consequences.**
  - The bundled catalogue's copy of `cspa-431` gains the field, marking its testing pack as asked by the agent (decision 64).
  - The parity test of R-69 gives frozen crucible and ordin the same pack choices, so the two bundles stay comparable.
  - 02 — Agent Model, Resolver and Compiler, Checker; 05 — agent catalogue row; 10 — OQ-50 and OQ-52 settled; 11 — *Add* shows the pick, with **Change**.
- **Alternatives considered.**
  - *The person picks on* **Add** *(c).* Not chosen: a technical question before the agent can be used (ADR-012).
  - *Only the maker fixes packs (a).* Not chosen alone: one agent could not serve both a Go team and a Python team.

### ADR-090: What ordin keeps of crucible's agent guarantees: an agent's own prohibitions, borrowed parts, memory rules, evaluations, overlays, part withdrawal and authorship

- **Status.** Decided by the user, January 2026 ("proceed with recommendations", decision 65), on an audit of every crucible capability against this design that found nine neither carried, replaced nor dropped. The mechanism is proposed. **Amends ADR-082** (borrowed parts, part withdrawal), **ADR-083** (the authoring tools), **ADR-084** (the pull request records authorship) **and ADR-088** (the port map). *Amended by ADR-091: §6's withdrawn list gets a file format of ordin's own, which ordin writes as well as reads.*
- **Decision.**
  - **1. An agent's own prohibitions are enforced by ordin.** crucible resolves each agent's tool policy (`crucible/pkg/types/plan.go`, `ResolvedToolPolicy`): the verbs it prohibits (`toolPermissions.workspaceOverrides.prohibited`, such as `git.push`), the flags its tool bindings forbid (such as `--no-verify`), the actions its capabilities send for a person's approval (`defaultToolPolicy.requiresHumanApproval`), and its MCP servers. crucible's Claude Code installer writes the prohibitions into `permissions.deny` of the agent folder's settings (`crucible/internal/cli/converters/claudecode/toolpolicy.go`). ordin starts Claude Code with `--restricted`, which ignores project settings files (C-26, C-61), so a settings file is not where the policy can live. In ordin:
    - the Resolver and Compiler hand the resolved tool policy to the install record;
    - the Rules Engine applies it on every tool call, through the ordin hook (ADR-045), on every harness: a prohibited verb or forbidden flag is denied, and an action the policy sends for approval reaches the Approval Service;
    - the agent's policy only narrows: it can never allow what the organisation's trust policy or the Rules Engine refuses;
    - an MCP server the agent names starts only through the trust policy (ADR-082), and any credential it needs is never written into the agent folder; where the credential comes from is fixed in `/spec`;
    - the parity test of R-69 compares, for every WorkspaceProfile in `crucible/models/@default/workspaces/`, the policy ordin enforces with the policy frozen crucible renders (R-77).
  - **2. Borrowed parts.** A part names other parts by identifier (`urn:ordin:<namespace>:…`). crucible lets a company's agents use parts from the shipped baseline, and from nowhere else (`crucible/pkg/types/artefact.go`, `ChildTenant`). In ordin a reference resolves in this order: the agent's own agent repository; the agent repositories the organisation profile names; the bundled catalogue. An agent repository the person added is never borrowed from by agents in other repositories. Two repositories that carry the same identifier are both refused for that part, and the conflict goes to Diagnostics. The lock file already records the repository, commit and digest of every part (ADR-082). Publishing keeps a borrowed part as a reference; the Checker fails the draft when the target repository's readers could not reach the source. How the APM format plug-in writes a borrowed part is fixed in `/spec`; until then a draft with borrowed parts cannot be published to an APM-format repository, and the check says so.
  - **3. Memory rules.** A `memory` part states what the agent may keep (`retains`), what it must not (`excludes`), for how long (`retention.maxAgeDays`) and where (`store.location`) (`crucible/models/@default/memories/engineering-default.yaml`). The Memory Service honours them for that agent: it keeps only the named classes, refuses the excluded ones, removes facts older than the limit, and uses no store but its own; a `store.location` other than `local` needs the trust policy.
  - **4. Evaluations and the publish gate.** Every bundled WorkspaceProfile names a `publishGate` (`urn:ordin:default:evals`). The deterministic expectations of an `evaluation` part become rehearsal situations with fixed expectations; model-judged ones become opinions (ADR-085). The Checker fails a draft whose publish gate is not met by its rehearsal.
  - **5. Overlays chosen like packs.** A WorkspaceProfile offers overlays with a default (`languageOverlays`, with `available` and `default`, in `crucible/models/@default/workspaces/cspa-431/workspace-profile.yaml`). The language overlay follows the person's interface language where the agent offers it (ADR-042), otherwise the agent's default; other overlays take the agent's default. Each shows on the agent's "Uses:" line with **Change** (ADR-089). Overlays that bind themselves to an agent from outside it, through `appliesTo` (the handoff overlays in `crucible/models/@default/overlays/handoff/`), are read and checked but not applied in the first release.
  - **6. Withdrawal and replacement per part.** An entry on an agent repository's withdrawn list can name a single part and the part that replaces it (crucible's `RevocationEntry`, `recommendedUpgrade`, `crucible/pkg/types/revocation.go`). Every agent that uses a withdrawn part stops, as a withdrawn agent does (ADR-082). Where a replacement is named, the Resolver and Compiler rebuild the agent with it; if the Checker passes, it is an update under the existing rules (A-16). A part marked `deprecated` keeps working in installed agents, and the agent maker does not reuse it in new drafts.
  - **7. Authorship in the review.** The Authoring Service records, for each field of a draft, whether the person wrote it, a model drafted it and the person changed it, or a model drafted it (crucible's `human`, `assisted` and `generated`, `crucible/pkg/types/attestation.go`). The pull request lists it, so reviewers on GitHub see what a model wrote.
  - **8. Journeys are dropped.** A `journey` part lists hand-offs between workspaces and is never installed. ordin's crews and workflows coordinate agents instead (ADR-064). Journey parts are read and checked, never offered. *Amended in revision 0.38.0 (ADR-092): a journey is read as a lifecycle, the stages of a piece of work handed between people, and ordin-format lifecycles are written as part drafts.*
  - **9. Agent identities are dropped.** `agent-identity` parts belong with crucible's CrewProfile, which is not ported (ADR-081). They are read and checked, never offered.
  - **Loose ends.**
    - The authoring tools leave out `draft_propose` (publishing is ordin's own action, ADR-084) and `attestation_get` (there is no registry attestation; item 7 replaces it).
    - A failed Foundry run is started again from the person's answers; there is no resume of a half-made set.
    - No check can be waived. crucible's steward waiver (`crucible/internal/registry/forge/assaygate.go`) has no counterpart; what IT can change is the trust policy.
    - The Installers' port includes `crucible/internal/cli/commands/activate.go` and `uninstall.go`, which restore merged files on removal, and the control-loop check of `crucible/internal/cli/converters/controlloop.go`.
    - An agent whose control loop needs a person present is never given a step that runs unattended (a triggered run, ADR-074); such a step waits for the person.
- **Context.** An audit of January 2026 classified 53 crucible capabilities: 21 carried, 13 replaced, 10 dropped on purpose and 9 with no place in this design. The two that matter most were an agent's own prohibitions, which `--restricted` would silently drop, and parts borrowed from another repository, without which a made agent reusing the library had no defined result.
- **Consequences.**
  - 02 — Rules Engine, Memory Service, Catalogue Service, Resolver and Compiler, Installers, Authoring Service; 08 — authoring tools; 10 — R-77, C-65; 11 — §5.2 *Uses:* line includes overlays, a withdrawn part, §5.26 authorship in the review.
  - The port grows by the policy hand-over and the memory rules; R-69.
- **Alternatives considered.**
  - *Write the deny list into the agent folder's settings, as crucible does.* Rejected: `--restricted` ignores it, and the ordin hook already sees every call.
  - *Copy borrowed parts into the target repository on publish.* Not chosen: two copies drift, and withdrawing the original would not reach the copy.
  - *Carry journeys as a seam check across workflows.* Not chosen for the first release: Ordin and saved workflows already pass work between agents (ADR-064, ADR-065); a check that every output has a consumer can follow if workflows need it.

### ADR-091: ordin curates the catalogue of an agent repository, brought from crucible: parts changed on their own, their lifecycle, withdrawal, vocabularies, writing guides, applicability and layout, every change a pull request

- **Status.** Decided by the user, January 2026 ("the backend registry is a github repo but its structure and agent structure and taxonomy of combinable parts is the management side ordin can do", recorded as G41 and OQ-54; then "whatever is needed from a design perspective should be brought from ../crucible"; decision 67). Settles OQ-54 as option (b): a catalogue view in ordin, each change published as a pull request. The mechanism is proposed. *Revision 0.37.1, user decision of January 2026 ("go with recommendations"): three of its proposals are decided. A changed part takes a minor version unless the change is a correction, and the person never chooses a version number. A repository in the APM format offers only **Withdraw…**. The rehearsal checks for each kind (the assay policy on evaluation parts) are read-only in ordin. From revision 0.37.4 (UI-D103): adding a word to a word list is a minor version, and retiring one a major version.* **Amends G41** (curating the catalogue is now designed), **ADR-082 and ADR-084** (a withdrawal can be prepared in ordin and sent for review, as well as committed on GitHub), **ADR-088** (the port map grows), **ADR-090 §6** (the withdrawn list gets a file format of ordin's own, and ordin writes it), and the Catalogue Service, the Authoring Service and the Checker (02 — Architecture). **Keeps ADR-084** (ordin adds no approval role), **ADR-085** (every check, number and file comes from fixed code) **and ADR-090** (no check can be waived). Source: an audit of crucible's curation features (`../crucible`, commit `e270904`) on January 2026, which found twelve capabilities ordin lacked; eleven are brought here and one is not carried.
- **Decision.**
  - **Who uses it, and where.** The catalogue view is a screen, **Parts of [display name]**, for anyone who can open a review in at least one agent repository (ADR-086). It opens from **Show its parts** on each such repository in Settings › Agent repositories, and from **Parts of an agent repository** on the agent library screen (11 — §5.27). A person who cannot open a review in any agent repository sees neither entry. A business user sees parts in plain words and never sees files, versions or identifiers, the rule of the agent draft (11 — §5.26); in a workspace that links code, the same screen also shows files, kinds, identifiers and versions. The bundled catalogue is read-only: it changes only with a release of ordin.
  - **Which repositories.** Every capability below applies to ordin-format agent repositories. In an APM-format repository the screen lists the agents and parts with **Used by** (item 8) and offers withdrawal only (item 3), because the withdrawn list is ordin's own file and the APM format has no status field, vocabularies or writing guides (ADR-087).
  - **1. One part on its own.** A **part draft** creates or changes one part outside any agent draft, as crucible's Forge does for one part (`crucible/internal/registry/forge/forge.go`, `CreateDraft`, `UpdateDraft`, `DeleteDraft`).
    - The kinds written this way are crucible's `ForgeAuthorableKinds` (`crucible/pkg/types/kinds.go`): Rule, Skill, PracticePack, Guideline, Memory and ControlLoop. An agent (WorkspaceProfile) stays with the agent maker (ADR-083). Parts of every other kind are read, checked and listed, and their content changes only through an agent draft or by hand in Git. *Amended in revision 0.38.0 (ADR-092): `Journey` is written this way too, as a lifecycle.*
    - The draft shows the kind's form, generated in fixed code from the Agent Model, with that kind's writing guide beside it (item 5). **Suggest wording** runs assisted drafting (`crucible/internal/registry/assist`) as a session on the person's harness (ADR-088); what it writes is recorded as model-drafted (ADR-090 §7). The Assayer's scored and narrative checks are opinions (ADR-085).
    - ordin writes the file in fixed code where crucible's Forge places it (`pathFor`, `crucible/internal/registry/forge/render.go`), on a branch of the person's clone, as for an agent draft (ADR-083).
    - **New part…** scaffolds, from a kind and a name, a part that passes the checks, with its identifier minted from the name, as `crucible init <kind> <name>` does (`crucible/internal/cli/commands/init.go`).
    - **Versions** follow crucible's cadence (`crucible/models/REGISTRY.md` § "Version cadence") and are computed in fixed code. A new part starts at 1.0.0. A change the person marks as a correction (the draft asks "Does this change what the part does?"; **No**) is a patch. Any other change to a part is a minor version, as crucible's Forge bumps a republished composition (`bumpMinor` and `nextCompositionVersion`, `crucible/internal/registry/forge/composition.go`). Retiring a vocabulary term (item 4) is a major version of that vocabulary part. Reading the cadence's minor step, written for new parts, as the step for a change in behaviour is ordin's choice; `/spec` confirms it.
    - **Every change is a pull request:** one per part draft, sent through the Push Service after an approval card (ADR-080, ADR-084).
  - **2. Lifecycle.**
    - **Stop using in new agents…** sets `metadata.status: deprecated` (`crucible/pkg/types/status.go`) in a pull request. As in crucible, a deprecated part resolves inside an agent with a warning and is refused as an agent of its own; installed agents keep working; the agent maker does not reuse it (ADR-090 §6). It is offered on a part of any kind, because it changes the status and not the content.
    - A new part can name the part it replaces in `metadata.supersedes` (`crucible/pkg/types/artefact.go`). The Checker fails the draft when the named part does not exist, as crucible's ingest does (`sync_unknown_supersedes_target`, `crucible/internal/registry/syncworker/syncworker.go`).
    - **History of this part** shows its lineage: its versions on the protected branch, read from the repository's history, and the chain of parts it replaces and that replace it.
  - **3. Withdrawal, written by ordin.** crucible has no writer for its withdrawn list: `RevocationEntry` (`crucible/pkg/types/revocation.go`) is kept in a PostgreSQL table that only a test fills, and no route, command or screen adds one. ordin defines the file and writes it.
    - **The file (proposed; `/spec` confirms the path and the schema).** One YAML file per agent repository, `withdrawn.yaml` at the repository root, outside `models/`, so that it is never read as a part (C-66). Its fields come from crucible's `RevocationList` and `RevocationEntry`:

      | Field | From crucible | Meaning |
      |---|---|---|
      | `listSchema` | `RevocationList.ListSchema` | The schema version: `ordin.withdrawn/v1` |
      | `entries` | `RevocationList.Entries` | Append-only, oldest first |
      | `entries[].part.kind`, `entries[].part.id` | `RevocationEntry.Artefact` (`ArtefactRef`) | The part, by kind and `metadata.id` |
      | `entries[].part.version`, `entries[].part.contentHash` | `ArtefactRef.Version`, `ArtefactRef.ContentHash` | One version and its digest from the Agent Model's canonical form; when both are absent, every version is withdrawn |
      | `entries[].reason` | `RevocationEntry.Reason` | Why, in the person's words; required |
      | `entries[].withdrawnAt` | `RevocationEntry.WithdrawnAt` | When ordin wrote the entry, in RFC 3339 |
      | `entries[].replacement.id`, `entries[].replacement.version` | `RevocationEntry.RecommendedUpgrade` | The part that replaces it; optional |

      Not carried: the signing fields of `ArtefactRef` and `RevocationList.PublishedAt`, because there is no registry signature (ADR-084, R-71). The merge commit records when the entry was published, and by whom.
    - **Append-only.** The Checker fails a change that edits or removes an existing entry, or that adds a second entry for a version already withdrawn (crucible's store refuses the same, `ErrAlreadyRevoked`, `crucible/internal/registry/datastore/store.go`). The Catalogue Service keeps every entry it has read: an entry later removed by hand stays in force, and the removal is reported to Diagnostics. A withdrawn part comes back only as a new part that names it in `metadata.supersedes`.
    - **Withdraw…** shows **Used by** first (item 8): the agents that will stop, and those that would move to the replacement. The person writes the reason and, optionally, picks the replacement. The Checker passes the entry only when the part exists in the repository and is not already withdrawn, the replacement resolves (the same kind, neither deprecated nor withdrawn, and reachable by every agent that uses the withdrawn part under the order of ADR-090 §2), and every agent that uses the part rebuilds with the replacement and passes the Checker. Approving sends a pull request that appends one entry. *Amended in revision 0.37.4 (user decision of January 2026, UI-D99): the check covers only the agents that move. An agent that does not rebuild and pass with the replacement is listed under* Stop *on the Withdraw screen before anything is sent, and stops when the withdrawal is published; the withdrawal is never blocked by it (§5.27).*
    - Once merged, the effects of ADR-090 §6 follow on every computer that reads the repository.
    - **A list that cannot be read fails closed.** The Catalogue Service keeps the last list it read, offers no update from that repository until the list reads again, and reports the failure to Diagnostics (R-78).
  - **4. Vocabularies.** The eight axes are a closed set in crucible's code (`VocabularyAxis`, `crucible/pkg/types/vocabulary_set.go`): pack-class, category, audience-role, rule-subvariant, artefact-status, memory-content-class, loop-strategy and runtime-capability. Their terms are data, in each repository's `vocabulary` parts. The view adds a term (code, label, description) or retires one (removes it from the list), each as a pull request. The checks fail closed, as crucible's ingest does (`sync_unknown_vocabulary_term`, `crucible/internal/registry/authoringcheck/stageone.go`; `sync_duplicate_vocabulary_axis`):
    - retiring a term that a part in the same repository still cites fails, and the check names those parts;
    - a part in another agent repository ordin reads that cites the same term is a warning, named on the card and in the pull request, because that repository keeps its own vocabulary;
    - the status terms `draft`, `approved` and `deprecated` cannot be retired, because ordin's code gives them meaning (`crucible/pkg/types/status.go`).
  - **5. Writing guides and rehearsal policy.** A repository holds one Guideline part per kind (`sync_duplicate_guideline_target_kind`, `crucible/pkg/types/guideline.go`), changed through a part draft like any kind of item 1. Its `authorGuidance` is shown beside that kind's form; its `rubric` feeds the Assayer's model-scored check, which is an opinion (ADR-085). The rehearsal policy for a kind is the `assay` block of an evaluation part (`AssayCheck`, `crucible/pkg/types/assaypolicy.go`). The view shows it on the kind's page and, because Evaluation is not among `ForgeAuthorableKinds`, it changes by hand in Git. In ordin only a deterministic check's `blocks` can fail a draft; a scored check's `threshold` labels the opinion and fails nothing (ADR-085).
  - **6. Applicability.** On a part draft for a working practice or an overlay, `appliesTo.workspaces` is a field of the form. The agents' side (`selectablePracticePacks`, `requiredPracticePacks`, and `governanceMetadata.<axis>Overlays` with `available` and `default`, as in `crucible/models/@default/workspaces/cspa-431/workspace-profile.yaml`) belongs to the WorkspaceProfile and changes through an agent draft opened from the repository (ADR-087, *Import stays*). The view's **Fits** panel shows both sides for each practice and overlay, and the Checker runs crucible's checks on every change (`sync_pack_not_applicable`, `sync_overlay_not_applicable`, `sync_applies_to_unsatisfiable`, `sync_applies_to_unbound`, `crucible/internal/registry/authoringcheck/authoringcheck.go`; `crucible/models/REGISTRY.md` § "Applicability (normative)").
  - **7. Layout and kinds.** **How this repository is laid out** lists the 25 kinds, what each is for and how many parts of each the repository holds. In a workspace that links code it adds each kind's directory (`KindDirectories`, `crucible/pkg/types/kinds.go`), the identifier pattern `urn:ordin:<publisher>:<kind-dir>[:<category>]:<slug>` with the categories the repository uses (`crucible/models/REGISTRY.md` § "Canonical URN grammar"), and each kind directory's `README.md` authoring contract. The view reads the layout; it moves nothing. New parts are placed by item 1.
  - **8. Used by.** For any part: the agents and parts that use it, from the reverse closure over the dependency edges in the local index, across every agent repository ordin reads (crucible's `GetReverseDependencyClosure`, `crucible/internal/registry/datastore/store.go`, rewritten on SQLite). It is shown on every part, and first, before **Change…**, **Stop using in new agents…** and **Withdraw…**; the pull request lists it.
  - **9. Health.** A report per repository, computed in fixed code from the local index after each fetch. It decides nothing and blocks nothing. It lists:
    - parts no agent uses;
    - parts of one kind that look alike, found by the search of `similarityAdvisories` (`crucible/internal/registry/forge/similarity.go`) run across the whole repository instead of one draft;
    - identifiers that two agent repositories both carry (ADR-090 §2);
    - references to borrowed parts that no longer resolve;
    - parts still used while deprecated or withdrawn;
    - vocabulary terms no part cites.

    Each line opens its part.
  - **10. The whole repository is checked before publishing.** Every part draft, withdrawal and vocabulary change passes the Checker over the repository's tree with the change, all or nothing as crucible's ingest is: the partial-tree checks of `CrossCheckPartial` (`crucible/internal/registry/authoringcheck/authoringcheck.go`) and the reference checks of `CrossCheckReferences` (`crucible/internal/registry/authoringcheck/references.go`), given the repositories this one borrows from (ADR-090 §2) as the catalogue to resolve against. Every agent that uses the changed part is rebuilt and checked, including the conflict detection of ADR-088. **Publish…** waits until it passes.
  - **11. Roles are GitHub's.** crucible's roles (`crucible/pkg/types/tenancy.go`) map onto GitHub, and ordin adds no approval role of its own (ADR-084):

    | crucible | In ordin |
    |---|---|
    | Any member of the tenant writes a draft | Anyone who can open a review in the repository |
    | `RolePublisher` (team `stewards`) approves or rejects | The required reviewers, from the repository's `CODEOWNERS` file and branch protection |
    | `RoleGuidelineAuthor` (team `administrators`), the only one who may draft a Guideline (`guidelineKindGate`) | `CODEOWNERS` entries on the repository's `guidelines` directory |
    | `RoleTenantAdmin` | The GitHub repository administrator |

    crucible refuses a steward's approval of their own draft (`ErrSelfApproval`, `crucible/internal/registry/forge/forge.go`); in ordin that refusal and the separation of duties come from branch protection, because GitHub does not let the author of a pull request approve it. The approval card names who reviews, from `CODEOWNERS` where ordin can read it (C-67); where the repository requires no review, the card says so (R-79). No check can be waived (ADR-090): crucible's steward waiver (`waiveBlocks`, `crucible/internal/registry/forge/assaygate.go`) has no counterpart.
  - **Not carried.** Demand signals drawn from requirements the library does not cover, and counts of how often each part is used: crucible has neither, so there is nothing to bring, and adding them would be a new feature outside this decision.
  - **Port map additions** (extending ADR-088's table):

    | Component | Carries from crucible | Kind of work |
    |---|---|---|
    | Authoring Service | The single-part draft of `crucible/internal/registry/forge` (`forge.go`, `render.go`, the version bump of `composition.go`, `similarity.go`); the scaffold of `crucible/internal/cli/commands/init.go` | Port; `Approve` (`approve.go`), `Propose`, `Reject` and the waiver are replaced by the pull request |
    | Catalogue Service | `GetReverseDependencyClosure` of `crucible/internal/registry/datastore` | Rewrite on SQLite |
    | Checker | `CrossCheckPartial` and `CrossCheckReferences` of `crucible/internal/registry/authoringcheck`, and `crucible/pkg/types/vocabulary_set.go` | Port, within the `authoringcheck` port of ADR-088 |
    | Catalogue Service, Authoring Service, Checker | The withdrawn list's reader, writer and append-only check; the health report | New code: crucible has no writer for the list and no report |
- **Context.** G41 records that ordin curates the registry as well as reading it, and OQ-54 asked how. The audit found that ordin carried crucible's Forge, Assayer and assisted drafting only inside agent drafts; that crucible's withdrawn list had a reader but no writer; and that crucible keeps its vocabularies, writing guides, applicability, layout and reverse dependencies as data and checks that ordin already ports or can port. crucible's server, PostgreSQL store, tenancy and signing are not ported (ADR-081), so each capability is adapted to a Git repository on GitHub, where publication is a merged pull request (ADR-084).
- **Consequences.**
  - 00 — G41, the fourth idea, glossary, decision 67; 02 — Agent Model, Catalogue Service, Checker, Authoring Service; 03 — sequences *Change a part in an agent repository* and *Withdraw a part, with its replacement*; 04 — Part, Part draft, Withdrawn list entry, Vocabulary term; 10 — OQ-54 settled, A-42, R-78 to R-80, C-66, C-67, a note on R-69; 11 — §3, §4, §5.15, §5.27, §12 item 43, UX-A60; 12 — UI-D98.
  - The port grows by the single-part draft, the reverse closure and the scaffold, and by new code for the withdrawn list and the health report (R-69).
  - A merged change to a shared part reaches every agent that uses it (R-80).
- **Alternatives considered.**
  - *(a) Parts change only through agent drafts.* Not chosen: a part that many agents share would change only as a side effect of one agent, and withdrawal, vocabularies and writing guides only by hand in Git.
  - *(c) Curation stays in the repository, outside ordin.* Not chosen: a change would meet ordin's checks only after it was merged, when the whole commit is refused for everyone who reads the repository.
  - *An approval role inside ordin, as crucible's steward.* Rejected by the user in ADR-084.
  - *Withdraw a part with a status term.* Not chosen: crucible keeps `revoked` out of the status vocabulary on purpose (`crucible/models/@default/vocabulary/artefact-status.yaml`), because a list reaches installed versions that a change to the part's own file cannot.
  - *Every kind written as a single part.* Not chosen in this step: crucible adds a kind to `forgeAuthorableKinds` only with its file convention, and agents stay with the agent maker (ADR-083).

### ADR-092: Work is handed over between people through places they already share; a lifecycle says who comes next and what the work must hold

- *Amended in revision 0.45.0 by ADR-099: a lifecycle may branch, run stages side by side and send work back to any earlier stage; a straight lifecycle reads as before.*
- *Amended in revision 0.54.0 by ADR-106: a Jira project is a third handover place, after Git and SharePoint; the handover travels on one issue, its record is the issue's comments, and the receiver is chosen among the people who can be assigned issues there. Item 3's Git and SharePoint places read as before.*
- **Status.** Decided by the user, January 2026 ("when a business user who is part of a lifecycle like SDLC completes a task, they might need to hand off to another user who can now continue using the user output as their input. the concept of handoffs is defined in ../crucible metadata model but I feel is not a first class citizen in ordin", then "go with recommendations"; decision 68). The mechanism is proposed. **Amends ADR-090 item 8**: journeys are no longer dropped; they are read as lifecycles. **Amends ADR-091 item 1**: a lifecycle is a kind written as a part draft. **Keeps ADR-025 and ADR-073**: each installation serves one person, and no installation takes a request from another.
- **Decision.**
  - **A handover is a first-class item.** When a person finishes their part of a piece of work, they hand it over to the next person, who continues from it. A handover has a sender, one receiver, a title, the results handed over (documents where they live), a **handover note** and a state. The receiver starts their own work from it in their own ordin.
  - **The handover note.** Ordin drafts it from the workspace, and the person changes it in the full-pane editor (UX-A59). It has four parts, in this order: **What's done**, **Decisions made**, **Open questions** and **What's needed next**. The draft is marked "Drafted by Ordin" until the person changes it, as other drafted text is (ADR-090).
  - **What crucible has, and what ordin adds.** crucible models a handover as a typed crossing: a producing workspace and a consuming workspace name the same `OutputContract` in a `HandoffOverlay` (`crucible/pkg/types/artefact.go`, `HandoffOverlaySpec`, `Seam`), and a `Journey` lists the crossings (`JourneySpec{Seams, Trigger}`). crucible checks that every crossing has a producer and a consumer (`crossCheckJourneySeams`, `crucible/internal/registry/authoringcheck/authoringcheck.go`) and never runs one: it has no receiver, no note, no state, no answer and no notice. ordin keeps crucible's declarations and adds the person-to-person part.
  - **1. Lifecycles.** A **lifecycle** is a part in an agent repository that names the stages of a piece of work in order. Each stage has a name, the role that does it in plain words, the agent the stage uses (optional), what the stage hands over (an `OutputContract`, optional), where its handovers go (a handover place), and how many days the receiver has to answer (optional).
    - An ordin-format lifecycle is the `Journey` kind with a `stages` list (08 — Interfaces, *Lifecycle part*).
    - A crucible journey is read as a lifecycle with nothing lost: each seam becomes the boundary between the stage of the workspace that produces its contract and the stage of the workspace that consumes it; the stage order comes from the producer-to-consumer graph, as crucible orders it (`crucible/models/@default/journeys/README.md`); a stage's name and role come from the agent's SystemRole; its handover place comes from the overlay's `publishedTo`. A crucible journey whose seams do not join into one chain is read, listed under Health (ADR-091 item 9) and not offered. *Amended in revision 0.45.0 (ADR-099 item 7): seams that branch or join are read as a lifecycle that branches; only a loop is not offered.*
    - Lifecycles are written as part drafts and published through a pull request (ADR-091 item 1). The kinds written as a part draft gain `Journey`.
    - A workspace can follow one stage of one lifecycle. It does when it was started from a handover, when the person chooses the stage among the workspace's actions (**Follow a stage…**; *amended in revision 0.38.1: workspaces have no settings screen*), or when the agent a stage names is added to it and the person agrees. The workspace's header then says the lifecycle, the stage and who comes next.
  - **2. Handing over.** **Hand over…** is offered by Ordin when work in a workspace that follows a stage is finished, and is always an action of the workspace and of each result document. It opens a **handover draft**, a screen:
    - **To:** one person. In a stage that has handed over before, the person it last went to is suggested. People are found in the company directory through the Microsoft sign-in, or among the people who can review in the repository when the handover place is a Git repository.
    - **What's handed over:** the results, chosen from the workspace's documents, each where it lives.
    - **The handover note**, as above.
    - **Checked by ordin**, in fixed code:
      - each result is in a place the receiver can read: a document still only on this computer fails, with **Put it in [handover place]…**;
      - when the stage names an `OutputContract`, the result holds every required section: the headings of a Markdown, Word or Confluence document are matched against the contract's sections, as crucible matches a dossier (`crucible/internal/registry/dossier/dossier.go`); a missing section fails, named in plain words, with **Ask Ordin to add it**;
      - the receiver is a person, not a group, and has an address ordin can reach.

      **Hand over…** waits until every check passes. Nothing can be waived (ADR-090). A handover outside any lifecycle has no contract check.
    - **Hand over…** opens an approval card (UX-A53): *Where:* the handover place; *Affects:* the receiver, by name, marked when outside the company (§5.22); *Changes:* "[name] is asked to take this over. They see the note and the results; nothing else in this workspace." The card's first button is **See what will be sent**. *Amended in revision 0.50.0 (ADR-102): the draft also offers **Also tell them in Slack**, and the card lists every Slack post it causes — the direct message and the stage channel's post — each "as you".*
  - **3. The handover place.** A handover travels through a place both people can already read and write. ordin runs no service between people.
    - **A Git repository, first.** ordin writes the note and the handover's record on a branch and opens a pull request that asks the receiver to review it, labelled `ordin-handover`. Results that live in the same repository are part of the pull request; others are linked from the note. Accepting approves and merges the pull request; sending back asks for changes, with the reason. GitHub's own review request is the notice when the receiver does not use ordin.
    - **A SharePoint folder, second.** ordin writes a folder per handover, named with the date and the title, holding the note and the handover's record. Results stay where they live and are linked. Each ordin adds its answers to the record and never rewrites what the other wrote (08 — Interfaces, *Handover record*; R-83).
    - **The lifecycle names the place** for each stage, from the overlay's `publishedTo`. A handover outside any lifecycle uses a place the workspace links, chosen on the draft.
    - Confluence as a handover place is OQ-56.
    - *Amended in revision 0.54.0 (ADR-106): **a Jira project, third**, on the company's Jira Cloud site.*
  - **4. Receiving.** The Handover Service finds handovers addressed to the signed-in person:
    - in Git, through GitHub's list of review requests for the person, filtered by the label, across every repository they can read, so no place needs linking first;
    - in SharePoint, in the handover places of every lifecycle stage the person's workspaces follow and of every place their workspaces link;
    - at start, when a workspace opens, and every 15 minutes (A-43).

    A handover found appears in *Waiting for you* on Home and in *Needs you*, as "Handed to you: [title] — from [name]", with a notice outside the window (§7). A handover that arrives only by email, in a place ordin does not follow, is OQ-55. *Amended in revision 0.50.0 (ADR-102): OQ-55 is settled with (a) — every Slack notice and handover email carries a link that opens the handover in the receiver's ordin and follows its place; also SharePoint places followed from such a link are read.*
  - **5. The handover screen.** It shows the sender, the lifecycle and stage when there is one, the note, the results (each opens where it lives), and the checks the sender's ordin passed. Its actions:
    - **Accept and start** records the acceptance, and opens a new workspace, or one the person chooses, that follows the next stage, links the results as inputs and adds the stage's agent, asking before anything is installed (ADR-012). Ordin then offers the first step in words, using the note.
    - **Send back…** asks for the reason, required, in the person's words, and opens an approval card, because it reaches another person.
    - **Ask Ordin about this.**

    The note and the results come from another person, so ordin treats them as data. Agents read them as inputs; nothing in them is followed as an instruction to ordin, and a line that tries is marked, as on a Telegram message (§5.25; R-81).
  - **6. States.** *Draft — only you can see it* → *Waiting for [name]* → *Seen by [name]* → *Accepted by [name]* or *Sent back by [name]*. A handover sent back returns to its sender's *Waiting for you*, with the reason; **Change and hand over again…** opens the draft as its next round, and the record keeps every round. The sender may **Take it back…** until it is accepted. Every state change is journalled on both sides.
  - **7. No answer in time.** When a stage sets the days to answer and they pass, the sender's Home shows "No answer from [name] in [N] days", with **Remind [name]…**, which drafts an email the sender approves. Nothing is sent by itself. *Amended in revision 0.50.0 (ADR-102): when the stage names a Slack channel, **Remind [name]…** posts a reminder there as the sender, mentioning the receiver, after its card or under the stage's rule, in place of the email draft.*
  - **8. The lifecycle screen.** *Amended in revision 0.39.2: a lifecycle's definition stays a part in an agent repository, shared and reviewed, and never part of an agent; what a workspace does with it is the workspace's, shown as its own group in the tree (stage, handed to you, handed over), and the screen draws the stages as a path with "You are here" (11 — §5.28).* **Lifecycle: [name]** shows the stages in order, each with its role, its agent, what it hands over, and the handovers ordin can read in that stage's place, with their states. It shows only what is in places the person can read; it is not a shared board kept by anyone.
  - **9. Agents never hand over to a person.** Only the person sends a handover, through its card. A crew or workflow may prepare a handover draft as its last step and stop there for the person (crucible's `prohibited-in-crew-mode`, `crucible/pkg/types/workflow.go`). Crews keep their own hand-offs between agents (ADR-006).
  - **10. The ordin server.** The same handovers can be sent and received from the chat, with the approval card in the chat (ADR-073).
  - **Port map additions** (extending ADR-088's table):

    | Component | Carries from crucible | Kind of work |
    |---|---|---|
    | Agent Model | `JourneySpec`, `HandoffOverlaySpec`, `Seam`, `PublishTarget` and `HandoffGate` (`crucible/pkg/types/artefact.go`, `workflow.go`) | Port, with the ordin-format `stages` list added |
    | Checker | `crossCheckJourneySeams`; the section matching of `crucible/internal/registry/dossier` | Port |
    | Handover Service | Nothing: crucible never runs a handover | New code |
- **Context.** People in a lifecycle such as software delivery pass work to each other: a product manager's brief goes to an architect, whose design goes to an engineer. Each of them may use ordin, each on their own installation. crucible describes these crossings but leaves the pass itself to a line of prose in a skill (`/spec <topic>`), so the receiver learns of it by being told. Without a handover in ordin, the person copies links into an email, and nothing checks that the work holds what the next stage needs.
- **Consequences.**
  - 00 — G42, the third idea, glossary, decision 68, non-goals; 02 — Handover Service, Agent Model, Checker; 03 — sequences *Hand work over* and *Receive a handover*; 04 — Lifecycle, Handover, Handover place; 08 — lifecycle part and handover record; 10 — A-43, A-44, OQ-55 to OQ-57, C-68, C-69, R-81 to R-83; 11 — §5.28, UX-A61, §5.17 and §7; 12 — the prototype group "§5.28 Handing work over".
  - Each person still has their own ordin; what two people share is the place the handover lives in.
- **Alternatives considered.**
  - *(a) Ordin drafts a message to the next person.* Kept only as the email a receiver without ordin gets; on its own nothing is checked, tracked or answered.
  - *(c) Handovers through a work tracker such as Jira, GitHub Issues or Azure Boards.* Not chosen first: each tracker needs its own connector. A later carrier, as OQ-57 notes. *Taken up for Jira in revision 0.54.0 (ADR-106); GitHub Issues and Azure Boards stay later carriers.*
  - *(d) A shared team ordin that passes work between people.* Rejected: it breaks one person per installation (ADR-025, ADR-073) and the harness terms (ADR-044).

### ADR-093: Every agent shows what it is made of, and every part can be read

- **Status.** Decided by the user, January 2026 ("investigate if the design and prototype cater for displaying the agent's artefact taxonomy and provide inspection of artefact content", then "go with recommendations"; decision 69). The mechanism is proposed. **Amends ADR-091**: reading a part is open to everyone who can read the agent repository, not only to people who can send it a review. **Keeps ADR-085**: what is shown is computed in fixed code from the Agent Model and the lock file; no model writes it.
- **Decision.**
  - **1. What it's made of.** Every agent, in the agent library and under *Agents* in the tree, has **What it's made of**: its parts grouped by kind in plain words (§5.27 table), in this order: role, rules, working modes, abilities, tasks, reference material, tools, memory rules, way of working, working practices, language, writing style and hand-over, rehearsal checks, what a result must hold. Each part shows its plain name and its one-sentence *what it does*. A part borrowed from another agent repository says "From: [display name]" (ADR-090 §2). A withdrawn or not-for-new-agents part carries its state label. The list is the forward closure the Resolver computes when the agent is added (ADR-088), drawn from the local index, for library agents from the protected branch and for installed agents from their lock file.
  - **2. Read it.** Every part, of all 25 kinds, has **Read it**, which opens the part as a read-only page: its name, kind in plain words, what it does, and its text, drawn by kind from the Agent Model (a rule's statements, an ability's steps, a task's description and the abilities it uses, a tool's name and what it may do, the fields of memory rules, a working practice's guidance, a result's required sections). **Read it** never edits; **Change…** stays with the seven kinds a part draft writes (ADR-091 item 1, ADR-092). Text the page shows is data; ordin obeys none of it.
  - **3. For people who code.** In a workspace that links code, the same views add each part's kind, identifier and version, and the lock file's repository, commit and digest for an installed agent's parts; **Read it** adds **Show the file**, which opens the part's file read-only in the code editor (§5.11). An installed agent adds **Show the installed files**: its agent folder, as the Installers wrote it for the session's harness, read-only, with the lock file first.
  - **4. Both ways.** A part under **What it's made of** opens its page, and the page has **Used by** (ADR-091 item 8). Every agent named under **Used by** opens that agent's **What it's made of**.
  - **5. Who sees it.** Everyone who can read the agent's repository. **Parts of [repository]** stays for people who can send a review (UX-A60); a part page reached from an agent offers **Change…** only to them.
  - **6. The Package Inspector** reads an installed agent's folder and its lock file (ADR-085), not an `apm.yml`; the APM-era sources are read only through the APM format plug-in (ADR-087).
- **Context.** The investigation of January 2026 found that no screen shows one agent's parts, that a part's text could be read only by opening a part draft through **Change…**, for seven kinds, and only by people who curate a repository, and that the installed folder and lock file could not be inspected. A person who relies on an agent could not see what it is told to do.
- **Consequences.**
  - 00 — G43, decision 69; 02 — Package Inspector, Catalogue Service; 11 — §5.29, UX-A62, §5.2, §5.27, §5.15 fixed screens, checklist item 45, the `invokes` row; 12 — the prototype group "§5.29 What an agent is made of"; 10 — A-45.
- **Alternatives considered.**
  - *Part text only for curators, through* **Change…**. Not chosen: people who use an agent could not see what it is told to do.
  - *Show the compiled files to everyone.* Not chosen: files are technical detail, shown only in a workspace that links code (G10).

### ADR-094: ordin imports, checks and fixes agents and parts, and builds parts of every kind for the library

- **Status.** Decided by the user, January 2026 ("ordin should be able to import, inspect and fix parts and whole agents, and be able to build individual parts for the library", with "go with recommendations"; decision 70). The mechanism is proposed. **Amends ADR-087** (two read-only import plug-ins; import open to business users), **ADR-091 item 1** (part drafts for every kind a person can write) and **the §5.26 rule that business users never see import**. **Keeps ADR-085 and ADR-090**: files and checks come from fixed code, nothing is waived, and model-written text is marked.
- **Decision.**
  - **1. Import a whole agent.** **Import…** on the agent library, and **Import as an agent…** on a file or folder in a workspace that ordin recognises, read an agent into an **agent draft** (ADR-083) through a format plug-in's *read* (ADR-087):
    - the ordin format, from a folder, an archive, or an agent of a repository the person has not added;
    - the APM format, from a package (ADR-082);
    - **two read-only import plug-ins, new:** a Claude Code set-up (`CLAUDE.md`, `.claude/agents/`, `.claude/skills/*/SKILL.md`, `.claude/commands/`, and the MCP servers it lists) and a GitHub Copilot set-up (`.github/copilot-instructions.md`, `.github/prompts/`, and its agent and chat-mode files). Each maps what it reads to Agent Model kinds with a mapping report, and never guesses: what does not map is a failed check on the draft. They have no *write*: ordin publishes only in the ordin and APM formats.

    The draft's *where it came from* line names the source. Everyone can import; publishing still needs the right to open a review in an agent repository (ADR-084).
  - **2. Import one part.** **Import…** also takes a single file, and **Import as a part…** is on a file ordin recognises (a `SKILL.md`, a rule or instruction file, a command file, an MCP server entry). ordin proposes the kind in plain words ("This reads like an ability"), the person confirms or chooses another, and the file becomes a **part draft** (ADR-091) of that kind, in the repository the person chooses.
  - **3. Check.** Importing always runs the Checker on the result. **Check it** on any agent (on **What it's made of**, ADR-093) and on any part page runs the same checks on demand, read-only, against the version in use: for an installed agent, its lock file. The result is *Checked by ordin*, with the same heading line, pass or fail, label and icon as the agent draft, one line per finding in plain words, naming the part it concerns.
  - **4. Fix.** Every failed check offers its fix, in this order:
    - **Fix it**, a fix in fixed code, where the Checker knows the one right answer: a name turned into a valid identifier, a retired word replaced by the word that replaced it, a reference to a withdrawn part pointed at its replacement, a missing required field asked as one question, an unknown field left out after the person agrees;
    - **Ask Ordin to fix it**, where judgement is needed: the agent maker suggests text, marked "Suggested by a model" until the person changes it, and listed in the review as model-drafted (ADR-090);
    - **Change it yourself**, opening the field.

    A fix is always made on a draft. Fixing a library agent or part opens an agent draft or part draft with the fix applied, published by a pull request (ADR-084). Fixing an installed agent whose repository the person cannot write to makes **your own fixed copy**, tried on their own computer only (§5.26 *Try it*) and offered to the repository's maintainers with **Suggest the fix to [display name]…**, an approval card that opens a pull request from the person's fork where GitHub allows it.
  - **5. Build parts of every kind.** A part draft writes every kind a person can write: role, rule, working mode, ability, group of abilities, task, reference material, tool, memory rules, way of working, working practice, language, writing style, code workflow, hand-over, rehearsal checks, what a result must hold, writing guide and lifecycle. Not written as a part: the agent itself (the agent maker, ADR-083), words (the **Words** tab), and the kinds ordin reads but does not offer (crew, agent identity, brand, jurisdiction). ordin places each file where crucible's layout puts that kind (`KindDirectories`, `crucible/pkg/types/kinds.go`), and the Checker runs crucible's per-kind checks on it. A tool part must name a tool the organisation profile allows. Code workflow parts are offered only in a workspace that links code.
  - **6. Make a part with help.** **Make a part…**, beside **Make an agent…** on the agent library for people who can open a review in an agent repository, starts the agent maker for one part: it asks what the part is for, proposes its kind and drafts it one question at a time, reusing words from the repository. The result is a part draft.
  - **Port map additions** (extending ADR-088's table):

    | Component | Carries from crucible | Kind of work |
    |---|---|---|
    | Format Plug-ins | Nothing for the two import plug-ins: crucible's converters write Claude Code, Copilot (VS Code) and AGENTS.md layouts (`crucible/internal/cli/converters`), which show the layouts the import plug-ins read | New code, read only |
    | Authoring Service | `pathFor` for every kind written | Port, widened |
    | Checker | The per-kind checks of `crucible/internal/registry/authoringcheck` | Port |
- **Context.** ADR-087 imported only APM packages, only in a workspace that links code. ADR-091 wrote six kinds of part. People already hold agents and instructions in Claude Code and Copilot set-ups, and a company's library needs parts of every kind. Checks ran only on drafts, so a problem in an installed agent could be seen only when it failed.
- **Consequences.**
  - 00 — G44, decision 70; 02 — Format Plug-ins, Authoring Service, Checker; 03 — sequence *Import an agent or a part, check it and fix it*; 08 — the two import layouts; 10 — R-84, C-70; 11 — §5.30, UX-A63, §5.26, §5.27; 12 — the prototype group "§5.30 Importing, checking and fixing".
- **Alternatives considered.**
  - *Write Claude Code and Copilot layouts too.* Not chosen: ordin installs for those harnesses through its Installers already (ADR-085); publishing in them would make a third and fourth library format.
  - *Fix installed agents in place.* Rejected: an installed agent is pinned by its lock file, and changing it outside a draft would bypass the checks and the review.

### ADR-095: A workspace starts from a lifecycle stage in one step; lifecycles have their own draft; inspection shows only what is needed

- **Status.** Decided by the user, January 2026 ("is there an easy way to setup a workspace for a stage in a lifecycle?", "inspecting artefacts in a registry it does not follow the UX principle of showing only what is needed, and allowing to show more on demand", "do lifecycles include an editor and publisher in ordin?", then "go with recommendations, create scenes and commit"; decision 71). The mechanism is proposed. **Amends ADR-092 items 1 and 8**, **ADR-091 item 1** for the `Journey` kind, and **ADR-093 item 1** for what the screen shows first.
- **Decision.**
  - **1. A workspace for a stage, in one step.** **Start a workspace for this stage…** (lifecycle screen), **For a stage of a lifecycle** (New workspace) and a request to Ordin open one screen; **Start** creates the workspace, makes it follow the stage, links the stage's handover place for receiving (ADR-092 item 4) and offers the stage's agent, asking before anything is installed (ADR-012). The stage's required result sections, read from its `OutputContract`, are shown as **What this stage hands on** and ticked by the Checker's handover check (ADR-092 item 2) as the result takes shape.
  - **2. The lifecycle draft.** A `Journey` part is written in its own draft whose form is the `stages` list of the ordin format (08 — Interfaces, *Lifecycle part*), edited as a path of stages with a form each, not as lines of text. The Checker adds, for a lifecycle: every stage named with a role; each agent and each `OutputContract` resolves in the repository or a repository it borrows from (ADR-090 §2); each handover place is reachable with the person's sign-in; and, where the next stage's agent declares the contract it consumes, the producing stage hands on that contract — crucible's seam check (`crossCheckJourneySeams`) applied to the ordin-format stages. Publishing is a pull request (ADR-084); the card names the workspaces on this computer that follow the lifecycle and any open handover in it. **Make a lifecycle…** runs the agent maker for one lifecycle (ADR-094 item 6). *Amended in revision 0.41.0: the review on GitHub has the part draft's review round (ADR-091): changes asked for reopen the draft for editing, and the change goes to the same review through a new approval card.*
  - **3. Show what is needed, and more on demand.** Screens that inspect agents and parts show name, kind and state first; summaries, **Used by**, history, fits, technical details and curator actions are one choice away (UX-A64). Nothing is removed: every fact the screens showed is still on them, folded.
- **Context.** A person could follow a stage only by changing an existing workspace or accepting a handover; a lifecycle could be written only through the generic part draft, as text lines with no stage-aware check; and the inspection screens of revisions 0.37.0 and 0.39.0 showed every fact about every part at once, against P0.
- **Consequences.** 00 — decision 71, G42 and G43 notes; 03 — sequence *Start a workspace for a stage*; 11 — §5.27, §5.28, §5.29, UX-A64, UX-A65, checklist items 47 and 48; 12 — scenes in "§5.27", "§5.28" and "§5.29".
- **Alternatives considered.**
  - *Workspace templates of their own.* Not chosen: the stage already carries everything a workspace needs (agent, place, what it hands on), so a second definition would drift from it.
  - *A lifecycle written as YAML in a code editor.* Kept only under **Details** in a workspace that links code; business users, who own lifecycles, never see it.

### ADR-096: ordin is a modular monolith whose screens use one Service API, and a desktop ordin is the screen for the person's own ordin servers through a REST API with Server-Sent Events

- **Status.** Decided by the user, January 2026 ("Ordin should be built as a modular monolith", "Ordin local instance should be able to connect and offer an UI for remote headless instances", "Ordin should have an UI module that consumes services from the headless backend in such a way that the backend can be deployed remotely and connected to a local instance"; on the transport, "I would connect using a fast rest api" and, for notifications, Server-Sent Events; then "proceed with recommendations"; decision 72). The mechanism is proposed. **Amends ADR-073**: the server gains a second way in, the Remote API, beside the Channel Gateway; its rule of no port open on the network becomes a default that the organisation profile can lift for the company network only (item 4); its rejected alternative *a web interface on the server* stays rejected for browsers (item 9). **Amends ADR-072** for the server only, in the same way. **Keeps ADR-028** (screens are drawn from the fixed block catalogue), **ADR-044** (one person per installation, each signed in to their own harness), **ADR-045** and decision 10 (approvals), and **OQ-39** (the desktop and the server keep separate workspaces, journals and memory). *Amended in revision 0.43.0 (ADR-097, ADR-098): an agent-profile server refuses desktop pairing, and a desktop reaches it with the person's company sign-in by role; every module that owns tables also has a PostgreSQL storage implementation, for the scaled shape.*
- **Decision.**
  - **1. A modular monolith.** ordin is one Cargo workspace, built into two programs: the **desktop** (Tauri, the screens module and the core) and the **ordin server** (the core, the Channel Gateway and the Remote API, without Tauri). Inside the core:
    - **Each module is a crate.** A module is a component of 02 — Architecture, or a small group of components that always change together; the grouping is fixed in `/spec`. A module's crate exposes one service interface (Rust traits and data types) and keeps everything else private to the crate.
    - **A module owns its data.** The tables a module's *Owns* list names are written and read only by that module's crate. Another module asks through the owning module's service interface and never reads its tables. The journal stays one SQLite file (ADR-010); ownership is by table.
    - **Dependencies point one way.** A module depends only on the modules its *Depends on* list names. A test in the build reads the crate graph (`cargo metadata`) and fails on any dependency that the list does not name, and on any cycle.
    - **Everything runs in one process.** Modules call each other in-process. No module is a separate service, and nothing crosses a machine between modules (05 — *Message broker*: none).
  - **2. The Service API.** The Command Bridge's command and event catalogue becomes the **Service API**: a versioned set of commands, each a request with one reply, and events, each a record in one ordered stream, defined once as Rust data types that serialise to JSON. The **screens module** (the WebView code and the Block Renderer's drawing side) depends only on the Service API. It holds one **backend handle** per backend it shows, and never knows whether the backend is in the same process or on another machine. The Service API carries no markup, script or style (ADR-028).
    - **Two transports.** On the desktop's own core, the Service API travels over Tauri's IPC, under Tauri capabilities, as the Command Bridge does today; the desktop opens no port. To an ordin server, it travels over the server's Remote API (item 3), always through the desktop's own core: the WebView makes no network call and holds no server key (01 — trust boundary 1).
    - **Events carry a sequence number.** Every event a backend emits carries that backend's journal sequence number, so the screens can tell a missing event from a late one on either transport.
    - **Versions.** The Service API has a major and a minor version. A desktop works with a server of the same major version and the same or the previous minor version. Any other pairing is refused with one sentence naming which side to update (11 — UX guidelines §6).
    - *Amended in revision 0.49.0 (`/spec`):* one command streams its results outside the journalled event stream: `dictation.start` returns its recognised text on the command's own IPC channel, on the desktop only and never over the Remote API, so text the person has not sent is never journalled (ADR-043).
  - **3. The Remote API.** The ordin server serves the Service API over HTTPS:
    - **Commands** are REST calls over HTTP/2 with TLS 1.3, under one path prefix carrying the major version (`/v1/`); the resource paths are fixed in `/spec` from the Service API's command list. Bodies are JSON. A more compact encoding is added only if A-46's targets are missed with JSON.
    - **Events** are a Server-Sent Events stream, `GET /v1/events`, held open by the desktop. Each event's `id:` is its sequence number. A desktop reconnecting sends the last number it received as `Last-Event-ID`, and the server sends every later event in order. Where that number is older than the journal still keeps (after the weekly compaction of ADR-076), the server sends one `reset` event and the desktop reloads the state it shows through ordinary commands.
    - **Heartbeat.** The server sends an SSE comment line every 20 seconds, so a company proxy does not close an idle stream; a desktop that hears nothing for 45 seconds treats the link as dropped and reconnects, with backoff from 1 second to 60 seconds, never giving up while it is open.
    - **One stream per desktop.** A desktop opens one event stream per server. Commands share the stream's HTTP/2 connection.
  - **4. Where the server listens.** Set by the server's setup command (ADR-073), within the organisation profile:
    - **Default: one private address.** *Amended in revision 0.47.0 (decision 78):* the server listens on one address and one port above 1024, chosen at setup, and the desktop reaches it directly over HTTPS. The desktop opens no SSH tunnel and holds no SSH key. SSH stays for the server owner's setup command and the harness sign-in (ADR-073). The first version of this item made `127.0.0.1` the default, reached through a tunnel.
    - **Certificate.** The server uses a TLS certificate and key the server owner gives the setup command (the company's own certificate authority; the desktop trusts it through the computer's trusted certificates, A-39). The setup command accepts only loopback and private addresses (RFC 1918 for IPv4, RFC 4193 for IPv6) and refuses any other. The profile can forbid listening, which switches the Remote API off, and can also require client certificates; the desktop then presents the certificate the profile names from the computer's certificate store.
    - **Never the internet.** A public address, a public certificate and the protections an internet service needs are outside this design. The Remote API serves ordin desktops only: no browser screens are served (item 9).
  - **5. Pairing a desktop.** A desktop reaches a server only once paired with it, as a chat account is (ADR-073):
    - The setup command's **pair a desktop** step shows a one-time code, valid for 10 minutes; after 5 wrong codes in 10 minutes pairing stops until the setup command starts it again.
    - On the desktop, **Settings › Servers › Connect to a server…** asks for the server's address and the code. Where the server's certificate is not one the computer already trusts, the setup command's pair step prints one **pairing line** holding the address, the certificate's fingerprint and the code; the person pastes the line instead of typing the address and code, and the desktop pins the fingerprint from then on (OQ-61, decision 78). The server answers with a **desktop key**: 32 random bytes, kept on the desktop in the keychain and on the server only as a hash. Every Remote API call carries it as a bearer token over TLS.
    - A server may pair several desktops of its one person (a laptop and an office computer). Each is listed by the computer's name, when it was paired and when it was last connected. **Disconnect this computer** on the desktop, or the setup command, ends one pairing at once and closes its stream.
    - The server accepts at most 10 failed key checks a minute from one address, then refuses that address for 10 minutes and records it in the journal without the key.
  - **6. The desktop keeps its own backend.** A desktop always runs its own core, as today, and shows its paired servers beside it. Without a server, nothing changes for the person. A screen-only desktop, with no core of its own, is not offered in this step.
  - **7. What the desktop shows of a server.** Each paired server appears in the side rail's tree under its own name, with its workspaces, agents, workflows and documents as the server holds them; it has its own conversation with Ordin, because the server's Ordin is another session in another installation. *Needs you* and Home list the server's items with the server's name. An approval card from a server opens on the desktop as any approval card does; it is also sent to the server's paired chats, and the first answer decides (ADR-076). The Approval Service on the server checks a press from the desktop as it checks a chat button: open card, single use, from a paired desktop.
    - **Separate, as OQ-39 decided.** The desktop shows the server's work live and copies none of it into its own workspaces. **Save a copy on this computer** saves a file the person chooses, and the copy is the desktop's own from then on.
    - **What works on a server's workspaces from the desktop:** every business screen that the Service API carries — the conversation, forms, progress, approvals, results, documents, workflows, reports and dashboards, knowledge pages, email drafts and memory pages — with hand editing as on the desktop (ADR-048). Dictation works, because the desktop turns speech into text before it is sent.
    - **What does not:** folders on this computer cannot be linked to a server's workspace; a server's document opens in the person's own app only as a copy saved on this computer, which ordin says before opening it; ArtCraft windows open only for files on this computer. Engineering mode's screens (editor, change review, terminal) on a server's workspaces come later (OQ-60).
  - **8. When the link drops.** The server keeps working: runs, approvals through the chats and triggers go on. The desktop marks the server **Not connected** in the tree and in its workspaces' screens, keeps the last state it received, read-only, and reconnects as item 3 says. On reconnecting it catches up through `Last-Event-ID`, and *Needs you* is complete again.
  - **9. A web interface in a browser stays out of scope.** The Remote API is consumed only by ordin desktops through their own core. ADR-073's reasons hold for a browser: it would need a public address, its own sign-in and protections against cross-site requests.
- **Context.** ADR-073 gave the ordin server no screen: the person reaches it through a chat channel, and the Command Bridge was bound to Tauri's IPC. ADR-081 already made ordin one Rust build, and every component already lists what it owns and depends on, but nothing kept those lists true in the code. The user wants the screens separate from the backend so that a backend can run on a server and be shown by the person's desktop, and chose a REST API with Server-Sent Events as the way the two talk. ADR-072 already allows local channels on loopback. *Amended in revision 0.47.0: the Remote API is not a local channel, so it listens on a private address.*
- **Consequences.**
  - The Command Bridge becomes the Service API on Tauri's IPC; the Remote API on the server and the Server Connector on the desktop are new components (02).
  - A new trust boundary between the desktop and the server (01, 07), with a desktop key that is as binding as the paired chat account (R-85), and a server reachable on the company network where the profile allows it (R-86).
  - The build gains a check of the crate graph against the *Depends on* lists.
  - 00 — decision 72, G45, G35 note, non-goals; 01 — the person's ordin server, trust boundary 6; 02 — module structure, Service API, Remote API, Server Connector, Command Bridge and ordin server notes; 03 — sequence *Connect the desktop to an ordin server*; 04 — Desktop pairing, Server connection; 05 — module structure and Remote API rows; 07 — security posture, threat model, performance; 08 — Remote API; 10 — A-46, A-47, OQ-58 to OQ-60, C-71 to C-73, R-85, R-86, dependencies, rollout; 11 — §4, §5.31, §6, §7, UX-A66, checklist item 49; 12 — group "§5.31 Your ordin server on the desktop".
- **Alternatives considered.**
  - *SSH to a socket file on the server, with the Service API spoken over it.* Not chosen as the API: the user chose a REST API. *Amended in revision 0.47.0 (decision 78):* SSH is no longer a way to reach the REST API; the desktop connects straight to it.
  - *WebSocket for events.* Not chosen: commands already go up as REST calls, so events need one direction only; Server-Sent Events are plain HTTP, resume with `Last-Event-ID`, and pass company proxies more often.
  - *Polling for events.* Rejected: slow at a useful interval and wasteful at a fast one.
  - *gRPC with server streaming.* Not chosen: fast, but a second toolset beside REST and harder to pass through company proxies.
  - *The company's NATS servers between desktop and server.* Not chosen: only where the company runs NATS, and its operators could read the traffic.
  - *Microservices, one process per module.* Rejected: the user asked for a monolith; one process keeps one journal, one Rules Engine decision path and no network between modules.
  - *The desktop's own screens over a local REST server.* Rejected: Tauri's IPC carries the same Service API without opening a port on the person's computer, and the Service API types are the same on both transports.

### ADR-097: The ordin server runs in one of two profiles: as one person, or as a hosted agent with an identity of its own

- **Status.** Decided by the user, January 2026 ("what if the ordin server could be deployed to serve multiple users via inbound chanels? why building two systems if one can act in multiple ways?"; "what if the server runs under an agent identity, no multitenancy, just a hosted agent (in addition to the option to run as a specific person)"; "go with recommendations, but ensure it can fully replcase wrightrun, so only one build is required"; and, on desktops, "need to consider if when a local node connect to a headless instance running as an agent identity what happens? should this be possible?"; decision 73). The mechanism is proposed. **Amends ADR-044** (the agent profile gives the harness an organisation key), **ADR-073** (one person per installation becomes the personal profile), **ADR-096** (a desktop connects to an agent-profile server by role, not by pairing), and **ADR-011, ADR-060, ADR-074 and decision 41** for the agent profile only (it schedules recurring duties). **Keeps ADR-045, ADR-072 and decision 10.** ADR-098 states how this profile replaces wrightrun. *Amended in revision 0.61.0 (ADR-113): an agent-profile server whose configuration has a `knowledge-agent` section is a knowledge agent. Its Caller API port also serves a knowledge query route group and a read-only MCP endpoint, apart from the caller interface, to askers in its audience (item 3); its knowledge graph is kept in one scope of kind knowledge (item 5); fetching and reading is a built-in recurring duty (item 6).*
  - *Open question settled by the user, January 2026 ("go with recommendations and commit", decision 74): a crew run as one hosted agent waits until single hosted agents are in use (OQ-64).*
- **Decision.**
  - **1. Two profiles, one build.** The ordin server runs in exactly one profile, chosen by its setup command when it is installed and fixed for the installation's life:
    - **Personal profile.** Today's ordin server (ADR-073 to ADR-076, ADR-096), unchanged: one person, their own harness sign-in, their own sign-ins to documents and mail, approvals from their paired chats and desktops, nothing scheduled.
    - **Agent profile.** One **hosted agent** with an identity of its own, serving the people the organisation allows. One installation runs one agent package, with the agent configuration file that ADR-098 defines. Ordin, the coordinating agent, is not part of this profile: the people who talk to a hosted agent talk to that agent, as they would to a wrightrun deployment. A crew run as one hosted agent is OQ-64.
  - **2. The agent acts as itself.** No person's credential is used anywhere in the agent profile:
    - **Model.** An organisation API key, or the address of the organisation's model gateway with its key, from the server's secrets (ADR-073's secret store; a Kubernetes secret in the scaled shape, ADR-098). No person's subscription is used, so Anthropic's rule against routing Free, Pro or Max credentials on behalf of other users (ADR-044, *Context*) does not arise. In this profile the Claude adapter gives the key to Claude Code and never runs `claude auth login`. The key reaches the Claude Code process only; no hook, MCP server, sandbox or tool receives it (C-74).
    - **Connections.** Each system the agent reaches is reached with the credentials the agent configuration names for that connection: a service account, a bot token or an application registration. The agent never acts for the asking person with that person's own credentials (wrightrun ADR-017, carried across by ADR-098).
    - **Copilot.** Whether an account that is not a person's can hold a Copilot seat and sign in to the Copilot CLI is not confirmed (C-75). Until it is, the agent profile offers Claude only. The check command refuses an agent configuration that names Copilot, and states why.
  - **3. Who may talk to it.** Closed by default:
    - **Chat.** The agent answers in a channel only where its chat account has been added (an *enabled scope*, as in wrightrun), and only to messages addressed to it: a mention, a command, or a mention in a thread it is part of. The configuration can narrow this to the channels and users it lists. A direct message is answered only from a user the configuration lists. A message from anyone else gets no reply and starts nothing; the journal counts it without its content.
    - **The caller interface and intakes.** As ADR-098 item 6 states: an identity-provider token with the caller role, or the token or signature an intake names.
    - **Stated at start.** Whoever may talk to the agent gets everything the agent may reach. The start-up report therefore lists, beside each connection, who may reach the agent and through which way in (R-91).
  - **4. Approvals.** An action that needs approval pauses the run, and its card goes to the **approvers** the configuration names (users, or a group the identity provider asserts), in each approver's direct chat with the agent and on the desktops of approvers connected as item 9 says. The first approver to decide decides, once. The person who asked may not approve an action of their own run that leaves the company: email to an outside address, a post in a channel the configuration does not list, or a call to a connection the configuration marks external. For every other action the configuration may let the person who asked approve it. Where the configuration names no approvers, operators approve through the caller interface's resume, as in wrightrun. A paused run has no deadline in this profile: the harness process ends at the pause, and the run resumes from its saved session when an approver decides (ADR-098 item 3). A typed "yes" is never an approval (decision 10).
  - **5. Memory and scopes.** What the agent keeps — notes, knowledge, settings, memory pages and conversation — is kept by **scope**, as wrightrun's is: one scope per channel, one per direct chat, and one agent-wide scope. What is said in a channel is remembered for that channel. A direct chat's content is never written to the agent-wide scope or to any other scope. The agent's first reply in each scope says this in one sentence. This is a separation inside one organisation, not a tenant boundary (07 — Multi-tenancy).
  - **6. Recurring duties.** In the agent profile the Duty Timer runs a package's recurring duties on their schedules, and keeps each duty's lock, heartbeat, state and escalation files between cycles (ADR-098; wrightrun G4). The personal profile and the desktop still schedule nothing (ADR-011, ADR-060, decision 41).
  - **7. Chat systems.** Slack first, through the Slack Channel Adapter in Socket Mode (ADR-076), extended for channels, mentions, threads, reply actions and the settings form (ADR-098). Telegram (ADR-073, ADR-075) and Matrix follow for the agent profile once the Slack route is in use.
  - **8. Not in the agent profile.** Ordin and its crews; workspaces linked with a person's own sign-ins; folders on a person's computer; engineering mode; ArtCraft windows; dictation. The check command and the start-up report name each of these as not carried across when a package asks for one (G9).
  - **9. Desktops and an agent-profile server.** A person's ordin desktop can reach an agent-profile server, but never as *their* server. The desktop-key pairing of ADR-096 belongs to the personal profile, where one person owns the server. An agent-profile server refuses it, and the setup command does not offer it. A desktop reaches a hosted agent with the person's company sign-in (the identity provider's token), and what it shows depends on the person's role:
    - **Caller.** The hosted agent appears in the person's agent list as an agent that runs elsewhere (11 — §5.8). The person asks it through Ordin, as they would any agent that runs elsewhere, or makes it a member of a crew, through the Hosted Agent Adapter (ADR-098 item 8). The desktop shows only the threads and runs that person started. The agent's other conversations, scopes and memory are not shown.
    - **Approver.** Cards for that agent appear in the person's *Needs you*, with the person who asked and the scope. The person decides there as they would in chat.
    - **Operator.** The desktop shows the hosted agent under **Hosted agents** in the side rail: its runs with their state and who started them, paused approvals, recurring duties with their heartbeats and locks, settings by scope, knowledge entries, notes, and the start-up report of the running release. These are the operator routes of ADR-098 item 6 behind screens. Erasure is offered here with its full statement of what it covers. An operator does not read the content of the agent's conversations from the desktop, beyond what the operator routes return.
    - **What does not happen.** The server's state is never merged into the desktop's own workspaces (OQ-39 holds). The desktop's Ordin does not take over the hosted agent's sessions. The hosted agent never reaches the desktop's files or the person's sign-ins.
    - The screens for the operator role are drawn in a later revision (UX-A67). *Drawn in revision 0.43.2: prototype group "§5.8 Hosted agents".*
- **Context.** ADR-073 made one person per server installation a rule. It did so to keep within Anthropic's terms: a person's subscription may run Claude Code for that person, "including where a platform hosts Claude Code", and for no one else. The user asked why ordin and wrightrun should be two systems. The analysis of January 2026 found that the one-person rule follows from whose credentials the server uses, not from how the server is built. A server that uses an organisation key and credentials held for each connection can serve many people without any per-person separation: wrightrun's own model (one deployment, one agent, one organisation). Per-user tenancy would need on-behalf-of credentials and separated data, and was not chosen.
- **Consequences.**
  - The setup command gains the choice of profile, and the agent profile's own set-up: the agent release or package with its configuration, the model key, the connection credentials, and the chat app.
  - The Claude Local Adapter has two sign-in modes: the person's own sign-in (ADR-044) and the organisation key (item 2).
  - The Approval Service gains approver lists and the rule that bars the asker from approving their own actions that leave the company.
  - The Memory Service, the Knowledge Service and the journal keep content by scope in this profile.
  - Content from many people is now stored. Retention by kind of content and erasure for one person or one scope apply (ADR-098 item 6; 06).
  - 00 — decision 73, G46, G35 note, non-goals, glossary; 01 — hosted agents, callers and approvers, trust boundary 7; 02 — *Server profiles and replacing wrightrun*; 03 — sequences *A hosted agent answers in a channel and an approver approves* and *A desktop uses a hosted agent*; 04 — Server profile, Agent configuration, Scope, Approver list; 06; 07; 08; 10 — A-48, OQ-61 to OQ-64, C-74, C-75, R-88, R-91; 11 — UX-A67.
- **Alternatives considered.**
  - *One server serving many people, each as themselves.* Not chosen: every action would need that person's credentials, through an on-behalf-of flow for each system, and memory, journals and documents would need separating per person. Both are new work, and neither is needed for a hosted agent.
  - *A hosted agent on one person's subscription.* Rejected: Anthropic forbids routing subscription credentials for other users (ADR-044).
  - *A desktop pairs with an agent-profile server as it does with a personal server.* Rejected: the pairing gives one person the whole server, and an agent-profile server has no single owner. Access by company sign-in and role gives each person only what their role allows.
  - *Hosted agents stay in wrightrun.* Rejected by ADR-098.
- *Amended in revision 0.55.0 by ADR-107: item 3 — a content check the configuration declares receives every question, every answer or both; the start-up report names it beside its connection, as it names who may reach the agent.*

### ADR-098: ordin replaces wrightrun: one build runs the desktop, the personal server and hosted agents, from one machine to a scaled deployment

- **Status.** Decided by the user, January 2026 ("ensure it can fully replcase wrightrun, so only one build is required", decision 73). The mechanism is proposed. **Supersedes ADR-002** (there is no separate wrightrun to reconsider as a harness) **and ADR-007** (one Rules Engine, so no second implementation to agree with). **Amends ADR-003** (the adapter vocabulary is ordin's own from now), **ADR-008 and ADR-009** (the Slack route reaches hosted ordin agents), **ADR-060** (IT sets up repeating work as a hosted ordin agent), **ADR-096** (each module's storage on SQLite or PostgreSQL), **A-10** (metrics and traces in the agent profile) **and 07 — Multi-tenancy** (unchanged in substance, restated for hosted agents). *Amended in revision 0.61.0 (ADR-113): item 6 — the Caller API gains the knowledge query route group and the knowledge MCP endpoint, and Caller Authentication reads a groups claim and tells people from services; item 5 — a knowledge agent runs single or scaled like any hosted agent, with its cache, tables of contents and sources record in its own tables.*
  - *Open questions settled by the user, January 2026 ("go with recommendations and commit", decision 74): Gemini models are not offered, and the check refuses such a configuration with the reason, until C-83 confirms Google's Gemini CLI can serve as a third harness adapter, which is then the route (OQ-65); Claude Code is accepted as the one exception to the open-source rule, installed by the deploying organisation and never shipped in ordin's image, with the rule kept for everything ordin ships (OQ-62); the wrightrun repository's README carries a notice that ordin supersedes it, and its documents stay as the requirements record (OQ-63).*
- **Decision.**
  - **1. One build replaces two.** wrightrun is not built. Every capability that wrightrun's design states (`agent-wright-runtime/docs/design/agent-runtime/`, revision 0.24.2, with its spec set in `agent-wright-runtime/docs/spec/`) is provided by the ordin server in the agent profile (ADR-097). 02 — *Replacing wrightrun* maps each of wrightrun's components and cross-cutting concerns to the ordin module that provides it, and leaves none unmapped. One Cargo workspace (ADR-096) builds the desktop and the ordin server, and one container image of the ordin server runs the personal profile, the agent profile on one machine, and the agent profile scaled on Kubernetes.
  - **2. What stays compatible.** wrightrun's public forms are kept, so that packages, configurations and callers written for wrightrun work unchanged with ordin:
    - **Package declaration.** `wrightrun-declaration.yaml` keeps its file name, its format and its format and contract version ranges, as `agent-wright-runtime/docs/spec/components/PrepareStage.md` fixes them. ordin owns the format from now; builder RFC 35 is unchanged.
    - **Agent configuration file.** The agent profile reads wrightrun's configuration file: one YAML file, no secret, validated against a schema, with every key `agent-wright-runtime/docs/design/agent-runtime/08-interfaces.md` lists. ADR-097's keys (who may talk to it, approvers, connections marked external) are added as optional keys. Where they are absent, the agent behaves as wrightrun would.
    - **Caller interface.** The paths, bodies, error form and page form that `agent-wright-runtime/docs/spec/components/RuntimeServer.md` and `agent-wright-runtime/docs/spec/cross-cutting/Pagination.md` fix: start a run on a new thread or an existing one, resume a paused run, read a run's state and result. Two additions, which wrightrun lacked (its A-33): an event stream for one run (Server-Sent Events, with the resume rule of ADR-096 item 3), and cancelling a run.
    - **Tools the agent sees.** The runtime contract's tools (notes, events, facts of the run), the reply-action tool, the channel-document tool and the knowledge tools, with the names, argument forms and result texts fixed in `agent-wright-runtime/docs/spec/components/ToolBinder.md`, served on the MCP Front. How those names appear to the model through MCP is C-76.
    - **Agent release.** The same artefact in the organisation's container registry: the bundle, the configuration and the files it names, and the record, with the same reference form, media types and digest check (`agent-wright-runtime/docs/spec/components/AgentRelease.md`). A release is immutable, and a deployment names it and obtains it at start.
    - **Operator commands.** wrightrun's command list (prepare, publish, obtain a release, check, run once, expand a command, duty status, runs, work folder, apply the database schema, settings, jobs, notes, knowledge entries, erase, list connection hosts, start with what it serves, run one job), as sub-commands of the ordin server's command, whose name `/spec` fixes.
    - **What operators read.** The run summary's fields (`agent-wright-runtime/docs/spec/cross-cutting/Logging.md`), the metric names `/spec` carries across, and the items of the start-up report.
    - **Not kept: the connector plug-in contract.** wrightrun's was a Python entry point. In ordin a connector plug-in is an MCP server the organisation installs into its image and declares as a connection. The exposure rule, the approval list and the argument rules apply to its tools as to any other connection's. No wrightrun plug-in exists, so nothing breaks.
  - **3. The engine is the vendor harness, not Deep Agents.** A hosted agent runs in Claude Code through the Claude Local Adapter, as every ordin agent does (ADR-036, ADR-038). Deep Agents and LangGraph are not used. Where that changes what wrightrun designed:
    - **Model policy.** Primary model, a fallback model tried once, the gateway address and the safe reply text, through the harness's own settings (C-77). The providers are those Claude Code supports with an organisation key: Anthropic directly, a gateway, or any provider whose settings the configuration declares (*amended in revision 0.55.0 by ADR-107*; the list named two vendors before). **Not carried, and reported as not carried:** a sampling temperature, which Claude Code does not expose (C-77), and Google's Gemini models (wrightrun's `google_genai`, OQ-65).
    - **Single-call answer.** One harness turn with no tool offered (C-77). Where that cannot be had, the start-up report states the form in use, as wrightrun's did.
    - **Reviewer.** The package's reviewer runs as a sub-agent of the harness. The Rules Engine refuses every call it makes except reading and searching files, which needs the hook to tell which sub-agent made a call (C-78).
    - **Work folder.** The bundle is on the instance's local disk, read-only, as the obtained release. The work folder is kept in the datastore, under one namespace per agent, so it survives restarts and new releases with no volume mounted. The agent reaches it only through the File Service's file tools on the MCP Front. In this profile, the harness's own file-writing tools are switched off, and its reading tools are limited to the bundle (C-79). A duty's `STOP` and `escalation.md` cannot be removed by the agent.
    - **Pause and resume across instances.** At a pause, the Session Supervisor stores the harness's session record in the datastore and ends the harness process. On a decision, whichever run instance takes the run restores the record and resumes it (`--resume`). The approved call is allowed once, matched by tool and arguments (C-80). A run whose instance stops without warning is recorded as interrupted and is not started again, so no tool call is made twice (wrightrun ADR-047).
    - **Shell.** Off, as in wrightrun's first release. Where the configuration grants it, commands run only in the sandbox (ADR-037, ADR-040).
  - **4. One Rules Engine with the full vocabulary.** The Rules Engine implements wrightrun's ControlEvaluator semantics in full, for every profile: at the start of a run (message rules; the single-call answer; a stated reply; starting a stated command; limits on starts of a command, of a scope or of an asker within a period; limits on runs in progress; suppression of a repeated message), before a tool call (gates, argument rules and the approval list), and at the end of a run. Counters and locks are kept in the datastore, so a count is exact across instances. Patterns are matched with the Rust `regex` crate, whose matching time grows with the length of the text only (wrightrun R-24). The scenarios of `agent-wright-runtime/docs/spec/components/ControlEvaluator.md` become ordin's own tests; there is no second implementation to agree with.
  - **5. Two deployment shapes of the agent profile.**
    - **Single.** One process on one Linux machine, or one container, with the SQLite datastore, kept running by a systemd user unit or a container restart policy (ADR-076). It needs no other service. It suits one agent at the load of A-48.
    - **Scaled.** Kubernetes, with the PostgreSQL datastore as the only service, and the one image started in roles: requests, runs, chat connections, jobs, or chat connections with jobs, each role scaled on its own (wrightrun ADR-032, ADR-046). Runs wait in a **Run Queue** in PostgreSQL, and a run instance takes each run with a lease it renews. The Duty Timer records each firing and queues its run in one statement, so one firing starts one run across instances (wrightrun ADR-065). The deployment files are a Helm chart of ordin's own, with a values schema, the schema step as a hook, and an example Argo CD application (wrightrun ADR-075). The scale targets are A-48's.
    - **Datastore.** Every module that owns tables has two storage implementations behind its own interface: SQLite (the desktop, the personal profile and the single shape) and PostgreSQL (the scaled shape). ADR-096's ownership by table holds on both. The schema is versioned with ordin and applied by the schema command before the instances of a new release start; within one minor line a change only adds, so the previous image can still read what the next one wrote (wrightrun ADR-025, ADR-080). Search uses SQLite FTS5 or PostgreSQL text search, and the recall test runs on both (R-89).
  - **6. Operating a hosted agent, as wrightrun designed it.**
    - **Authentication and roles.** Every route but health, metrics and a signed intake needs an identity-provider token, checked for signature, issuer and audience in ordin's own code. A caller may start runs and read and resume the runs they started. An operator may also read and resume any run, and use the operator routes and the work-folder routes. The claim that carries the role is named in the configuration (wrightrun ADR-011, ADR-012, ADR-043, ADR-054).
    - **Intakes.** An HTTP intake route for each intake, verified by a token or a signature made with a shared secret, starting one fixed command with the event as its arguments, within the intake's limit of runs in progress (wrightrun ADR-018). This adds to the NATS triggers of ADR-074.
    - **Knowledge sources.** Chat history of enabled scopes, entries operators add and verify, and the agent's notes, ingested by the Job Runner in batches with checkpoints, and searched by ranked full-text match (wrightrun ADR-026, ADR-044, ADR-058).
    - **Settings.** The Settings Store keeps declared settings by scope, changed through the chat's settings form, the entry commands and the operator routes (wrightrun ADR-028, ADR-033).
    - **Observability.** JSON log lines to standard output with one run summary per stretch of a run; Prometheus counters on every instance; OpenTelemetry traces to the organisation's collector when the configuration switches them on, with no content in a span (wrightrun ADR-013, ADR-029, ADR-076). A-10 still holds for the desktop and the personal profile.
    - **Retention and erasure.** A retention period for each kind of stored content, applied by a retention job; erasure of one person's stored content, or of one scope's, with a statement of what it covers and what it does not (wrightrun ADR-049, ADR-069, ADR-070).
    - **Health and shutdown.** Liveness and readiness on every instance. An instance told to stop takes no new run and lets its runs finish within the stop period. A worker records each job's checkpoint before it exits.
    - **Build checks.** Every dependency and every service a deployment runs carries a licence the Open Source Initiative approves, other than the GPL and the AGPL, and a check of the build fails otherwise. The locked dependencies are audited for known vulnerabilities. The tools are chosen in `/spec` (wrightrun ADR-071, ADR-072, ADR-077). The harness is the exception in item 7.
  - **7. The harness and the licence rule.** ordin is Apache-2.0 (`LICENSE`). Claude Code is Anthropic's own program, used unmodified under Anthropic's terms. It is not inside ordin's image. The deploying organisation adds it from Anthropic's installer, at the version ordin pins, in an image layer of its own or at the server's setup, and supplies its own key. wrightrun's rule of "no vendor licence key" becomes "no key of ordin's vendor": the only keys are the organisation's own, for its model provider (C-81, OQ-62).
  - **8. What changes on the desktop.**
    - The **wrightrun HTTP Adapter** becomes the **Hosted Agent Adapter**. It reaches a hosted agent's caller interface with the person's company sign-in, and gains live progress and cancel from the run event stream. It still queues a second request on a busy thread.
    - The **Slack Thread Adapter** reaches hosted ordin agents through Slack, as it reached wrightrun agents (ADR-008, ADR-009).
    - The administrator registers hosted ordin agents in the profile's remote agent catalogue, as they registered wrightrun agents.
    - When Ordin offers to ask IT to set up repeating work (ADR-060), the request now asks for a hosted ordin agent with a recurring duty.
    - A person's desktop reaches a hosted agent by role (ADR-097 item 9).
  - **9. The wrightrun repository.** Its design and spec set stay as the record of the requirements this decision covers, cited by path from 02 — *Replacing wrightrun*. Marking that repository as superseded is a change in that repository (OQ-63).
- **Context.** Neither ordin nor wrightrun has code. ordin already holds the parts wrightrun would have duplicated: a Rules Engine that had to reach the same decisions as wrightrun's on the same scenarios (ADR-007), an MCP Front, a Session Supervisor, a journal, chat channels and triggers. Keeping both meant two rules engines in two languages kept in agreement, two release processes, and two ways to run an agent. ADR-096 made ordin a modular monolith whose modules own their data, which lets a module's storage change without touching its callers. ADR-097 gave the server an agent identity, which removes the reason ordin could not serve many people.
- **Consequences.**
  - The ordin server grows by the size of wrightrun's design: the Agent Configuration, Release Builder, Agent Loader, Caller API, Caller Authentication, Run Queue, Duty Timer, Job Runner, Connection Binder (with the HTTP Connector and Outbound Publisher), Settings Store, Model Policy, Telemetry, and Retention and Erasure modules are new, and the File Service, Knowledge Service, Rules Engine, Session Supervisor, Trigger Service, Slack Channel Adapter and Start-up report are extended (02). R-90 records the size, and the rollout puts the single shape first.
  - Every module that owns tables gains a PostgreSQL storage implementation.
  - The pause-and-resume path of a hosted agent depends on Claude Code's session record being portable between machines (C-80, R-87).
  - wrightrun's open items that concern a library it no longer uses — Deep Agents, LangGraph and its checkpointer, `slack_sdk`, `google-re2` and the Python toolchain — fall away. Its items about the registry artefact, the Helm hook and Slack hold for ordin.
  - 00 — decision 73, G47, non-goals, glossary; 01 — hosted agents, the container registry, PostgreSQL, the model provider; 02 — *Server profiles and replacing wrightrun*, the mapping table, new and extended components, the four control loops; 03 — sequences *Publish and deploy a hosted agent* and *A hosted agent answers in a channel and an approver approves*; 04 — Agent release, Run record, Schedule firing, Job record, Setting value, Knowledge entry, data lifecycle rows; 05 — datastore, deployment, observability, identity and licence rows; 06; 07 — authentication, authorisation, observability, migrations, scale, multi-tenancy, security; 08 — the interfaces of item 2; 10 — A-48, A-49, OQ-61 to OQ-64, C-76 to C-83, R-87 to R-92, dependencies, rollout; 11 — UX-A67.
- **Alternatives considered.**
  - *Keep wrightrun for hosted agents, and ordin for people.* Rejected by the user: two builds.
  - *Run Deep Agents inside ordin as a second engine.* Rejected: a Python runtime inside a Rust build, and a second engine beside the vendor harnesses that every other ordin agent uses.
  - *ordin's own agent loop over the providers' model interfaces.* Not chosen now: ordin is an exo-harness (ADR-036) and leaves the inner loop to the vendor's harness. It remains a way to reach Gemini models (OQ-65).
  - *The agent profile on one machine only, without Kubernetes.* Rejected: it would not replace wrightrun's scale (wrightrun's 07, *Scale*), which the user requires.
- *Amended in revision 0.55.0 by ADR-107: item 3, Model policy — the providers are no longer a closed list of named vendors. A deployment reaches any provider Claude Code supports by declaring the settings that provider needs, which ordin sets in the harness child's environment only; ordin holds no provider-specific code. The sampling temperature stays not carried. Item 4 — the Rules Engine gains a content check through a declared connection, and the hosted chat's context before the model becomes a configured list of steps.*

### ADR-099: A lifecycle may branch, run stages side by side and send work back to any earlier stage

- **Status.** Decided by the user, January 2026 ("I have the concern that lifecycles do not cater for complex workflows they seem linear?", then "allow branches in lifecycles"; decision 76). *Amended in revision 0.46.0 by the user's decisions of January 2026 ("found it but it does not have feedback loops?", "reviews typically triggered a loop", "is there a diagram that could be also shown as per the workflow pane?"; decision 77): items 5 and 6 below.* The mechanism is proposed. **Amends ADR-092 items 1, 2, 5, 6 and 8**, which made a lifecycle one chain of stages with one receiver per handover, and **ADR-095 item 2**, which drew and edited the stages as a path. A lifecycle with no branch reads, draws and hands over exactly as before.
- **Decision.**
  - **1. What comes next.** Each stage may say what comes next. With nothing said, the stage after it in the list comes next, so every lifecycle written before this decision keeps its meaning. Otherwise a stage names one of:
    - **one of several stages, chosen when handing over** — each with *when* it is chosen, in the person's words ("The supplier is rated high risk", "Otherwise");
    - **all of several stages, side by side** — each gets its own handover, at once;
    - **nothing** — the lifecycle ends at this stage. A lifecycle may have several ends.

    An optional stage is a choice that leads past it ("Otherwise: Contracting").
  - **2. Where stages meet.** A stage that several stages lead to starts when every one of them that is under way has handed over. A branch that was not taken is never waited for: the handover record of a side-by-side split names the stages sent together (08 — Interfaces, *Handover record*), so the receiving ordin knows what is still under way without a shared service. The receiver may accept each handover as it arrives; the handover screen says how many are here ("1 of 2 handed over") and which are still under way.
  - **3. Handing over at a branch.**
    - *At a choice,* the handover draft asks **Which stage comes next?** first, listing each choice with its *when*. Ordin recommends one from what the result says, first and marked, with the reason (P5). **The person chooses; no agent chooses** — the choice is part of a handover, which only the person sends (ADR-092 item 9). Choosing a stage suggests who took it last and runs that stage's checks.
    - *Side by side,* the draft has one **To:** for each stage, and **Hand over…** opens one approval card that names every receiver and stage. Each receiver gets their own handover, through the place of their stage.
    - Each handover still goes to one person (ADR-092 item 2); handing to several people is handing to several stages.
  - **4. Checks a lifecycle that branches adds** (to ADR-095 item 2's, in fixed code): every stage can be reached from the first; no stage leads back to an earlier one; every choice of two or more stages says when each is chosen; and crucible's seam check runs on every link, not only on the stage after it in the list. Nothing can be waived (ADR-090).
  - **5. Going back, and feedback loops.** *Amended in revision 0.46.0:* a stage — typically a review — may declare **where it sends work back**: an earlier stage, *when* in plain words ("Changes are needed"), and at most how many rounds (1 to 10). The lifecycle screen and diagram draw each as a dashed ↩ arrow. **Return to…** at that stage offers the declared send-backs first, each with its round ("Priya Shah — Analysis: changes are needed (round 2 of 3)"). After the last round, returning again stays the person's choice and is not blocked: the card says "round 4; the lifecycle allows 3" and the record marks it as beyond the agreed rounds, which the lifecycle's owners can see. Work goes back only by **Return to…** on a handover, never by a stage that leads back, so a lifecycle never loops by itself. In a lifecycle that branches, **Return to…** offers the person who handed it over and, for every earlier stage on the way to this one, whoever did it, read from the handover records. Going back past the sender tells the sender too. The record keeps every round (ADR-092 item 6).
  - **6. The screens.** *Amended in revision 0.46.0:* both screens also have **Show as diagram**, the lifecycle drawn in the workflow diagram's language (UX-A40), for reading only: a box per stage, a diamond where the person chooses, each way out labelled with its *when*, a bar where stages start side by side, an arrow down the right margin where a link skips stages, dashed ↩ arrows for send-backs with their rounds, and "end" under each last stage; the stage cards stay below it. The lifecycle screen and the lifecycle draft draw a lifecycle that branches as columns of stages joined by arrows, each stage saying "Then: …", "Then one of: …" or "Then, side by side: …", and where stages meet "Starts when … have handed over". A straight lifecycle is still drawn as a path. The stage's form gains **What comes next** (11 — UX guidelines §5.28, UX-A71).
  - **7. crucible journeys.** A crucible journey whose seams branch or join is now read as a lifecycle that branches, from crucible's producer-to-consumer graph, and offered. Only a journey whose seams form a loop is read, listed under Health and not offered (amends ADR-092 item 1).
- **Context.** ADR-092 made a lifecycle an ordered list with one receiver per handover. Real approval processes branch on what the work found (a high-risk supplier goes through extra reviews), run reviews side by side (security and legal), skip stages, and send work back further than the last sender. Workflows already branch (ADR-065, UX-A39), but only inside one person's stage.
- **Consequences.** 00 — decision 76; 04 — Lifecycle and Handover; 08 — *Lifecycle part* (`next`, `when`, `together`; `sendsBackTo` from 0.46.0) and *Handover record* (`nextStage`, `sentWith`, `returnedTo`; `sendBackRound` from 0.46.0); 11 — §5.28, UX-A71, checklist item 50; 12 — scenes `lcBranch`, `lcDraftBranch`, `lcDraftLoop`, `hoChooseNext`, `hoSideBySide`, `hoJoin`, `hoReturnEarlier`.
- **Alternatives considered.**
  - *Keep lifecycles straight and branch only inside workflows.* Rejected by the user: the branches are between people, each in their own ordin, which a workflow inside one stage cannot reach.
  - *Let the application choose the branch from the result.* Not chosen: choosing who works next is a decision with consequences for other people, so it stays the person's, with Ordin's recommendation. A condition the application could check in fixed code may be added later.
  - *Stages that lead back, with a limit on rounds.* Not chosen as forward links: **Return to…** already carries work back with the reason, and a lifecycle whose forward links never loop can be checked and drawn simply. *From revision 0.46.0 the expected loops are declared instead, as send-backs that Return to… offers (item 5).*
  - *Wait at a meeting point for every stage that could lead there.* Rejected: it would wait for ever on a branch that was not taken.

### ADR-100: Hub modules own the traits their providers implement, and only the two programs wire modules together

- **Status.** Decided by the user, January 2026 ("go with your recommendations"; decision 79), on the proposal of `/spec` Phase 2 (HLD-impact-pass of `docs/spec/ordin-desktop/phase-2-architecture.md`). **Completes ADR-096 item 1**, whose build test fails on any cycle, by fixing how the *Depends on* lists of 02 — Architecture are kept acyclic.
- **Decision.**
  - **1. Hubs own traits.** Where a module must call modules that themselves call it, the module owns a Rust trait and the callers implement it and register at start-up. Five hubs do this:
    - the **MCP Front** owns `ToolProvider`; the Sandbox, Memory Service, Craft Toolkit, Ordin Host, Knowledge Service, Mail Service, Telegram Service, Authoring Service, Project Service, Connection Binder and Agent Loader (the agent profile's file tools, served from the File Service) implement it;
    - the **Crew Orchestrator** owns `Coordinator`; the Ordin Host implements it and passes it when it starts a crew run;
    - the **Agent Model** owns `PartSource`; the Catalogue Service implements it, and the Resolver and Compiler receives it;
    - the **Journal and Artefact Store** owns `EventSink`; every module writes its events to the Journal, which gives each its sequence number, and the Command Bridge, the Remote API and the Channel Gateway subscribe;
    - the **Service API** owns `ServiceApi`, one method per command; the two programs implement it.
    - *Amended in revision 0.49.0 (`/spec` Phase 3; decision 80):* a module may also own a narrow interface that only a program implements, for facts, wording or actions of modules it has no edge to; each is wired by the program at start. The Journal and Artefact Store owns `PruneParticipant`, through which every module prunes its own history to the Journal's cut-off; the Setup Assistant owns `HarnessProbe`, which the program implements for the Copilot CLI; the Package Catalogue owns `InstanceActivity`, implemented over the Session Supervisor; the Rules Engine owns `RulesHost`, the Session Supervisor `SupervisorHost`, the Crew Orchestrator `CrewHost`; the Sandbox owns `PlaceFiles`, implemented over the File Service; the Release Builder owns `PackageReader` and `HarnessLayoutWriter`, and the Agent Loader `LoaderStore`; Retention and Erasure owns `RetainedContent`; the Caller API owns `JobDesk`.
  - **2. Only programs wire.** The desktop program and the ordin server program are the only crates that depend on concrete adapters and on every module. They construct the modules, register providers with their hubs and pass trait objects. No module crate depends on a concrete Runtime Adapter.
  - **3. Contracts are crates of their own.** The shared types, the Service API (with the block catalogue and its schema check) and the Runtime Adapter interface are three crates that depend on nothing in the workspace except the shared types.
- **Context.** ADR-096 made each component a crate whose dependencies must match its *Depends on* list, with a build test that fails on a cycle. Read literally, the lists of 02 contain nine cycles — among them Approval Service → Command Bridge → Approval Service, File Service ↔ Workspace Service, MCP Front → Ordin Host → local adapter → MCP Front, and Checker → Resolver → Catalogue Service → Checker — so the first build would fail the test ADR-096 requires. Each cycle comes from a hub that both serves and calls the same modules.
- **Consequences.**
  - 02 — Architecture: the *Depends on* lists of Approval Service, File Service, MCP Front, Project Service, Server Connector, Ordin Host, Crew Orchestrator, Resolver and Compiler, Usage Planner, Rules Engine, Workspace Service, Authoring Service, Channel Gateway, Trigger Service, Dictation Service, Copilot Local Adapter and Handover Service are corrected (`docs/spec/ordin-desktop/phase-2-architecture.md` §2B-2 lists each).
  - No responsibility moves between components; each *Owns* list is unchanged.
  - The build test of ADR-096 checks the corrected lists.
  - Registration order at start-up becomes part of each program's specification.
- **Alternatives considered.**
  - *Merge each cycle into one crate.* Rejected: the MCP Front cycle alone would merge eleven components into one crate, which is the monolith ADR-096 set out to divide.
  - *An in-process message bus between modules.* Rejected: it hides dependencies from `cargo metadata`, so the build test could no longer see them, and adds a second way for modules to talk.
  - *Callbacks passed as closures at each call.* Not chosen: a named trait states the contract once and shows in the crate graph; closures do neither.

### ADR-101: The memory component keeps its store behind a storage interface on ordin's own datastores; SurrealDB is not part of ordin's build

- **Status.** Decided by the user, January 2026 ("go with your recommendations"; decision 79), settling OQ-66. **Amends ADR-030 and ADR-034** in where the embedded memory component keeps its data, and **joins C-82**.
- **Decision.**
  - **1. A storage interface.** The memory component (`memory/`) reads and writes its facts, entities, relationships, sources and embeddings through one storage interface of its own (Rust traits), in place of its direct use of SurrealDB.
  - **2. Two implementations, on ordin's datastores.** One on SQLite, in the journal's file, owned by the Memory Service's tables (ADR-010, ADR-096 item 1), for the desktop, the personal server and the agent profile's single shape; one on PostgreSQL for the agent profile's scaled shape (ADR-098, C-82).
  - **3. SurrealDB leaves ordin's build.** No crate that ordin builds depends on `surrealdb`. Whether the standalone `memory` service keeps a SurrealDB implementation of the same interface is that component's own decision and outside this design.
  - **4. Behaviour unchanged.** Recall, the write gate, bi-temporal facts and deletion with proof keep their meaning; the memory component's tests run against each implementation.
- **Context.** `/spec` Phase 1 found that the memory component depends on SurrealDB, released under the Business Source License 1.1, which the Open Source Initiative does not approve; the licence check of 05 — Tech stack fails the first build on it (C-16). ordin is released under Apache-2.0 (00 — Overview). C-82 already asks for a storage interface so that the scaled shape can keep memory in PostgreSQL.
- **Consequences.**
  - A storage port is added to `memory/` before the Memory Service is built; it is the first step of the Generation Playbook for the Memory Service.
  - Vector search and reranking run over SQLite and PostgreSQL: the extension or the in-process search is chosen in `/spec` (C-18, C-35).
  - 02 — Memory Service; 04 — Memory fact storage; 05 — Datastore row; 07 — licence check; 10 — OQ-66 settled, C-16, C-82, R-93.
- **Alternatives considered.**
  - *Accept SurrealDB as a named exception in the licence check.* Rejected by the user: a source-available store inside an open-source release.
  - *Keep SurrealDB in the standalone service and give ordin a separate memory store.* Rejected: it forks the memory component.

### ADR-102: Slack for working together: handover notices and Ordin in linked channels, posted as the person after approval or under a narrow rule the person set once

- **Status.** Decided by the user, January 2026 ("apply decision recommendations"; decision 82). **Amends ADR-074** (a Slack-message trigger kind on the personal server), **ADR-075** (for Slack only: a rule the person sets once may stand in for the card), **ADR-076** (Slack tools for agents are added; linked channels are read with the person's own token) **and ADR-092** (items 2, 4 and 7). **Settles OQ-55** with option (a). **Keeps ADR-008** (the Slack Thread Adapter), **ADR-011** and **ADR-072** (no background service on the desktop; user-space only).
- **Decision.**
  - **1. A Slack Service.** A new core module, the **Slack Service**, on the desktop and the personal server. It keeps the Slack channels each workspace links, the messages read from them, drafts, the person's Slack rules and a record of every post. It is offered to sessions as one MCP server, the **Slack tools** (*search Slack*, *read Slack*, *draft Slack post*, *ask to post*; named in `/spec`), and it posts only after an approval card or under a rule. It uses the person's own Slack user token from Identity and Credentials. Agents get the tools in a workspace only after the person grants **Read Slack** or **Read and draft Slack** there, as for Telegram.
  - **2. Posted as the person, never as a bot.** Every Slack post uses the person's user token (as ADR-008's route) and every card says it "will appear as you". No company Slack app, bot or relay is needed.
  - **3. Handover notices.** The handover draft offers **Also tell them in Slack** when the sender is signed in to Slack and the receiver's address matches a Slack member; on approval of the handover card the receiver gets one direct message from the sender. A lifecycle stage may name a **stage Slack channel**: each handover of that stage that is handed over, accepted, returned, taken back or reminded is posted there as the person who acted, mentioning the other person. Each post is listed on the card of the action that causes it, or covered by a rule; an action with no card of its own (accepting; a reminder) gets a Slack card unless a rule covers it. **Remind [name]…** posts in the stage channel instead of drafting an email when the stage names one.
  - **4. The handover link.** Every Slack notice and every handover email carries a link that opens the handover in the receiver's ordin (`ordin://handover/open…`, registered for the current user only) and, beside it, the place's own link for a computer without ordin. Opening the link starts following the handover's SharePoint place when the receiver's ordin does not yet follow it (OQ-55 (a)).
  - **5. Linked channels, read as data.** A workspace can link Slack channels the person belongs to. The Slack Service reads them with the person's own token every 60 seconds (A-51) — on the desktop only while ordin is open, on the personal server continuously — from the time of linking. Direct messages and group direct messages are never read. Messages are content, never instructions; a line that reads as an instruction is marked.
  - **6. Ordin may react, and nothing starts by itself.** When the person turns on **Let Ordin look at new messages** for a link, Ordin is asked about new messages from other people at most once every 5 minutes per link and at most 20 times a day; it may draft an answer, propose work or propose a saved workflow, each waiting for the person. On the personal server, a turned-on trigger of the new kind *Slack message* (a phrase in a linked channel) starts a saved workflow within ADR-074's limits, every external action still asking.
  - **7. Rules the person sets once.** A **Slack rule** names one kind of post — handover notices of one stage, or Ordin's answers in one linked channel — one channel, and a daily cap of 1 to 20 posts (default 20). Posts under a rule go without a card, are journalled with the rule's identifier, and stop at the cap or when the rule is turned off. Agent drafts are never covered by a rule. The profile key `slack.rules_allowed = false` turns every rule off; on the personal server `server.external_actions_off` naming `slack_post` turns every Slack post off.
  - **8. Optional and self-service.** The person signs in to Slack themselves; where the Slack workspace uses the company's single sign-on, Slack's own sign-in page goes through it. When the profile names no Slack app, the person may connect their own. Where Slack is not configured, not allowed, the receiver is not found or a post fails, the handover goes by email exactly as before. On the personal server the person signs in through the paired desktop's browser while the server keeps the token (decision 82).
- **Context.** Handovers reach a receiver who does not use ordin only by email or GitHub's review request, and a handover in a SharePoint place the receiver does not follow may never be seen (OQ-55). Teams that coordinate in Slack get no signal there. The person's Slack user token already carries `chat:write`, `channels:history` and `groups:history` (SA-SLACK-01). The Slack Thread Adapter keeps no table and does not depend on the Approval Service (02 — Slack Thread Adapter), and the server's Slack Channel Adapter is a company-created app whose personal bot has no channel-history scope.
- **Consequences.**
  - The Handover Service and the Trigger Service depend on the Slack Service; the Slack Service owns `SlackReactor`, which the programs implement over the Ordin Host.
  - A lifecycle stage gains an optional Slack channel; a handover record gains an optional list of Slack message links, appended by the installation that posted (R-83 unchanged).
  - New Slack user scopes are asked for by incremental consent at first use (`/spec` SA-SLACK-02); existing sign-ins keep working.
  - The desktop registers a per-user `ordin` link handler (`tauri-plugin-deep-link`; C-85).
  - 00 — glossary, decision 82; 01 — Slack workspace; 02 — Slack Service, Handover Service, Trigger Service, Ordin Host; 03 — two sequences; 04 — Slack link, Kept Slack message, Slack draft and post record, Slack rule, Lifecycle, Handover; 05 — Desktop shell plugins; 07 — Slack tools; 08 — Slack Web API, Slack tools, handover link; 10 — OQ-55 settled, R-06, R-97, R-98, A-51, C-84, C-85; 11 — §5.25a, §5.28, UX-A72.
- **Alternatives considered.**
  - *Extend the Slack Thread Adapter.* Rejected: it is a runtime adapter that keeps no table and does not depend on the Approval Service (ADR-100); links, rules and drafts would break its dependency rules.
  - *Read channels through Socket Mode with the company's Slack app.* Rejected: it needs an app IT creates and approves, its personal bot has no channel-history scope, and it works on the server only.
  - *Post as a bot.* Rejected by the user: colleagues should read an ordinary message from the person.
  - *A relay service that receives Slack events for ordin.* Rejected: ordin runs no service between people and nothing leaves the computer but the approved posts (local-only).
  - *Agents post without approval, within rules.* Rejected, as for email (ADR-070) and Telegram (ADR-075): only handover notices from fixed text and Ordin's own answers may be covered by a rule.

### ADR-103: Team knowledge is shared through a folder in a GitHub or GitLab repository, with no server

- **Status.** Decided by the user, January 2026 (decision 83). The mechanism is proposed. *Amended in revision 0.59.0 (ADR-110): a workspace still shares to one folder, its own (item 1), and may also read any number of other teams' folders; nothing got from a read folder is shared, so item 3's "everything in the workspace" means everything that is the workspace's own; the copy of a repository is kept once per repository, not once per workspace.* **Amends ADR-030** (memory may leave the computer, only as files in a team folder after a card), **ADR-057** (pages may leave the computer the same way, beside the Confluence copy) **and ADR-066** (the repository path that carries workflows also carries team knowledge). **Keeps ADR-025's rejection of a shared team workspace and ADR-092's rejection of a shared team ordin:** each person keeps their own ordin, workspace, memory store and journal; only files in a repository folder are shared. **Keeps ADR-029** (every send is approved). *Amended in revision 0.61.0 (ADR-112, ADR-113): the folder also carries an `index.md` and a `log.md` (item 2's format 1 unchanged); knowledge agents may read a team folder as a plain repository, read-only, so what a team shares can reach a wider audience; the Team Knowledge Service stays out of the agent profile.*
- **Decision.**
  - **1. A Team Knowledge Service.** A new core module on the desktop and the personal server. A workspace put into **Team Knowledge** mode is bound to one folder of a GitHub or GitLab repository that the workspace links. The service keeps its own copy of that repository in the application data folder, fetched and pushed with the person's own sign-in; it never touches a checkout the person works in.
  - **2. One file per item, in a format with a version.** The team folder holds a format file and one plain-text file per memory fact, entity, source and relationship (JSON, fields in a fixed order) and one Markdown file per knowledge page (the page file the Knowledge Service already writes, plus who first shared it and when). Two people adding items never touch the same file. An installation never writes a newer format than it reads, and refuses to read a newer one.
  - **3. Everything in the workspace is shared, with one exception.** Every fact of the workspace's memory scope and every knowledge page of the workspace is shared, with no per-item choice, including items written before the mode was turned on. Facts flagged as possibly holding personal data wait until the person has checked them. Never shared: facts used in all workspaces (they belong to no team), facts drawn from page sections (the pages travel themselves), search vectors, and the fields that record how often a fact is used.
  - **4. Share what we've learned.** One action, one approval card with counts in plain words and no Git words. The service first takes in what the team shared, then builds one commit from file objects on top of the branch's current head and hands it to the Push Service, which pushes it without force, or opens a pull request or merge request where the branch requires review. If the branch moves after the card, the commit is rebuilt on the new head without a new card, at most three times.
  - **5. Get what the team has learned.** The service checks the folder at start, when the workspace opens and every 15 minutes while ordin runs (A-52), and says what is new and from whom. **Get them** brings everything in at once; it is used by Ordin and the workspace's agents straight away, with no review step. Every fact passes the memory component's personal-data scan and write gate as data; a fact the gate refuses (text that reads as an instruction) is not taken in and is listed.
  - **6. Deletions travel both ways.** Deleting a fact or removing a page deletes its file at the next share, as an ordinary commit; getting a colleague's deletion deletes the item locally. History is never rewritten, so a deleted item stays in the repository's history (R-99).
  - **7. Two people, one item.** A fact's text never changes in place (a correction is a new fact), so two edits of one fact merge field by field: the earlier end of validity wins. When two people changed the same page, the newer version is used and the other is kept as an earlier version of the page, which the person can restore (R-101).
  - **8. Access is the repository's.** Who may read and who may share is decided by the repository's own access and branch rules; ordin adds none. A person who can read but not write can **Get** and cannot **Share**.
  - **9. The local database stays the working copy.** Recall, page search and freshness never read the repository; without a network only **Share** and **Get** are unavailable.
  - **10. Stop sharing with the team** leaves every local item and every shared file as they are.
- **Context.** Each installation's memory and pages stay on one computer, so what one person's agents learn is lost to the colleague who picks up the next piece of work. The user asked for sharing through repositories people already use, with no server, no company set-up and no Git skill (decision 83). ADR-066 already shares workflows through repository folders with the person's own sign-in, and the Catalogue Service already keeps its own repository copies.
- **Consequences.**
  - The memory component gains an export and an import of structured facts that keep their identifiers, validity times and replacement links and pass the write gate's personal-data and instruction checks without fact extraction; the person can mark a flagged fact as checked.
  - The Knowledge Service exports and imports page files and resolves a page changed on both sides; a page that cites a file on another person's computer is withheld from agents under ADR-057's rule (R-100).
  - The Push Service sends a prepared team-knowledge commit after the card; the Workspace Service tells other modules when a link is removed, which stops sharing.
  - 00 — glossary, decision 83, non-goals, *How a team works together through ordin*; 02 — Team Knowledge Service, Memory Service, Knowledge Service, Push Service; 03 — two sequences; 04 — Team knowledge binding, Team share; 07 — team knowledge policy; 08 — Team knowledge folder; 10 — R-99, R-100, R-101, A-52; 11 — §5.31a, UX-A73.
- **Alternatives considered.**
  - *A shared memory server, or the memory component's collective profile.* Rejected by the user: no server and no company set-up.
  - *A per-item choice of what to share.* Rejected by the user: everything in a Team Knowledge workspace is shared; the person decides when, not what.
  - *Reading and writing each file through the GitHub and GitLab APIs.* Rejected: 5,000 facts would cost 5,000 calls per get and one commit per file per share.
  - *Writing into a linked local repository folder.* Rejected: it would disturb the person's checkout, and local folders cannot be linked on the personal server.
  - *YAML item files.* Rejected: ordin reads YAML but has no writer, and a byte-identical round trip needs a fixed serialisation.
  - *Rewriting history to erase a deleted item.* Rejected by the user: deletions are ordinary commits.
  - *Reviewing colleagues' items before they are used.* Rejected by the user: they are used at once and corrected afterwards.

### ADR-104: Knowledge pages link to each other, and the knowledge folder can be edited in another Markdown app

- **Status.** Decided by the user, January 2026 (decision 84). The mechanism is proposed. **Amends ADR-057** (pages may be opened and edited in another Markdown app; pages link to each other; two changes the person did not make on the page itself — link text rewritten by a rename, and ordin's header added to an adopted file — are written without asking, each as a restorable version) **and ADR-030** (a fact from a page names its section). *Amended in revision 0.61.0 (ADR-112): item 1 — the header carries the Open Knowledge Format keys beside ordin's, and `sources` entry fields ordin does not own are kept byte for byte as header keys are; item 2 — `index.md` and `log.md` are never adopted as pages; item 3 — links ordin writes are standard Markdown links to the page's path from the folder root, `[[Title]]` links are still read and are rewritten only by a rename.*
- **Decision.**
  - **1. The folder is safe to edit elsewhere.** A workspace's `knowledge/` folder may be opened in another Markdown app (Obsidian is the reference). Header keys ordin does not own are kept exactly as written. A page file renamed or moved there stays the same page; a file deleted there is a removed page with **Undo**; another app's settings folder and non-Markdown files are ignored.
  - **2. New files become pages.** A Markdown file someone creates in `knowledge/` is adopted as a page written by the person, titled from its file name, and ordin adds its header to the file (K8).
  - **3. Links between pages.** Pages link with `[[Title]]`, `[[Title#Heading]]`, `[[Title|shown text]]` and Markdown links to a page file; a link may name a page's title, another name of it, or its file name. ordin keeps the links as a graph keyed by page id and shows, on every page, the pages it links to and the pages that link to it. A link to no page is shown as such and starts working when the page is created.
  - **4. Renames keep links working.** A page's file name follows its title. Renaming a page, in ordin or in another app, rewrites the link text in the pages that link to it, at once, each as a new version the person can restore (K9). ordin writes another app's file only if it is unchanged since ordin read it.
  - **5. Other names and tags.** A page may carry other names, unique with titles across the workspace, and tags. Both live in the page's header, so another app reads them too.
  - **6. Suggestions, never silent changes.** In application code, with no model, ordin suggests a link where a page names another page without linking it, and says when two pages look like one subject. Nothing changes until the person accepts; a dismissed suggestion stays dismissed. Pages are never merged.
  - **7. Agents see links under the same rule.** The *read page* tool returns a page's links and the pages that link to it, leaving out every page the session may not read and counting them; *search pages* filters by state, author and tag.
  - **8. A recalled fact names its section.** A fact recalled from a page carries the heading of the section it came from, and opening it shows that section.
- **Context.** ADR-057 already keeps pages as Markdown files with a header beside a search index, the shape Markdown note apps use. Two gaps remained: an edit made in such an app could leave a page with its header read as body text, and pages could not refer to each other, so a reader moved between related explanations only by guessing search words. The user accepted the recommendations to close both (decision 84).
- **Consequences.**
  - The Knowledge Service keeps header keys it does not own, a link graph, names and tags, suggestions and an adoption scan; the File Service gains a move that keeps a file's versions; the Memory Service returns a fact's section; the Knowledge screen shows links, *Pages that link here*, other names, tags and suggestions.
  - New pages' file names keep the title's letters and spaces instead of lower case with hyphens; existing files keep their names until their title next changes.
  - A rename changes pages the person did not open (R-102).
  - 00 — glossary, decision 84; 02 — Knowledge Service, File Service; 04 — Knowledge page; 08 — Knowledge page file; 10 — R-102; 11 — §5.10, §5.6 table, UX-A74.
- **Alternatives considered.**
  - *Links kept only as page ids in the files.* Rejected: another app could not read them.
  - *Rewriting links only after the person accepts.* Rejected by the user (K9): links would be broken until accepted.
  - *Ignoring files created outside ordin.* Rejected by the user (K8): notes made in another app would be invisible to agents and search.
  - *Renaming every existing page file on upgrade.* Rejected: it moves files the person did not touch.
  - *Merging pages that look alike.* Rejected: a merge writes text neither page held; the person decides.
  - *Inline `#tags` in page text.* Rejected: `#` in text is too often a heading, a number or a colour to read as a tag reliably.
  - *A graph view, embeds shown in place, block references, plug-ins.* Out of scope (K7).

### ADR-105: Repeating work on a timetable is started by a standard scheduler outside ordin; ordin is the runtime

- **Status.** Decided by the user, January 2026 (decision 85: "you can still use ordin as runtime but have a standard scheduler sitting outside of it triggering the work"). The mechanism is proposed. **Amends ADR-060** (what the request to IT asks for) **and ADR-074** (makes the outside schedule its stated route for repeating work). **Keeps ADR-011** (nothing is scheduled on the desktop) and decision 41 (the personal server keeps no timetable).
- **Decision.**
  - **ordin keeps no timetable** on the desktop or on an ordin server in the personal profile. This is unchanged.
  - **Repeating work is started from outside.** A standard scheduler the company already runs keeps the timetable and, at each time it names, publishes a NATS event on a subject the organisation profile allows. An event trigger the person turned on starts the saved workflow (ADR-074). ordin is the runtime: the trigger's filter, its limits, the daily limit and every approval apply as they do to any other event. A missed or late event is the scheduler's concern, and the trigger reports the last event it received.
  - **What Ordin offers.** **Every week…**, or asking to repeat work on a schedule, has Ordin say in one line that ordin keeps no timetable and that the company's scheduler can start the work. **Ask IT to set it up…** opens the approval card of ADR-060. When the person has an ordin server whose profile lists NATS subjects, the request names the workflow, the inputs by name only and how often, and asks IT to have the company's scheduler send an event for it. When IT replies with the subject, the person asks Ordin to turn on a trigger for it, through the trigger card of ADR-074. Without such a server, the request asks for a hosted ordin agent with a recurring duty, as before.
  - **The agent profile is unchanged.** A hosted agent's Duty Timer and HTTP intakes stay as ADR-097 and ADR-098 state.
- **Context.** ADR-074 already observed that a regular event from the company's own systems amounts to a schedule kept outside ordin. ADR-060 then sent every request for repeating work to a hosted agent, which asks a company to run a hosted agent for a timetable its existing scheduler already provides. The user stated that ordin can be the runtime with a standard scheduler outside it triggering the work.
- **Consequences.**
  - 00 — G25 and the non-goal on recurring duties gain amendment notes; decision 85.
  - 08 — the repeating-work request may ask for a scheduled event in place of a hosted agent.
  - 11 — UX guidelines §5 (*Every week… goes to IT*) gains an amendment note.
  - The low-level specs that implement ADR-060's request are to be checked by `/sync-check` against this amendment.
  - A scheduler sending events too often is bounded by the per-trigger queue and the daily limit of ADR-074 (R-64).
- **Alternatives considered.**
  - *A timer inside ordin on the personal server.* Rejected: decision 41 and ADR-074; the timetable would live in two places.
  - *Repeating work only through a hosted agent.* Rejected: it requires a hosted agent where the company's scheduler and ordin's event triggers already do the job.

### ADR-106: Jira is a handover place and a workspace link; ordin reaches it with a token the person makes or the company's Atlassian app

- **Status.** Decided by the user, January 2026 (decision 86: Jira as a handover place **and** Jira issues as workspace links, in one change; the open questions' recommendations taken). The mechanism is proposed. **Settles OQ-57** with its option (a), Jira Cloud first. **Amends ADR-092** (a third handover place, item 3) **and ADR-029** (a new kind of workspace link). **Keeps ADR-026** (Confluence is unchanged), **ADR-025 and ADR-073** (one person per installation; nothing of ordin's runs between people) **and ADR-090** (nothing in the handover checks can be waived).
- **Decision.**
  - **1. A Jira project is a handover place.** A lifecycle stage, or a workspace outside a lifecycle, may name a Jira project on the company's Jira Cloud site as its handover place, by the project's address, chosen as a Git or SharePoint place is. The handover flow is ADR-092's: draft, ordin's fixed-code checks, one approval card, the record, receiving, accepting, returning and taking back. A separate "assign in Jira" action that skips the checks is not offered.
  - **2. One issue carries the handover.** It is the issue the workspace links in that project; when the workspace links several, the draft asks which, the most recently linked first; when it links none, ordin creates one, of the project's first issue type that is not a sub-task, preferring one named "Task" (OQ-69), and the approval card says "A new issue will be created in [project]" before anything is written.
  - **3. The receiver and the checks.** The receiver is chosen among the people who can be assigned issues in the project; no address is typed and no company directory is read. *Receiver reachable* means the receiver can be assigned the issue, checked again when the checks run. *Results readable* fails for a result only on this computer, as for every place. Nothing is waived.
  - **4. The record is the issue's comments.** Jira has no conditional write, so the record cannot be one file rewritten under a version check. After the card, ordin adds an **opening comment** holding the note and the record without its events, then **one comment per event**, each a plain line and a code block holding that one event (08 — Interfaces, *Handover record*). Comments only append, so two installations answering at the same moment never overwrite each other (A-44, R-83 hold). Events are read in comment creation order (C-86); the first answer counts and the other person is told what happened first. An event comment counts only when Jira names as its author the person the event names; an edited record comment is ignored and listed in Diagnostics (R-104). Each event carries its own identifier, so a write whose outcome is unknown — including one cut short by a restart, which the Handover Service keeps until it is confirmed (A-56) — is looked for before it is tried again and is never recorded twice. The opening comment stays within Jira's comment size (A-55).
  - **5. Sending.** The order is: create the issue when needed, the opening comment, the *sent* event, the label `ordin-handover`, then assigning the issue to the receiver. A refusal before the *sent* event leaves the draft a draft. Jira's own notification is the notice for a receiver without ordin, so no email is sent. ordin never changes an issue's status, fields or work log (a stage naming a status is a later addition).
  - **6. Receiving and answering.** On the handover check's schedule (A-43), ordin runs one issue search per Jira site for the issues assigned to the person with the label and for the open handovers the person sent, and reads the comments only of issues changed since the last read (A-54). A handover found appears as "Handed to you". **Accept and start** adds the *accepted* event and links the issue and every Git or SharePoint result to the receiving workspace. **Return to…** and **Take it back…** add their event and assign the issue back to the sender. A receiver without ordin answers in Jira: assigning the issue back to the sender with a comment reads as returned, with the comment as the reason, and a status in Jira's Done category as accepted; the sender's ordin only reads these and writes nothing for the receiver.
  - **7. A Jira issue is a workspace link.** From **Add to this workspace**, by pasting the issue's address or searching, on the desktop. The issue's summary and description, each comment except ordin's record comments, each attachment and one list of its sub-tasks and linked issues become read-only working-copy items, each recording the version read (the issue's last-updated time, a comment's, an attachment's identifier). A change in Jira is shown as changed at its origin on the existing origin check and never overwrites the local copy. Removing the link removes the local copy only. Nothing read from Jira — description, comments or record — is followed as an instruction; a line that tries is marked (R-81).
  - **8. Signing in needs no IT.** The person connects Jira with the email address of their Atlassian account and an API token they make themselves, kept in the keychain and sent with Basic authentication to the site (C-87). When the organisation profile names the Atlassian OAuth app IT registered for Confluence, **Sign in with Atlassian** is offered too, as a separate Jira grant through the same app; no new profile key is added. When the company blocks tokens and has no app, Jira stays off with a plain message (R-103).
  - **9. Where it runs.** On the desktop only. The ordin server and hosted agents have no Jira: Atlassian has no device grant, and the server has no safe way yet to receive a token. Jira Cloud only; the site's base address stays configurable so Jira Data Center can follow (A-53).
  - **10. The Jira Adapter.** A new edge component, modelled on the Confluence and SharePoint adapters, holds a typed client over Jira's REST API version 3 and owns nothing but the calls (02 — Architecture). It builds every Jira query itself from typed requests and turns Markdown into Atlassian Document Format and back.
- **Context.** Software delivery teams already move work by assigning Jira issues. ADR-092 put tracker handovers off because each tracker needs its own connector, and OQ-57 recommended one per tracker after Git and SharePoint. Without one, a person in a team that runs its work in Jira hands over twice — once in ordin and again by reassigning the issue by hand — and the receiver copies the issue's content into their workspace before Ordin can use it. Today Jira reaches ordin only as a tool an agent may be given through an MCP connection; ordin itself cannot read or write it.
- **Consequences.**
  - 00 — decision 86, glossary terms, the note on working together; 01 — Jira as an external system; 02 — Jira Adapter, Handover Service, Workspace Service, Identity and Credentials; 03 — sequence *Hand work over on a Jira issue*; 04 — Source link, Handover, Handover place; 07 — Migrations, a rebuild that only widens a closed list of values counts as adding; 08 — Handover record on Jira, the lifecycle part's place type, the handover link, Jira Cloud REST interface; 10 — OQ-57 settled, OQ-69, A-53 to A-56, C-86 to C-88, R-103 to R-105, the Jira dependency; 11 — §5.13, §5.28, a vocabulary row, UX-A75.
  - The stored lists of handover place types and workspace link kinds grow by one each, through forward-only migrations.
  - The record keeps `record_schema: ordin.handover/v1` and adds only optional keys, so an installation that does not know Jira places still reads a record written from one.
  - The profile key named for Confluence's OAuth app also serves Jira's sign-in; renaming it is left for a later change.
  - Jira as an agent tool stays as it is today, through an MCP connection the company or person adds.
- **Alternatives considered.**
  - *A separate "assign in Jira" button.* Rejected: it would reach another person without the checks or the approval card of ADR-092.
  - *The record as an issue entity property, read again after each write.* Rejected: a property is overwritten whole, so two installations writing at once could lose an event, and it is invisible to people reading the issue.
  - *An attachment per event.* Rejected: attachments clutter the issue for its readers and are removable by anyone with edit rights, with no gain over comments.
  - *Signing in only through an OAuth app IT registers.* Rejected: it makes Jira wait on company set-up; the app is offered as an extra, never required.
  - *Jira Data Center now.* Not chosen: it needs personal access tokens and other endpoints, and no pilot company is known to use it; the configurable base address keeps the way open.
  - *Uploading results to the issue.* Not chosen: results stay where they live and are linked, which keeps one source of truth and the *results readable* check.

### ADR-107: What must happen on every run is configured and done by ordin, never left to the model

- **Status.** Decided by the user, January 2026 (decision 87: a hosted agent that answers in team channels runs on the agent profile; a content check is applied by ordin itself; anything that must happen on every run is done by ordin in fixed code and switched on in configuration; the whole-question time is accepted for now and measured on the real server before the first release). The mechanism is proposed. **Amends ADR-098** (item 3, *Model policy*: no closed list of providers; item 4: a content check and configured context steps) **and ADR-097** (item 3, *Stated at start*: a content check is named beside its connection). **Keeps ADR-036** (ordin leaves the inner loop to the vendor harness), **ADR-044** (the organisation key reaches the harness child only) **and ADR-100** (the owning module declares the trait its provider implements).
- **Decision.**
  - **1. Fixed code, switched on in configuration.** Whatever a hosted agent must do on every run — check the question or the answer, gather context before the model — is done by ordin in fixed code, in a fixed order, and switched on and sized in the agent configuration. None of it is left to the model's choice or to the package's instructions. The model still chooses what to open after a search; that choice is not one ordin can state in advance.
  - **2. Any model provider, by declared settings.** The agent configuration declares, under `model.environment`, the environment settings the harness's own provider mode needs: each a name and either a literal value or a secret named by reference. ordin checks only their form, refuses a name it sets or forbids itself, resolves secret values from the instance's secret source and sets every declared name in the harness child's environment only; nothing is inherited, logged or shown, and the start-up report lists the names without values. ordin contains no provider-specific code and names no provider. The organisation key stays required for the default provider and becomes optional for any other. Local test T1 (C-77) showed that settings given in the child's environment are honoured with no sign-in.
  - **3. A content check.** A control kind `content-check` sends the question (point *input*), the final answer (point *output*) or both to a checker the organisation supplies behind one operation of a declared HTTP connection. ordin fixes the contract and names no vendor: it sends the point, the run and the text, cut to a stated length, and expects back *allow* or *block* with an optional category. A block on input ends the run before any model call; a block on output either ends it or retries once without the gathered context, as the model policy's retry after a provider's block does. Both give the configured safe reply. A checker that does not answer in time blocks by default, or allows and logs when the configuration says so. The checker operation is called by the Rules Engine only and is never offered to the model as a tool. Checks are counted by point and verdict.
  - **4. Context steps.** The context a hosted chat run receives before the model is a configured, ordered list of steps, each with its own size limit, each run by ordin, and each left out and logged when it fails or finds nothing. The first three steps are the ones ordin already had — the thread's messages, its images and the channel's documents — and a configuration written with the earlier fixed keys loads as those three steps unchanged. New steps: a knowledge lookup of the question, recent messages chosen by a keyword rule, and a note on which source wins when the channel's documents and the knowledge found disagree, added only when both supplied something. The run summary records every step's outcome and size, including a step that returned nothing.
  - **5. Hosted chat in Slack.** Chat-history knowledge reads thread replies as well as thread starts; a message whose content is held in rich blocks keeps that content; a hand-off can name a target channel and people or groups to mention, and is offered only when it has somewhere to go; link buttons can come from a list setting whose entries are matched against the question by keywords in fixed code, including Slack workflow buttons; canvases are read from the channel and from its tabs, by type, within the size limit, and converted to text; an asker-only reply can be replaced after a button is used; and a *working* signal shows while a question's run is queued or running.
  - **6. Deployment.** The Helm chart takes pod annotations, shared and per role, so a deployment can attach its own controller that restarts pods when a mounted Secret changes. No sidecar and no model gateway are added; ordin runs with nothing in front of the model provider.
  - **7. Not carried.** A pinned sampling temperature (Claude Code exposes none; C-77). Passive listening and automatic responses in channels stay out (ADR-097 item 3: only addressed messages are answered).
- **Context.** A hosted agent that answers operations questions in team channels was assessed against the agent profile. Most of its behaviour already had a home in the specifications. What did not was of one kind: behaviour that must happen on every run but depended on the model choosing it, such as searching before answering or screening a question, or that the specification limited to one vendor's names for model providers. A deployment that runs on a provider other than the default could not start, and a content guardrail had nothing in ordin to attach to once no model gateway sits in front of the harness.
- **Consequences.**
  - 00 — decision 87, glossary terms *Content check*, *Context step*, *Provider setting*, *List-backed link*, *Working signal*; 02 — Rules Engine, Session Supervisor, Connection Binder, Channel Gateway, Knowledge Service, Slack Channel Adapter responsibilities; 07 — the *Fixed-code decisions* stance extended to hosted agents' runs; 08 — the agent configuration's `model.environment`, `content-check` and context steps, the content-check contract, the Slack interface row; 10 — C-74 widened, C-77 settled, OQ-70, OQ-71, C-89, R-106.
  - The whole-question time measured on a laptop was 3 to 5 seconds, several times a single direct model call, because each question starts a harness process. It is accepted for now; it is measured on the target server before the first release, and the engine choice of ADR-098 item 3 is reopened only if that figure is unacceptable (OQ-70).
  - Every check adds one call per point to each question's time.
- **Alternatives considered.**
  - *Let the harness or a gateway attach the guardrail.* Rejected: with no gateway nothing attaches one, and the user decided ordin is coupled to no other product.
  - *Keep a closed list of providers and add each vendor's settings to ordin.* Rejected: ordin would carry vendor code for every provider and name third-party systems in its own design.
  - *Leave the first search and the screening to the package's instructions.* Rejected: a model may skip an instruction; what must happen on every run is ordin's.
  - *A per-step failure rule.* Not chosen: the only rule asked for is to leave the step out and log it, so a key would have one value.

### ADR-108: An earlier conversation can be continued; continuing puts the current one away, as Clear does

- **Status.** Decided by the user, January 2026 (decision 88). The mechanism is proposed.
- **Decision.**
  - **Continue this conversation**, on an earlier conversation's tab and on its node in the tree, makes it the workspace's live conversation again. A workspace still has one live conversation (ADR-027): the current one is put away, kept read-only among the earlier conversations.
  - **Putting away follows Clear's rules** (ADR-049, UX-A57): the same confirmation with **Keep the key points first**, nothing deleted, not offered while a crew Ordin leads is running, and items still waiting move into the conversation that becomes live. A current conversation with no messages is put away without asking.
  - **A new conversation that refers to the one it continues.** The Ordin Host opens a new live conversation linked to the one continued, and the two are shown as one: one entry in the conversation picker and in *Earlier conversations*, with the earlier messages folded under "Continued [time]". Earlier conversations stay records that nothing appends to.
  - **What Ordin works from.** The new session is seeded as every session is (ADR-062), plus the continued conversation's history quoted as data: its stored summary when it has one, then its most recent messages, at most 32,000 characters together — the bound Ordin already uses when asked about an earlier conversation. No extra model turn writes a summary first.
  - **A conversation picker** in the conversation panel's header names the live conversation and lists the earlier ones, newest first, and **Clear**. Choosing an earlier one opens it to read; continuing is the button on its tab. The **Earlier** button stays.
  - **Harness-independent**, as ADR-049: session start and seeding only, the same on Claude Code and the Copilot CLI.
- **Context.** People come back to work they set aside. Until now an earlier conversation could only be read or asked about, so its messages came back as quoted material inside another conversation. The user asked for conversations that can be picked up from a history list, as in a coding assistant's chat panel, and accepted continuing only as the first step.
- **Consequences.**
  - Amends ADR-049's revision 0.36.0 note: an earlier conversation is "kept to read" until the person continues it; Ordin still never draws on one by itself.
  - A continued conversation's first seed is larger, by at most 32,000 characters, once per continue, on the person's own plan (R-31).
  - A conversation removed by the retention of UX-A58 drops out of a later conversation's folded history; the later conversation stays.
  - Waiting items move by the same rule on summarising, clearing and continuing, which the specification now states once.
  - 11 — UX guidelines §5.12 changes (UX-A76); the Ordin Host gains one command; the prototype follows (12 §4, UI-D109).
- **Alternatives considered.**
  - *Reopen the earlier conversation in place.* Not chosen: it appends to a record kept as finished, and breaks the rule that a live conversation has no end time.
  - *Have Ordin summarise the conversation before continuing.* Not chosen: one extra model turn each time, and it depends on reading context use (C-29).
  - *Switch at once when an earlier conversation is chosen in the picker.* Not chosen: the person sees what they go back to before Ordin's context changes, for one more click.
  - *Several live conversations, or a New conversation button.* Deferred (decision 88; ADR-046 left side-by-side conversations for later).

### ADR-109: People make, edit, export and import the interface's language files; Ordin is told the person's language and changes it when asked

- **Status.** Decided by the user, January 2026 (decision 89). The mechanism is proposed.
- **Decision.**
  - **Ordin knows the language.** The person's interface language reaches Ordin as data: one line in the voice section of its seed, and one line before the next turn when the language changes, whether through Settings, words or a newer organisation profile. Ordin's package is not rebuilt per language, and the change needs no new session. Ordin writes its messages, summaries and the blocks it fills in that language; approval cards, errors and other fixed text still come from the catalogue.
  - **Changing the language in words.** "Speak French" changes the interface language, as "be more formal" changes the voice settings (ADR-063). It is a setting: no approval card. One Ordin tool carries every setting changed in words, voice and language alike, and it changes nothing when the words came from other people, such as a message in a linked Slack channel.
  - **Ordin may offer a missing language.** Asked for a language ordin has no wording for, Ordin says so in one sentence and offers to draft it from the en-GB wording, showing the estimated use of the person's plan first. Nothing is drafted until the person accepts. **Make a new language…** in Settings does the same.
  - **Drafts pass one fixed check.** Every message of a draft must parse, keep each argument of the en-GB message, use only the plural forms its language has and refer only to what en-GB holds. A message that fails is left out and shows in en-GB, and the person is told how many.
  - **Review before use is limited to the safety wording.** Before a drafted or imported language can be used, the person checks the messages that appear on approval cards and in the statements of who acts and where it goes. These are a small, fixed set marked in the en-GB wording. Every other message may be used unchecked, labelled "Drafted by Ordin" or "Imported". Once in use the wording is fixed: no model is called to show a message, and nothing changes it without the person's action.
  - **Editing a language.** The person opens any language but en-GB, finds messages by text or by screen area, and changes a message's wording. The same check applies on saving; a refused edit names the message and the problem in plain words. The change shows at once, without a restart. A language that comes with ordin keeps its shipped wording underneath: edits are kept per message, so new and corrected shipped messages still arrive with updates, and **Restore** brings back the shipped text of one message or of the whole language.
  - **Export and import.** A language is exported as one plain Fluent file that names its language, the ordin version it was made for and how it was made, readable and editable in any text editor. Importing runs the same check: messages this installation does not know are dropped and counted, missing ones show in en-GB and are counted, and the safety wording is checked before use. A file that cannot be read is refused in one sentence and nothing changes.
  - **After an update.** When a new version adds messages, a language the person made or imported shows how many are still in English and offers to ask Ordin to draft them, with the same check and review. A message an update has made unusable shows in en-GB until the person changes it.
  - **The person's own languages are always theirs.** The company's list of offered languages governs the languages that come with ordin and the fallback; it does not hide a language the person made or imported. No organisation-profile key is added.
  - **Right-to-left** languages the person makes or imports draw right-to-left, as shipped ones do.
  - **Where it lives.** Settings gains a **Language** section after **How Ordin talks to you**: the picker, **Edit wording**, **Export…**, **Import…** and **Make a new language…** (11 §5.19a). A personal ordin server keeps its own languages; a language made on the desktop is not copied to it, its messages show in en-GB there, and the person imports the file on the server from **Settings › Servers**.
  - **Local only.** No language file is fetched from the network or carried in the organisation profile; import reads only a file the person picks, and export writes only where the person chooses. The en-GB wording stays compiled in, cannot be edited, and is what every check runs against. The agent profile has no person's languages.
- **Context.** ADR-042 promised that Ordin converses in the person's locale, and ADR-063 put the person's language in Ordin's fixed instructions, but nothing told Ordin's session which language that was. People whose language ordin does not ship had to wait for a release, people could not fix wording they disliked, and colleagues could not share their wording across installations. ordin is self-service and local-first, so languages are too.
- **Consequences.**
  - Amends ADR-042: catalogue text is no longer only compiled in, and model translation, rejected for interface text at run time, is accepted as a draft the person asks for. ADR-042's reason still holds in substance: approval and who-acts wording is never shown before the person has checked it, and is never translated again at display time.
  - Amends ADR-063's **Words**: the interface language changes in words too, through the same single tool as the voice settings.
  - Localisation gains a person-owned layer in ordin's own storage and a fixed check; the Ordin Host depends on Localisation, states the language in the seed, offers the settings tool and runs drafting as a background job on the person's plan, through the Usage Planner.
  - A drafted or imported message outside the safety wording can still mislead (R-107); it is labelled, and can be edited or restored.
  - Plural forms come from the CLDR data compiled into the build (A-57); whether drafting keeps most messages through the check on each harness is C-90.
  - 11 — UX guidelines §4, §5.19 and §5.19a change (UX-A77); 08 — Interfaces gains the language file; the prototype follows (12 §4, UI-D110).
- **Alternatives considered.**
  - *Rebuild Ordin's package for each language, with the language in its fixed instructions.* Not chosen: a reinstall on every change, and nothing for a profile-driven change.
  - *Review every message before a drafted or imported language is used.* Not chosen: thousands of messages make a new language take days to become usable, for wording that decides nothing.
  - *No review, with the safety wording left in English until checked one by one.* Not chosen: approval cards would mix two languages for as long as checking takes, and a person could use a language without ever seeing its approval wording.
  - *Edits to a shipped language kept as a full copy.* Not chosen: corrections and new messages in updates would no longer arrive.
  - *Let the company's list of offered languages hide the person's own languages.* Not chosen: it would make a self-service feature depend on the company's set-up.
  - *A shared online library of language files.* Not chosen: ordin is local-first; files move by export and import.

### ADR-110: A workspace reads several teams' knowledge, shares to one, and a person adopts across

- **Status.** Decided by the user, January 2026 (decision 90). The mechanism is proposed. **Amends ADR-103** (one own folder, any number of read folders; one repository copy per repository). **Keeps ADR-103 item 8** (access is the repository's), **ADR-029** (every send is approved) and the rule that ordin needs no company set-up: folders the company names are optional.
- **Decision.**
  - **1. Read many, share to one.** A Team Knowledge workspace keeps one *own* folder and may read any number of other teams' folders, read-only, up to 20. Each read folder is checked, got and reported on its own, with its own counts, sharer names and problems. The person adds one with **Read another team's knowledge…** and removes it with **Stop reading**. The repository's access decides whether the person can read it.
  - **2. Nothing travels between folders except by Adopt.** Nothing got from a read folder is written to the own folder or any other folder. A correction the person makes to such a fact stays on the computer and leaves with the folder. A read folder's pages are read-only and kept apart from the workspace's own pages. **Stop reading** removes the folder's facts and pages from recall and search and changes nothing in its repository.
  - **3. Adopt.** On a fact or page from a read folder, the person chooses **Adopt into [own folder]…**, sees the text, may change it, and confirms. The copy is a new item of the own folder with a new id. It passes the personal-data scan, the instruction check and the write gate before it is kept; anything that may hold personal data or reads as an instruction is refused in one sentence and nothing is written. Each adopted item has an adoption record (the read folder, the source item and its version, who, when, whether the text was changed), shared with the item at the next **Share** behind the usual card and listed in the review request where the own folder's branch requires review. Adopt itself writes nothing to a repository. Only a person can adopt: no tool, command reachable from a run, or workflow step does it, and Ordin does not suggest items to adopt in this change.
  - **4. The source changes.** When a later get brings a new version of an adopted item's source, or removes it, the copy is marked "Changed at its source since you adopted it" or "Removed at its source". The copy changes only when the person chooses **Adopt again**; **Keep as it is** clears the mark until the source changes again. Any installation that reads the source folder raises the mark; one that does not shows "adopted from" without it.
  - **5. Agents know where knowledge came from.** Every recalled fact and searched page carries its folder's name and the description of the deepest matching path the team wrote, and items from a company-named folder say so. Agents may restrict a recall to some folders, which is applied before the search, and may send up to four sub-queries marked keyword or meaning.
  - **6. Small folders are not drowned out.** The memory component fuses its search signals by rank within each folder (reciprocal rank fusion, memory ADR-020), so each folder's best matches reach the reranker whatever the folder's size. The final order is still the reranker's, weighted by recency.
  - **7. Folders the company names (optional).** The organisation profile may name read folders, each applied to every Team Knowledge workspace or also to every workspace that links a code repository. They are shown *Set by your company*, without **Stop reading** unless the entry allows it, read with the person's own sign-in, and never granted access by the profile. A folder the profile marks as needing review is read only while ordin can see that its changes require review; where the person's access does not show the rule, the folder is not taken in and the status says why. A folder the profile stops naming leaves recall; adopted copies stay. Without a profile, or with no such key, nothing changes.
  - **8. Scale.** One copy of each repository is kept per installation and fetched once per check for every folder in it; checks start at a random point in the 15-minute interval; a read folder may be a part of another team's folder; a workspace has a soft limit of 100,000 facts across its folders (A-58). No sharded store is added to the desktop or the personal server.
  - **9. New folder files, the same format.** A folder may hold a descriptions file and a recall-check file, and holds one adoption-record file per adopted item. Format 1 stays; installations before revision 0.59.0 ignore all three (A-59).
- **Context.** ADR-103 lets one team build up shared knowledge, but work that depends on another team still needs a person to ask or to copy. The user asked for knowledge to reach across teams with no server, no company set-up and no new access system, and for a way to promote a lesson from one team to a group that curates across teams with its origin kept. Searching several folders on one computer is the setting the open-source on-device search engine qmd (MIT) was built for; its rank fusion, per-collection descriptions and typed sub-queries are the ideas taken, and no code is copied.
- **Consequences.**
  - The Team Knowledge Service holds several folders per workspace, reconciles company-named folders on each verified profile, reads branch review rules with a three-way answer (required, not required, cannot tell), and treats "cannot tell" as review required when sharing to the own folder. Reading a branch's classic protection on GitHub needs admin rights, and GitLab Free cannot require approvals, so a company folder that needs review is usable only where readers can see the rule (C-92, R-109).
  - The Memory Service tags each fact with its folder, never exports a read folder's facts, removes them in one step when the folder goes, and adopts a fact through the memory component's checks; the memory component gains collections, per-collection rank fusion, typed sub-queries and a content check (memory ADR-020).
  - The Knowledge Service keeps read folders' pages in their own read-only table, links within each folder, and adopts a page as a new own page.
  - The organisation profile gains `team_knowledge.read_folders[]`; installations before this change keep their last verified profile when it appears (R-110).
  - 00 — decision 90, glossary; 01 — the administrator names read folders; 02 — Team Knowledge Service, Memory Service, Knowledge Service, Organisation Profile; 03 — adopt and the company-folder sequence; 04 — Team folder, Adoption record; 07 — team knowledge policy; 08 — three folder files and the profile key; 10 — A-58, A-59, C-91, C-92, R-108 to R-111; 11 — §5.31a, UX-A78; 12 — UI-D111.
- **Alternatives considered.**
  - *Writing to several folders.* Rejected by the user: read-only.
  - *A server, an index service or a shared vector store.* Rejected: decision 83 D1 and the no-company-set-up rule.
  - *Adopted items as links that follow their source.* Not chosen: a curated folder would change whenever another team's folder changed, without a person deciding.
  - *Letting agents or Ordin adopt.* Not chosen: content in one team's folder could tell an agent to copy itself into another team's folder.
  - *An Ordin tool that proposes an item to adopt, as a card.* Not chosen for this change: leaving it out keeps every step towards adoption with the person; it can be added later as a card that only opens the adopt dialog.
  - *Sharding the desktop's store.* Not chosen: each team folder already is a shard whose permission is the repository's; an organisation-wide index belongs to a hosted knowledge agent.
  - *Copying another team's pages into the person's knowledge folder.* Not chosen: titles would clash, another Markdown app could edit them, and a share could carry them on.
  - *Treating "cannot tell" as "no review needed" for a company folder.* Not chosen: the company asked for review because the content reaches every agent.

### ADR-111: Agentic memory is a core capability: one memory every agent uses, a graph that is written and used, and all of it visible to the person

- **Status.** Decided by the user, January 2026 (decision 91). The mechanism is proposed. **Amends ADR-030** (every agent session remembers, not only Ordin and crew members; facts carry things and links; Ordin writes summaries; the person sees the whole memory). **Keeps** memory ADR-015 (no model call inside memory: the agent states what a fact is about), ADR-057 (knowledge pages are written only by Ordin and the person, cite versioned sources and do not fade), ADR-090 item 3 (each agent's memory rules), ADR-097 item 5 (a hosted agent's memory by scope), ADR-103 and ADR-110 (team knowledge), and the rule that nothing new leaves the computer. *Amended in revision 0.61.0 (ADR-112, ADR-113): item 8 — a page-backed fact's trust follows its verification (human-reviewed, machine-confirmed, unverified) above document trust, and a deprecated page's facts are recalled only when nothing else matches; item 3 — in a workspace connected to a front door, recall adds the front door's answer as one more ranked list when its own answer is not confident; item 6 — **What Ordin used** names each knowledge-agent fact's agent, repository, file, commit and "as of".*
- **Decision.**
  - **1. One memory, one model, every agent.** Agentic memory is a named core capability (G48). Ordin, crew members and agent task sessions remember, learn and forget through the one memory under one model — kinds of memory (something that happened, something known, a summary), write authority by class, scopes, fading and replacement — and under each agent's own memory rules. An agent task session is offered *remember* and *recall* only when its agent has memory rules; an agent without them gets none. The desktop, the personal server and the agent profile share the model and differ only in their store and scopes, so a later knowledge agent builds on it and adds only document ingestion.
  - **2. The graph is written.** A fact may carry the things it is about (each a name and other names) and the links it states between them ("Borealis" supplies "packaging", with a confidence). The memory component resolves things as it resolves names today, stores each link with its own validity, keeps one live link for the same two things and words however many facts state it, and closes a link — never overwrites it — when a fact says it no longer holds or when every fact that stated it is replaced or retired. Link words are free text, lower-cased and merged with a near-identical existing kind. Names and link words pass the personal-data scan and the instruction check as statements do. Memory makes no model call (memory ADR-021).
  - **3. The graph is used in recall.** Recall finds the things a question names, by name and other name, with no filter from the caller, adds the facts that name them, and follows live links one step on the desktop (two at most by configuration) to add facts about linked things — each as one more list fused by rank with the others (memory ADR-020's fusion). A link is followed only when the reader may read a current fact that states it, so nothing crosses a workspace or a scope. Recall stays within its time limit; the expansion is bounded by steps and a cap on candidates.
  - **4. Summaries are Ordin's.** At the end of a run that wrote five or more facts about one thing, with no summary of that thing in the last day, Ordin is given one fixed turn to write a summary, kept with the facts it was made from (at most three a run). The memory component makes no summaries of its own: it has none since memory ADR-015, so there is nothing to switch off. Forgetting a fact also forgets every summary made from it, and the person is told so before confirming.
  - **5. Fading is shown, never hidden.** A fact that happened and has not been used for the last fifth of the time memory gives it before letting it go shows "Fading — not used since [date]" with **Keep**, which makes it something known that no longer fades. Something known and summaries do not fade.
  - **6. The person sees the whole memory, in this step.** In *What Ordin remembers*: **Things**, searchable, each with its facts and its links in words ("Borealis supplies packaging · since 3 Oct"); each fact's kind in words, when it was true, what it replaced, its history and, for a summary, what it was made from; fading with **Keep**; on a link **Correct** and **Forget**; on a thing **Forget everything about this…** with the counts first, and **Same as…** to make two things one. Under an answer that used memory, **What Ordin used** lists the facts and links recall returned for it, each with **Correct** and **Forget**, read from a record of recall ids that holds ids only. A run's progress lists what was remembered during it, by which agent. Things and links are lists, not a drawn diagram. Nothing changes without the person's choice; every action is journalled with ids and counts, never statements or names.
  - **7. Hosted agents.** A hosted agent's memory is seen by its owners, scope by scope, from the operator screen through the hosted agent's Caller API operator route; channel members see only what an answer in their channel used, through a **What I used** reply in that channel.
  - **8. The wiki is the trusted layer.** Knowledge pages stay what ADR-057 makes them. When a page section is written into memory, the page's title and other names (ADR-104 item 5) name its thing, and a link between two pages becomes a link between the things they explain, closed when the link is removed and the page next indexed. A thing whose name matches a page's title or other name shows "Explained in [page]" with **Open the page**; a fact from a page shows the page and section. In recall a page-backed fact ranks above an otherwise equal document-backed one, through the trust each source already carries (pages 1.0, documents 0.5).
  - **9. Additive change.** New columns and tables only, on SQLite and PostgreSQL; facts written before revision 0.60.0 stay visible, each attached to the thing its subject named, with no links. Team knowledge folders keep format 1: link files, empty until now, carry links, and on import a link is tied to the facts of the same get that name both its things.
- **Context.** Memory was recall, embedded (ADR-030), but its graph was half used: no link was ever written, the graph signal ran only when a caller passed a thing, and nothing in the design named memory as a capability every agent builds on. The user wants ordin's agents to have agentic memory as recall describes it, wants the knowledge agent to build its enterprise knowledge graph on the same memory, wants every agent to get memory, and wants the person to see all of it, the graph included, now.
- **Consequences.**
  - The memory component gains things and links on a remember, link resolution and closing, matching things from the query text, bounded link expansion as a ranked signal, source trust in the final order, a fading mark, a fact's history and origins, listing things with their facts and links, and the person's operations — keep, forget a thing with proof, make two things one, correct or forget a link — the last group only for the in-process caller (memory ADR-021; ordin change M-9). Its decay constant is read in seconds, so ordin sets it to ten days in-process (A-60).
  - The Memory Service maps the new fields and views, offers memory to agent task sessions with memory rules, keeps the recall record, and serves the new Service API commands; the Session Supervisor registers each task session's memory rules; the Ordin Host carries things, links and summaries in Ordin's *remember*, runs the end-of-run summary turn and records the recalls each answer used; the Knowledge Service gives each page's title, other names and links when it indexes the page and answers which page explains a thing; the MCP Front's memory tools change shape; the Remote API serves a hosted agent's memory to its owners; the Channel Gateway answers **What I used**; the screens draw the new lists and dialogs.
  - 00 — G48, decision 91, glossary; 02 — Memory Service, Ordin Host, Knowledge Service, Session Supervisor; 03 — remember with things and links, recall with link expansion, summary at the end of a run, forget everything about a thing; 04 — Memory fact amended, Thing, Link, Recall record; 07 — agentic memory stance; 08 — MCP Front and memory component rows; 10 — A-60, A-61, C-93, C-94, R-112 to R-115; 11 — §5.10, vocabulary, the screens table, UX-A79; 12 — UI-D112.
- **Alternatives considered.**
  - *A new memory service or a separate graph store.* Rejected: "first class" means one memory every caller uses, not another store; the graph already lives in the memory component's tables.
  - *Memory extracting things and links from statements with a model.* Rejected: memory ADR-015 — memory makes no model call; the agent that wrote the fact knows what it is about.
  - *A fixed list of link kinds.* Not chosen: kinds differ by team and work; free words merged when nearly the same keep the list short without a vocabulary to maintain.
  - *Expanding two steps by default.* Not chosen: the cost on the desktop's store is unmeasured (C-93); one step answers "who supplies packaging?" and two stays available by configuration.
  - *Keeping a summary when one of its facts is forgotten, marked "made from a forgotten fact".* Not chosen: a summary may restate the forgotten fact, so forgetting must reach it; the person is told which summaries go before confirming. This departs from the change request's wording and is recorded for the user's review.
  - *A drawn diagram of things and links.* Not chosen for this change: lists are accessible and searchable; a diagram can be added later.
  - *Letting agents correct, forget or merge.* Rejected: those are the person's (ADR-030).

### ADR-112: ordin's knowledge wiki is an Open Knowledge Format bundle; ordin writes and reads the Open Knowledge Format 0.2

- **Status.** Decided by the user, January 2026 ("also ensure OKF is first class citizen", then "yes, apply recommendations", decision 92). The mechanism is proposed. **Amends ADR-104** items 1–3 (format keys beside ordin's; `index.md` and `log.md` reserved; standard Markdown links written), **ADR-057** (pages carry a type, a description, who generated them and who verified them; trust is derived from verification) **and ADR-103, ADR-110** (team folders carry an index and a log; a read folder may be any bundle of the format, a glossary included). **Keeps** ADR-104 item 1's rule that keys ordin does not own are kept as written, ADR-057's rule that only Ordin and the person write pages, and the rule that ordin needs no company set-up. Buildable on the desktop before knowledge agents exist (ADR-113).
- **Decision.**
  - **1. Every page ordin writes is a concept.** The Open Knowledge Format (version 0.2) keeps knowledge as a tree of Markdown files with YAML headers, best kept in Git; `type` is its one required key. A page file keeps ordin's keys (`ordin_knowledge_page`, `page_id`, `written_by`, `last_changed`, `shared_by`, `shared_at`) and always carries `type`, `title`, `description` (one sentence, written with the page), `tags` and `generated` (`by: ordin/<version>`, `at`). Each `sources` entry keeps `origin`, `location` and `version_read` and gains `resource`, `id` and `last_modified`. Other names stay in ordin's key `aliases`, which other readers keep as an unknown key.
  - **2. Type.** Ordin picks one of `Explanation`, `How-to`, `Decision`, `Reference` and `Glossary`, `Explanation` by default; the person can change it beside the tags. Any other value read from a bundle is kept as written.
  - **3. Verification.** A page the person writes or edits, or whose update they accept with **Use the update**, gains a `verified` entry `human:<id>`, where the id is the person's Git host login — the identity a share already reveals through `shared_by` — never an email address. Trust is derived as the format says: no verifier is *unverified*, programs or processes only is *machine-confirmed*, any `human:` verifier is *human-reviewed*.
  - **4. Reading any bundle.** ordin reads concepts it did not write wherever it reads knowledge: a workspace's knowledge folder, a team folder and every repository a knowledge agent reads. A concept becomes a page and, in memory, a thing named by its title, of the kind its type names, with its description and tags; its links become links between things; its `sources` become provenance; each body section becomes facts whose source is the concept and section. Unknown types, unknown keys, broken links and a missing `index.md` are never errors.
  - **5. Signals rank and mark.** In recall and in a knowledge agent's ranking, a human-reviewed concept ranks above a machine-confirmed one and that above an unverified one, before the page-over-document rule. `status: deprecated` concepts are recalled only when nothing else matches and are shown "Deprecated"; `draft` ones are shown "Draft"; a concept whose `stale_after` has passed shows "may be out of date", as a page with a changed source does.
  - **6. Reserved files.** `index.md` and `log.md` are never pages. ordin writes an `index.md` at the root of a workspace's knowledge folder and of a team folder, with `okf_version: "0.2"` and one line per page with its description, regenerated when pages change. On each share to a team folder it adds one dated entry to `log.md` naming what was created, updated or deprecated, without statement text.
  - **7. Links.** When ordin writes or rewrites a link it writes a standard Markdown link to the page's path from the folder root (`[Title](/path.md)`). It still reads `[[Title]]` links and never rewrites one the person typed unless a rename requires it.
  - **8. Round trip.** Rewriting a page keeps every key and every `sources` entry field ordin does not own, in its original order and form.
  - **9. Scope of the format.** ordin implements the format and copies none of its specification's text. It reads a version 0.1 bundle only as far as 0.2 asks readers to (`timestamp` as `generated.at`, a `# Citations` section as sources). Attested computations are read and shown as concepts and never run. A future major version is read only after a decision to adopt it. Until the format's licence is confirmed (C-97), documentation says ordin "writes and reads the Open Knowledge Format 0.2".
- **Context.** ordin's page files were already Markdown with YAML headers, close to the format but without its one required key, and an `index.md` or `log.md` in the knowledge folder would have been adopted as a page. Making the wiki a bundle of an open format lets any tool that reads the format read ordin's pages, lets ordin read other teams' bundles and glossaries with no conversion, and gives knowledge agents (ADR-113) one shape to read pages as pages. The format prescribes no storage, serving, query or access control, so it adds no service.
- **Consequences.**
  - The Knowledge Service writes and reads the format's keys, keeps unknown `sources` fields, derives trust and marks, writes the folder's `index.md`, writes standard Markdown links and adopts other producers' concepts; its reader and writer are the ones the Knowledge Reader uses. The Memory Service ranks page facts by trust tier and suppresses deprecated ones. The Team Knowledge Service writes `index.md` and `log.md` and reads a plain bundle as a read folder. The screens gain the page's type and description and the Draft and Deprecated marks.
  - A page written before revision 0.61.0 opens unchanged and gains the new keys at its next save.
  - 00 — G49, decision 92, glossary; 02 — Knowledge Service, Memory Service, Team Knowledge Service; 03 — ordin writes a page as a concept; 04 — Knowledge page, Glossary concept; 07 — Open Knowledge Format stance; 08 — Knowledge page file, Team knowledge folder, Open Knowledge Format 0.2; 10 — A-66, C-97, R-122; 11 — §5.10 (Knowledge: type, description, marks) and §5.31a (glossaries as read folders), vocabulary, UX-A80; 12 — UI-D113.
- **Alternatives considered.**
  - *Keep ordin's own header only.* Rejected by the user: knowledge would stay readable only by ordin, and other teams' bundles would need converting.
  - *Write `[[Title]]` links as well.* Not chosen: standard Markdown links are followed by every reader of the format and every Markdown app; `[[Title]]` is still read.
  - *A typed link vocabulary in page files.* Not chosen: the format's links are untyped Markdown links; typed links stay in memory, written by agents (ADR-111).
  - *Write `human:` verifiers as email addresses.* Rejected: the file is readable by everyone the repository allows; the Git host login is already visible through `shared_by`.
  - *Run attested computations.* Not chosen: it would need the sandbox and a decision of its own.

### ADR-113: Knowledge agents — a team's front door to the organisation's knowledge, read from Git into the one memory graph, in the same build

- **Status.** Decided by the user, January 2026 ("yes, realised using an ordin headless deployment"; "I suggest ordin carries on working as it does but can add a connection to knowledge agents that facilitate reaching to wider company knowledge repos"; "consider that a team can talk to one knowledge agent as a front door but which can communicate with multiple other proxies for information retrieval, latency of course is a concern"; "yes, by audience first — apply recommendations"; "yes, only hosted agents answer — apply recommendations"; "yes, add the wiki role — apply recommendations"; and, after a review of published writing on enterprise knowledge fabrics, "apply recommendations"; decision 92). The mechanism is proposed. **Amends ADR-097** (the agent profile gains the knowledge role: a knowledge query route group and a read-only MCP endpoint on the Caller API port, the audience check, a knowledge scope, a built-in fetch duty), **ADR-098** item 6 (Caller API routes and Caller Authentication), **ADR-111** (desktop recall gains front doors as a ranked source; **What Ordin used** names agent and source) **and ADR-103** for knowledge agents only (they read team folders as plain repositories, read-only). **Keeps** memory ADR-015 (memory makes no model call; the agent extracts, and memory still receives facts, never documents), ADR-057 (knowledge agents write no pages), ADR-103's rule that the Team Knowledge Service is not in the agent profile, ADR-110 item 8 (no sharded store on the desktop or the personal server), and the rule that the desktop needs no company set-up.
  - *Settled by the user, January 2026 ("apply recommendations"), after the twenty-first spec audit: (a) trust tier and page-over-document break ties among equally matching answers only, so the best match still comes first; (b) for the first step the operator's statement that every member of the audience may read a repository is accepted, with no check against the Git host (A-65); (c) knowledge agents are built in the single shape first, and the scaled shape waits until the memory component confirms that more than one instance may share one store (its RISK-014; R-123).*
  - *Amended in revision 0.63.0 (ADR-115): items 2, 3, 4, 7, 8 and 12 — per-person access for sources the whole audience may not read, checked at the source while the person asks; Confluence and SharePoint sources; tables of contents limited to what the whole audience may read; asker statements to proxies; saved results filtered per person.*
- **Decision.**
  - **1. A knowledge agent is ordin in the agent profile.** An agent-profile server whose configuration has a `knowledge-agent` section reads the Git repositories (or sub-paths) it lists, read-only, with each repository's connection credential, and answers retrieval questions. It is the same build as every ordin, deployed single or scaled as ADR-098 allows.
  - **2. Background reading.** A built-in recurring duty fetches the repositories on an interval with a random start (15 minutes by default) and reads each changed Markdown or text file. A knowledge page or a concept of the Open Knowledge Format (ADR-112) is read as a page, with no model call: its title and other names name a thing, its links become links between things, and each section becomes a fact whose source is the page and section, at the page's trust. Any other document is read with one fixed model turn, billed to the organisation key the agent runs with, whose facts, things and links are checked in Rust and written at document trust. Each fact's source names repository, path, section and commit; a changed file's earlier facts are replaced and unchanged files are not read again. A daily reading budget (1,000 documents by default) bounds the cost; documents left unread are reported. The agent writes no page and nothing to any repository.
  - **3. Table of contents.** After each fetch an agent publishes what it covers: its repositories with their last commits and fact counts, and its 500 most-connected things with their other names and shared identities; no statements. It is also published as an Open Knowledge Format `index.md`.
  - **4. Front door and proxies.** A workspace talks to one knowledge agent, its front door. The front door answers from its own graph and asks the proxies its configuration lists whose tables of contents match the question's things and words (at most 8 by default), in parallel. A proxy answers from its own graph only. Every agent can be a front door, a proxy or both.
  - **5. Only knowledge agents answer.** Knowledge queries are answered on a route group of the agent's Caller API port, kept apart from the caller interface, and on a read-only MCP endpoint (item 12). The query client — fan-out, deadlines, backup requests, rank fusion — and the query server are modules of ordin's one build, under the architecture test. Desktops and personal servers ask and never answer, so a person's own memory is never reachable by others; what a person shares reaches knowledge agents only through a team folder in Git (ADR-103).
  - **6. A question's path.** Recall in a connected workspace searches the local graph and folders first. When the local answer is confident — at least three facts above the reranker's threshold, and no name in the question that a front door's table of contents knows and the workspace does not — nothing leaves the computer. Otherwise, or when the person asks to look wider, the desktop sends the question and its filters, never the conversation, to its front doors with the person's company sign-in, and fuses the answers with its own by rank. The model writes the reply on the person's own ordin.
  - **7. Latency.** No model call and no Git fetch on a question's path at a front door or a proxy. The front door returns what has arrived by the first deadline (1 s), sends later results as **More found** until the second (5 s) and names what did not answer in time; it asks a backup proxy covering the same repositories when the first has not answered after 300 ms and takes the first answer; a request carries a hop budget (1 by default) and the agents it has visited, and an agent never answers a request it has already seen. Results are cached by question, access group and the commits relied on; a proxy streams repository moves to its front doors, which drop affected entries at once. A front door may read a repository itself when it asks a proxy about it more than a threshold a day (a **hot copy**). Connections to proxies are kept open.
  - **8. Access by audience.** A knowledge agent declares the identity-provider groups it serves and reads only repositories every member of that audience may read; the operator states this for each repository, and the check refuses a repository without the statement. An asker outside the audience is refused with a plain reason and receives no facts. A proxy answers a front door only when its configuration lists that front door and the access group asked for is one of its own. Caches are kept per access group, never shared. Per-person checks against each asker's Git rights are a later decision.
  - **9. Safety.** Content is data, never instructions, including tables of contents, names and link words from other agents. A result says which sources did not answer and when each was last fetched. Front doors and proxies keep no record of who asked what beyond counts, except the sources record of item 13.
  - **10. Checked knowledge first.** A page-backed fact ranks above an otherwise equal document-backed one, after the trust tiers of ADR-112, and is returned with the page's title, section and location, so **What Ordin used** shows the page.
  - **11. Shared glossaries.** A glossary is an Open Knowledge Format bundle of `Glossary` concepts in Git, owned by a team. An agent reads the glossaries it lists before its other repositories; a thing whose name or other name matches a glossary concept takes the preferred name and keeps the document's as another name, through memory's existing things, with the glossary as the most trusted source. When two listed glossaries give one other name to different things, the first listed wins and the agent's status names the conflict and both glossaries. The table of contents marks each glossary thing with its **shared identity** (repository and concept path); a front door groups facts with the same shared identity and returns a fact only once when several proxies return it from the same repository, path and commit. A person's ordin reads a glossary as a read folder (ADR-110), so the same names resolve with no server.
  - **12. Outside tools over MCP.** Each knowledge agent serves a read-only MCP endpoint (streamable HTTP) on the same port with two tools, search and read the table of contents, for any MCP client signed in as a person or a service in its audience, through the same cache, deadlines, hop budget and **More found**. On unless the configuration turns it off. The MCP Front, which serves ordin's own sessions, is unchanged.
  - **13. Sources record (optional).** Off by default. When the operator turns it on, the agent records for each answer an answer id, the time, the access group and each returned fact's agent, repository, path and commit — never the question, the asker's identity or a statement — and deletes entries after the retention period (365 days by default). Each answer then carries its id, which the desktop shows under **What Ordin used** and keeps in the person's own record beside the conversation, so only both records together link a question to its sources. The person is told whether a front door keeps a record, and for how long, when adding it and in its details.
  - **14. Connecting a desktop.** A person adds a front door to a workspace from Settings or in words, with their company sign-in, and removes it the same way; one per workspace by default, more if the person adds them. The organisation profile may name default front doors; the person can remove one and it is not added again. A workspace with no front door works exactly as before.
- **Context.** One person's ordin holds only the folders that person reads (ADR-110 item 8), while a large organisation keeps knowledge in more repositories than any laptop holds. The user asked for a route to that wider knowledge that keeps ordin's rules — Git as the source of truth, no company set-up for the desktop, content as data — reuses the one memory graph (ADR-111), and answers fast enough to use in conversation. A review of published writing on enterprise knowledge fabrics found the design short of a shared vocabulary, access for other tools and a record of the sources an answer used; items 11 to 13 add them with no central service.
- **Consequences.**
  - Three modules are new: the Knowledge Query Client (every program), the Knowledge Query Server and the Knowledge Reader (agent profile only). The Memory Service asks front doors from recall and keeps a knowledge scope in the agent profile; the Caller API mounts the route group and the MCP endpoint; Caller Authentication reads a groups claim and tells people from services; the Duty Timer runs built-in duties; the Agent Configuration checks the `knowledge` section; the Organisation Profile may name default front doors; the screens gain Settings › Knowledge agents, knowledge-agent rows under **What Ordin used**, **More found**, and a knowledge status for operators.
  - Every graph, table of contents and cache can be deleted and rebuilt from Git.
  - Out of this decision, each for a later one: per-person access checks; sharding one agent's store; agents that read sources other than Git; agents suggesting pages to a team; following links across agents' graphs; acting on knowledge; structured business systems; settling glossary conflicts automatically; a directory service for discovering agents.
  - 00 — G49, decision 92, glossary, non-goals; 01 — knowledge agents, company knowledge repositories, outside tools, trust boundary 8; 02 — *Knowledge agents*, three new components, notes on Memory Service, Caller API, Caller Authentication, Duty Timer, Agent Configuration, Organisation Profile; 03 — the four knowledge-agent sequences; 04 — Knowledge connection, Table of contents, Knowledge answer, Sources record entry, Glossary concept, Scope kind knowledge, Recall record, data lifecycle rows; 07 — authentication, authorisation, logging, latency, security; 08 — knowledge query route group, knowledge MCP endpoint, default front doors, Git repositories read, agent configuration; 10 — A-62 to A-67, C-95, C-96, C-98, R-116 to R-121, R-123; 11 — §4 Settings › Knowledge agents, §5.8 knowledge status, §5.10 knowledge from knowledge agents, vocabulary, UX-A80; 12 — UI-D113.
- **Alternatives considered.**
  - *A central knowledge service, index or directory.* Rejected: no central service; each knowledge agent is a hosted ordin its owners run, and front doors list their proxies in configuration.
  - *Desktops and personal servers answering each other.* Rejected by the user: a person's memory would be reachable by others; shared knowledge reaches agents through Git.
  - *A model call at the front door to rewrite questions or merge answers.* Rejected: it would add seconds to every question; graphs are built in the background and the person's own ordin writes the reply.
  - *Carrying knowledge queries on the caller interface or the Remote API.* Rejected: the caller interface carries threads and runs, not ranked results, and the Remote API serves only a personal server's paired desktops.
  - *Per-person checks against each asker's Git rights now.* Deferred: they need credentials on the person's behalf and a decision of their own; audiences give the same guarantee for a group whose members may all read the repositories.
  - *Knowledge agents writing or suggesting pages.* Rejected for writing (ADR-057: pages are written by Ordin and the person); suggesting pages to a team through Git review is a later step.
  - *Forwarding requests through several hops by default.* Not chosen: each hop adds latency and loop risk; the hop budget is 1 by default, and a hot copy removes a frequent hop.
  - *Keeping the cache and tables of contents in memory's store.* Not chosen: separate tables let the cache be cleared alone and forgetting in memory never touches them.

### ADR-114: ordin ships in increments chosen by build switches; each build answers its own module groups; ordin updates itself from one signed release feed, and a personal server updates itself when the person asks

- **Status.** Decided by the user, January 2026 (decision 93: "apply recommendations" on the release plan and its two open questions; "I need to ship an update module that allows users to check and install new releases"; "apply recommendations, add to design and spec it, then update the release plan"; and, on keeping the release plan out of the specs, "decouple please"). **Amends ADR-076** (the *Updates* item: a personal server now replaces itself when the person asks, never unasked), **ADR-041** (build layer: the unbranded build carries the project's feed address) **and ADR-072** (every update stays per-user, with no administrator).
- **Decision.**
  - **1. Increments.** ordin reaches people in increments, each adding features to programs that already ship. Which features each increment carries, and in what order, is a plan, kept in the maintainers' release roadmap outside the repository over the Generation Playbook's dependency order; neither this design nor the specs name an increment. Changing the plan changes no design or spec.
  - **2. Build switches; each build answers its own groups.** A module that a program can be built without sits behind a Cargo feature of the program named after what it gives the person (`sandbox`, `dictation`, `copilot`, `slack`, `mail`, `team-knowledge`, `authoring`, `handover`, `telegram`, `hosted-agents`, `knowledge-agents`, and on the server `personal-profile` and `agent-profile`). A program composes only the modules of the switches it is built with. A backend reports the command groups it answers when the screens greet it, and a command whose group the build lacks is refused with one plain sentence ("This needs a newer version of ordin."). The screens show no entry for a group the backend does not answer: nothing is shown greyed out or announced ahead.
  - **3. One signed release feed.** Every release publishes one feed in Tauri's updater format, carrying the macOS and Windows installers and the Linux server archives, each signed with ordin's update key. The unbranded open-source build carries the project's feed address, so updates work with no company set-up; a branded build may carry another address; a verified organisation profile may still redirect the feed (ADR-013 as amended in revision 0.49.0). One channel; an early-release channel is a later decision.
  - **4. The desktop updates itself.** ordin checks 60 seconds after start and every 24 hours, and whenever the person presses **Check for updates**. A newer signed version is downloaded and verified in the background without a question. The person then sees one line, "ordin <version> is ready", with **Restart to update**; pressing it installs and restarts, asking first when work is running, as quitting does. When the person quits ordin instead, the update installs on the way out. After the first start of a new version, a **What's new** card shows that version's release notes once; Settings › About shows them again.
  - **5. A personal server updates itself when asked.** `ordin-server update` fetches the same feed, takes the archive for its own architecture, verifies it with the same key, replaces its programs and restarts its user unit. A paired desktop shows "<server> can update to ordin <version>" in Settings › Servers; **Update server…** asks first, saying that running work stops, and then asks the server to run the same steps. A server never updates unasked. A server in a container, or one not run by its user unit, cannot replace itself and says so; its owner installs the new image or archive.
  - **6. Hosted agents are updated by their operators.** The agent profile never replaces itself and only announces a new version, as before; operators deploy the new image or archive through their own tooling.
  - **7. Data is kept safe across an update.** Before a new version applies its first migration, the Journal copies the datastore (JournalAndArtefactStore, *Backup copy before migrating*; newest two kept). Migrations stay forward-only; a release is undone by restoring that copy with the previous version.
  - **8. The update key.** One key signs every release. Its private half is a file on the maintainer's own computer, never in the repository (the repository ignores every `*.key` file) and never given to a CI service; the maintainer keeps a backup copy themselves. Its public half is built into every program (2D-2b). Losing the key means every installation must be reinstalled by hand; a leaked key lets someone sign a false update (R-124). *Custody recorded January 2026 by the user: "I will keep the signing key local gitignored … I will keep the backup myself."*
  - **9. The release pipeline.** A maintainer releases when they judge a version ready, by running make targets on their own computer: they build the installers and archives, sign them with the update key, write the feed with the release notes `/releasegen` drafts, and publish artefacts and feed together. No CI job builds, signs or publishes a release. *Amended January 2026 by the user: "I will build, sign and create release when I consider that appropriate, using a make function"; this replaces a CI job started by a pushed tag.*
- **Context.** The Generation Playbook's dependency order shipped nothing until the last step. Shipping in increments needs programs that run with only some modules, and successive releases need a way to reach people's computers and servers without IT (ADR-072; ordin is self-service). The desktop updater existed (Setup Assistant, *F*), but an unbranded build had no feed, the person could not ask for a check, and a server could only announce.
- **Consequences.**
  - The Service API greets with the groups a backend answers and gains one refusal; the Setup Assistant gains the on-demand check, background download, **What's new** and the server's own update; the desktop's updater downloads, installs and restarts; the server gains `ordin-server update` and two commands a paired desktop sends; the screens gain Settings › About, the update line, the **What's new** card and the server's update row, and hide absent groups.
  - The Generation Playbook gains the release pipeline and the build switches; the error-code registry check counts only the crates built with the switches being checked.
  - Re-planning an increment — moving a feature earlier or later, or leaving the sandbox out if its spike fails — edits the roadmap only. *Decoupled January 2026 ("decouple please"): an earlier draft of this decision named six releases and release-numbered features; both moved to the roadmap.*
  - 00 — decision 93, glossary; 02 — Setup Assistant, programs; 03 — two update sequences; 05 — deployment substrate; 07 — security; 08 — release feed; 10 — A-02 amended, A-68, A-69, C-99, C-100, R-124, R-125, rollout pointing to the roadmap; 11 — Settings › About, the update line, **What's new**, the server's update row.
- **Alternatives considered.**
  - *Ship everything at once.* Rejected: nothing reaches people until the last module is built.
  - *Name the increments in the design and specs, with one feature per increment.* Rejected by the user ("decouple please"): every re-plan would amend and re-audit specs that describe no change to the system.
  - *Show later features greyed out with "coming soon".* Rejected: an entry that does nothing is a dead end (11 — UX guidelines).
  - *Ask before downloading an update.* Not chosen: one more question for no gain; the download is verified and installs only when the person restarts or quits.
  - *Leave the server's update to its owner (ADR-076 as written).* Rejected: a person with a personal server would download and install archives by hand, which ordin's self-service rule excludes.
  - *Servers updating themselves automatically.* Rejected: a restart stops running work, so the person chooses when.
  - *A separate feed per program.* Not chosen: one feed keeps desktop and server on one version line, which pairing already requires.

### ADR-115: Knowledge inherits its readers — per-document read access for knowledge agents, checked at the source while a person asks

- **Status.** Decided by the user, January 2026 (decision 94). The mechanism is proposed. **Amends ADR-113** item 8 (access by audience becomes the rule for sources every member of the audience may read; other sources are checked per person), item 2 (a knowledge agent also reads Confluence spaces and SharePoint libraries), item 3 (a table of contents names only what the whole audience may read), item 4 (a front door passes the person it asks for to its proxies), item 7 (a saved result is filtered for each person when served) and item 12 (an MCP client signed in as a person is checked as that person); and the *Consequences* list item that deferred per-person checks. **Keeps** ADR-113's rules that no model call and no Git fetch happen while a person waits, that only knowledge agents answer, and that content is data; ADR-110 and ADR-103 (the desktop reads with the person's own sign-in) unchanged; ADR-026 (the person's own Atlassian token on the desktop).
- **Decision.**
  - **1. Knowledge inherits its readers.** A person receives a fact only if the system that holds its document would let them open that document at the time they ask. A fact consolidated from several documents needs every one. A thing, and a link between things, is shown when at least one fact that names it is shown. A saved answer is filtered for each person each time it is served. Hidden items are removed before results are ranked and cut, so an answer keeps its length and its order.
  - **2. The owner's decision stays where it is made.** A knowledge agent stores no reader list. For each fact it knows only the document's source and identifier; it asks the source system, while answering, whether this person may open the document's **access unit** — the repository for GitHub and GitLab, the page for Confluence, the file or folder item for SharePoint.
  - **3. Ask only about the shortlist.** The agent shortlists three times the facts it needs, checks the units behind the ten most likely to be shown, and checks further only when some are refused. Checks to one system go in parallel, batched where the system allows, within 300 ms inside the first deadline of one second; a check that completes later releases its facts under **More found**.
  - **4. Remember briefly.** Each yes or no is kept for one person and one access unit, in the agent's process only, for 60 seconds by default (0 to 300). A source may be set to **Check on every question**, which keeps nothing. A sharing-change notice from GitHub or GitLab ends the memory for the units it names at once.
  - **5. Doubt withholds.** A check that fails, is refused by the system, or misses its deadline withholds its facts. The answer says "Some sources couldn't be checked." and names no source, document or count.
  - **6. Never reveal.** GitHub, GitLab, Confluence and SharePoint hide restricted items from people who cannot open them, so a knowledge agent never tells a person a document's identifier, title or existence before its check has passed. No check is made on the person's computer, because that would need naming the document to it.
  - **7. Linked accounts.** A person's ordin already holds their sign-ins to the systems they use. On connecting to a front door, and whenever the person signs in to or out of GitHub, GitLab or Confluence in ordin, it presents each sign-in once; the agent asks that system whose sign-in it is, records the person's company identity beside the account the system names, and drops the sign-in. SharePoint needs no link: the token sent with each question (item 10) identifies the person. A person not signed in to a system in ordin gets none of that system's restricted knowledge, and ordin may tell them "Sign in to Confluence in ordin to include what you can read there."
  - **8. GitHub and GitLab.** A repository the operator states the whole audience may read is served by audience, as before. Any other repository is checked per person, per repository, by the agent with the repository's own read credential: GitHub's repository permission for a user; GitLab's effective project membership, Reporter or above for a private project. Repository owners may send the agent sharing-change notices.
  - **9. Confluence.** A knowledge agent reads Confluence spaces with an Atlassian account of its own. Checking another person's access needs the Confluence Administrator permission, so per-person checks are off by default; the agent then reads only unrestricted pages of spaces the operator states the whole audience may view, and restricted pages stay reachable through each person's own ordin. A company may grant the permission and turn per-person checks on.
  - **10. SharePoint.** A knowledge agent reads SharePoint libraries with a Microsoft account of its own. Every SharePoint file is checked per person, because a file's sharing can narrow without the agent seeing it. To check, the person's ordin sends, with each question, a Microsoft token addressed to the front door; the front door exchanges it on the person's behalf for a token it uses only to ask Microsoft Graph whether the person can open each file, in batches of up to 20, never to read content. The token lives for that one question, in memory, and is never stored or logged. A company-wide application permission, which needs an administrator, is an optional alternative.
  - **11. Through front doors.** A front door adds to its request to each proxy, which it already makes signed in as itself, an **asker statement** valid for 30 seconds naming the person and their linked accounts; a proxy trusts it as it already trusts a listed front door's access group, and checks for that person. A result that holds facts a proxy checked for one person is not saved for others. A proxy returns facts it cannot check itself (SharePoint) to the front door only, marked for checking, and the front door checks them before anything reaches the person. An MCP client signed in as a person is checked as that person; a service identity receives only knowledge the whole audience may read.
  - **12. Tables of contents.** A table of contents names only sources, and things known from documents, that the whole audience may read.
- **Context.** ADR-113 served knowledge by audience and required every member to be able to read every source, which left out exactly the documents owners share with a few people. The user asked that the owner's grant, made in the system holding the document, keep deciding who sees anything derived from it, without a copy of reader lists that would be large and silently stale. GitHub, GitLab, Confluence and SharePoint each answer an access question differently: per repository for Git, only with an administrator permission for another person in Confluence, and as the person in SharePoint (A-70, C-101 to C-104).
- **Consequences.**
  - One module is new: **Source Access** (agent profile), the knowledge agent's client of the four systems for linking, access checks, notices, and reading Confluence and SharePoint. The Knowledge Query Server filters per person, links accounts, issues and checks asker statements; the Knowledge Query Client links accounts and sends the SharePoint token; the Knowledge Reader reads Confluence and SharePoint through Source Access; the Agent Configuration accepts per-person sources and the Confluence and SharePoint sections; Identity and Credentials gives the Knowledge Query Client the sign-ins it holds and a Microsoft token for a front door; the Caller API and Caller Authentication gain five routes (four knowledge routes and one operator route that clears the remembered check answers); **What Ordin used** gains one sentence and a knowledge agent's connection details one line per checked system.
  - Answers may take up to 300 ms longer at the 95th percentile; the number of requests to each system per question is bounded by the shortlist, batching and the 60-second memory (R-127, R-128).
  - A person whose access is removed without a notice can still see that knowledge for up to the remember period (R-130).
  - Out of this decision, for a later one: reading Word, PDF and other non-text files from SharePoint libraries (A-72).
  - 00 — decision 94, glossary; 01 — knowledge agents' sources and notices, trust boundary 8; 02 — Source Access and edges; 03 — *A question with per-person checks*; 07 — authentication, authorisation, security; 08 — the five routes and changed bodies; 10 — A-70 to A-72, C-101 to C-107, R-126 to R-132.
- **Alternatives considered.**
  - *Copy each document's reader list into the agent.* Rejected by the user: lists can be large and change without notice.
  - *Send the person's own Git, Atlassian or Microsoft token with each question.* Rejected: the agent would hold credentials able to read everything the person can, on every question; only the SharePoint exchange is kept, scoped to one question and used only to check.
  - *Check on the person's computer, which opens each document with its own sign-in.* Rejected: the agent would first have to name the document to the person, which the four systems' visibility rules forbid.
  - *Mirror permissions with an administrator's rights.* Rejected for the default: it needs IT, and the copy drifts; kept as the company's choice for Confluence only.
  - *Match accounts by e-mail.* Rejected: personal GitHub accounts often carry other addresses, and a wrong match shows documents to the wrong person.
  - *Keep check answers in the datastore so every instance shares them.* Rejected: a stored row would tie a person to a document; each instance asks for itself within 60 seconds.
