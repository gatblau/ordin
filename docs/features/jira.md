# Jira

**Hand work over on a Jira ticket, and keep the tickets you work on in your workspace.**

Category: [Teamwork](README.md#teamwork) · Status: Designed

## What you get

- A handover can travel on a Jira ticket. ordin adds the note and the results to the ticket and assigns it to the next person.
- The next person's ordin finds the ticket and shows it as handed to them. Accepting starts a workspace that already links the ticket.
- Link any Jira ticket to a workspace. Its description, comments, attachments and related tickets become items Ordin and the agents can read.
- When the ticket changes in Jira, the workspace shows it as changed. Nothing you have locally is overwritten.

## How it works

1. Connect Jira with an API token you create in your own Atlassian account. If your company has set up an Atlassian sign-in, you can use that instead.
2. Choose a Jira project as a stage's handover place, or paste a ticket's address under **Add to this workspace**.
3. Pick the next person from the people who can be assigned tickets in that project.
4. Approve the handover once. The ticket is updated and assigned.

## You stay in charge

- Nothing is written to Jira without your approval. If a handover has no ticket yet, the approval says a new one will be created.
- ordin only comments on, labels and assigns tickets. It does not change a ticket's status or fields.
- Removing a linked ticket removes your local copy only. Nothing in Jira changes.

## Good to know

- Jira Cloud only for now.
- If the next person doesn't use ordin, Jira's own notification tells them. ordin reads their answer from the ticket.
- Jira is not offered on the [ordin server](own-server.md).
- Agents can also use Jira as a tool through an MCP connection, as before.

## See also

[Handovers](handovers.md) · [Lifecycles](lifecycles.md) · [Confluence](confluence.md)
