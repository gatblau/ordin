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

async function draw(listing: (input: { folder: string }) => unknown, initiallyOpen = true) {
  const { app, transport } = await appWith((t) => t.on('workspaces.links', ok([LINK])).on('files.list_folder', listing as never));
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
