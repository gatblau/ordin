# Privacy

> **Placeholder, not final.** This page states only what the design documents and the program's data folder
> show today. The maintainer has not yet reviewed or finished it. Treat it as a draft until this note is gone.

## What ordin keeps on your computer

ordin keeps its working data in one folder on your computer (on macOS,
`~/Library/Application Support/org.ordin.desktop`). It holds the journal of what happened
(`journal.db`), the program's log files (`logs/`), and the folders `agents/`, `brand/`, `bundles/`,
`inspections/`, `run/` and `versions/`.

## What ordin sends

- **No telemetry.** The first release sends no usage data of its own, exports no metrics and sends no
  traces (design assumption A-10).
- **Update check.** To look for a new version, ordin asks the update feed address it was built with for a
  small description of the latest version.

## What leaves your computer because you asked

- Text an agent works on is handled by the coding tool you chose to run it (for example Claude Code or
  Copilot), under that tool's own terms. Content sent to Copilot leaves your computer under GitHub's terms
  for your subscription.
- Files you publish to Confluence or SharePoint are visible to everyone those locations allow. The approval
  card names that audience before the first push.
- Email is only read from messages matching the links you set on a workspace, and nothing is sent without
  your approval on a card.

## Still to write

The maintainer decides and writes: a contact address for privacy questions, how long data is kept, how to
erase it, and anything the final release changes.
