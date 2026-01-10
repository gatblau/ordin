# Your own server

**The same ordin, with no window, running all the time on a Linux server you choose.**

Category: [Where it runs](README.md#where-it-runs) · Status: Designed

## What you get

- The same agents, workflows, rules, memory and approvals as your desktop.
- Work that keeps going while your laptop is closed.
- Conversations and approvals in [Telegram](telegram.md) or [Slack](slack.md), answered on either.
- Work that starts from a message, **/run** with a workflow's name, a matching file or a [NATS event](nats-events.md).

## How it works

1. Set the server up once. It needs no administrator rights.
2. Pair it with your desktop, which then shows the server's workspaces beside your own.
3. The desktop talks to it over HTTPS, and the server sends live updates back as they happen.

## You stay in charge

- It serves only you.
- Nothing starts from a clock.
- After a crash or hang, it is restarted automatically and every channel reconnects.

## Good to know

- When your AI engine sign-in ends, ordin tells you straight away and asks you to have it signed in again.
- Confluence is not offered on the server.

## See also

[Desktop app](desktop.md) · [Hosted company agent](hosted-agent.md)
