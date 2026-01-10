# Knowledge agents

**One front door for your team's questions, backed by agents that read the rest of the organisation's knowledge.**

Category: [Context](README.md#context) · Status: Designed

## What you get

- Your team talks to one knowledge agent, its front door. You never need to know which agent knows what.
- The front door asks other knowledge agents at the same time and returns one merged answer, best first, with each fact's source.
- Answers arrive in about a second. A result that comes late is added as **More found**, and the answer says which sources did not reply.
- Under an answer, **What Ordin used** shows each fact, which agent it came from, the file, the version and when it was read.
- Anything that speaks MCP can ask a knowledge agent, such as another team's agent or a developer's coding tool, under the same rules.

## How it works

1. Your ordin answers from what it already knows. Only if it is not confident does it send the question, and nothing else, to the front door.
2. The front door reads other agents' tables of contents and asks only the few that fit, in parallel, with a deadline.
3. It merges what comes back, checks that you may read it, and returns one ranked, sourced result.
4. Your ordin writes the reply. Facts backed by a checked knowledge page come first.

## You stay in charge

- A knowledge agent shows you something only if you could open the document it came from, in GitHub, GitLab, Confluence or SharePoint, at the moment you ask. It keeps no list of who may read what; it asks.
- Documents are information, never instructions. That also holds for answers from other agents.
- Only the question leaves your computer, never your conversation.
- Knowledge agents only read. They write no pages, and nothing is shared except by a person who approves it.
- An agent's owner can turn off the MCP route. A sources record, which holds answer numbers and sources and never the question or the asker, is off unless the owner turns it on.

## Good to know

- Knowledge agents are ordin running as a [hosted agent](hosted-agent.md), so there is no separate product to install.
- Your own ordin works fully without any of this.
- An agent builds its knowledge in the background. A question never waits for a model call or a download.
- Your ordin links your accounts from the sign-ins it already has. If you are not signed in to a system in ordin, you won't see its restricted knowledge, and ordin tells you where to sign in.
- From SharePoint, knowledge agents read Markdown and text files for now. Word and PDF files come in a later version.

## See also

[Hosted company agent](hosted-agent.md) · [Memory](memory.md) · [Knowledge pages](knowledge-pages.md) · [Open Knowledge Format](open-knowledge-format.md) · [Team knowledge](team-knowledge.md) · [The paper: an agentic knowledge fabric](../knowledge-fabric.md)
