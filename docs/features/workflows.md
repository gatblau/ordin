# Workflows

**Save a job's steps as a workflow, and it runs the same way next time.**

Category: [Automation](README.md#automation) · Status: Designed

## What you get

- A numbered list of steps, each one agent's job, with a diagram to read it by.
- Control blocks: If / otherwise, Ask me, For each, Repeat until and Run another workflow.
- Every save is a version. Duplicate, export and import workflows as files.
- Sharing through GitHub and GitLab, or a company workflow repository.

## How it works

1. Ask Ordin to save a crew's steps as a workflow.
2. Run it again: the steps run without Ordin deciding each round.
3. If the steps don't fit, Ordin takes over and offers to update the workflow.

## You stay in charge

- Every loop has a maximum.
- An imported workflow grants nothing and never runs before you save it.
- A shared workflow says where it lives, shows a newer version before it is used, and names the agent repository each of its agents comes from.

## Good to know

- On the [ordin server](own-server.md), a workflow can start from a message, **/run**, a matching file or a [NATS event](nats-events.md).

## See also

[Crews](crews.md) · [NATS events](nats-events.md) · [Reports and dashboards](reports.md)
