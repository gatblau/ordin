// Settings and updates (§5.19 How Ordin talks, §5.31 About ordin): every section of Settings opens with
// no error, its form fields one to a line and no offer to send anything to IT; the person asks Ordin to be more direct and Settings shows it; only complete languages are
// offered; About ordin checks for updates. Needs journey 02.
import { expect, test } from '../support/test';
import { script } from '../support/stand-ins';
import { goTo, openWorkspace, ordinShows, regions, say } from '../support/ui';

const SECTIONS = [
  'Sign-ins',
  'Agent repositories',
  'Jira',
  'Servers',
  'How Ordin talks to you',
  'Language',
  'Using your plan',
  'Notifications',
  'History',
  'Display',
  'Code',
  'What Ordin remembers',
  'Diagnostics',
  'About ordin',
];

test('settings and updates', async ({ app }) => {
  const { page } = app;
  const { screen } = regions(page);
  const section = (name: string) => screen.locator('nav').getByRole('button', { name: new RegExp(`^${name.replace(/ /g, '[\\s\\u2068\\u2069]+')}$`) });
  /** The labelled fields of the open section whose rows overlap on the screen (they would read as one run of words). */
  const fieldsSideBySide = () =>
    page.evaluate(() => {
      const scope = document.querySelector('#screen-panel') ?? document.body;
      const boxes = [...scope.querySelectorAll('label')]
        .filter((l) => l.querySelector('input:not([type=checkbox]):not([type=radio]):not([type=range]), select, textarea'))
        .map((l) => ({ text: (l.textContent ?? '').trim().slice(0, 40), box: l.getBoundingClientRect() }))
        .filter((b) => b.box.height > 0);
      const clashes: string[] = [];
      for (let i = 0; i < boxes.length; i++)
        for (let j = i + 1; j < boxes.length; j++) {
          const a = boxes[i]!.box, b = boxes[j]!.box;
          if (Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > 2) clashes.push(`${boxes[i]!.text} | ${boxes[j]!.text}`);
        }
      return clashes;
    });
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
      await section(name).click();
      await expect(section(name)).toHaveAttribute('aria-current', 'page');
      // SM-FL-1: fields with a label stand one to a line, never side by side; SM-ER-3, SM-NS-3: no help request to IT.
      expect(await fieldsSideBySide(), `${name}: two form fields share a line`).toEqual([]);
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
    await section('How Ordin talks to you').click();
    await expect(screen.getByRole('radio', { name: /direct/i }).first()).toBeChecked();
  });

  await app.step('Language offers only the languages ordin is complete in', async () => {
    await section('Language').click();
    // Following the computer, and the one language ordin is complete in.
    const options = screen.getByRole('listbox').getByRole('option');
    await expect(options).toHaveCount(2);
    await expect(options.nth(0)).toHaveText('Follow this computer');
    await expect(options.nth(0)).toHaveAttribute('aria-selected', 'true');
    await expect(options.nth(1)).toHaveText(/English \(UK\)/);
  });

  await app.step('About ordin checks for updates and says what it found', async () => {
    await section('About ordin').click();
    await screen.getByRole('button', { name: 'Check for updates' }).click();
    await expect(screen.getByRole('button', { name: 'Check for updates' })).toBeEnabled({ timeout: 60_000 });
    await expect(screen.getByRole('status').first()).not.toBeEmpty();
  });
});
