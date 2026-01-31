// Asking about a document on a fresh installation (plan 05 Phases 2 to 6B): the person has just installed
// ordin, links a folder holding a PDF, opens the PDF and presses Ask Ordin about this. Ordin is told which
// folder and file (OH-ATT-1), hands the work to the document agent that came installed (DSK-DOC-1), the
// person allows it to read the folder with one card (OH-ACC-2), the task starts, and the agent writes its
// summary in its own folder: Allow lets it read the person's folder, not change it. Then the person removes the agent,
// and it stays removed after a restart (PKG-DOC-3). Runs after journey 09 and makes its own data root, so
// nothing the earlier journeys made is needed or changed.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '../support/test';
import { freshDataRoot, stopOrdin, where } from '../support/ordin';
import { script, writeStandInsFile } from '../support/stand-ins';
import type { Page } from '@playwright/test';
import { createWorkspace, linkFolder, openWorkspace, openWorkspacePart, ordinShows, regions, showPlaces, words, workspaceGroup } from '../support/ui';

/** The Document agent's row under Agents in the tree. */
const agentRow = (page: Page) => workspaceGroup(page, 'Reports').locator('button.tree-agent', { hasText: words('Document agent') });

const SUMMARY = 'Quarterly plan - summary.md';
// The agent writes its files by a plain name, which land in its own folder ("the run's folder" of its role), the
// way the real document agent saves what it makes; the folder policy must allow it (RulesEngine.md H.2).
const RUN_NOTES = 'Quarterly plan — run notes.md';
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
  const { tree, conversation } = regions(page);
  script(
    [
      {
        // The rule matches only when Ordin's turn carries the location line: without it, Ordin gets the default reply.
        when: LOCATION_LINE,
        steps: [
          { tool: 'delegate', input: { agent: '{{key:Document agent}}', brief: 'Summarise Quarterly plan.pdf in the folder Sample documents for the team.' } },
          { text: "I'll have the **document agent** read Quarterly plan.pdf." },
        ],
      },
      {
        // After Allow the task starts by itself, and Ordin is told so (OH-ACC-2 item 3).
        when: 'Started:',
        steps: [{ text: "I've given the quarterly plan to the **document agent**.\n\n- It reads the PDF.\n- It writes a summary in its own folder." }],
      },
      {
        // The stand-in model cannot read a PDF, so the agent writes the summary without reading it: this
        // journey proves the route to the agent and its write, not the agent's reading.
        who: 'agent',
        when: 'Summarise Quarterly plan.pdf',
        steps: [
          { tool: 'Write', input: { file_path: SUMMARY, content: '# Quarterly plan - summary\n\nThe plan holds three goals for the quarter.\n' } },
          { tool: 'Write', input: { file_path: RUN_NOTES, content: 'Run notes: read the folder, wrote the summary.\n' } },
          { text: `I wrote ${SUMMARY} and ${RUN_NOTES}.` },
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
    // The side tree is the one list of agents (SM-AG-6).
    await openWorkspacePart(page, 'Reports', 'Agents');
    await expect(agentRow(page)).toBeVisible();
  });

  await app.step('they open the PDF and press Ask Ordin about this; Ordin knows the folder and the file', async () => {
    // The document opens from the tree; its actions are in its ⋯ menu there (SM-RL-3, SM-ASK-1).
    await showPlaces(page, 'Reports');
    await tree.getByRole('button', { name: words('Sample documents') }).first().click();
    await tree.getByRole('button', { name: /^Quarterly plan\.pdf/ }).first().click();
    await tree.getByRole('button', { name: words('More actions for Quarterly plan.pdf') }).first().click();
    await tree.getByRole('menuitem', { name: /^Ask .* about this/ }).click();
    // A folder the agent cannot read is asked for with one card, Allow or Not now (OH-ACC-2).
    await ordinShows(page, 'Document agent needs to read Sample documents to answer.');
  });

  await app.step('they press Allow; the task starts, the summary is in the agent\'s folder, and Ordin answers formatted', async () => {
    await conversation.getByRole('button', { name: 'Allow', exact: true }).last().click();
    await ordinShows(page, "I've given the quarterly plan to the document agent.");
    const answer = conversation.locator('li', { hasText: "I've given the quarterly plan" }).last();
    await expect(answer.locator('strong', { hasText: 'document agent' })).toBeVisible();
    await expect(answer.locator('li')).toHaveCount(2);
    await expect(answer).not.toContainText('**');
    // Files by a plain name are in the agent's own folder ("the run's folder" of its role), and the folder
    // policy refused nothing; the person's folder is left as it was, since Allow lets the agent read it only.
    const agentsDir = join(where.dataRoot, 'agents');
    const ownFile = (name: string) => readdirSync(agentsDir).map((id) => join(agentsDir, id, 'folder', name)).find((f) => existsSync(f));
    await expect.poll(() => ownFile(SUMMARY) !== undefined, { message: `${SUMMARY} is in the agent's own folder`, timeout: 180_000 }).toBe(true);
    await expect.poll(() => ownFile(RUN_NOTES) !== undefined, { message: `${RUN_NOTES} is in the agent's own folder`, timeout: 60_000 }).toBe(true);
    expect(existsSync(join(where.sampleDocuments, SUMMARY)), "nothing was written into the person's folder").toBe(false);
    const logs = join(where.dataRoot, 'logs');
    const refused = readdirSync(logs)
      .flatMap((n) => readFileSync(join(logs, n), 'utf8').split('\n'))
      .filter((l) => l.includes('"event":"rules_decided"') && l.includes('"decision":"deny"'));
    expect(refused, 'no tool call was refused by the rules').toEqual([]);
    expect(readFileSync(ownFile(SUMMARY)!, 'utf8')).toContain('three goals');
  });

  await app.step('they remove the document agent from the workspace', async () => {
    // An agent's ⋯ menu in the tree holds Remove…, which asks first (SM-AG-6).
    await expect(agentRow(page)).toBeVisible();
    // The ⋯ button shows while the row is pointed at.
    await agentRow(page).hover();
    await tree.getByRole('button', { name: words('More actions for Document agent') }).first().click();
    await tree.getByRole('menuitem', { name: words('Remove Document agent…') }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Remove', exact: true }).click();
    await expect(agentRow(page)).toHaveCount(0);
  });

  await app.step('after a restart the agent is still gone', async () => {
    await app.restart();
    await openWorkspace(app.page, 'Reports');
    await openWorkspacePart(app.page, 'Reports', 'Agents');
    // The Agents row is open and the list has been read: the library is offered, and no agent is listed.
    await expect(workspaceGroup(app.page, 'Reports').locator('[role=treeitem][aria-expanded=true]').first()).toBeVisible();
    await expect(agentRow(app.page)).toHaveCount(0);
  });
});
