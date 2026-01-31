// Getting ready (UX guidelines §5.1): the very first start of ordin on an empty data root, with Claude Code
// on the computer. The run's global setup gave this journey a fresh data root and has not started ordin.
import { expect, test } from '../support/test';
import { script } from '../support/stand-ins';
import { createWorkspace, goTo, ordinShows, regions, say } from '../support/ui';

test('getting ready: the first start opens ordin, and Ordin answers', async ({ app }) => {
  const { page } = app;
  const { tree, screen } = regions(page);

  await app.step('the person starts ordin for the first time and it opens on Home', async () => {
    // The guided start shows only while a check needs the person; with Claude Code here it ends by itself.
    await expect(page.getByText('Getting you ready…')).toBeHidden({ timeout: 180_000 });
    await expect(tree.getByRole('button', { name: 'Home' })).toBeVisible({ timeout: 60_000 });
    await expect(screen.getByText('Nothing is waiting for you')).toBeVisible();
    await expect(screen.getByText('Nothing running now')).toBeVisible();
  });

  await app.step('they make a first workspace and say hello; Ordin answers', async () => {
    script([{ when: 'Hello Ordin', steps: [{ text: 'Hello. This workspace has nothing linked yet.' }] }]);
    await createWorkspace(page, 'First steps');
    await say(page, 'Hello Ordin');
    await ordinShows(page, 'Hello. This workspace has nothing linked yet.');
  });

  await app.step('Settings › Diagnostics opens', async () => {
    await goTo(page, 'Settings');
    await screen.locator('nav').getByRole('button', { name: 'Diagnostics', exact: true }).click();
    await expect(screen.locator('nav').getByRole('button', { name: 'Diagnostics', exact: true })).toHaveAttribute('aria-current', 'page');
  });
});
