# Code workspace

**Agents work on code next to the documents it relates to, under the same rules and approvals.**

Category: [Code](README.md#code) · Status: Designed

## What you get

- Code from GitHub or GitLab in the workspace tree, opening in a full-screen code editor.
- Syntax highlighting for every language the editor knows. Language servers add completion and error checking, with Go, Rust, Java and TypeScript first.
- Change review: accept or reject each change an agent proposes, with the new problems each change brings.
- One click to open the code in IntelliJ IDEA or VS Code. ordin notices the edits you make there.
- A terminal of your own, which agents can't use.

## How it works

1. Link a code repository to a workspace.
2. Each coding task gets its own copy of the code on its own branch.
3. Several agents work at once, or try the same task while you keep the best attempt.

## You stay in charge

- Every change goes through change review.
- You choose which kinds of GitHub action need your approval.

## Good to know

- ordin is not a full IDE: no debugger, notebooks or refactoring tools. It works beside yours.
- Technical detail appears only in a workspace that links code.

## See also

[GitHub and GitLab](github-gitlab.md) · [Sandbox](sandbox.md)
