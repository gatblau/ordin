# Crews

**Several agents work on one goal, with Ordin as coordinator and limits that ordin enforces.**

Category: [Agents](README.md#agents) · Status: Designed

## What you get

- Several agents at once, each kept separate from the others.
- A crew: Ordin plus the member agents, working towards one stated purpose.
- Clear limits, such as the number of rounds or the time allowed, and a stop button.

## How it works

1. You ask for something bigger than one agent can do.
2. Ordin forms a crew and chooses each next step.
3. Every message between members passes through ordin, and Ordin checks the result before reporting it done.

## You stay in charge

- Which agents may join, what they may hand to each other, the limits and the approval points are enforced by fixed code. No AI model decides whether a rule applies.
- You can keep a crew and save its steps as a [workflow](workflows.md).

## See also

[Talking to Ordin](conversation.md) · [Workflows](workflows.md) · [Approvals and rules](approvals-and-rules.md)
