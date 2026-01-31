// Settings and updates (§5.19 How Ordin talks, §5.31 About ordin): every section of Settings opens with
// no error, its form fields one to a line and no offer to send anything to IT; the person asks Ordin to be more direct and Settings shows it; only complete languages are
// offered; About ordin checks for updates. Needs journey 02.
import { expect, test } from '../support/test';
import { script } from '../support/stand-ins';
import { fieldsSideBySide, goTo, openSettingsSection, openWorkspace, ordinShows, regions, say } from '../support/ui';

const SECTIONS = [
  'Sign-ins',
  'Agent repositories',
  'Jira',
  'Servers',
  'Meeting minutes',
  'How Ordin talks to you',
  'Language',
  'Using your plan',
  'Agents and models',
  'Notifications',
  'History',
  'Display',
  'Code',
  'Memory',
  'Diagnostics',
  'Updates',
];

test('settings and updates', async ({ app }) => {
  const { page } = app;
  const { screen } = regions(page);
  script([
    {
      when: 'be more direct',
      steps: [
        { tool: 'change_setting', input: { setting: 'tone', value: 'direct' } },
        { text: "Done: I'll be more direct from now on." },
      ],
    },
  ]);

  await app.step('every section of Settings opens', async () => {
    await goTo(page, 'Settings');
    for (const name of SECTIONS) {
      await openSettingsSection(page, name);
      // SM-FL-1: fields with a label stand one to a line, never side by side; SM-ER-3, SM-NS-3: no help request to IT.
      expect(await fieldsSideBySide(page), `${name}: two form fields share a line`).toEqual([]);
      await expect(screen).not.toContainText('Send to IT');
      await expect(screen).not.toContainText('Ask IT for');
    }
  });

  await app.step('the person asks Ordin to be more direct', async () => {
    await openWorkspace(page, 'Supplier review');
    await say(page, 'Could you be more direct with me?');
    await ordinShows(page, "Done: I'll be more direct from now on.");
  });

  await app.step('How Ordin talks to you shows the change', async () => {
    await goTo(page, 'Settings');
    await openSettingsSection(page, 'How Ordin talks to you');
    await expect(screen.getByRole('radio', { name: /direct/i }).first()).toBeChecked();
  });

  await app.step('Language offers only the languages ordin is complete in', async () => {
    await openSettingsSection(page, 'Language');
    // Following the computer, and the one language ordin is complete in.
    const options = screen.getByRole('radio');
    await expect(options).toHaveCount(2);
    await expect(screen.getByRole('radio', { name: 'Same as this computer' })).toBeChecked();
    await expect(options.nth(1)).toHaveAccessibleName(/English \(UK\)/);
  });

  await app.step('Updates checks for updates and says what it found', async () => {
    await openSettingsSection(page, 'Updates');
    await screen.getByRole('button', { name: 'Check for updates' }).click();
    await expect(screen.getByRole('button', { name: 'Check for updates' })).toBeEnabled({ timeout: 60_000 });
    await expect(screen.getByRole('status').first()).not.toBeEmpty();
  });
});
