# Agent repositories

**Git repositories are ordin's agent registry, and ordin is the tool for managing them.**

Category: [Agents](README.md#agents) · Status: Designed

## What you get

- A catalogue of combinable parts in 25 kinds, such as skills, rules, memory rules and evaluations. Every agent is built from parts.
- A map of which part depends on which, and which agents use each part.
- Import of agents and single parts from ordin's format, APM packages, and Claude Code and GitHub Copilot set-ups.
- A check of any agent or part, with fixes made on a draft.
- A health check of the whole catalogue that finds unused parts, likely duplicates and broken references.

## How it works

1. ordin reads a repository, groups its files into parts and checks each part and how parts fit together.
2. Curators change one part on its own, retire it for a newer one, or withdraw it with its replacement.
3. When a part is withdrawn with a named replacement, ordin rebuilds every agent that uses it.

## You stay in charge

- Every change goes for review as a pull request, and the repository's own GitHub rules decide who approves it.
- Fixes are made on a draft, never on an agent in use. Where there is no single right answer, you decide.
- Lock files pin the exact version of every part. There is no registry server to run.

## Good to know

- If you can't publish to an agent's repository, you fix your own copy and can suggest the fix to its curators.
- A new, empty repository gets the starting files every agent there needs.
- [Lifecycles](lifecycles.md) also live in agent repositories.

## See also

[Agent library](agent-library.md) · [Agent maker](agent-maker.md) · [GitHub and GitLab](github-gitlab.md)
