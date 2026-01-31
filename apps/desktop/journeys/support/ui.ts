// What a person does in the window, in their words: the steps the journeys share.
//
// Names the screens draw around a person's own words carry Unicode isolation marks (U+2068, U+2069), so
// a locator by a name the person chose uses a regular expression, not an exact string.
import type { Locator, Page } from '@playwright/test';
import { expect } from '@playwright/test';
import { answerNextDialog } from './ordin';

export const regions = (page: Page) => ({
  tree: page.locator('[data-region=tree]'),
  screen: page.locator('#screen-panel'),
  conversation: page.locator('[data-region=conversation]'),
});

/** A pattern for words the screens may have wrapped in isolation marks, anywhere among them. */
export function words(text: string): RegExp {
  const marks = '[\\u2068\\u2069]*';
  const pattern = [...text]
    .map((c) => (c === ' ' ? '[\\s\\u2068\\u2069]+' : c.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')))
    .join(marks);
  return new RegExp(pattern);
}

/** Search or go to… (Ctrl+K), then the first result for `query`. */
export async function goTo(page: Page, query: string): Promise<void> {
  await page.keyboard.press('Escape');
  await page.keyboard.press('Control+k');
  const dialog = page.getByRole('dialog');
  await dialog.locator('input').fill(query);
  await page.waitForTimeout(1200);
  await page.keyboard.press('Enter');
}

export async function createWorkspace(page: Page, name: string): Promise<void> {
  const { tree } = regions(page);
  await tree.getByRole('button', { name: 'New workspace' }).click();
  await tree.locator('input').fill(name);
  await tree.getByRole('button', { name: 'Create' }).click();
  await expect(workspaceRow(page, name)).toBeVisible();
}

export const workspaceRow = (page: Page, name: string): Locator =>
  regions(page).tree.locator('[role=treeitem]').filter({ has: page.locator('span', { hasText: words(name) }) }).first();

/** Opens a workspace from the side rail: its conversation opens beside the screen. */
export async function openWorkspace(page: Page, name: string): Promise<void> {
  await workspaceRow(page, name).locator('span', { hasText: words(name) }).first().click();
  await expect(regions(page).conversation.getByRole('tab', { name: words(name) })).toBeVisible();
}

/** A row under a workspace in the side rail: Documents, Agents, Knowledge, What Ordin remembers… */
export async function openWorkspacePart(page: Page, workspace: string, part: string): Promise<void> {
  const row = workspaceRow(page, workspace);
  if ((await row.getAttribute('aria-expanded')) !== 'true') await row.locator('.tree-toggle').click();
  // The workspace's rows are the group drawn just after its node.
  const group = row.locator('xpath=following-sibling::*[@role="group"][1]');
  await group.locator('button', { hasText: words(part) }).first().click();
}

/** Add to this workspace, from Search or go to… */
export async function openAddToWorkspace(page: Page): Promise<void> {
  await goTo(page, 'Add documents');
  await expect(regions(page).screen.getByRole('heading', { name: 'Add to this workspace' })).toBeVisible();
}

/** Back to the choices of Add to this workspace after a link was added. */
async function addAgain(page: Page): Promise<void> {
  const { screen } = regions(page);
  const again = screen.getByRole('button', { name: 'Add something else' });
  if (await again.isVisible()) await again.click();
  else await openAddToWorkspace(page);
}

/** Links a folder on this computer: the folder picker is answered through the journeys' answer file. */
export async function linkFolder(page: Page, folder: string, name: string): Promise<void> {
  const { screen } = regions(page);
  await addAgain(page);
  await screen.getByRole('button', { name: /^This computer/ }).click();
  answerNextDialog(folder);
  await screen.getByRole('button', { name: 'Choose a folder…' }).click();
  await expect(screen.getByText(words(`${name} is linked to this workspace.`))).toBeVisible();
}

/** Links a SharePoint folder or a Confluence page found by `query`. */
export async function linkOnline(page: Page, service: 'SharePoint' | 'Confluence', query: string, result: string): Promise<void> {
  const { screen } = regions(page);
  await addAgain(page);
  await screen.getByRole('button', { name: new RegExp(`^${service}`) }).click();
  await screen.locator('input').first().fill(query);
  await screen.locator('input').first().press('Enter');
  await screen.locator('li', { hasText: result }).getByRole('button', { name: 'Add' }).click();
  await expect(screen.getByText(words(`${result.split(' › ').pop()} is linked to this workspace.`))).toBeVisible();
}

/** Links a Jira issue by its address. */
export async function linkJiraIssue(page: Page, address: string): Promise<void> {
  const { screen } = regions(page);
  await addAgain(page);
  await screen.getByRole('button', { name: 'Jira issue…' }).click();
  await screen.locator('input').first().fill(address);
  await screen.getByRole('button', { name: 'Add', exact: true }).click();
}

/** Sends a message to Ordin in the open conversation. */
export async function say(page: Page, text: string): Promise<void> {
  const box = page.locator('#conversation-message-box');
  await box.fill(text);
  await regions(page).conversation.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(regions(page).conversation.getByText(text, { exact: true }).last()).toBeVisible();
}

/** Waits until Ordin's conversation shows `text` (Ordin's words, a card, a line). */
export async function ordinShows(page: Page, text: string | RegExp, timeout = 90_000): Promise<void> {
  await expect(regions(page).conversation.getByText(typeof text === 'string' ? words(text) : text).last()).toBeVisible({ timeout });
}

/** Opens a document or folder by name in the Documents screen. */
export async function openInDocuments(page: Page, name: string | RegExp): Promise<void> {
  await regions(page).screen.getByRole('button', { name: typeof name === 'string' ? words(name) : name }).first().click();
}
