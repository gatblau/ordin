# Open Knowledge Format

**Your knowledge pages are plain Markdown files in an open format, so other tools can read them and ordin can read theirs.**

Category: [Context](README.md#context) · Status: Designed

## What you get

- Every knowledge page ordin writes is a Markdown file with a short header, in the Open Knowledge Format 0.2. ordin writes and reads this format.
- Any tool that reads the format can read your pages. You can keep them in Git and leave with them at any time.
- ordin reads other teams' folders and glossaries in the same format with no conversion.
- Each page shows its type (Explanation, How-to, Decision, Reference or Glossary) and a one-sentence description.
- Each page shows whether a person has checked it. Pages a person checked rank above the rest when ordin looks for an answer.

## How it works

1. ordin writes a page with its type, description, tags, sources and who wrote it.
2. When you write or edit a page, or accept an update to it, ordin records you as having checked it, using your Git host login, never your email address.
3. ordin keeps an `index.md` and a `log.md` in the knowledge folder and in a team folder. They list the pages and the changes, and are never treated as pages.
4. When ordin reads a folder it did not write, each file becomes a page you can open, and its links and sources are kept.

## You stay in charge

- Nothing leaves your computer until you approve a share.
- Anything in a header that ordin does not use is kept exactly as written.
- A page marked deprecated is shown as Deprecated and is used only when nothing else matches.
- A page marked draft is shown as Draft. A page past its review date shows "may be out of date".

## Good to know

- Pages written before this version open unchanged and gain the new header keys the next time they are saved.
- An unknown type, an unknown key, a broken link or a missing `index.md` is never an error.
- ordin shows computations that other tools attached to a page but never runs them.
- The format covers how knowledge is written down. Where it is stored and who may read it are still decided by your computer and your repository.

## See also

[Knowledge pages](knowledge-pages.md) · [Team knowledge](team-knowledge.md) · [Memory](memory.md) · [Knowledge agents](knowledge-agents.md) · [Markdown documents](markdown.md) · [The paper](../knowledge-fabric.md)
