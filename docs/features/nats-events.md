# NATS events

**Start saved workflows on your ordin server from events on your company's NATS messaging system.**

Category: [Messaging](README.md#messaging) · Status: Designed

## What you get

- A trigger starts one saved [workflow](workflows.md) when a matching event arrives, for example "When an invoice-created event arrives, run Invoice check with its PDF".
- The workflow's inputs are filled from the event.

## How it works

1. Your company names the subjects ordin may listen to.
2. You turn on a trigger for a subject and a workflow.
3. A matching event starts the workflow, and approvals still come to you.

## You stay in charge

- Filters are decided by fixed code, not by an AI model.
- Every trigger has limits.
- An event's content is treated as information, never as instructions.

## Good to know

- Available on the [ordin server](own-server.md), not on the desktop.
- Events are JSON. ordin listens only and publishes nothing back.
- Where the company keeps events in JetStream, none are lost across a restart.
- ordin keeps no timetable. To run a workflow on a timetable, have a standard scheduler send an event.

## See also

[Workflows](workflows.md) · [Your own server](own-server.md)
