// Browsing and opening documents (§5.6): a Markdown file, a PDF and a Word file from the folder on this
// computer, a Word file from SharePoint, a Confluence page and a Jira issue; the tree opens a location to
// its files. Needs journey 02's workspace.
import { expect, test } from '../support/test';
import { openInDocuments, openWorkspace, openWorkspacePart, regions, words } from '../support/ui';

test('browsing and opening documents', async ({ app }) => {
  const { page } = app;
  const { screen } = regions(page);
  const back = async (tab: string) => page.getByRole('tab', { name: words(tab) }).click();

  await app.step('the person opens Documents in Supplier review', async () => {
    await openWorkspace(page, 'Supplier review');
    await openWorkspacePart(page, 'Supplier review', 'Documents');
  });

  await app.step('in the tree, Sample documents opens to its files and its folder, and a file opens from there (SM-RL-3)', async () => {
    const { tree } = regions(page);
    // The Documents row opens and closes with each press (an earlier journey may have left it open).
    const documents = tree.locator('.tree-leaf[aria-expanded]').first();
    if ((await documents.getAttribute('aria-expanded')) !== 'true') await documents.locator(':scope > .tree-line > button.tree-row').click();
    await expect(documents).toHaveAttribute('aria-expanded', 'true');
    await tree.getByRole('button', { name: words('Sample documents') }).first().click();
    for (const name of ['Meeting notes.md', 'Quarterly plan.pdf', 'Supplier terms.docx', 'Q3'])
      await expect(tree.getByRole('button', { name: words(name) }).first()).toBeVisible();
    await tree.getByRole('button', { name: words('Meeting notes.md') }).first().click();
    await expect(screen.getByText('Kestrel Freight: 1,240 GBP per container from 1 January.')).toBeVisible();
    // Back to the Documents list, where the next step starts.
    await back('Documents');
  });

  await app.step('in Sample documents the folder lists its files and its subfolder', async () => {
    await openInDocuments(page, 'Sample documents');
    for (const name of ['Meeting notes.md', 'Quarterly plan.pdf', 'Supplier terms.docx', 'Q3'])
      await expect(screen.getByRole('button', { name: words(name) }).first()).toBeVisible();
  });

  await app.step('a Markdown file shows its text, and can be edited', async () => {
    await openInDocuments(page, /^Meeting notes\.md/);
    await expect(screen.getByText('Kestrel Freight: 1,240 GBP per container from 1 January.')).toBeVisible();
    await expect(screen.getByRole('button', { name: 'Edit' })).toBeVisible();
    await back('Sample documents');
  });

  await app.step('a PDF says it opens in its own app', async () => {
    await openInDocuments(page, /^Quarterly plan\.pdf/);
    await expect(screen.getByText('PDFs open in their own app. Use Open in its app to read this one.')).toBeVisible();
    await expect(screen.getByRole('button', { name: 'Open in its app' })).toBeVisible();
    await back('Sample documents');
  });

  await app.step('a Word file shows its text', async () => {
    await openInDocuments(page, /^Supplier terms\.docx/);
    await expect(screen.getByText(/Payment within 30 days of the invoice\./)).toBeVisible();
    await back('Sample documents');
  });

  await app.step('the subfolder opens', async () => {
    await openInDocuments(page, /^Q3/);
    await expect(screen.getByRole('button', { name: words('Supplier rates.txt') }).first()).toBeVisible();
  });

  await app.step('a Word file in SharePoint shows its text', async () => {
    await openWorkspacePart(page, 'Supplier review', 'Documents');
    await openInDocuments(page, 'Supplier contracts');
    await openInDocuments(page, /^Kestrel terms\.docx/);
    await expect(screen.getByText(/Rate: 1,180 GBP per container/)).toBeVisible();
  });

  await app.step('a Confluence page shows its words under its title', async () => {
    await openWorkspacePart(page, 'Supplier review', 'Documents');
    await openInDocuments(page, 'Supplier pages');
    await openInDocuments(page, /^Supplier pages$/);
    await openInDocuments(page, /^Rates/);
    await expect(screen.getByText('Kestrel Freight: 1,180 GBP per container.')).toBeVisible();
  });

  await app.step('a Jira issue shows its summary and description', async () => {
    await openWorkspacePart(page, 'Supplier review', 'Documents');
    await openInDocuments(page, /^PROC-12/);
    await openInDocuments(page, /^summary-and-description/);
    await expect(screen.getByText(/Renew Kestrel contract/).first()).toBeVisible();
  });
});
