# ordin features

Each feature has its own short card. Cards are grouped by category below.

**Status** on each card says how far the feature has got:

- **Designed**: described in the [design](../design/ordin-desktop/) and the [clickable prototype](../design/ordin-desktop/user-interface.html). Not built yet.
- **Working code**: code exists in this repository.

## Everyday use

| Feature | In one line |
|---|---|
| [Talking to Ordin](conversation.md) | One conversation per piece of work, typed or dictated. |
| [Workspaces](workspaces.md) | A named home for one piece of work. |
| [Home and search](home-and-search.md) | What needs you, across every workspace, and one search box. |

## Documents

| Feature | In one line |
|---|---|
| [Local files and folders](local-files.md) | Your computer's files, by name, not by path. |
| [SharePoint](sharepoint.md) | Word and PowerPoint where they live, changed after you approve. |
| [Confluence](confluence.md) | Read pages, and publish your documents as pages. |
| [GitHub and GitLab](github-gitlab.md) | Repositories for documents, code, workflows and knowledge. |
| [Markdown documents](markdown.md) | Formatted pages, and the original of published copies. |
| [Office, PDF and media files](office-and-media.md) | Agents change files through the ArtCraft apps. |

## Context

| Feature | In one line |
|---|---|
| [Memory](memory.md) | Every agent remembers facts, the things they are about and the links between them; you can see, correct and forget all of it. |
| [Knowledge pages](knowledge-pages.md) | Explanations with their sources, per workspace. |
| [Team knowledge](team-knowledge.md) | Share what a workspace learned, through a repository folder. |
| [Open Knowledge Format](open-knowledge-format.md) | Your knowledge pages are plain files in an open format any tool can read. |
| [Knowledge agents](knowledge-agents.md) | One front door for your team's questions, backed by agents that read the wider organisation's knowledge. |

## Messaging

| Feature | In one line |
|---|---|
| [Email](email.md) | Agents read and draft. Nothing is sent without your approval. |
| [Slack](slack.md) | Your ordin, hosted agents and handover notices, in Slack. |
| [Telegram](telegram.md) | Talk to your server, and let agents read groups and draft posts. |
| [NATS events](nats-events.md) | Company events start saved workflows on your server. |

## Agents

| Feature | In one line |
|---|---|
| [Crews](crews.md) | Several agents on one goal, within fixed limits. |
| [Agent library](agent-library.md) | Find and add ready-made agents in one click. |
| [Agent maker](agent-maker.md) | When no agent fits, make one in plain words. |
| [Agent repositories](agent-repositories.md) | Git as the agent registry, and the tools to curate it. |

## Teamwork

| Feature | In one line |
|---|---|
| [Handovers](handovers.md) | Pass your results and a note to the next person. |
| [Lifecycles](lifecycles.md) | The stages of a piece of work, and who does each. |
| [Jira](jira.md) | Hand over on a Jira ticket, and keep tickets in your workspace. |

## Automation

| Feature | In one line |
|---|---|
| [Workflows](workflows.md) | Save a job's steps and run them the same way again. |
| [Reports and dashboards](reports.md) | Tables, charts and figures, each with its source. |

## Code

| Feature | In one line |
|---|---|
| [Code workspace](code-workspace.md) | Agents work on code beside the documents it relates to. |

## Where it runs

| Feature | In one line |
|---|---|
| [Desktop app](desktop.md) | Your everyday ordin, on your own computer. |
| [Your own server](own-server.md) | The same ordin, running all the time on a server you choose. |
| [Hosted company agent](hosted-agent.md) | One agent for your company, with an identity of its own. |

## Trust and safety

| Feature | In one line |
|---|---|
| [Approvals and rules](approvals-and-rules.md) | Outside actions wait for you. Fixed code enforces the rules. |
| [Sandbox](sandbox.md) | Agent-written code runs away from your files and network. |
| [Privacy and reliability](privacy-and-reliability.md) | Data stays on your machine. A restart loses nothing. |

## Open and adaptable

| Feature | In one line |
|---|---|
| [AI engines](ai-engines.md) | Claude first, then GitHub Copilot. Change engine without losing your work. |
| [Branding, languages and accessibility](branding-and-languages.md) | Your brand, your language, accessible by design. |

## Writing a feature card

Every card follows the same shape, so a reader knows where to look. Keep each card to one screen.

```markdown
# <Feature name>

**<One sentence: what it does for you.>**

Category: [<Category>](README.md#<category>) · Status: Designed | Working code

## What you get
- 3 to 5 short bullets, in plain words.

## How it works
1. 2 to 4 short steps, from the person's point of view.

## You stay in charge
- What waits for approval, and what limits apply.

## Good to know
- Limits, conditions and what it doesn't do.

## See also
[Related card](related.md) · [Another card](another.md)
```

Leave out a section that would be empty. Add the card to the table of its category above.
