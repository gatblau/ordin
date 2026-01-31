// Keeping or setting aside data this version does not recognise (OrdinDesktop.md DSK-DATA-1 to -3): the
// saved data was set up by a different ordin, and the person keeps it; then it comes from a newer ordin,
// and the person starts with empty data, which moves the old data to a folder. Runs last: it empties the
// data root.
import { existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '../support/test';
import { addAStepFromANewerOrdin, answerMessageBox, changeAnAppliedStep } from '../support/data';
import { startOrdin, stopOrdin, where } from '../support/ordin';
import { createWorkspace, regions, workspaceRow } from '../support/ui';

test('keeping or setting aside data a different version left', async ({ app }) => {
  // The program saying why it asked (DSK-DATA-1: start_failed, its reference, the journal's refusal),
  // and the journal recording the step the person accepted (DSK-DATA-2).
  app.expectLog('start_failed', 'start_failed_reference', 'journal.migrate.refused', 'journal.migrate.accepted');

  await app.step('the person has a workspace', async () => {
    await createWorkspace(app.page, 'Kept workspace');
  });

  await app.step('saved data from a different ordin: the person keeps it', async () => {
    await app.detach();
    await stopOrdin();
    changeAnAppliedStep();
    startOrdin();
    const box = await answerMessageBox('Keep my data');
    expect(box).toContain("ordin's saved data was set up by a different version of ordin");
    expect(box).toContain('Start with empty data');
    await app.open();
    await expect(workspaceRow(app.page, 'Kept workspace')).toBeVisible({ timeout: 120_000 });
  });

  await app.step('saved data from a newer ordin: the person starts with empty data', async () => {
    await app.detach();
    await stopOrdin();
    addAStepFromANewerOrdin();
    startOrdin();
    const box = await answerMessageBox('Start with empty data');
    expect(box).toContain("ordin's saved data comes from a newer version of ordin");
    await app.open();
    await expect(regions(app.page).screen.getByText('Nothing is waiting for you')).toBeVisible({ timeout: 120_000 });
    await expect(workspaceRow(app.page, 'Kept workspace')).toBeHidden();
  });

  await app.step('the old data is in a set-aside folder, not deleted', async () => {
    const aside = readdirSync(where.dataRoot).filter((n) => n.startsWith('set-aside-'));
    expect(aside).toHaveLength(1);
    expect(existsSync(join(where.dataRoot, aside[0], 'journal.db'))).toBe(true);
  });
});
