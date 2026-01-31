# Hosted company agent

**Your company runs the ordin server as one agent with an identity of its own.**

Category: [Where it runs](README.md#where-it-runs) · Status: Designed

## What you get

- One agent that answers the people and channels your company allows, for example in a Slack channel.
- Repeating work that runs on a schedule.
- A deployment on one machine, or scaled out on Kubernetes.
- Any AI model provider your company uses. ordin passes on the settings the company declares and names no provider of its own.
- Answers that start from what is already known. Before the AI runs, ordin gathers the thread, the channel's canvases and a search of past messages, as the company configures.
- In Slack: answers that read thread replies and alert messages, buttons that start Slack workflows, and a hand-off that posts to the right channel and mentions the right people.

## How it works

1. Your company sets up the agent with its own AI key and accounts, never yours.
2. People ask it for help, from Slack or through their own ordin desktop.
3. Those who look after it see its runs and settings from their desktop.

## You stay in charge

- Anything it wants to do that leaves the company waits for a named approver.
- Its memory is kept by channel, and a direct chat stays apart.
- Your company can have every question and every answer checked by a checker of its own choosing. A blocked question gets a safe reply and never reaches the AI.
- What must happen on every question is done by ordin itself and set in the agent's configuration. It is never left to the AI to remember.

## Good to know

- You never connect to a hosted agent as your own server.

## See also

[Your own server](own-server.md) · [Slack](slack.md) · [Memory](memory.md) · [Knowledge agents](knowledge-agents.md) · [Approvals and rules](approvals-and-rules.md)
