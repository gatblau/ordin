<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo/ordin-lockup-on-dark.svg">
    <img src="docs/logo/ordin-lockup-on-light.svg" alt="ordin" width="320">
  </picture>
</p>

<p align="center"><b>Your AI workplace for everyday business.</b><br>
Agents do the work, you stay in charge, and no technical skills are needed.</p>

---

**ordin** is an app where AI agents work on your documents, email, chat and code. You talk to **Ordin**, a helper that ships with the app, and say what you need. Ordin picks the right agent, or puts a small team of agents together, keeps them on track and checks the result. Anything that reaches other people waits for your approval.

It runs on your desktop, on a server of your own, or as a hosted agent for your company: one system, with the same agents, rules and memory in each.

> **ordin** (lower case) is the app. **Ordin** (capital O) is the helper you talk to inside it.

## Why use it

- **No technical set-up.** Sign in with accounts you already have. ordin installs and repairs what it needs.
- **Your time goes on decisions.** Agents do the steps. You approve the results.
- **Bigger jobs get done.** Several agents work on one goal, and Ordin checks the result against what you asked for.
- **Less risk.** Agent-written code runs locked away, rules are enforced by fixed code, and every step is recorded.
- **No lock-in.** Change AI engine, and your agents, memory, workflows and rules come with you.

Read [Why ordin](docs/why-ordin.md) for the ideas behind it, where it fits beside the tools you already use, and an example.

## Features

Each feature has a one-page card. See the [full list](docs/features/README.md).

| Category | Features |
|---|---|
| **Everyday use** | [Talking to Ordin](docs/features/conversation.md) · [Workspaces](docs/features/workspaces.md) · [Home and search](docs/features/home-and-search.md) |
| **Documents** | [Local files](docs/features/local-files.md) · [SharePoint](docs/features/sharepoint.md) · [Confluence](docs/features/confluence.md) · [GitHub and GitLab](docs/features/github-gitlab.md) · [Markdown](docs/features/markdown.md) · [Office, PDF and media](docs/features/office-and-media.md) |
| **Context** | [Memory](docs/features/memory.md) — facts, things and links you can see, correct and forget · [Knowledge pages](docs/features/knowledge-pages.md) · [Team knowledge](docs/features/team-knowledge.md) · [Open Knowledge Format](docs/features/open-knowledge-format.md) · [Knowledge agents](docs/features/knowledge-agents.md) |
| **Messaging** | [Email](docs/features/email.md) · [Slack](docs/features/slack.md) · [Telegram](docs/features/telegram.md) · [NATS events](docs/features/nats-events.md) |
| **Agents** | [Crews](docs/features/crews.md) · [Agent library](docs/features/agent-library.md) · [Agent maker](docs/features/agent-maker.md) · [Agent repositories](docs/features/agent-repositories.md) |
| **Teamwork** | [Handovers](docs/features/handovers.md) · [Lifecycles](docs/features/lifecycles.md) · [Jira](docs/features/jira.md) |
| **Automation** | [Workflows](docs/features/workflows.md) · [Reports and dashboards](docs/features/reports.md) |
| **Code** | [Code workspace](docs/features/code-workspace.md) |
| **Where it runs** | [Desktop](docs/features/desktop.md) · [Your own server](docs/features/own-server.md) · [Hosted company agent](docs/features/hosted-agent.md) |
| **Trust and safety** | [Approvals and rules](docs/features/approvals-and-rules.md) · [Sandbox](docs/features/sandbox.md) · [Privacy and reliability](docs/features/privacy-and-reliability.md) |
| **Open and adaptable** | [AI engines](docs/features/ai-engines.md) · [Branding, languages and accessibility](docs/features/branding-and-languages.md) |

## Status

Most of **ordin** is a design and a clickable prototype. The [memory service](memory/) and the [sandbox](sandbox/) have working code.

## Try the prototype

[`docs/design/ordin-desktop/user-interface.html`](docs/design/ordin-desktop/user-interface.html) is a clickable prototype of the whole app. It runs from sample data, needs no network and installs nothing. Download the file and open it in your browser (GitHub shows it as source code, so it won't run there).

Use the **Show** menu to jump to any of more than 300 scenes, or choose **From the beginning** for a short story of **ordin** at work.

## What's in this repository

| Folder | What it holds |
|---|---|
| `docs/features/` | One card per feature |
| `docs/design/ordin-desktop/` | The high-level design: goals, architecture, decisions, user experience guidelines and the clickable prototype |
| `docs/spec/` | The low-level design specifications |
| `memory/` | The memory service agents use to remember and share what they learn, reached over HTTP |
| `sandbox/` | The sandbox that runs agent-written code away from your files and network. Its background service, `sandboxd`, serves agents through one MCP tool |
| `docs/logo/` | Logos, app icons (light and dark) and the Jost typeface |

## Licence

**ordin** is open source under the [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for attributions, including the Jost typeface under the SIL Open Font License.
