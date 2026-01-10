# Sandbox

**Code that agents write runs locked away from your files and the network, and your passwords never reach it.**

Category: [Trust and safety](README.md#trust-and-safety) · Status: Working code in [`sandbox/`](../../sandbox/)

## What you get

- Python, shell scripts and JavaScript written by agents run in the sandbox.
- No network of its own and no files except those passed in.
- A separate broker calls company systems on its behalf.

## How it works

1. An agent writes code to do a job, such as bulk work on data.
2. ordin passes in only the files the job needs.
3. The broker makes any outside call, so passwords never enter the sandbox.

## Good to know

- Its background service, `sandboxd`, serves agents through one MCP tool.

## See also

[Approvals and rules](approvals-and-rules.md) · [Code workspace](code-workspace.md)
