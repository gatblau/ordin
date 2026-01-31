// @vitest-environment happy-dom
// A knowledge page can be removed, and the removal undone (SM-KN-2). User report on 0.1.1: there was no way
// to remove a page from the knowledge base.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import Knowledge from '../src/lib/views/fixed/Knowledge.svelte';
import { err, ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const PAGE = uid('page-summary');
const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const view = (readOnly = false) => ({
  page: { id: PAGE, title: 'Manchester overview', body_markdown: 'A short page.', last_changed_at: '2026-01-12T10:00:00Z' },
  relative_path: 'knowledge/Manchester overview.md',
  aliases: [], tags: [], links_in: [], links_out: [], links_truncated: 0, suggestions: [], unregistered_aliases: [], changes: [],
  cites_other_computer: false, read_only: readOnly, past_stale_after: false, pending_update: false,
});
const listing = (titles: string[]) => ({
  this_workspace: titles.map((title) => ({ id: PAGE, title, tags: [], last_changed_at: '2026-01-12T10:00:00Z' })),
  from_other_teams: [], from_agents: [], not_adopted: [],
});
async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}
const press = (label: string): void => {
  // A button's accessible name: an icon button's aria-label (amended January 2026), else its words.
  const button = [...document.querySelectorAll('button')].filter((b) => (b.getAttribute('aria-label') ?? clean(b.textContent)) === label).pop();
  expect(button, `a button named ${label}`).toBeDefined();
  button?.click();
};

async function draw(read_only = false, remove: unknown = ok({})) {
  let listed = 0;
  const b = await backendWith({
    setup: (t) =>
      t
        .on('knowledge.list', (() => (listed++ === 0 ? ok(listing(['Manchester overview'])) : ok(listing([])))) as never)
        .on('knowledge.get', ok(view(read_only)))
        .on('knowledge.remove', remove as never)
        .on('knowledge.undo_remove', ok({})),
  });
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(Knowledge as never, { target, props: { backend: b.state, workspaceId: WS, instance: PAGE } as never });
  await flush();
  return { ...b, target };
}

describe('SM-KN-2 a knowledge page can be removed', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-KN-2
  it('Happy path — Remove… asks, then knowledge.remove is sent, the page closes and Undo is offered', async () => {
    const { target, transport } = await draw();
    press('Remove…');
    await flush();
    expect(clean(document.body.textContent)).toContain('Remove “Manchester overview”?');
    expect(transport.sentOf('knowledge.remove')).toHaveLength(0);
    press('Remove');
    await flush();
    expect(transport.sentOf('knowledge.remove').map((c) => c.input)).toEqual([{ page_id: PAGE }]);
    expect(clean(target.textContent)).toContain('Removed “Manchester overview”.');
    expect(clean(target.textContent)).not.toContain('A short page.');
    press('Undo');
    await flush();
    expect(transport.sentOf('knowledge.undo_remove').map((c) => c.input)).toEqual([{ page_id: PAGE }]);
    expect(clean(target.textContent)).not.toContain('Removed “Manchester overview”.');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-KN-2
  it('Edge case — Cancel sends nothing, and a read-only page offers no Remove', async () => {
    const { transport } = await draw();
    press('Remove…');
    await flush();
    press('Cancel');
    await flush();
    expect(transport.sentOf('knowledge.remove')).toHaveLength(0);
    document.body.innerHTML = '';
    const again = await draw(true);
    expect([...document.querySelectorAll('button')].some((b) => (b.getAttribute('aria-label') ?? clean(b.textContent)) === 'Remove…')).toBe(false);
    expect([...document.querySelectorAll('button')].some((b) => b.getAttribute('aria-label') === 'Edit')).toBe(false);
    // Back stays, so the person can leave a page they may only read.
    expect([...document.querySelectorAll('button')].some((b) => b.getAttribute('aria-label') === 'Back')).toBe(true);
    expect(again.transport.sentOf('knowledge.remove')).toHaveLength(0);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-KN-2
  it('Error path — a refused removal is drawn and the page stays open', async () => {
    const { target } = await draw(false, err('KNOW_PAGE_REMOVED', 'That page was already removed.', {}));
    press('Remove…');
    await flush();
    press('Remove');
    await flush();
    expect(clean(target.textContent)).toContain('That page was already removed.');
    expect(clean(target.textContent)).toContain('A short page.');
  });
});

const named = (label: string): HTMLButtonElement | undefined =>
  [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.getAttribute('aria-label') === label);

describe('A knowledge page\'s head and its details (amended January 2026)', () => {
  it('Happy path — Back, Edit and Remove… are icon buttons on one line with the title, each named and titled', async () => {
    const { target } = await draw();
    const head = target.querySelector('.page-head')!;
    expect(clean(head.querySelector('h3')?.textContent)).toBe('Manchester overview');
    for (const [label, icon] of [['Back', 'back'], ['Edit', 'edit'], ['Remove…', 'bin']] as const) {
      const b = named(label)!;
      expect(b, label).toBeDefined();
      expect(head.contains(b)).toBe(true);
      expect(b.title).toBe(label);
      expect(clean(b.textContent)).toBe('');
      expect(b.querySelector('use')?.getAttribute('href')).toBe(`#i-${icon}`);
    }
  });

  it('Happy path — in Edit the page\'s details are hidden until asked for, then shown and hidden again', async () => {
    await draw();
    named('Edit')!.click();
    await flush();
    const words = clean(document.body.textContent);
    expect(words).toContain('Title');
    for (const field of ['Kind of page', 'Summary in one sentence', 'Tags, separated by commas']) expect(words).not.toContain(field);
    const toggle = named("Show the page's details")!;
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(toggle.title).toBe("Show the page's details");
    toggle.click();
    await flush();
    const open = named("Hide the page's details")!;
    expect(open.getAttribute('aria-expanded')).toBe('true');
    expect(open.getAttribute('aria-controls')).toBe('knowledge-details');
    for (const field of ['Kind of page', 'Summary in one sentence', 'Other names for this page', 'Tags, separated by commas']) expect(clean(document.getElementById('knowledge-details')?.textContent)).toContain(field);
    open.click();
    await flush();
    expect(document.getElementById('knowledge-details')).toBeNull();
    // While editing, the page's Edit and Remove… are not offered.
    expect(named('Edit')).toBeUndefined();
    expect(named('Remove…')).toBeUndefined();
  });

  it('Happy path — in Edit, Done and Cancel are icon buttons at the top and the details button sits beside the title box', async () => {
    await draw();
    named('Edit')!.click();
    await flush();
    const form = document.querySelector('form')!;
    for (const [label, icon] of [['Done', 'check'], ['Cancel', 'remove']] as const) {
      const b = named(label)!;
      expect(b, label).toBeDefined();
      expect(b.title).toBe(label);
      expect(clean(b.textContent)).toBe('');
      expect(b.querySelector('use')?.getAttribute('href')).toBe(`#i-${icon}`);
      expect(form.querySelector('.edit-tools')?.contains(b)).toBe(true);
    }
    expect(named('Done')!.getAttribute('data-default')).toBe('true');
    expect(form.firstElementChild?.classList.contains('edit-tools')).toBe(true);
    const row = form.querySelector('.title-row')!;
    expect(row.querySelector('input[type="text"]')).not.toBeNull();
    expect(row.contains(named("Show the page's details")!)).toBe(true);
  });

  it('Edge — what was typed in a hidden detail is kept and Cancel asks first', async () => {
    await draw();
    named('Edit')!.click();
    await flush();
    named("Show the page's details")!.click();
    await flush();
    const tags = [...document.querySelectorAll<HTMLInputElement>('#knowledge-details input[type="text"]')].pop()!;
    tags.value = 'suppliers';
    tags.dispatchEvent(new Event('input', { bubbles: true }));
    await flush();
    named("Hide the page's details")!.click();
    await flush();
    named("Show the page's details")!.click();
    await flush();
    expect([...document.querySelectorAll<HTMLInputElement>('#knowledge-details input[type="text"]')].pop()!.value).toBe('suppliers');
    press('Cancel');
    await flush();
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  });
});
