// Meeting minutes (§7.15, PB-MIN-2). With a build that has `minutes`: minutes from `meeting.vtt` after the
// one-time notice; the minutes edited, saved and reopened; exported as Word; `speech-60s.m4a` with Not now on
// the speech-model question and nothing downloaded; then, with the fake OBS of Step 81 on port 4455, Record,
// the reminder, the indicator, Stop, and minutes arriving with no further input. With a build without
// `minutes`: no Minutes row, no Record button, no Meeting minutes settings. Needs journey 02's workspace.
// spec: ordin-desktop/phase-5-playbook#PB-MIN-2
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { answerNextDialog, where } from '../support/ordin';
import { fixture, minutesBuild, placeLargerModel, speechModelFiles, startFakeObs } from '../support/minutes';
import { script } from '../support/stand-ins';
import { expect, test } from '../support/test';
import { createWorkspace, expectSettingsSectionCount, goTo, openWorkspace, openWorkspacePart, regions, words, workspaceGroup, workspaceRow } from '../support/ui';

const WORKSPACE = 'Supplier review';
const FROM_TRANSCRIPT = 'Northwind renewal';
const FROM_RECORDING = 'Recorded supplier call';
const HEADINGS = ['Summary', 'Decisions', 'Actions', 'Open questions'];

/** Ordin's minutes run: the stand-in calls `keep_minutes` once (OH-MIN-2). The later rule wins for the VTT. */
function scriptTheWriter(): void {
  const keep = (title: string) => ({
    tool: 'keep_minutes',
    input: {
      title,
      summary: 'The supplier contract was reviewed.',
      decisions: ['Stay with the current supplier for another year.'],
      actions: [{ text: 'Send the renewal terms', owner: 'Priya Shah' }, { text: 'Check the new rates' }],
      open_questions: ['Whether the rates change in January.'],
    },
  });
  script([
    { who: 'ordin', when: 'Write minutes of the meeting', steps: [keep(FROM_RECORDING), { text: 'Minutes kept.' }] },
    { who: 'ordin', when: 'Northwind', steps: [keep(FROM_TRANSCRIPT), { text: 'Minutes kept.' }] },
  ]);
}

test('meeting minutes', async ({ app }) => {
  const { page } = app;
  const { screen, tree } = regions(page);
  const workspaceRows = () => tree.locator('[role=treeitem]').filter({ hasText: words(WORKSPACE) }).first().locator('xpath=following-sibling::*[@role="group"][1]');

  // Journeys 09 and 10 leave a fresh data root behind them: the workspace is made here when it is missing.
  await app.step('the person has the workspace Supplier review', async () => {
    if ((await workspaceRow(page, WORKSPACE).count()) === 0) await createWorkspace(page, WORKSPACE);
  });

  if (!minutesBuild) {
    await app.step('without the minutes switch there is no Minutes row, Record button or Meeting minutes settings (SM-UP-1)', async () => {
      await openWorkspace(page, WORKSPACE);
      const row = tree.locator('[role=treeitem]').filter({ hasText: words(WORKSPACE) }).first();
      if ((await row.getAttribute('aria-expanded')) !== 'true') await row.locator('.tree-toggle').click();
      await expect(workspaceRows().locator('button', { hasText: words('Documents') }).first()).toBeVisible();
      await expect(page.getByTestId('minutes-tree')).toHaveCount(0);
      await expect(page.getByTestId('minutes-record')).toHaveCount(0);
      await goTo(page, 'Settings');
      await expectSettingsSectionCount(page, 'How Ordin talks to you', 1);
      await expectSettingsSectionCount(page, 'Meeting minutes', 0);
    });
    return;
  }

  scriptTheWriter();
  const obs = startFakeObs();
  try {
    // There is no Minutes screen: minutes, their jobs and the Minutes menu live in the tree (SM-MIN-17 to
    // SM-MIN-19); the questions a job waits on are lines in Needs you (SM-MIN-21).
    // Each workspace has its own Minutes row: this one's is in Supplier review's group.
    const minutesRow = workspaceGroup(page, WORKSPACE).getByTestId('minutes-tree');
    const openMinutesRow = async () => {
      await openWorkspace(page, WORKSPACE);
      // The workspace's rows are drawn only while its row is open.
      const row = workspaceRow(page, WORKSPACE);
      if ((await row.getAttribute('aria-expanded')) !== 'true') await row.locator('.tree-toggle').click();
      if ((await minutesRow.getAttribute('aria-expanded')) !== 'true') await openWorkspacePart(page, WORKSPACE, 'Minutes');
      await expect(minutesRow).toHaveAttribute('aria-expanded', 'true');
    };
    const minutesMenu = async (item: 'minutes-record' | 'minutes-import') => {
      await openMinutesRow();
      // The ⋯ button shows while the row is pointed at.
      await minutesRow.locator('.tree-line').first().hover();
      await minutesRow.getByTestId('minutes-tree-menu').click();
      await minutesRow.getByTestId(item).click();
    };
    const openQuestion = async (kind: 'model' | 'notice') => {
      await page.getByRole('tab', { name: words('Needs you') }).click();
      const line = page.getByTestId(`minutes-needs-${kind}`);
      await expect(line).toBeVisible({ timeout: 60_000 });
      await line.getByRole('button', { name: 'Open' }).click();
    };
    const openKept = async (title: string) => {
      await openMinutesRow();
      await minutesRow.getByRole('button', { name: words(title) }).first().click();
      await expect(screen.getByTestId('minutes-save')).toBeVisible();
    };

    await app.step('the person makes minutes from meeting.vtt and accepts the one-time notice (SM-MIN-4)', async () => {
      answerNextDialog(fixture('meeting.vtt'));
      await minutesMenu('minutes-import');
      await openQuestion('notice');
      const notice = page.getByRole('dialog').filter({ hasText: 'Before Ordin writes minutes' });
      await expect(notice).toBeVisible({ timeout: 60_000 });
      await notice.getByRole('button', { name: 'Write the minutes' }).click();
      await expect(minutesRow.getByRole('button', { name: words(FROM_TRANSCRIPT) }).first()).toBeVisible({ timeout: 180_000 });
    });

    await app.step('the minutes open with their four headings; an edit is saved and still there on reopening (SM-MIN-8)', async () => {
      await openKept(FROM_TRANSCRIPT);
      const editor = screen.locator('[contenteditable=true]').first();
      for (const heading of HEADINGS) await expect(editor).toContainText(heading);
      await editor.click();
      await page.keyboard.press('Control+End');
      await page.keyboard.type('\nAgreed by everyone present.');
      await expect(screen.getByTestId('minutes-save')).toBeEnabled();
      await screen.getByTestId('minutes-save').click();
      await expect(screen.getByTestId('minutes-save')).toBeDisabled();
      await openKept(FROM_TRANSCRIPT);
      await expect(screen.locator('[contenteditable=true]').first()).toContainText('Agreed by everyone present.');
    });

    await app.step('Export › Word… saves a Word file (SM-MIN-8)', async () => {
      const file = join(where.run, 'Northwind renewal.docx');
      answerNextDialog(file);
      // The page's ⋯ menu holds Export (SM-MIN-13, SM-MIN-14).
      await screen.locator('.minutes-page').getByTestId('minutes-menu').click();
      await screen.getByTestId('minutes-export').click();
      await screen.getByTestId('minutes-export-word').click();
      await expect.poll(() => existsSync(file), { timeout: 30_000 }).toBe(true);
    });

    await app.step('speech-60s.m4a asks for the speech model; Not now leaves nothing downloaded (SM-MIN-3)', async () => {
      answerNextDialog(fixture('speech-60s.m4a'));
      await minutesMenu('minutes-import');
      await openQuestion('model');
      const question = page.getByRole('dialog').filter({ hasText: "Download Ordin's speech model?" });
      await expect(question).toBeVisible({ timeout: 60_000 });
      // The size is a formatted number (1,533), which the app may wrap in isolation marks.
      await expect(question).toContainText(/1\D?533\D{0,4}MB/);
      await question.getByRole('button', { name: 'Not now' }).click();
      await expect(minutesRow.getByTestId('minutes-job')).toHaveCount(0, { timeout: 30_000 });
      expect(speechModelFiles(), 'the speech-model folder is empty after Not now').toEqual([]);
    });

    await app.step('with the speech model in place and the fake OBS ready, Record shows the reminder, then the indicator (SM-MIN-5, SM-MIN-7)', async () => {
      placeLargerModel();
      await minutesMenu('minutes-record');
      const reminder = page.getByRole('dialog').filter({ hasText: "Tell everyone you're recording" });
      await expect(reminder).toBeVisible({ timeout: 30_000 });
      await expect(reminder.getByTestId('minutes-record-mic-only')).toHaveCount(0);
      await reminder.getByRole('button', { name: 'Start recording' }).click();
      await expect(page.getByTestId('minutes-recording')).toBeVisible();
    });

    await app.step('Stop ends the recording, and minutes arrive with no further input', async () => {
      await page.waitForTimeout(3000);
      await page.getByTestId('minutes-stop').click();
      await expect(page.getByTestId('minutes-recording')).toHaveCount(0, { timeout: 30_000 });
      await openMinutesRow();
      await expect(minutesRow.getByRole('button', { name: words(FROM_RECORDING) }).first()).toBeVisible({ timeout: 15 * 60_000 });
    });

    await app.step('two minutes pages are listed under Minutes, each with the four headings (PB-MIN-2)', async () => {
      for (const title of [FROM_TRANSCRIPT, FROM_RECORDING])
        await expect(workspaceRows().getByRole('button', { name: words(title) }).first()).toBeVisible();
      await openKept(FROM_RECORDING);
      const editor = screen.locator('[contenteditable=true]').first();
      for (const heading of HEADINGS) await expect(editor).toContainText(heading);
    });
  } finally {
    obs.stop();
  }
});
