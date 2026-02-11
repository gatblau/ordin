// @vitest-environment happy-dom
// Actions in menus, content in panes (ScreensModule.md SM-RL-4, SM-ASK-1, SM-NS-4; amended January 2026,
// user decision): a document's and a folder's actions live in the tree row's ⋯ menu and its right-click
// menu, and send the same commands the document page used to send.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { APP_KEY } from '../src/lib/shell/context';
import TreeFolder from '../src/lib/shell/TreeFolder.svelte';
import { hello, LOCAL, ok, R1_GROUPS, settle } from './support/fake-backend';
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
const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const entry = (name: string, path: string, is_folder: boolean) =>
  ({ name, path, is_folder, kind: is_folder ? '' : 'pdf', place_id: PLACE, size_bytes: 1, changed_at: '2026-01-10T10:00:00Z', has_versions: false });
const DOC = entry('Quarterly plan.pdf', 'Q3/Quarterly plan.pdf', false);
const FOLDER = entry('Contracts', 'Contracts', true);

async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

async function draw(groups: string[] = [...R1_GROUPS, 'craft']) {
  const { app, transport } = await appWith((t) =>
    t
      .on('service.hello', ok(hello(LOCAL, groups)))
      .on('files.list_folder', ok({ items: [FOLDER, DOC], next_cursor: null }))
      .on('ordin.send_message', ok({ accepted: true, position_in_queue: 0 }))
      .on('craft.open', ok({ kind: 'no_editor' }))
      .on('craft.open_with_system_default', ok({ kind: 'opened_with_system_default' }))
      .on('files.move_to_bin', ok({ ticket: 't1' })),
  );
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(TreeFolder as never, { target, props: { backend: app.local, workspaceId: WS, placeId: PLACE } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await flush();
  transport.sent.length = 0;
  return { app, transport, target };
}

const rowOf = (target: HTMLElement, name: string): HTMLElement =>
  [...target.querySelectorAll<HTMLElement>('.tree-line')].find((l) => clean(l.querySelector('.tree-row')?.textContent) === name) as HTMLElement;
const items = (): string[] => [...document.querySelectorAll('[role=menuitem]')].map((b) => clean(b.textContent));
async function openMenu(target: HTMLElement, name: string): Promise<void> {
  (rowOf(target, name).querySelector('.tree-action') as HTMLButtonElement).click();
  await flush();
}
async function pick(label: string): Promise<void> {
  ([...document.querySelectorAll<HTMLButtonElement>('[role=menuitem]')].find((b) => clean(b.textContent) === label) as HTMLButtonElement).click();
  await flush();
}

describe("A document's ⋯ menu in the tree (SM-RL-4)", () => {
  // spec: ordin-desktop/components/ScreensModule#SM-RL-4
  it('Happy path — the menu holds Ask, Add to knowledge, Add to memory, Open in its app and Move to Bin…, in that order', async () => {
    const { target } = await draw();
    await openMenu(target, 'Quarterly plan.pdf');
    expect(items()).toEqual(['Ask Ordin about this', 'Add to knowledge', 'Add to memory', 'Open in its app', 'Move to Bin…']);
  });

  it('a right-click on the row opens the same menu and stops the browser menu', async () => {
    const { target } = await draw();
    const row = rowOf(target, 'Quarterly plan.pdf').querySelector('.tree-row') as HTMLElement;
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    row.dispatchEvent(event);
    await flush();
    expect(event.defaultPrevented).toBe(true);
    expect(items()).toContain('Open in its app');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-ASK-1
  it('Ask Ordin about this sends the message and the file as one place_file attachment, as the page did', async () => {
    const { target, transport } = await draw();
    await openMenu(target, 'Quarterly plan.pdf');
    await pick('Ask Ordin about this');
    const sent = transport.sentOf('ordin.send_message').map((s) => s.input) as { workspace_id: string; text: string; attachments: unknown[] }[];
    expect(sent).toHaveLength(1);
    expect(sent[0]?.workspace_id).toBe(WS);
    expect(clean(sent[0]?.text)).toContain('Quarterly plan.pdf');
    expect(sent[0]?.attachments).toEqual([{ kind: 'place_file', place_id: PLACE, relative_path: DOC.path }]);
  });

  it.each([
    ['Add to knowledge', 'Write a knowledge page from “Quarterly plan.pdf”.'],
    ['Add to memory', 'Add the facts in “Quarterly plan.pdf” to memory.'],
  ])('%s asks Ordin in the conversation with the file attached and brings the conversation into view', async (label, words) => {
    const { app, target, transport } = await draw();
    await openMenu(target, 'Quarterly plan.pdf');
    await pick(label);
    const sent = transport.sentOf('ordin.send_message').map((s) => s.input) as { text: string; attachments: unknown[] }[];
    expect(clean(sent[0]?.text)).toBe(words);
    expect(sent[0]?.attachments).toEqual([{ kind: 'place_file', place_id: PLACE, relative_path: DOC.path }]);
    expect(app.shell.openWorkspace).toBe(WS);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-NS-4
  it('Open in its app sends craft.open; no_editor asks under the row and Open in your usual app sends the system default', async () => {
    const { target, transport } = await draw();
    await openMenu(target, 'Quarterly plan.pdf');
    await pick('Open in its app');
    expect(transport.sentOf('craft.open')[0]?.input).toEqual({ place_id: PLACE, path: DOC.path });
    expect(clean(target.textContent)).toContain('No editor for this kind of file');
    ([...target.querySelectorAll('button')].find((b) => clean(b.textContent) === 'Open in your usual app') as HTMLButtonElement).click();
    await flush();
    expect(transport.sentOf('craft.open_with_system_default')[0]?.input).toEqual({ place_id: PLACE, path: DOC.path });
  });

  it('Move to Bin… asks first, then sends files.move_to_bin', async () => {
    const { target, transport } = await draw();
    await openMenu(target, 'Quarterly plan.pdf');
    await pick('Move to Bin…');
    expect(transport.sentOf('files.move_to_bin')).toHaveLength(0);
    ([...document.querySelectorAll('button')].find((b) => clean(b.textContent) === 'Move to Bin') as HTMLButtonElement).click();
    await flush();
    expect(transport.sentOf('files.move_to_bin')[0]?.input).toEqual({ place_id: PLACE, path: DOC.path });
  });

  it('without craft, Open in its app is not offered', async () => {
    const { target } = await draw([...R1_GROUPS]);
    await openMenu(target, 'Quarterly plan.pdf');
    expect(items()).not.toContain('Open in its app');
  });
});

describe("A folder's ⋯ menu in the tree (SM-RL-4)", () => {
  it('holds Add to knowledge only, which asks Ordin with the folder attached', async () => {
    const { target, transport } = await draw();
    await openMenu(target, 'Contracts');
    expect(items()).toEqual(['Add to knowledge']);
    await pick('Add to knowledge');
    const sent = transport.sentOf('ordin.send_message').map((s) => s.input) as { text: string; attachments: unknown[] }[];
    expect(clean(sent[0]?.text)).toBe('Write a knowledge page from “Contracts”.');
    expect(sent[0]?.attachments).toEqual([{ kind: 'place_file', place_id: PLACE, relative_path: 'Contracts' }]);
    // The press is the menu's own: the folder does not also open.
    expect(transport.sentOf('files.list_folder')).toHaveLength(0);
  });
});

describe("A file's tree icon (SM-IC-5)", () => {
  // spec: ordin-desktop/components/ScreensModule#SM-IC-5
  it("Happy path — a PDF's row shows the PDF icon in its tint, as its tab does", async () => {
    const { target } = await draw();
    const tile = rowOf(target, 'Quarterly plan.pdf').querySelector('.tree-row .icon-tile') as HTMLElement;
    expect(tile.querySelector('use')?.getAttribute('href')).toBe('#i-pdf');
    expect(tile.dataset.tint).toBe('pdf');
    const folder = rowOf(target, 'Contracts').querySelector('.tree-row .icon-tile use');
    expect(folder?.getAttribute('href')).toBe('#i-folder');
  });
});
