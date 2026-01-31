// Browsing and opening documents (§5.6): a Markdown file, a PDF and a Word file from the folder on this
// computer, a Word file from SharePoint, a Confluence page and a Jira issue; the tree opens a location to
// its files. Needs journey 02's workspace.
import { expect, test } from '../support/test';
import { openWorkspace, openWorkspacePart, regions, showPlaces, words } from '../support/ui';

test('browsing and opening documents', async ({ app }) => {
  const { page } = app;
  const { screen, tree } = regions(page);
  // Places and their files are browsed in the tree; a document opens in the screen area (SM-RL-3).
  const inTree = (name: string | RegExp) => tree.getByRole('button', { name: typeof name === 'string' ? words(name) : name }).first();
  const open = async (...path: (string | RegExp)[]) => {
    await showPlaces(page, 'Supplier review');
    for (const name of path) await inTree(name).click();
  };

  await app.step('the person opens Documents in Supplier review', async () => {
    await openWorkspace(page, 'Supplier review');
    await openWorkspacePart(page, 'Supplier review', 'Documents');
    await showPlaces(page, 'Supplier review');
  });

  await app.step('in the tree, Sample documents opens to its files and its folder (SM-RL-3)', async () => {
    await open('Sample documents');
    for (const name of ['Meeting notes.md', 'Quarterly plan.pdf', 'Supplier terms.docx', 'Q3'])
      await expect(inTree(name)).toBeVisible();
  });

  await app.step('a Markdown file shows its text, and can be edited', async () => {
    await inTree(/^Meeting notes\.md/).click();
    await expect(screen.getByText('Kestrel Freight: 1,240 GBP per container from 1 January.')).toBeVisible();
    await expect(screen.getByRole('button', { name: 'Edit' })).toBeVisible();
  });

  await app.step('a PDF is shown in the screen area, with its zoom', async () => {
    await inTree(/^Quarterly plan\.pdf/).click();
    await expect(screen.getByRole('heading', { name: words('Quarterly plan.pdf') })).toBeVisible();
    await expect(screen.getByRole('button', { name: 'Zoom in' })).toBeVisible();
  });

  await app.step('a Word file shows its text', async () => {
    await inTree(/^Supplier terms\.docx/).click();
    await expect(screen.getByText(/Payment within 30 days of the invoice\./)).toBeVisible();
  });

  await app.step('the subfolder opens', async () => {
    await inTree(/^Q3/).click();
    await expect(inTree('Supplier rates.txt')).toBeVisible();
  });

  await app.step('a Word file in SharePoint shows its text', async () => {
    await open('Supplier contracts', /^Kestrel terms\.docx/);
    await expect(screen.getByText(/Rate: 1,180 GBP per container/)).toBeVisible();
  });

  await app.step('a Confluence page shows its words under its title', async () => {
    // The linked page tree opens to its top page, Supplier pages, which holds Rates.
    await open('Supplier pages');
    await tree.getByRole('button', { name: /^Supplier pages$/ }).nth(1).click();
    await inTree(/^Rates/).click();
    await expect(screen.getByText('Kestrel Freight: 1,180 GBP per container.')).toBeVisible();
  });

  await app.step('a Jira issue shows its summary and description', async () => {
    await open(/^PROC-12/);
    // The issue opens to its parts in the tree, as a folder does.
    await inTree(/^summary-and-description/).click();
    await expect(screen.getByText(/Renew Kestrel contract/).first()).toBeVisible();
  });

  await app.step('Earlier conversations opens in the screen area from the conversation pane (SM-WR-7)', async () => {
    await page.getByRole('button', { name: /^Earlier conversations/ }).first().click();
    await expect(screen.getByRole('heading', { name: words('Earlier conversations') })).toBeVisible();
  });
});
