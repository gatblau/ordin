// An approval (§5.5, §5.6 Documents and changes): the document agent changes the linked copy of a
// Confluence page; Ordin proposes sending the change back; the person reads the card and approves it; the
// page in Confluence changes. Needs journeys 02 and 04 (the workspace, the agent and its access).
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { where } from '../support/ordin';
import { expect, test } from '../support/test';
import { confluencePage, script } from '../support/stand-ins';
import { openWorkspace, ordinShows, regions, say } from '../support/ui';

/** What the linked copy of the Confluence page Rates holds now ('' while there is none). */
function ratesCopy(): string {
  const found = readdirSync(where.linkedCopies, { recursive: true, encoding: 'utf8' }).filter((f) => f.endsWith('Rates.confluence.xhtml'));
  return found.length ? readFileSync(join(where.linkedCopies, found[found.length - 1]), 'utf8') : '';
}

test('an approval: a change goes back to Confluence only after the person approves it', async ({ app }) => {
  const { page } = app;
  const { screen, conversation } = regions(page);
  const before = confluencePage('Rates');
  script([
    {
      when: 'lower the Kestrel rate',
      steps: [
        { tool: 'delegate', input: { agent: '{{key:Document agent}}', brief: 'Lower the Kestrel rate in the Rates page to 1,100 GBP per container.' } },
        { text: "I've asked the document agent to lower the rate." },
      ],
    },
    {
      who: 'agent',
      when: 'Lower the Kestrel rate in the Rates page',
      steps: [
        { tool: 'Read', input: { file_path: '{{file:Rates.confluence.xhtml}}' } },
        { tool: 'Write', input: { file_path: '{{file:Rates.confluence.xhtml}}', content: '<h1>Rates</h1><p>Kestrel Freight: 1,100 GBP per container from 1 March.</p>' } },
        { text: 'The Rates page now says 1,100 GBP per container from 1 March.' },
      ],
    },
    {
      when: 'Is the rate done',
      steps: [
        { tool: 'propose_push', input: { request: 'Send the lower Kestrel rate back to Confluence.', items: [{ item: '{{item:Rates}}', summary: 'Kestrel Freight now 1,100 GBP per container' }] } },
        { text: "Yes. I've proposed sending the new rate back to Confluence." },
      ],
    },
  ]);

  await app.step('the person asks Ordin to lower the rate; the agent changes the copy', async () => {
    await openWorkspace(page, 'Supplier review');
    await say(page, 'Please lower the Kestrel rate');
    await ordinShows(page, "I've asked the document agent to lower the rate.");
    await expect.poll(ratesCopy, { message: 'the linked copy of Rates holds the new rate', timeout: 180_000 }).toContain('1,100 GBP');
    expect(confluencePage('Rates')?.v, 'nothing has gone to Confluence yet').toBe(before?.v);
  });

  await app.step('they ask whether it is done; Ordin proposes sending it back', async () => {
    await say(page, 'Is the rate done?');
    await ordinShows(page, "Yes. I've proposed sending the new rate back to Confluence.");
  });

  await app.step('they open the card: it names Confluence and the page', async () => {
    await conversation.getByRole('button', { name: 'Open' }).last().click();
    await expect(screen.getByText(/Confluence/).first()).toBeVisible();
    await expect(screen.getByText(/Rates/).first()).toBeVisible();
  });

  await app.step('they approve it, and the page in Confluence changes', async () => {
    await screen.getByRole('button', { name: 'Send back', exact: true }).click();
    await expect.poll(() => confluencePage('Rates')?.v, { timeout: 60_000 }).toBe((before?.v ?? 0) + 1);
    expect(confluencePage('Rates')?.body).toContain('1,100 GBP');
  });
});
