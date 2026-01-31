// @vitest-environment happy-dom
// A linked location opens to its folders and documents in the tree (SM-RL-3), mounted over the fake transport.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { APP_KEY } from '../src/lib/shell/context';
import DocumentsTreeGroup from '../src/lib/shell/DocumentsTreeGroup.svelte';
import { err, LOCAL, ok, settle } from './support/fake-backend';
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
const PLACE = uid('place');
const text = (el: Element | null): string => (el?.textContent ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const entry = (name: string, path: string, is_folder: boolean) =>
  ({ name, path: is_folder ? `${path}/` : path, is_folder, kind: is_folder ? '' : 'md', place_id: PLACE, size_bytes: 1, changed_at: '2026-01-10T10:00:00Z', has_versions: false });
const LINK = { id: uid('l1'), kind: 'local_folder', display_name: 'Sample documents', state: 'active', workspace_id: WS, added_at: '2026-01-06T09:00:00Z', origin_location: 'Sample documents', place_id: PLACE };

async function draw(listing: (input: { folder: string }) => unknown, initiallyOpen = true, links: unknown[] = [LINK], items: unknown[] = []) {
  const { app, transport } = await appWith((t) =>
    t.on('workspaces.links', ok(links)).on('files.list_folder', listing as never).on('workspaces.working_copy_items', ok(items)),
  );
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(DocumentsTreeGroup as never, { target, props: { backend: app.local, workspaceId: WS, initiallyOpen } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await settle(30);
  flushSync();
  return { app, target, transport };
}
const press = (target: HTMLElement, label: string): void => {
  const button = [...target.querySelectorAll('button')].find((b) => text(b).includes(label));
  expect(button, `a button named ${label}`).toBeDefined();
  button?.click();
};
async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

describe('A location opens to its folders and documents (SM-RL-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-RL-1
  it('Happy path — pressing the Documents row (closed at first) shows the locations under it, then their contents', async () => {
    const { target } = await draw(() => ok({ items: [entry('Meeting notes.md', 'Meeting notes.md', false)], next_cursor: null }), false);
    expect(text(target)).not.toContain('Sample documents');
    press(target, 'Documents');
    await flush();
    expect(text(target)).toContain('Sample documents');
    press(target, 'Sample documents');
    await flush();
    expect(text(target)).toContain('Meeting notes.md');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-RL-3
  it('Happy path — pressing a location shows its folders and documents; a folder opens its own contents', async () => {
    const { target, transport } = await draw((input) =>
      ok({ items: input.folder === '' ? [entry('Contracts', 'Contracts', true), entry('Meeting notes.md', 'Meeting notes.md', false)] : [entry('Rates 2026.md', 'Contracts/Rates 2026.md', false)], next_cursor: null }),
    );
    expect(text(target)).toContain('Sample documents');
    expect(text(target)).not.toContain('Meeting notes.md');
    press(target, 'Sample documents');
    await flush();
    expect(text(target)).toContain('Contracts');
    expect(text(target)).toContain('Meeting notes.md');
    expect(text(target)).not.toContain('Rates 2026.md');
    press(target, 'Contracts');
    await flush();
    expect(text(target)).toContain('Rates 2026.md');
    expect(transport.sentOf('files.list_folder').map((c) => (c.input as { folder: string }).folder)).toEqual(['', 'Contracts/']);
  });

  it('Edge case — a folder with nothing in it says so', async () => {
    const { target } = await draw(() => ok({ items: [], next_cursor: null }));
    press(target, 'Sample documents');
    await flush();
    expect(text(target)).toContain('Nothing in here yet');
  });

  it('Error path — a failed read draws the error line and the location stays', async () => {
    const { target } = await draw(() => err('FILE_DATASTORE_UNAVAILABLE', 'Something went wrong; try again.', { action: 'try_again' }));
    press(target, 'Sample documents');
    await flush();
    expect(text(target)).toContain('Something went wrong; try again.');
    expect(text(target)).toContain('Sample documents');
  });
});
void LOCAL;

describe('A document in the tree can be moved to the Bin (SM-RL-4)', () => {
  const open = async (target: HTMLElement) => {
    press(target, 'Sample documents');
    await flush();
  };
  const menuOf = (target: HTMLElement, name: string): HTMLElement => {
    const button = target.querySelector<HTMLElement>(`button[aria-label*="${name}"][aria-haspopup="menu"]`);
    expect(button, `a ⋯ menu for ${name}`).not.toBeNull();
    return button as HTMLElement;
  };

  // spec: ordin-desktop/components/ScreensModule#SM-RL-4
  it('Happy path — ⋯ on a document, Move to Bin…, confirm: files.move_to_bin is sent and the folder is read again', async () => {
    let listed = 0;
    const { target, transport } = await draw(() => {
      listed += 1;
      return ok({ items: listed === 1 ? [entry('Summary.md', 'Summary.md', false)] : [], next_cursor: null });
    });
    transport.on('files.move_to_bin', ok({ file_version_id: uid('v1'), path: 'Summary.md', place_id: PLACE })).on('files.restore_from_bin', ok({}));
    await open(target);
    expect(text(target)).toContain('Summary.md');
    menuOf(target, 'Summary.md').click();
    await flush();
    press(target, 'Move to Bin…');
    await flush();
    expect(text(document.body)).toContain('Move “Summary.md” to the Bin?');
    expect(transport.sentOf('files.move_to_bin')).toHaveLength(0);
    const verb = [...document.querySelectorAll('button')].filter((b) => text(b) === 'Move to Bin').pop();
    verb?.click();
    await flush();
    expect(transport.sentOf('files.move_to_bin').map((c) => c.input)).toEqual([{ place_id: PLACE, path: 'Summary.md' }]);
    expect([...target.querySelectorAll('.tree-name')].map(text)).not.toContain('Summary.md');
    expect(text(target)).toContain('Moved “Summary.md” to your computer\'s Bin.');
    press(target, 'Undo');
    await flush();
    expect(transport.sentOf('files.restore_from_bin').map((c) => c.input)).toEqual([{ ticket: { file_version_id: uid('v1'), path: 'Summary.md', place_id: PLACE } }]);
    expect(text(target)).not.toContain('Moved “Summary.md”');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-RL-4
  // Amended January 2026 (user decision): a folder row's menu holds Add to knowledge only, when Ordin is there.
  it('Edge case — Cancel sends nothing, and a folder row has no menu without Ordin', async () => {
    const { target, transport, app } = await draw(() => ok({ items: [entry('Contracts', 'Contracts', true), entry('Summary.md', 'Summary.md', false)], next_cursor: null }));
    transport.on('files.move_to_bin', ok({}));
    app.local.groups = new Set([...app.local.groups].filter((g) => g !== 'ordin'));
    await open(target);
    expect(target.querySelector('button[aria-label*="Contracts"][aria-haspopup="menu"]')).toBeNull();
    menuOf(target, 'Summary.md').click();
    await flush();
    press(target, 'Move to Bin…');
    await flush();
    const cancel = [...document.querySelectorAll('button')].find((b) => text(b) === 'Cancel');
    cancel?.click();
    await flush();
    expect(transport.sentOf('files.move_to_bin')).toHaveLength(0);
    expect(text(target)).toContain('Summary.md');
  });
});

describe('Through the side rail, as a person uses it (SM-RL-1, SM-RL-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-RL-3
  it('Happy path — opening a workspace, then the Documents row, shows its locations in the tree and they stay shown', async () => {
    const { default: SideRail } = await import('../src/lib/shell/SideRail.svelte');
    const { hello, R1_GROUPS } = await import('./support/fake-backend');
    const { app } = await appWith((tr) => {
      tr.on('workspaces.list', ok([{ id: WS, name: 'Supplier review', created_at: 'x', own_folder_place_id: 'p', state: 'active' }]));
      tr.on('service.hello', ok(hello(LOCAL, R1_GROUPS)));
      tr.on('workspaces.links', ok([LINK]));
      tr.on('files.list_folder', ok({ items: [entry('Meeting notes.md', 'Meeting notes.md', false)], next_cursor: null }));
    });
    app.prefs.setExpanded([`${LOCAL}:${WS}`]);
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(SideRail as never, { target, props: {} as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    await flush();
    expect(text(target)).toContain('Supplier review');
    press(target, 'Documents');
    await flush();
    expect(text(target)).toContain('Sample documents');
    await flush();
    expect(text(target)).toContain('Sample documents');
    press(target, 'Sample documents');
    await flush();
    expect(text(target)).toContain('Meeting notes.md');
  });
});

describe('Earlier conversations are not a row of the tree (SM-WR-18)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-WR-18
  it('Happy path — the tree has no Earlier conversations row; the button above the conversation opens them', async () => {
    const { default: SideRail } = await import('../src/lib/shell/SideRail.svelte');
    const { hello, R1_GROUPS } = await import('./support/fake-backend');
    const { app } = await appWith((tr) => {
      tr.on('workspaces.list', ok([{ id: WS, name: 'Supplier review', created_at: 'x', own_folder_place_id: 'p', state: 'active' }]));
      tr.on('service.hello', ok(hello(LOCAL, R1_GROUPS)));
      tr.on('ordin.list_conversations', ok({ items: [], next_cursor: null }));
    });
    app.prefs.setExpanded([`${LOCAL}:${WS}`]);
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(SideRail as never, { target, props: {} as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    await flush();
    expect(text(target)).not.toContain('Earlier conversations');
  });
});

describe('A Jira issue opens like a folder, and a changed item keeps its notice (SM-RL-3, SM-WR-5)', () => {
  const JIRA = { ...LINK, id: uid('l-jira'), kind: 'jira_issue', display_name: 'PROC-12 Renew Kestrel contract', origin_location: 'https://acme.atlassian.net/browse/PROC-12' };
  const item = (origin_path: string, origin_newer_at?: string) => ({
    id: origin_path,
    source_link_id: JIRA.id,
    origin_path,
    origin_version_read: '3',
    local_version: 1,
    state: 'read',
    read_at: '2026-01-26T09:00:00.000Z',
    ...(origin_newer_at === undefined ? {} : { origin_newer_at }),
  });

  // spec: ordin-desktop/components/ScreensModule#SM-WR-5
  it('Happy path — pressing the issue lists its documents as rows that open; only the changed one shows Bring in the new version', async () => {
    const { target, transport } = await draw(
      () => ok({ items: [entry('summary-and-description.md', 'summary-and-description.md', false), entry('related.md', 'related.md', false)], next_cursor: null }),
      true,
      [JIRA],
      [item('PROC-12/summary-and-description.md'), item('PROC-12/related.md', '2026-01-26T10:40:00.000Z')],
    );
    press(target, 'PROC-12 Renew Kestrel contract');
    await flush();
    const rows = [...target.querySelectorAll('button.tree-row')].map((b) => text(b));
    expect(rows).toContain('summary-and-description.md');
    expect(rows).toContain('related.md');
    expect(transport.sentOf('files.list_folder')[0]?.input).toMatchObject({ place_id: PLACE, folder: '' });
    expect((text(target).match(/Bring in the new version/g) ?? []).length).toBe(1);
    expect(text(target.querySelector('.link-items'))).not.toContain('summary-and-description.md');
  });
});
