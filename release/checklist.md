# Before each release: a walk by hand

The checks and the journeys prove that ordin does what its specifications say, with a stand-in model.
They cannot tell whether the specifications missed something a person needs, or whether Ordin, on the
real model, makes good choices. Before each release, walk this list with the release candidate and your
own Claude Code sign-in, on a Mac and on a Windows PC. On Windows, the journeys have already covered the
mechanics, so give most of your time to the parts marked **(real model)**.

Each line is what you do, then what you should see. Note anything that doesn't hold, or that reads
wrongly, with a screenshot and the log file (Mac: `~/Library/Application Support/org.ordin.desktop/logs/`;
Windows: `%LOCALAPPDATA%\org.ordin.desktop\logs\`). Every problem found becomes a spec change, a test and,
when a person can see it, a line here, so the next walk checks it again.

Before you start: quit ordin and move its data folder aside (Mac: `~/Library/Application Support/org.ordin.desktop`;
Windows: `%LOCALAPPDATA%\org.ordin.desktop`), and have Claude Code installed and signed in. Keep a folder
with a Markdown file, a PDF and a Word file to link.

## 1. Getting ready

- [ ] Start ordin. If "Getting you ready…" shows, it ends by itself; ordin opens on Home.
- [ ] Home says "Nothing is waiting for you" and "Nothing running now".
- [ ] Make a workspace **First steps**, say "Hello" to Ordin: Ordin answers.
- [ ] Settings › Diagnostics opens and shows nothing you have to do.

## 2. Adding a workspace

- [ ] Make a workspace **Supplier review**.
- [ ] Add to this workspace › This computer › Choose a folder…: pick your folder. It says the folder is
      linked, and nothing stays "Reading…".
- [ ] If you have SharePoint, Confluence or Jira: link a folder, a page and an issue. For Jira, ordin
      first asks you to connect it in Settings › Jira with an API token.
- [ ] Documents lists everything you linked.
- [ ] The Agents screen lists the document agent in this workspace, without you adding it.

## 3. Documents

- [ ] The Markdown file shows its text and offers Edit.
- [ ] The PDF says "PDFs open in their own app", and Open in its app opens it.
- [ ] The Word file shows its text (or a picture of its first page).
- [ ] A subfolder opens.

## 4. Talking to Ordin (real model)

- [ ] Send a message: within a second "Ordin is working…" shows under it, and the conversation moves so
      your message is in view. The line goes when Ordin answers.
- [ ] Ordin's answers are formatted: bold, lists and headings, with no `**` or `- ` marks showing.
- [ ] Your own messages show exactly as you typed them.
- [ ] Messages have no Close; cards show their close icon only when you point at them.
- [ ] Scroll up while Ordin is answering: the conversation stays put and "New messages ↓" appears.
- [ ] Ask "who are you?": Ordin's answer is short and true to what is happening now.

## 5. A question about a document (real model)

- [ ] On the PDF, choose Ask Ordin about this. Ordin hands it to the document agent without asking you to
      search the library, and the working line says what it is doing.
- [ ] Ordin asks which folders the document agent may use. Tick your folder, then say go ahead.
- [ ] The summary appears, formatted, and is about your PDF. Nothing in it is made up.
- [ ] Ordin never says the agent is working when nothing is running.

## 6. A task Ordin gives to an agent (real model)

- [ ] Ask Ordin to summarise a document for the team: the document agent's summary appears in your folder,
      and Ordin says so.
- [ ] Ask for something no installed agent does (for example, a review of code in a folder): Ordin
      searches the library itself and suggests an agent, with one sentence on why.

## 7. An approval

- [ ] With a Confluence or SharePoint link: ask Ordin to change a page through the agent, then ask whether
      it's done. A card asks you to send the change back, naming the service and the page.
- [ ] Open the card, read the change, choose Send back: the page changes in the service.

## 8. Recovery after a restart

- [ ] Give the agent a long task. While it runs, force ordin to quit (Mac: Force Quit from the Dock;
      Windows: Task Manager › End task).
- [ ] Start ordin and open the workspace: Ordin says "While ordin was closed: 1 task was interrupted."
- [ ] Open the card, then Try again: the task runs again.

## 9. Memory

- [ ] Ask Ordin to remember something. The card "Getting memory ready" asks for a one-time download, and
      Ordin does not call it an error.
- [ ] Download: it ends with "Memory is ready."
- [ ] Ask again: What Ordin remembers lists the fact.
- [ ] Ask Ordin to write a page: Knowledge shows it, formatted.

## 10. Settings and updates

- [ ] Every section of Settings opens with no error.
- [ ] Ask Ordin to be more direct: How Ordin talks to you shows it.
- [ ] Language offers only the languages with a full translation.
- [ ] About ordin › Check for updates says what it found.
- [ ] With the previous release installed: it offers this one, downloads it in the background, and
      **Restart to update** brings you to this version with your workspaces, followed by What's new.

## 11. Keeping data

- [ ] Quit ordin. Start the previous release's ordin on the same data, then this one again: it opens with
      your workspaces, and asks nothing.
- [ ] (Only on a test computer.) Quit ordin, and in the data folder's `journal.db` add a row to
      `schema_migrations` with a version above the highest. Start ordin: a message box says the data comes
      from a newer version and offers Start with empty data and Quit. Choose Start with empty data:
      ordin starts as on a first start, and the old data is in a `set-aside-…` folder.
