// Starting a task (§5.3, §5.2, §5.4): the person asks Ordin for work; the document agent is already in the
// workspace (it comes installed, DSK-DOC-1), Ordin hands it the work, the person lets it use the folders,
// and the agent's work lands in the folder. Also what the person sees while it happens: the working line,
// the conversation moving to the newest item, Ordin's Markdown drawn formatted, no Close on a plain
// message, a window that never scrolls, and Settings at the foot of the rail; at the end the task's own
// screen shows the agent's final message formatted. Needs journey 02's workspace.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '../support/test';
import { where } from '../support/ordin';
import { script } from '../support/stand-ins';
import { expectSettingsSectionCount, openWorkspace, openWorkspacePart, ordinShows, regions, say, words } from '../support/ui';

const SUMMARY = 'Meeting notes - summary.md';

test('a task with Ordin: an agent from the library does the work', async ({ app }) => {
  const { page } = app;
  const { tree, screen, conversation } = regions(page);
  script([
    {
      when: 'summarise the meeting notes',
      steps: [
        { tool: 'delegate', input: { agent: '{{key:Document agent}}', brief: 'Summarise Meeting notes.md in the folder Sample documents for the team.' } },
        // Held back so the working line is on screen for the check below.
        { text: "The **document agent** can't use your folders yet.\n\n- Tick the ones it may use.\n- Then ask me to go ahead.", delay: 4000 },
      ],
    },
    {
      when: 'Go ahead',
      steps: [
        { tool: 'delegate', input: { agent: '{{key:Document agent}}', brief: 'Summarise Meeting notes.md in the folder Sample documents for the team.' } },
        { text: "I've given the meeting notes to the **document agent**." },
      ],
    },
    {
      // OH-RC-2: the agent's answer reaches Ordin's next turn, so Ordin can use it (before this it said the
      // agent had not sent its summary back).
      when: 'Tasks finished since your last turn',
      steps: [{ text: 'Noted: the document agent wrote the meeting notes summary.' }],
    },
    {
      who: 'agent',
      when: 'Summarise Meeting notes.md',
      steps: [
        { tool: 'Read', input: { file_path: '{{file:Meeting notes.md}}' } },
        { tool: 'Write', input: { file_path: `{{dir:Meeting notes.md}}/${SUMMARY}`, content: '# Meeting notes - summary\n\nKestrel Freight: 1,240 GBP per container from 1 January.\n' } },
        { text: `**Done.** I wrote ${SUMMARY}:\n\n- Kestrel Freight stays at 1,240 GBP per container.` },
      ],
    },
  ]);

  /** Ordin's working line (SM-WK-1): the one status line under the newest item. */
  const working = conversation.getByRole('status').filter({ hasText: /^Ordin is .+…/ });
  /** Whether the window itself scrolls (V5-SCR-1): it never does. */
  // V5-SCR-1: a scrollbar on the window, or content taller than the window by more than the one pixel that
  // rounding leaves on a scaled display (which `overflow: hidden` on the page clips).
  const windowScrolls = () =>
    page.evaluate(
      () => window.innerWidth > document.documentElement.clientWidth || (document.scrollingElement?.scrollHeight ?? 0) - window.innerHeight > 1,
    );
  /** What is taller than its box, for a failure message: the page and each region (height as drawn / height of its content). */
  const heights = () =>
    page.evaluate(() =>
      ['html', 'body', '#app', '.shell', '[data-region=tree]', '#screen-panel', '[data-region=conversation]']
        .map((q) => {
          const el = document.querySelector(q);
          return el ? `${q} ${el.clientHeight}/${el.scrollHeight}` : `${q} missing`;
        })
        .concat(`inner ${window.innerHeight}`)
        .join(', '),
    );

  await app.step('the rail ends with Settings; it opens Settings and the workspace stays open', async () => {
    await openWorkspace(page, 'Supplier review');
    await tree.getByRole('button', { name: 'Settings', exact: true }).click();
    await expectSettingsSectionCount(page, 'Diagnostics', 1);
    await expect(conversation.getByRole('tab', { name: words('Supplier review') })).toBeVisible();
    expect(await windowScrolls(), `the window does not scroll while Settings is open (${await heights()})`).toBe(false);
  });

  await app.step('with a long screen open the window itself does not scroll', async () => {
    await openWorkspacePart(page, 'Supplier review', 'Documents');
    await expect(screen.getByRole('heading', { name: 'Documents' })).toBeVisible();
    expect(await windowScrolls(), `the window does not scroll while Documents is open (${await heights()})`).toBe(false);
  });

  await app.step('the person asks Ordin; Ordin is working…, then the answer is in view, formatted, with no Close', async () => {
    await say(page, 'Please summarise the meeting notes for the team');
    await expect(working).toBeVisible();
    const reply = "can't use your folders yet.";
    await ordinShows(page, reply);
    await expect(working).toBeHidden();
    const message = conversation.locator('li', { hasText: reply }).last();
    await expect(message).toBeInViewport();
    // Ordin's Markdown is drawn, not shown: bold and a list, and no asterisks or dashes left in the text.
    await expect(message.locator('strong', { hasText: 'document agent' })).toBeVisible();
    await expect(message.locator('li')).toHaveCount(2);
    await expect(message).not.toContainText('**');
    await expect(message.getByRole('button', { name: 'Close' })).toHaveCount(0);
  });

  await app.step('Ordin asks which folders the agent may use', async () => {
    await ordinShows(page, "Document agent can't use this workspace's folders yet. Tick the ones it may use, then ask me to go ahead.");
  });

  await app.step('they let it read and change the folder, the Confluence pages and the SharePoint folder', async () => {
    for (const place of ['Sample documents', 'Supplier pages', 'Supplier contracts']) {
      const box = conversation.getByRole('checkbox', { name: words(`can read and change ${place}`) }).last();
      await box.check();
      await expect(box).toBeChecked();
    }
  });

  await app.step('they say go ahead again; the agent writes the summary into the folder', async () => {
    await say(page, 'Go ahead');
    await ordinShows(page, "I've given the meeting notes to the document agent.");
    const file = join(where.sampleDocuments, SUMMARY);
    await expect.poll(() => existsSync(file), { message: `${SUMMARY} is in the folder`, timeout: 180_000 }).toBe(true);
    expect(readFileSync(file, 'utf8')).toContain('1,240 GBP');
    // The task's card in the conversation names the agent that did it, and its result is in the conversation
    // before the person's next message, so that message's turn carries it (OH-RC-2).
    await ordinShows(page, /Document agent/);
    await ordinShows(page, /I wrote Meeting notes/, 180_000);
  });

  await app.step('Ordin knows what the agent said when the person asks about it next (OH-RC-2)', async () => {
    await say(page, 'Please note that down');
    await ordinShows(page, 'Noted: the document agent wrote the meeting notes summary.');
  });

  await app.step("the task's own screen shows what the agent said as formatted text (SM-RES-1)", async () => {
    await conversation.getByRole('button', { name: 'Open' }).last().click();
    const review = screen.getByRole('button', { name: 'Review' });
    if (await review.isVisible()) await review.click();
    await expect(screen.locator('strong', { hasText: 'Done.' })).toBeVisible({ timeout: 60_000 });
    await expect(screen.locator('li', { hasText: 'Kestrel Freight' })).toBeVisible();
    await expect(screen).not.toContainText('**');
  });
});
