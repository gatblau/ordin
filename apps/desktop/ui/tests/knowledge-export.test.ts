// @vitest-environment happy-dom
// A knowledge page's ⋯ menu holds Export as PDF… and Export as Word… (ScreensModule.md SM-KN-3, amended January
// 2026; user decision; Approved by the user January 2026). On this computer only: the screen sends the page's id
// and the format, the program reads the page and opens the save dialog; a cancelled dialog shows nothing. The
// Knowledge screen is mounted over the fake backend so its sends run.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { startApp } from '../src/lib/app/bootstrap';
import { i18n } from '../src/lib/i18n/fluent';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import Knowledge from '../src/lib/views/fixed/Knowledge.svelte';
import { err, FakeTransport, ok, R1_GROUPS, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';

beforeAll(loadWindowCatalogue);

const PAGE = uid('page-summary');
const TITLE = 'Manchester overview';
const words = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const view = (readOnly = false) => ({
  page: { id: PAGE, title: TITLE, body_markdown: '# Overview\n\nA short page.', last_changed_at: '2026-01-12T10:00:00Z' },
  relative_path: 'knowledge/Manchester overview.md',
  aliases: [], tags: [], links_in: [], links_out: [], links_truncated: 0, suggestions: [], unregistered_aliases: [], changes: [],
  cites_other_computer: false, read_only: readOnly, past_stale_after: false, pending_update: false,
});
const listing = { this_workspace: [{ id: PAGE, title: TITLE, tags: [], last_changed_at: '2026-01-12T10:00:00Z' }], from_other_teams: [], from_agents: [], not_adopted: [] };

let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn.part);
  drawn?.target.remove();
  drawn = null;
  document.body.innerHTML = '';
});

async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

async function press(el: Element | null | undefined): Promise<void> {
  expect(el, 'the control').toBeDefined();
  (el as HTMLElement).click();
  await flush();
}

const more = (): HTMLButtonElement | undefined =>
  [...document.querySelectorAll<HTMLButtonElement>('button[aria-haspopup="menu"]')].find((b) => words(b.getAttribute('aria-label')) === `More actions for ${TITLE}`);
const menuItems = (): string[] => [...document.querySelectorAll('[role="menuitem"]')].map((b) => words(b.textContent));
const item = (name: string): HTMLButtonElement | undefined =>
  [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find((b) => words(b.textContent) === name);

/** The screen with the page open, inside an app so its announcements can be read. */
async function draw(exportReply: unknown = ok({ saved_path: '/Users/sam/Manchester overview.pdf' }), readOnly = false): Promise<{ app: AppContext; transport: FakeTransport; target: HTMLElement }> {
  const transport = new FakeTransport({ localGroups: R1_GROUPS });
  transport.on('knowledge.list', ok(listing)).on('knowledge.get', ok(view(readOnly))).on('knowledge.export', exportReply as never);
  const data = new Map<string, string>();
  const storage = { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
  const app = await startApp(transport, { document: null, storage, i18n });
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(Knowledge as never, { target, props: { backend: app.local, workspaceId: WS, instance: PAGE } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  drawn = { target, part };
  await flush();
  transport.sent.length = 0;
  return { app, transport, target };
}

async function announced(app: AppContext): Promise<string> {
  await flush();
  return words(app.shell.announcement);
}

describe('SM-KN-3 a knowledge page saved as a PDF or Word document', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-KN-3
  it('Happy path — the head holds a ⋯ beside Back, Edit and Remove…, opening to the two Export items', async () => {
    const { target } = await draw();
    const head = target.querySelector('.page-head')!;
    const button = more()!;
    expect(head.contains(button)).toBe(true);
    expect(button.title).toBe('More actions');
    expect(button.getAttribute('aria-expanded')).toBe('false');
    expect(words(button.textContent)).toBe('');
    expect(button.querySelector('use')?.getAttribute('href')).toBe('#i-more');
    expect(menuItems()).toEqual([]);
    await press(button);
    expect(button.getAttribute('aria-expanded')).toBe('true');
    expect(words(document.querySelector('[role="menu"]')?.getAttribute('aria-label'))).toBe(`Actions for ${TITLE}`);
    expect(menuItems()).toEqual(['Export as PDF…', 'Export as Word…']);
    expect(document.activeElement).toBe(item('Export as PDF…'));
  });

  it('Happy path — Export as PDF… sends the page id and pdf, closes the menu and says it was saved', async () => {
    const { app, transport } = await draw();
    await press(more());
    await press(item('Export as PDF…'));
    expect(transport.sentOf('knowledge.export').map((e) => e.input)).toEqual([{ page_id: PAGE, format: 'pdf' }]);
    expect(menuItems()).toEqual([]);
    expect(await announced(app)).toBe(`“${TITLE}” was saved.`);
  });

  it('Happy path — Export as Word… sends docx; no content or path is sent', async () => {
    const { transport } = await draw(ok({ saved_path: '/Users/sam/Manchester overview.docx' }));
    await press(more());
    await press(item('Export as Word…'));
    const sent = transport.sentOf('knowledge.export').map((e) => e.input);
    expect(sent).toEqual([{ page_id: PAGE, format: 'docx' }]);
  });

  it('Edge case — a cancelled save dialog shows nothing', async () => {
    const { app, target } = await draw(err('DSK_DIALOG_CANCELLED', 'Saving was cancelled.'));
    await press(more());
    await press(item('Export as PDF…'));
    expect(words(target.textContent)).not.toContain('Saving was cancelled.');
    expect(await announced(app)).toBe('');
  });

  it('Error path — a file that could not be written is drawn under the head, with Try again', async () => {
    const { app, target } = await draw(err('DSK_SAVE_COPY_FAILED', "ordin couldn't save the file where you chose.", { action: 'try_again' }));
    await press(more());
    await press(item('Export as Word…'));
    expect(words(target.textContent)).toContain("ordin couldn't save the file where you chose.");
    expect(await announced(app)).toBe('');
  });

  it('Edge case — Escape closes the menu and sends nothing', async () => {
    const { transport } = await draw();
    await press(more());
    document.querySelector('[role="menu"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await flush();
    expect(menuItems()).toEqual([]);
    expect(transport.sentOf('knowledge.export')).toHaveLength(0);
  });

  it('Edge case — a page that may only be read can still be exported', async () => {
    await draw(undefined, true);
    expect([...document.querySelectorAll('button')].some((b) => b.getAttribute('aria-label') === 'Edit')).toBe(false);
    await press(more());
    expect(menuItems()).toEqual(['Export as PDF…', 'Export as Word…']);
  });

  it('Edge case — a server-side workspace offers no Export', async () => {
    const b = await backendWith({ kind: 'server', setup: (t) => t.on('knowledge.list', ok(listing)).on('knowledge.get', ok(view())) });
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = { target, part: mount(Knowledge as never, { target, props: { backend: b.state, workspaceId: WS, instance: PAGE } as never }) };
    await flush();
    expect(target.querySelector('.page-head')).not.toBeNull();
    expect(more()).toBeUndefined();
  });
});
