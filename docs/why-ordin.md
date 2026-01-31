<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="logo/ordin-lockup-on-dark.svg">
    <img src="logo/ordin-lockup-on-light.svg" alt="ordin" width="320">
  </picture>
</p>

<p align="center"><b>Your AI workplace for everyday business.</b><br>
Agents do the work, you stay in charge, and no technical skills are needed.</p>

---

# Why ordin

The ideas behind **ordin**, where it fits beside the tools you already use, and an example of it at work. For the feature list, see [Features](features/README.md).

## The ideas behind ordin

### Business first

**ordin**'s main purpose is to let people who aren't technical work with AI agents. You sign in with accounts you already have, say what you want, answer questions in plain words and approve what happens. You never type a command, edit a settings file or judge a technical or security choice. **ordin** installs, checks and repairs what it needs by itself, and when it can't, it tells you in one plain sentence what you can do.

### Agent first

In most apps you do the work and AI helps. In **ordin** it's the other way round: agents do the work, and you direct, check and approve it. You don't need to know which agents exist, how to install them, or what commands they take. **ordin** answers the agents' routine questions itself, checks each result against what you asked for, and only comes back to you when something needs your say.

**ordin** doesn't build an agent engine of its own. It runs on the engines that already exist, such as Claude Code and GitHub Copilot, and gives people one place to work with agents on their documents and workflows: a coordinator for several agents, a memory that lasts, a safe place to run code the agents write, links to your company's documents, and fixed rules that keep you in charge.

### One system, local or remote

**ordin** is one build that runs on your desktop, on a server of your own, and as a hosted agent for your company, with the same agents, workflows, rules, memory and approvals in each. A job you start on your desktop can later run on your server without being rewritten. In every form, **ordin**'s own code enforces the rules. No AI model decides whether a rule applies.

### Documents, email and chat first

Work happens in documents, email and chat, so **ordin** works in them directly, with no plug-in to add. Documents stay where they live, and a change goes back only after you approve it.

There are no MCP servers to install or run for each system, either. **ordin**'s own services talk to SharePoint, Confluence, GitHub, GitLab, email and chat through those systems' own APIs, and give agents the tools they need. A company that already runs MCP servers can still connect them.

### Make and manage agents

**ordin** makes agents as well as running them. Git repositories are its agent registry, and **ordin** is the tool for managing them: publishing is a pull request, and the merge is the publication.

### An agentic workspace for code

Code is first-class work in **ordin**, next to the documents it relates to, under the same rules and approvals. **ordin** works beside your IDE rather than replacing it.

### Work passes between people

Most work moves through several people. In **ordin**, what one person finishes is what the next person starts from, through places both people already use.

## Where ordin fits

**ordin** doesn't replace the tools people already use. It works alongside them, as one place where people use agents on their documents and workflows.

### Documents and knowledge

| Tools you may already use | What they are for | Where **ordin** fits |
|---|---|---|
| **Office apps** such as Word and PowerPoint | Writing documents and slides by hand | **ordin** opens Word, Excel and PowerPoint files in your own Microsoft apps with one click. Agents change Word documents through WordCraft and slides through DeckCraft. They don't change spreadsheets yet. |
| **PDF readers and editors** | Reading and marking up PDFs | Agents work on PDFs through PdfCraft, and PDFs open there for you too. |
| **Markdown editors** | Writing in plain text | **ordin** opens Markdown as a formatted page that you edit with a simple toolbar, so you never see Markdown symbols. People who code can edit the text itself. |
| **Publishing to Confluence and SharePoint** | Sharing finished documents | One Markdown original becomes a Confluence page, or a Word document, PDF or slides in SharePoint. The copies follow the original when it changes, and an edit someone makes in a copy comes back to you as a suggestion. |
| **Spreadsheets and dashboard tools** | Turning data into tables and charts | Agents gather the data and **ordin** draws tables, charts and key figures, each number showing where it came from. Keep a report as a dashboard and refresh it when you ask, or save it as a spreadsheet, PDF, slides or document. |
| **Wikis and knowledge bases**, such as Confluence | Writing down what a team knows | **ordin** writes knowledge pages for each workspace as work finishes, with their sources, and marks a page when a source changes. Pages can be published to Confluence. |
| **Enterprise search and knowledge portals** | Finding what the wider organisation has written down | Knowledge agents read the repositories their owners choose and answer through one front door per team, each fact with its source. Nothing is copied into one central system. See [An agentic knowledge fabric](knowledge-fabric.md). |
| **Memory in AI assistants** | An assistant remembering you between conversations | **ordin**'s memory is shared by the agents working on a job, stays on the machine that runs **ordin**, and comes with you if you change AI engine. You can see, correct and delete what it remembers. |

### Agents

| Tools you may already use | What they are for | Where **ordin** fits |
|---|---|---|
| **Chat assistants** in the browser | A conversation with one assistant | **ordin** is for work on your own files and the company's documents where they live, with one or several agents on a job. |
| **Coding agents** such as Claude Code and GitHub Copilot | Running an agent, in an editor or terminal | **ordin** runs on them as its engine, and gives people who don't code a way to use them for everyday work. |
| **IDEs** and the agents inside them | Writing and changing code | **ordin** works beside your IDE, keeping code next to the documents it relates to, and opens the code in your IDE in one click. |
| **Agent frameworks** and libraries | Building agents in code | **ordin** is a finished app. You add ready-made agents from Git repositories, or make one with **ordin**'s help. |
| **Automation tools** | Repeating the same steps every time | Save a job's steps as a workflow and it runs the same way next time, with choices, loops and questions for you along the way. On your own server, a workflow can start from a message or a company event. You still approve anything that leaves. |
| **Chat bots and server agents** | Work that runs while you're away | **ordin** runs the same system on a server you choose or as a hosted agent for your company, with approvals coming to you in Telegram or Slack. |

### Working together

| Tools you may already use | What they are for | Where **ordin** fits |
|---|---|---|
| **Team collaboration**, through shared drives and repositories | Working together on the same files | **ordin** shares what a team has learned through a GitHub or GitLab folder the team already uses: a workspace's memory and knowledge pages go there, and colleagues take them into their own **ordin**. No server or company set-up is needed. |
| **Team chat and work trackers**, such as Slack, Jira and email | Talking about work and tracking it | **ordin** hands over the work itself: the results, where they live, and a note of what's done, decisions made and open questions. It checks the work holds what the next stage needs. The handover can be announced in Slack, or travel on a Jira ticket that ordin assigns to the next person. |

## An example

The quarterly supplier rate review. The new rates arrive as a spreadsheet in SharePoint and in suppliers' emails. The rates table lives in a Markdown document in GitHub, with a Confluence page and a Word copy published from it. The billing code holds the same rates, and each supplier needs a reply.

Done by hand, that is several separate jobs: copying text between places, updating each copy, and keeping track of what was sent where.

In **ordin** you ask once, and **ordin** forms a crew. One agent reads the spreadsheet and the emails, another updates the Markdown table and the code on its own branch, and a third drafts the replies. You approve the document and its copies in one step, see the code change in change review next to the documents it's based on, and check each email before it goes. The history shows what each agent did, and a restart doesn't send anything twice.

When the new rates are agreed, you hand them over to the person in Finance who updates invoicing. Save the job as a workflow, and next quarter it can run on your **ordin** server, started by a message or a company event.

## What ordin is not

- **Not a full IDE.** Its code editor is for review and quick edits, with no debugger or refactoring tools. It opens your code in IntelliJ IDEA or VS Code.
- **Not a scheduler.** ordin keeps no timetable on your desktop or your own server. To repeat work on a timetable, use a standard scheduler you already run to start it: it sends a company event to your **ordin** server, and **ordin** does the work. A hosted company agent can also run repeating work itself.
- **Not shared.** Each installation serves one person, except a hosted agent, which serves the people your company allows. People share work through the documents and repositories they already use.
- **Not finished with Office files.** Agents change Word and PowerPoint files only through the ArtCraft apps, as those apps gain support for the formats, and they don't change spreadsheets yet.
- **Mostly not built yet.** Most of **ordin** is a design and a clickable prototype. The memory service and the sandbox have working code.
