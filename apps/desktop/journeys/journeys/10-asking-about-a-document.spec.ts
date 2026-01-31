// Asking about a document on a fresh installation (plan 05 Phases 2 to 6B): the person has just installed
// ordin, links a folder holding a PDF, opens the PDF and presses Ask Ordin about this. Ordin is told which
// folder and file (OH-ATT-1), hands the work to the document agent that came installed (DSK-DOC-1), the
// person lets it use the folder, and the summary lands in the folder. Then the person removes the agent,
// and it stays removed after a restart (PKG-DOC-3). Runs after journey 09 and makes its own data root, so
// nothing the earlier journeys made is needed or changed.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '../support/test';
import { freshDataRoot, stopOrdin, where } from '../support/ordin';
import { script, writeStandInsFile } from '../support/stand-ins';
import { createWorkspace, linkFolder, openInDocuments, openWorkspace, openWorkspacePart, ordinShows, regions, say, words } from '../support/ui';

const SUMMARY = 'Quarterly plan - summary.md';
const LOCATION_LINE = 'Attached: Quarterly plan.pdf (in Sample documents: Quarterly plan.pdf)';

test('asking about a document on a fresh installation', async ({ app }) => {
  // A first start over an empty data root, as journey 01 has it: the earlier journeys' data is set aside.
  await app.detach();
  await stopOrdin();
  freshDataRoot();
  writeStandInsFile();
  // `open` starts the program: starting it here as well could start two copies on one data root.
  await app.open();

  const { page } = app;
  const { tree, screen, conversation } = regions(page);
  script(
    [
      {
        // The rule matches only when Ordin's turn carries the location line: without it, Ordin gets the default reply.
        when: LOCATION_LINE,
        steps: [
          { tool: 'delegate', input: { agent: '{{key:Document agent}}', brief: 'Summarise Quarterly plan.pdf in the folder Sample documents for the team.' } },
          { text: "The **document agent** can't use your folders yet. Tick the ones it may use, then ask me to go ahead." },
        ],
      },
      {
        when: 'Go ahead',
        steps: [
          { tool: 'delegate', input: { agent: '{{key:Document agent}}', brief: 'Summarise Quarterly plan.pdf in the folder Sample documents for the team.' } },
          { text: "I've given the quarterly plan to the **document agent**.\n\n- It reads the PDF.\n- It writes the summary beside it." },
        ],
      },
      {
        // The stand-in model cannot read a PDF, so the agent writes the summary without reading it: this
        // journey proves the route to the agent and its write, not the agent's reading.
        who: 'agent',
        when: 'Summarise Quarterly plan.pdf',
        steps: [
          { tool: 'Write', input: { file_path: `{{dir:Quarterly plan.pdf}}/${SUMMARY}`, content: '# Quarterly plan - summary\n\nThe plan holds three goals for the quarter.\n' } },
          { text: `I wrote ${SUMMARY}.` },
        ],
      },
    ],
    { ordin: 'Ordin was not told which file the person meant.' },
  );

  await app.step('a fresh installation starts and opens on Home', async () => {
    await expect(page.getByText('Getting you ready…')).toBeHidden({ timeout: 180_000 });
    await expect(tree.getByRole('button', { name: 'Home' })).toBeVisible({ timeout: 60_000 });
  });

  await app.step('the person makes a workspace and links the folder holding the PDF', async () => {
    await createWorkspace(page, 'Reports');
    await linkFolder(page, where.sampleDocuments, 'Sample documents');
  });

  await app.step('the document agent is already in the workspace', async () => {
    await openWorkspacePart(page, 'Reports', 'Agents');
    await expect(screen.getByRole('button', { name: words('Document agent') }).first()).toBeVisible();
  });

  await app.step('they open the PDF and press Ask Ordin about this; Ordin knows the folder and the file', async () => {
    await openWorkspacePart(page, 'Reports', 'Documents');
    await openInDocuments(page, 'Sample documents');
    await openInDocuments(page, /^Quarterly plan\.pdf/);
    await screen.getByRole('button', { name: /^Ask .* about this/ }).click();
    await ordinShows(page, "can't use your folders yet.");
    await ordinShows(page, "Document agent can't use this workspace's folders yet. Tick the ones it may use, then ask me to go ahead.");
  });

  await app.step('they let it use the folder and say go ahead; the summary is in the folder, and Ordin answers formatted', async () => {
    const box = conversation.getByRole('checkbox', { name: words('can read and change Sample documents') }).last();
    await box.check();
    await expect(box).toBeChecked();
    await say(page, 'Go ahead');
    await ordinShows(page, "I've given the quarterly plan to the document agent.");
    const answer = conversation.locator('li', { hasText: "I've given the quarterly plan" }).last();
    await expect(answer.locator('strong', { hasText: 'document agent' })).toBeVisible();
    await expect(answer.locator('li')).toHaveCount(2);
    await expect(answer).not.toContainText('**');
    const file = join(where.sampleDocuments, SUMMARY);
    await expect.poll(() => existsSync(file), { message: `${SUMMARY} is in the folder`, timeout: 180_000 }).toBe(true);
    expect(readFileSync(file, 'utf8')).toContain('three goals');
  });

  await app.step('they remove the document agent from the workspace', async () => {
    await openWorkspacePart(page, 'Reports', 'Agents');
    await screen.getByRole('button', { name: words('Document agent') }).first().click();
    await screen.getByRole('button', { name: words('Remove Document agent…') }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Remove', exact: true }).click();
    await expect(screen.getByRole('button', { name: words('Document agent') })).toHaveCount(0);
  });

  await app.step('after a restart the agent is still gone', async () => {
    await app.restart();
    await openWorkspace(app.page, 'Reports');
    await openWorkspacePart(app.page, 'Reports', 'Agents');
    await expect(regions(app.page).screen.getByText('No agents in this workspace yet.')).toBeVisible();
    await expect(regions(app.page).screen.getByRole('button', { name: words('Document agent') })).toHaveCount(0);
  });
});
