// Memory and knowledge (§5.10): Ordin needs memory for the first time, the person allows the one-time
// download, Ordin remembers a fact the person states, and writes a knowledge page. Needs journey 02.
import { expect, test } from '../support/test';
import { script } from '../support/stand-ins';
import { openWorkspace, openWorkspacePart, ordinShows, regions, say, words } from '../support/ui';

test('memory: the one-time download, a fact kept, a knowledge page', async ({ app }) => {
  const { page } = app;
  const { screen, conversation } = regions(page);
  script([
    {
      when: 'remember that we report costs in GBP',
      steps: [
        { tool: 'remember', input: { fact: 'This workspace reports all costs in GBP.', source: 'The person said so in the conversation.' } },
        { text: "I'll remember that this workspace reports costs in GBP." },
      ],
    },
    {
      when: 'write up how we pay suppliers',
      steps: [
        {
          tool: 'write_page',
          input: {
            title: 'Paying suppliers',
            body: '# Paying suppliers\n\nSupplier invoices are **due within 30 days**.\n\n- Kestrel Freight: monthly invoice\n- Pay by bank transfer\n',
            sources: [],
            description: 'How and when supplier invoices are paid.',
            type: 'How-to',
          },
        },
        { text: "I've written a page on paying suppliers." },
      ],
    },
  ]);

  await app.step('the person asks Ordin to remember something; Ordin asks for the download', async () => {
    // Memory has no models yet: the remember call is refused once, and the card asks the person.
    app.expectLog('ordin_host.dependency_failed');
    await openWorkspace(page, 'Supplier review');
    await say(page, 'Please remember that we report costs in GBP');
    await expect(conversation.getByText('Getting memory ready')).toBeVisible({ timeout: 90_000 });
    await expect(conversation.getByText(words('To remember things for you, Ordin needs a one-time download of 174 MB from github.com.'))).toBeVisible();
  });

  await app.step('they choose Download; memory becomes ready', async () => {
    await conversation.getByRole('button', { name: 'Download', exact: true }).click();
    await expect(conversation.getByText('Memory is ready.')).toBeVisible({ timeout: 600_000 });
  });

  await app.step('they say it again; Ordin keeps the fact, and What Ordin remembers lists it', async () => {
    // Ordin said the same words after the first, refused try: wait for a new answer.
    const answer = conversation.getByText(words("I'll remember that this workspace reports costs in GBP."));
    const before = await answer.count();
    await say(page, 'Please remember that we report costs in GBP');
    await expect(answer).toHaveCount(before + 1, { timeout: 90_000 });
    // The fact is stored a moment after Ordin's answer (the first time, once the model has loaded), then
    // the reconciler, every 2 s (MemoryService §6), marks it kept; What Ordin remembers lists the kept
    // facts as they were when it opened (FU-102).
    await app.waitForLog('write.persisted');
    await page.waitForTimeout(3_000);
    await openWorkspacePart(page, 'Supplier review', 'What Ordin remembers');
    await expect(screen.getByText('This workspace reports all costs in GBP.')).toBeVisible();
  });

  await app.step('they ask Ordin to write a page; Knowledge shows it formatted', async () => {
    await say(page, 'Please write up how we pay suppliers');
    await ordinShows(page, "I've written a page on paying suppliers.");
    await openWorkspacePart(page, 'Supplier review', 'Knowledge');
    await screen.getByText(words('Paying suppliers')).first().click();
    await expect(screen.locator('strong', { hasText: 'due within 30 days' })).toBeVisible();
    await expect(screen.locator('li', { hasText: 'Pay by bank transfer' })).toBeVisible();
  });
});
