# Memory

**Every agent in ordin remembers through one memory — facts, the things they are about and the links between them — and you can see, correct and forget all of it.**

Category: [Context](README.md#context) · Status: Working code in [`memory/`](../../memory/); things, links and the new views are designed and specified, not yet built

## What you get

- Ordin remembers your preferences, earlier decisions and what it learned about your documents.
- Every agent can remember too: Ordin, the members of a crew, and agents doing a task on their own, each under its own memory rules. An agent with no memory rules remembers nothing.
- A fact can say what it is about and how things relate: "Borealis supplies packaging". Asking "who supplies packaging?" then finds it, even when the fact used other words.
- Ordin writes short summaries of what it learned about one thing, and keeps the facts each summary was made from.
- Longer explanations are kept as [knowledge pages](knowledge-pages.md). A thing that a page explains says so, and facts from a page rank above the same facts from a plain document.

## How it works

1. As work happens, agents keep what is worth remembering, naming the things a fact is about and any links between them.
2. Later sessions recall it, together with facts about things linked to what you asked about, so you don't repeat yourself.
3. A link that stops being true is closed, not erased, so you can see when it held.

## You stay in charge

- **What Ordin remembers** shows every fact and every thing. A fact says what kind it is (something that happened, something known, a summary), since when it is true, what it replaced and how it changed.
- Each thing lists its facts and its links in plain words, for example "Borealis supplies packaging · since 3 Oct".
- You can correct or forget any fact or link, forget everything about a thing (you see how many facts and links first), and say two things are the same.
- A fact that has not been used for a long while is marked "Fading"; **Keep** stops it fading.
- Under an answer, **What Ordin used** shows the facts and links it relied on. A run's progress shows what was remembered during it, and by which agent.
- Nothing changes in memory without your choice, and memory stays on the machine that runs ordin: your computer or your own server.

## Good to know

- Memory belongs to one person. To share what a team learned, use [team knowledge](team-knowledge.md); links travel with the facts that state them.
- Forgetting a fact also forgets any summary made from it; ordin tells you before it does.
- A [hosted agent](hosted-agent.md) keeps memory by channel, and a direct chat is never written to another channel's memory. Its owners can see each channel's memory; people in a channel can ask what an answer used.

## See also

[Knowledge pages](knowledge-pages.md) · [Team knowledge](team-knowledge.md) · [Open Knowledge Format](open-knowledge-format.md) · [Knowledge agents](knowledge-agents.md)
