// Adding a workspace (§5.13, §5.14): a workspace with a folder on this computer, a SharePoint folder, a
// Confluence page tree and a Jira issue. Journeys 03 to 07 work in the workspace this journey makes.
import { expect, test } from '../support/test';
import { where } from '../support/ordin';
import { createWorkspace, linkFolder, linkJiraIssue, linkOnline, regions, showPlaces, words, workspaceGroup } from '../support/ui';

test('adding a workspace with a local folder, SharePoint, Confluence and Jira', async ({ app }) => {
  const { page } = app;
  const { screen } = regions(page);

  await app.step('the person makes the workspace Supplier review', async () => {
    await createWorkspace(page, 'Supplier review');
  });

  await app.step('they link the folder Sample documents from this computer', async () => {
    await linkFolder(page, where.sampleDocuments, 'Sample documents');
  });

  await app.step('they link the SharePoint folder Supplier contracts', async () => {
    await linkOnline(page, 'SharePoint', 'Supplier', 'Procurement › Supplier contracts');
  });

  await app.step('they link the Confluence page Supplier pages and the pages under it', async () => {
    await linkOnline(page, 'Confluence', 'Supplier', 'Procurement › Supplier pages');
  });

  await app.step('they paste a Jira issue; ordin asks them to connect Jira first', async () => {
    await linkJiraIssue(page, 'https://acme.atlassian.net/browse/PROC-12');
    await expect(screen.getByText('Connect Jira in Settings to link an issue.')).toBeVisible();
  });

  await app.step('they connect Jira with an API token in Settings', async () => {
    await screen.getByRole('button', { name: 'Open Settings › Jira' }).click();
    await screen.getByLabel(/Your Jira site/).fill('acme.atlassian.net');
    await screen.getByLabel(/The email address you sign in/).fill('sam@acme.example');
    await screen.getByLabel('API token').fill('journeys-token');
    await screen.getByRole('button', { name: 'Connect', exact: true }).click();
    await expect(screen.getByText(words('Connected to acme.atlassian.net as Sam Lee'))).toBeVisible();
  });

  await app.step('they go back and link the issue', async () => {
    await page.getByRole('tab', { name: 'Add to this workspace' }).click();
    await linkJiraIssue(page, 'https://acme.atlassian.net/browse/PROC-12');
    await expect(screen.getByText(words('PROC-12 Renew Kestrel contract is linked to this workspace.'))).toBeVisible();
  });

  await app.step('Documents in the tree lists all four (SM-RL-3)', async () => {
    await showPlaces(page, 'Supplier review');
    const places = workspaceGroup(page, 'Supplier review');
    for (const name of ['Sample documents', 'Supplier contracts', 'Supplier pages', 'PROC-12 Renew Kestrel contract'])
      await expect(places.getByRole('button', { name: words(name) }).first()).toBeVisible();
  });
});
