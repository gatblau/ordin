// Recovery after a restart (§5.9): ordin stops while an agent is working, as a power cut would; at the
// next start Ordin says what happened while ordin was closed, and the person tries the task again. Needs
// journeys 02 and 04.
import { expect, test } from '../support/test';
import { script, type Rule } from '../support/stand-ins';
import { openWorkspace, ordinShows, regions, say } from '../support/ui';

const ask: Rule = {
  when: 'check the supplier notes slowly',
  steps: [
    { tool: 'delegate', input: { agent: '{{key:Document agent}}', brief: 'Check the supplier notes carefully, taking your time.' } },
    { text: "I've asked the document agent to check the supplier notes." },
  ],
};
const answer = (delay: number): Rule => ({ who: 'agent', when: 'Check the supplier notes carefully', steps: [{ text: 'The supplier notes look complete.', delay }] });

test('recovery after a restart: an interrupted task is reported and tried again', async ({ app }) => {
  // After a restart the window is a new page: each step reads app.page afresh.
  script([ask, answer(600_000)]);

  await app.step('the person gives the agent a long task', async () => {
    const { page } = app;
    const { screen } = regions(page);
    await openWorkspace(page, 'Supplier review');
    await say(page, 'Please check the supplier notes slowly');
    await ordinShows(page, "I've asked the document agent to check the supplier notes.");
    await page.getByRole('button', { name: 'Home' }).first().click();
    await expect(screen.getByText('Nothing running now')).toBeHidden({ timeout: 60_000 });
  });

  await app.step('ordin stops while the agent is working, and starts again', async () => {
    // The agent's session ends with the program (on Windows its job ends with it); the run is left
    // unfinished in the journal, and the next start records it as interrupted.
    await app.restart();
  });

  await app.step('opening the workspace, Ordin says the task was interrupted', async () => {
    const { page } = app;
    await openWorkspace(page, 'Supplier review');
    await ordinShows(page, /While ordin was closed: 1 task was interrupted\./);
  });

  await app.step('they open the task from the card and try it again; it finishes', async () => {
    const { page } = app;
    const { screen, conversation } = regions(page);
    script([ask, answer(0)]);
    await conversation.getByRole('button', { name: 'Open' }).last().click();
    const review = screen.getByRole('button', { name: 'Review' });
    if (await review.isVisible()) await review.click();
    await screen.getByRole('button', { name: 'Try again' }).click();
    await ordinShows(page, 'The supplier notes look complete.', 180_000);
  });
});
