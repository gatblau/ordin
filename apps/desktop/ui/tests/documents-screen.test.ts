// @vitest-environment happy-dom
// The Documents screen in the real window (App mounted over startApp, so the screen has the app context, as
// in the program): with no place it lists where the workspace's documents live (SM-DS-1, SM-DS-2); with a
// place, a folder row opens the folder and a file row, a PDF among them, opens its document page (SM-DS-3).
// In test.1 the screen with no place sent `files.list_folder` with an empty place, and a PDF row did nothing.
import { mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';

import App from '../src/App.svelte';
import { startApp } from '../src/lib/app/bootstrap';
import type { AppContext } from '../src/lib/shell/context';
import '../src/lib/shell/load-registrations';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { makeKey, SCREEN_IDS } from '../src/lib/shell/screens';
import { FakeTransport, LOCAL, ok, settle } from './support/fake-backend';
import { uid } from './support/ids';

const WS = uid('ws');
const P1 = uid('p1');
const P2 = uid('p2');
const AT = '2026-01-10T10:43:58.120Z';

const link = (id: string, kind: string, name: string, place: string | null, state = 'active') => ({
  added_at: AT,
  display_name: name,
  id: uid(id),
  kind,
  origin_location: `/Users/sam/${name}`,
  place_id: place,
  state,
  workspace_id: WS,
});
const entry = (path: string, folder: boolean) => ({
  changed_at: AT,
  has_versions: false,
  is_folder: folder,
  kind: folder ? '' : (path.split('.').pop() ?? ''),
  name: path.replace(/\/$/, '').split('/').pop() ?? path,
  path,
  place_id: P1,
  size_bytes: 10,
});

let mounted: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (mounted !== null) unmount(mounted);
  mounted = null;
  document.body.innerHTML = '';
});

async function open(transport: FakeTransport, key: string): Promise<{ app: AppContext; target: HTMLElement }> {
  const app = await startApp(transport, { storage: null });
  const target = document.createElement('div');
  document.body.appendChild(target);
  mounted = mount(App, { target, props: { app } });
  await settle(20);
  app.shell.openWorkspaceTab(LOCAL, WS);
  app.screens.open(ScreenAreaState.scope(LOCAL, WS), key);
  await settle(30);
  return { app, target };
}

const panel = (target: HTMLElement): HTMLElement => target.querySelector('#screen-panel') as HTMLElement;
const button = (target: HTMLElement, text: string): HTMLButtonElement | undefined =>
  Array.from(panel(target).querySelectorAll('button')).find((b) => b.textContent?.trim() === text);

describe('Documents screen', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-DS-1
  it('Happy path — Documents from the tree lists where documents live (SM-DS-1)', async () => {
    const transport = new FakeTransport();
    transport.on('workspaces.links', ok([
      link('l1', 'local_folder', 'Quotes', P1),
      link('l2', 'confluence_location', 'Rates', P2),
      link('l3', 'sharepoint_location', 'Not copied yet', null),
      link('l4', 'local_folder', 'Proposed', uid('p4'), 'proposed'),
    ]));
    const { app, target } = await open(transport, SCREEN_IDS.documents);
    expect(transport.commandsSent()).toContain('workspaces.links');
    expect(transport.commandsSent()).not.toContain('files.list_folder');
    const rows = Array.from(panel(target).querySelectorAll('li button')).map((b) => b.textContent?.trim());
    expect(rows).toEqual(['Rates', 'Quotes']);
    button(target, 'Quotes')?.click();
    await settle(10);
    expect(app.screens.front(ScreenAreaState.scope(LOCAL, WS))).toBe(makeKey(SCREEN_IDS.documents, P1));
  });

  // spec: ordin-desktop/components/ScreensModule#SM-DS-2
  it('Edge case — no documents linked yet (SM-DS-2)', async () => {
    const transport = new FakeTransport();
    transport.on('workspaces.links', ok([]));
    const { target } = await open(transport, SCREEN_IDS.documents);
    expect(panel(target).textContent).toContain('No documents are linked to this workspace yet. Use Add to this workspace to link a folder, a site or a space.');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-DS-3
  it('a folder row opens the folder, and a PDF row opens its document page (SM-DS-3, SM-NS-4)', async () => {
    const transport = new FakeTransport();
    transport.on('files.list_folder', (input) => {
      const folder = (input as { folder: string }).folder;
      return ok({ items: folder === '' ? [entry('Q3/', true)] : [entry('Q3/Quarterly plan.pdf', false)], next_cursor: null });
    });
    const { app, target } = await open(transport, makeKey(SCREEN_IDS.documents, P1));
    expect(transport.sentOf('files.list_folder').map((e) => e.input)).toEqual([{ place_id: P1, folder: '', sort: 'name_asc' }]);

    button(target, 'Q3')?.click();
    await settle(20);
    expect(transport.sentOf('files.list_folder').map((e) => (e.input as { folder: string }).folder)).toEqual(['', 'Q3/']);

    button(target, 'Quarterly plan.pdf')?.click();
    await settle(10);
    expect(app.screens.front(ScreenAreaState.scope(LOCAL, WS))).toBe(makeKey(SCREEN_IDS.document, `${P1}:Q3/Quarterly plan.pdf`));
  });
});
