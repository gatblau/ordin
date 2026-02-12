// @vitest-environment happy-dom
// Knowledge pages in the tree, and screens that keep up (ScreensModule.md SM-KT-1 to SM-KT-3; plan 10
// Phase 9). User report of 2026-02-11: a page Ordin wrote did not appear in the tree or on the Knowledge
// screen until it was opened again.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { knowledgeTree, shownAtLevel } from '../src/lib/shell/knowledge-tree';
import KnowledgeTreeGroup from '../src/lib/shell/KnowledgeTreeGroup.svelte';
import { APP_KEY } from '../src/lib/shell/context';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import type { PageListing, PageSummary } from '../src/lib/service-api/types.gen';
import Knowledge from '../src/lib/views/fixed/Knowledge.svelte';
import { err, event, hello, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const WS = uid('ws');
const text = (el: Element | null): string => (el?.textContent ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();

let n = 0;
const page = (title: string, extra: Partial<PageSummary> = {}): PageSummary =>
  ({ id: uid(`page-${n++}`), workspace_id: WS, title, state: 'current', tags: [], last_changed_at: '2026-02-01T10:00:00Z', ...extra }) as PageSummary;
const listing = (own: PageSummary[], teams: PageSummary[] = []): PageListing =>
  ({ this_workspace: own, from_other_teams: teams, from_agents: [], not_adopted: [] }) as PageListing;

async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

/** Mounts the Knowledge row; `lists` answers `knowledge.list` in turn, the last one repeated. */
async function draw(lists: Array<PageListing | 'fail'>) {
  let call = 0;
  const reply = () => {
    const next = lists[Math.min(call++, lists.length - 1)];
    return next === 'fail' ? err('KNOW_STORE_UNAVAILABLE', 'The knowledge store cannot be read.', { action: 'try_again' }) : ok(next);
  };
  const { app, transport } = await appWith((t) => t.on('service.hello', ok(hello(LOCAL, R1_GROUPS))).on('knowledge.list', reply as never));
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(KnowledgeTreeGroup as never, { target, props: { backend: app.local, workspaceId: WS } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await flush();
  return { app, target, transport };
}
const rowNamed = (target: HTMLElement, name: string): HTMLElement => {
  const found = [...target.querySelectorAll('button')].find((b) => text(b) === name);
  expect(found, `a row named ${name}`).toBeDefined();
  return found as HTMLElement;
};
const unfold = async (target: HTMLElement): Promise<void> => {
  target.querySelector<HTMLElement>('[data-testid="knowledge-tree-toggle"]')!.click();
  await flush();
};
const pageTitles = (target: HTMLElement): string[] => [...target.querySelectorAll('[data-testid="knowledge-tree-page"]')].map((r) => text(r));

describe('Knowledge pages in the tree (SM-KT-1 to SM-KT-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-KT-1
  it('Choosing Knowledge still opens the screen, and the row is not unfolded', async () => {
    const { app, target, transport } = await draw([listing([page('Budget')])]);
    rowNamed(target, 'Knowledge').click();
    await flush();
    expect(app.screens.front(ScreenAreaState.scope(LOCAL, WS))).toBe('knowledge');
    expect(target.querySelector('[data-testid="knowledge-tree"]')?.getAttribute('aria-expanded')).toBe('false');
    expect(transport.sentOf('knowledge.list')).toHaveLength(0);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-KT-2
  it('Unfolding lists team folders, then the workspace\'s pages; a page opens its screen', async () => {
    const rates = page('Rates', { folder: 'Finance', folder_description: 'The finance team' });
    const { app, target, transport } = await draw([listing([page('Budget'), page('agenda')], [rates])]);
    await unfold(target);
    expect(transport.sentOf('knowledge.list').map((c) => c.input)).toEqual([{ workspace_id: WS }]);
    const rows = [...target.querySelectorAll('[role="group"] > [role="treeitem"]')].map((r) => text(r));
    expect(rows).toEqual(['Finance', 'agenda', 'Budget']);
    expect(rowNamed(target, 'Finance').getAttribute('title')).toBe('The finance team');
    rowNamed(target, 'Finance').click();
    await flush();
    expect(pageTitles(target)).toEqual(['Rates', 'agenda', 'Budget']);
    rowNamed(target, 'Rates').click();
    await flush();
    expect(app.screens.front(ScreenAreaState.scope(LOCAL, WS))).toBe(`knowledge:${rates.id}`);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-KT-1
  it('Right and Left on the row unfold and fold it, and the unfolded row is kept', async () => {
    const { app, target } = await draw([listing([page('Budget')])]);
    const row = target.querySelector<HTMLElement>('[data-testid="knowledge-tree"]')!;
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    await flush();
    expect(row.getAttribute('aria-expanded')).toBe('true');
    expect(app.prefs.knowledgeUnfolded()).toEqual([`${LOCAL}:${WS}`]);
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    await flush();
    expect(row.getAttribute('aria-expanded')).toBe('false');
    expect(app.prefs.knowledgeUnfolded()).toEqual([]);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-KT-3
  it('A page written while the row is unfolded appears', async () => {
    const { app, target, transport } = await draw([listing([page('Budget')]), listing([page('Budget'), page('Forecast')])]);
    await unfold(target);
    expect(pageTitles(target)).toEqual(['Budget']);
    await app.local.apply(event(LOCAL, app.local.lastSeq + 1, { kind: 'knowledge_pages_changed', workspace_id: WS }));
    await flush();
    expect(transport.sentOf('knowledge.list')).toHaveLength(2);
    expect(pageTitles(target)).toEqual(['Budget', 'Forecast']);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-KT-2
  it('Edge case — more than 10 pages: the 10 most recently changed by title, then Show all…', async () => {
    const pages = Array.from({ length: 12 }, (_, i) => page(`Page ${String.fromCharCode(65 + i)}`, { last_changed_at: `2026-02-${String(i + 1).padStart(2, '0')}T10:00:00Z` }));
    const { target } = await draw([listing(pages)]);
    await unfold(target);
    // A and B are the two oldest.
    expect(pageTitles(target)).toEqual(['C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L'].map((c) => `Page ${c}`));
    target.querySelector<HTMLElement>('[data-testid="knowledge-tree-show-all"]')!.click();
    await flush();
    expect(pageTitles(target)).toHaveLength(12);
    expect(target.querySelector('[data-testid="knowledge-tree-show-all"]')).toBeNull();
  });

  // spec: ordin-desktop/components/ScreensModule#SM-KT-2
  it('Edge case — no page at all says so; removed pages are not listed', async () => {
    const { target } = await draw([listing([page('Gone', { state: 'removed' })])]);
    await unfold(target);
    expect(text(target.querySelector('[data-testid="knowledge-tree-none"]'))).toBe('No pages yet.');
    expect(pageTitles(target)).toEqual([]);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-KT-2
  it('Error — the list cannot be read: an error line with Try again', async () => {
    const { target, transport } = await draw(['fail', listing([page('Budget')])]);
    await unfold(target);
    expect(text(target)).toContain('The knowledge store cannot be read.');
    const retry = [...target.querySelectorAll('button')].find((b) => text(b) === 'Try again');
    expect(retry).toBeDefined();
    retry!.click();
    await flush();
    expect(transport.sentOf('knowledge.list')).toHaveLength(2);
    expect(pageTitles(target)).toEqual(['Budget']);
  });
});

describe('knowledgeTree and shownAtLevel (SM-KT-2)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-KT-2
  it('groups team pages by folder name, folders and pages by name, ignoring case', () => {
    const tree = knowledgeTree(listing([page('beta'), page('Alpha')], [page('Z', { folder: 'legal' }), page('Y', { folder: 'Finance' }), page('X', { folder: 'Finance' })]));
    expect(tree.folders.map((f) => [f.name, f.pages.map((p) => p.title)])).toEqual([['Finance', ['X', 'Y']], ['legal', ['Z']]]);
    expect(tree.pages.map((p) => p.title)).toEqual(['Alpha', 'beta']);
  });

  it('shows every page at a level of 10 or fewer', () => {
    const pages = Array.from({ length: 10 }, (_, i) => page(`P${i}`));
    expect(shownAtLevel(pages, false)).toEqual({ shown: pages, more: false });
  });
});

describe('The Knowledge screen keeps up (SM-KT-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-KT-3
  it('An open Knowledge screen reads knowledge.list again on knowledge_pages_changed for its workspace', async () => {
    const { app, transport } = await appWith((t) => t.on('service.hello', ok(hello(LOCAL, R1_GROUPS))).on('knowledge.list', ok(listing([page('Budget')]))));
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(Knowledge as never, { target, props: { backend: app.local, workspaceId: WS } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    await flush();
    expect(transport.sentOf('knowledge.list')).toHaveLength(1);
    await app.local.apply(event(LOCAL, app.local.lastSeq + 1, { kind: 'knowledge_pages_changed', workspace_id: uid('other-ws') }));
    await flush();
    expect(transport.sentOf('knowledge.list')).toHaveLength(1);
    await app.local.apply(event(LOCAL, app.local.lastSeq + 1, { kind: 'knowledge_pages_changed', workspace_id: WS }));
    await flush();
    expect(transport.sentOf('knowledge.list')).toHaveLength(2);
  });
});
