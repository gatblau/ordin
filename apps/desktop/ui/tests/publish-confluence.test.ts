// @vitest-environment happy-dom
// A folder's Confluence publishing (SM-PC-1, SM-PC-2): the Ready to publish list of an active link, and
// Publish this folder to Confluence… for a folder with none. Mounted so their reads and sends run.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { R1_GROUPS, ok, settle, type FakeTransport } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';
import PublishingFolder from '../src/lib/views/fixed/PublishingFolder.svelte';
import { backendWith, WS } from './fixed-support';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const PLACE = uid('place-sample');
const LINK = uid('link-pricing');
const GROUPS = [...R1_GROUPS, 'confluence'];
const text = (el: Element): string => (el.textContent ?? '').replace(/[⁨⁩]/g, '');
const place = { id: PLACE, friendly_name: 'Sample documents', path: 'C:\\Users\\me\\Sample documents', kind: 'persons_folder', added_at: '2026-01-26T10:00:00.000Z' };
const tree = { workspace: { id: WS, name: 'Supplier review' }, places: [place], agents: [], running: [], dashboards: [], workflows: [], knowledge_pages: 0, memory_facts: 0, waiting_for_person: 0 };
const location = { space_key: 'PROC', space_name: 'Procurement', page_id: '4000', title: 'Supplier pages', web_url: 'https://acme.atlassian.net/wiki/spaces/PROC/pages/4000' };
const link = (state: string) => ({ id: LINK, workspace_id: WS, place_id: PLACE, folder: 'Pricing', confluence_space: 'PROC', parent_page_id: '4000', audience: 'Procurement team', state });

async function draw(setup: (t: FakeTransport) => void, folder: string) {
  const b = await backendWith({ groups: GROUPS, setup: (t) => { t.on('workspaces.tree', ok(tree)); setup(t); } });
  await b.state.loadTree(WS);
  b.transport.sent.length = 0;
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(PublishingFolder as never, { target, props: { backend: b.state, workspaceId: WS, placeId: PLACE, folder } as never });
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
  return { ...b, target };
}

describe('A folder linked to Confluence (SM-PC-2)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-PC-2
  it('Happy path — an active link shows its Ready to publish list with Publish checked items…', async () => {
    const { target, transport } = await draw(
      (t) =>
        t
          .on('workspaces.confluence_links', ok([link('active')]))
          .on('workspaces.confluence_publishing', ok({ confluence_link_id: LINK, automatic: 'on', turned_off_by: [] }))
          .on('confluence.ready_to_publish', ok([{ confluence_link_id: LINK, kind: 'new', path: 'Pricing notes.md', published_copy_id: uid('copy') }])),
      // As a folder row names it (files.list_folder): with a trailing slash.
      'Pricing/',
    );
    expect(transport.sentOf('workspaces.confluence_links').map((e) => e.input)).toEqual([{ workspace_id: WS }]);
    expect(text(target)).toContain('Ready to publish');
    expect(text(target)).toContain('Pricing notes.md');
    expect(text(target)).toContain('Publish checked items…');
    expect(text(target)).not.toContain('Publish this folder to Confluence…');
  });

  it('Edge case — a proposed link says it waits for approval; another folder of the place offers publishing', async () => {
    const waiting = await draw((t) => t.on('workspaces.confluence_links', ok([link('proposed')])), 'Pricing');
    expect(text(waiting.target)).toContain('Waiting for you to approve publishing this folder to Confluence.');
    document.body.innerHTML = '';
    if (drawn !== null) void unmount(drawn);
    const other = await draw((t) => t.on('workspaces.confluence_links', ok([link('proposed')])), 'Quotes');
    expect(text(other.target)).toContain('Publish this folder to Confluence…');
  });
});

describe('Publish this folder to Confluence… (SM-PC-1)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-PC-1
  it('Happy path — find the page, Publish here proposes the link for this folder', async () => {
    const { target, transport } = await draw(
      (t) =>
        t
          .on('workspaces.confluence_links', ok([]))
          .on('confluence.find', ok([{ space_key: 'PROC', space_name: 'Procurement', title: 'Procurement', web_url: 'https://acme.atlassian.net/wiki/spaces/PROC' }, location]))
          .on('workspaces.propose_confluence_link', ok({ ...link('proposed'), approval_id: uid('approval') })),
      'Pricing',
    );
    (target.querySelector('button') as HTMLButtonElement).click();
    flushSync();
    const input = target.querySelector('input[type="search"]') as HTMLInputElement;
    input.value = 'Supplier';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    (target.querySelector('form') as HTMLFormElement).dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await settle(10);
    flushSync();
    expect(transport.sentOf('confluence.find').map((e) => e.input)).toEqual([{ query: 'Supplier', limit: 10 }]);
    // A whole space is not a page to publish under.
    expect(text(target)).toContain('Procurement › Supplier pages');
    expect(text(target)).not.toContain('Procurement › Procurement');
    const here = [...target.querySelectorAll('button')].find((b) => b.textContent === 'Publish here') as HTMLButtonElement;
    here.click();
    await settle(10);
    expect(transport.sentOf('workspaces.propose_confluence_link').map((e) => e.input)).toEqual([{ workspace_id: WS, place_id: PLACE, folder: 'Pricing', location }]);
  });
});
