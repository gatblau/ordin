// @vitest-environment happy-dom
// SM-RL-5: a Confluence link's own row offers the document menu for its page.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import DocumentsTreeGroup from '../src/lib/shell/DocumentsTreeGroup.svelte';
import { APP_KEY } from '../src/lib/shell/context';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { DocumentLinksModel } from '../src/lib/views/add/links-model.svelte';
import { hello, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
import { appWith, loadWindowCatalogue } from './support/ssr';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const WS = uid('ws-1');
const PCF = uid('p-cf');
const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const link = (id: string, kind: string, display_name: string) =>
  ({ id, kind, display_name, state: 'active', workspace_id: WS, added_at: '2026-01-06T09:00:00.000Z', origin_location: display_name, place_id: uid(`p-${id}`) }) as never;
const PAGE = { name: 'Supplier pages.confluence.xhtml', path: 'Supplier pages.confluence.xhtml', is_folder: false, kind: 'xhtml', place_id: PCF, size_bytes: 1, changed_at: '2026-01-10T10:00:00Z', has_versions: false };
const CHILDREN = { name: 'Supplier pages', path: 'Supplier pages', is_folder: true, kind: '', place_id: PCF, size_bytes: 0, changed_at: '2026-01-10T10:00:00Z', has_versions: false };

const CHILD = { ...PAGE, name: '2026.confluence.xhtml', path: 'Supplier pages/2026.confluence.xhtml' };

async function draw(menuOn: string | null, children: unknown[] = [CHILD]) {
  const { app, transport } = await appWith((t) =>
    t
      .on('service.hello', ok(hello(LOCAL, [...R1_GROUPS, 'confluence', 'sharepoint'])))
      .on('workspaces.links', ok([link('cf', 'confluence_location', 'Supplier pages'), link('sp', 'sharepoint_location', 'Contracts')]))
      .on('files.list_folder', (input) =>
        ok({ items: (input as { folder: string }).folder === '' ? (children.length > 0 ? [CHILDREN, PAGE] : [PAGE]) : children, next_cursor: null }),
      )
      .on('ordin.send_message', ok({ accepted: true, position_in_queue: 0 })),
  );
  const model = new DocumentLinksModel(app.local.handle, WS);
  await model.load();
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(DocumentsTreeGroup as never, {
    target,
    props: { backend: app.local, workspaceId: WS, model, initiallyOpen: true, initialMenu: menuOn } as never,
    context: new Map<unknown, unknown>([[APP_KEY, app]]),
  });
  await settle(10);
  flushSync();
  transport.sent.length = 0;
  return { transport, target, app };
}

const items = (): string[] => [...document.querySelectorAll('[role=menuitem]')].map((b) => clean(b.textContent));

describe("A Confluence link row's document menu (SM-RL-5)", () => {
  // spec: ordin-desktop/components/ScreensModule#SM-RL-5
  it('Happy path — Ask, Add to knowledge and Add to memory, then Remove', async () => {
    await draw('cf');
    expect(items()).toEqual(['Ask Ordin about this', 'Add to knowledge', 'Add to memory', 'Remove from this workspace…']);
  });

  it('Add to memory sends the page file, found at the working copy root', async () => {
    const { transport } = await draw('cf');
    ([...document.querySelectorAll<HTMLButtonElement>('[role=menuitem]')].find((b) => clean(b.textContent) === 'Add to memory') as HTMLButtonElement).click();
    await settle(10);
    const sent = transport.sentOf('ordin.send_message')[0]?.input as { text: string; attachments: unknown[] };
    expect(clean(sent.text)).toBe('Add the facts in “Supplier pages” to memory.');
    expect(sent.attachments).toEqual([{ kind: 'place_file', place_id: PCF, relative_path: 'Supplier pages.confluence.xhtml' }]);
  });

  it('Edge case — a SharePoint link keeps only Remove', async () => {
    await draw('sp');
    expect(items()).toEqual(['Remove from this workspace…']);
  });
});

describe('A linked Confluence page is one row (SM-RL-6)', () => {
  const row = (): HTMLButtonElement => [...document.querySelectorAll<HTMLButtonElement>('.tree-link')].find((b) => clean(b.textContent) === 'Supplier pages') as HTMLButtonElement;
  const names = (): string[] => [...document.querySelectorAll('.tree-name')].map((n) => clean(n.textContent));

  // spec: ordin-desktop/components/ScreensModule#SM-RL-6
  it('Happy path — a page with no children opens itself and draws nothing under the link', async () => {
    const { app } = await draw(null, []);
    row().click();
    await settle(10);
    flushSync();
    const scope = ScreenAreaState.scope(app.local.id, WS);
    expect(app.screens.tabs(scope).find((t) => t.key === `document:${PCF}:Supplier pages.confluence.xhtml`)?.title).toBe('Supplier pages');
    expect(names()).toEqual([]);
    expect(row().closest('[role=treeitem]')?.hasAttribute('aria-expanded')).toBe(false);
  });

  it('A page with a child page draws the child, not the page again', async () => {
    await draw(null);
    row().click();
    await settle(10);
    flushSync();
    expect(names()).toEqual(['2026.confluence.xhtml']);
  });
});
