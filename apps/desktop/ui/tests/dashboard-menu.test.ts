// @vitest-environment happy-dom
// A dashboard's menu (ScreensModule.md SM-DB-5 to SM-DB-8, amended January 2026; user decision; Approved by
// the user January 2026): in the side tree, a dashboard row's ⋯ and right-click menu holds Refresh, Export ▸
// and Remove…; the dashboard's screen holds the same Export items in its ⋯ menu. Pure rules first, then the
// components mounted over the fake backend so their sends run.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { startApp } from '../src/lib/app/bootstrap';
import { i18n } from '../src/lib/i18n/fluent';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import DashboardsTreeGroup from '../src/lib/shell/DashboardsTreeGroup.svelte';
import { announcement, canExport, EXPORT_FORMATS } from '../src/lib/views/fixed/dashboard-export';
import Dashboards from '../src/lib/views/fixed/Dashboards.svelte';
import { err, FakeTransport, ok, R1_GROUPS, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const D = uid('dash');
const RUN = uid('run');
const REP = uid('rep');
const AT = '2026-01-10T10:43:58.120Z';
const GROUPS = [...R1_GROUPS, 'reports', 'runs'];
const dashboard = { id: D, workspace_id: WS, name: 'Prices', made_by: { kind: 'conversation' }, inputs: [], refresh_runs: [RUN], kept_at: AT };

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

const words = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
const buttonNamed = (root: ParentNode, name: string): HTMLButtonElement | undefined =>
  Array.from(root.querySelectorAll('button')).find((b) => words(b.textContent) === name);
const menuItems = (root: ParentNode): string[] => Array.from(root.querySelectorAll('[role="menuitem"]')).map((b) => words(b.textContent));

async function press(el: Element | null | undefined): Promise<void> {
  (el as HTMLElement | null)?.click();
  await flush();
}

async function appOn(setup: (tr: FakeTransport) => void, groups = GROUPS): Promise<{ app: AppContext; transport: FakeTransport }> {
  const transport = new FakeTransport({ localGroups: groups });
  setup(transport);
  const data = new Map<string, string>();
  const storage = { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
  const app = await startApp(transport, { document: null, storage, i18n });
  return { app, transport };
}

async function tree(setup: (tr: FakeTransport) => void = () => undefined, groups = GROUPS) {
  const { app, transport } = await appOn((tr) => {
    tr.on('dashboards.list', ok({ items: [dashboard], next_cursor: null }));
    setup(tr);
  }, groups);
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(DashboardsTreeGroup, { target, props: { backend: app.local, workspaceId: WS, initiallyOpen: true }, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  drawn = { target, part };
  await flush();
  transport.sent.length = 0;
  return { app, transport, target };
}

const moreFor = (target: HTMLElement, name: string) =>
  Array.from(target.querySelectorAll('button[aria-haspopup="menu"]')).find((b) => words(b.getAttribute('aria-label')) === `More actions for ${name}`);

/** The announced line, once its microtask has run. */
async function announced(app: AppContext): Promise<string> {
  await flush();
  return words(app.shell.announcement);
}

describe('Export rules (SM-DB-5)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-DB-5
  it('Happy path — the four formats, in order', () => {
    expect(EXPORT_FORMATS.map((f) => f.format)).toEqual(['pdf', 'pptx', 'xlsx', 'docx']);
  });

  it('Edge case — export is offered for a dashboard on this computer only', () => {
    expect(canExport({ kind: 'local', groups: new Set(['dashboards']) })).toBe(true);
    expect(canExport({ kind: 'server', groups: new Set(['dashboards']) })).toBe(false);
    expect(canExport({ kind: 'local', groups: new Set(['reports']) })).toBe(false);
  });

  it('pure rule — what each outcome announces', () => {
    expect(announcement({ kind: 'saved' })).toBe('dashboard-exported');
    expect(announcement({ kind: 'added', asCopy: false })).toBe('dashboard-added-to-presentation');
    expect(announcement({ kind: 'added', asCopy: true })).toBe('dashboard-added-as-copy');
    expect(announcement({ kind: 'cancelled' })).toBeNull();
    expect(announcement({ kind: 'error', error: { code: 'X', message: '' } })).toBeNull();
  });
});

describe('A dashboard row’s menu in the side tree (SM-DB-7)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-DB-7
  it('Happy path — the ⋯ menu holds Refresh, Export and Remove…, and Export opens to its items', async () => {
    const { target } = await tree();
    await press(moreFor(target, 'Prices'));
    expect(menuItems(target)).toEqual(['Refresh', 'Export', 'Remove…']);
    await press(buttonNamed(target, 'Export'));
    expect(menuItems(target)).toEqual(['Refresh', 'Export', 'PDF…', 'PowerPoint…', 'Excel…', 'Word…', 'Add to an existing PowerPoint…', 'Remove…']);
  });

  it('Happy path — right-clicking the row opens the same menu', async () => {
    const { target } = await tree();
    const row = buttonNamed(target, 'Prices');
    row?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await flush();
    expect(target.querySelector('[role="menu"]')).not.toBeNull();
    expect(menuItems(target)).toContain('Remove…');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-DB-5
  it('Happy path — PowerPoint… saves the dashboard and says so; the menu closes', async () => {
    const { app, transport, target } = await tree((tr) => tr.on('dashboards.export', ok({ saved_path: '/Users/sam/Prices.pptx' })));
    await press(moreFor(target, 'Prices'));
    await press(buttonNamed(target, 'Export'));
    await press(buttonNamed(target, 'PowerPoint…'));
    expect(transport.sentOf('dashboards.export').map((e) => e.input)).toEqual([{ dashboard_id: D, format: 'pptx' }]);
    expect(target.querySelector('[role="menu"]')).toBeNull();
    expect(await announced(app)).toBe('Prices was saved.');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-DB-6
  it('Happy path — Add to an existing PowerPoint… adds the slides, or says a copy was saved', async () => {
    let asCopy = false;
    const { app, transport, target } = await tree((tr) => tr.on('dashboards.add_to_presentation', () => ok({ saved_path: '/Users/sam/Board.pptx', as_copy: asCopy })));
    const add = async () => {
      await press(moreFor(target, 'Prices'));
      await press(buttonNamed(target, 'Export'));
      await press(buttonNamed(target, 'Add to an existing PowerPoint…'));
    };
    await add();
    expect(transport.sentOf('dashboards.add_to_presentation').map((e) => e.input)).toEqual([{ dashboard_id: D }]);
    expect(await announced(app)).toBe('Prices was added to the end of the presentation.');
    asCopy = true;
    await add();
    expect(await announced(app)).toBe("The presentation was open or couldn't be changed, so a copy with Prices added was saved instead.");
  });

  it('Edge case — a cancelled save dialog shows nothing', async () => {
    const { app, target } = await tree((tr) => tr.on('dashboards.export', err('DSK_DIALOG_CANCELLED', 'Saving was cancelled.')));
    await press(moreFor(target, 'Prices'));
    await press(buttonNamed(target, 'Export'));
    await press(buttonNamed(target, 'Excel…'));
    expect(await announced(app)).toBe('');
    expect(words(target.textContent)).not.toContain('Saving was cancelled.');
  });

  it('Error path — a dashboard with no report yet says so under Dashboards', async () => {
    const { app, target } = await tree((tr) => tr.on('dashboards.export', err('DSK_DASHBOARD_NO_REPORT', 'This dashboard has no report to save yet. Refresh it, then try again.')));
    await press(moreFor(target, 'Prices'));
    await press(buttonNamed(target, 'Export'));
    await press(buttonNamed(target, 'PDF…'));
    expect(words(target.textContent)).toContain('This dashboard has no report to save yet.');
    expect(await announced(app)).toBe('');
  });

  it('Happy path — Refresh starts a refresh and says so', async () => {
    const { app, transport, target } = await tree((tr) => tr.on('dashboards.refresh', ok({ kind: 'run', run_id: uid('r2') })));
    await press(moreFor(target, 'Prices'));
    await press(buttonNamed(target, 'Refresh'));
    expect(transport.sentOf('dashboards.refresh').map((e) => e.input)).toEqual([{ workspace_id: WS, dashboard_id: D }]);
    expect(await announced(app)).toContain('Prices is being made again.');
  });

  it('Happy path — Remove… asks first; Cancel sends nothing, Remove removes the row', async () => {
    const { app, transport, target } = await tree((tr) => tr.on('dashboards.remove', ok({})));
    await press(moreFor(target, 'Prices'));
    await press(buttonNamed(target, 'Remove…'));
    const dialog = document.querySelector('[role="dialog"], [role="alertdialog"]');
    expect(words(dialog?.textContent)).toContain('Remove Prices?');
    await press(buttonNamed(document, 'Cancel'));
    expect(transport.sentOf('dashboards.remove')).toHaveLength(0);
    await press(moreFor(target, 'Prices'));
    await press(buttonNamed(target, 'Remove…'));
    await press(buttonNamed(document, 'Remove'));
    expect(transport.sentOf('dashboards.remove').map((e) => e.input)).toEqual([{ dashboard_id: D }]);
    expect(buttonNamed(target, 'Prices')).toBeUndefined();
    expect(await announced(app)).toBe('Prices was removed.');
  });

  it('Error path — a refused removal keeps the dialog open with its sentence', async () => {
    const { transport, target } = await tree((tr) => tr.on('dashboards.remove', err('JRNL_NOT_FOUND', 'That dashboard is already gone.')));
    await press(moreFor(target, 'Prices'));
    await press(buttonNamed(target, 'Remove…'));
    await press(buttonNamed(document, 'Remove'));
    expect(transport.sentOf('dashboards.remove')).toHaveLength(1);
    expect(words(document.body.textContent)).toContain('That dashboard is already gone.');
    expect(buttonNamed(target, 'Prices')).toBeDefined();
  });

  it('Edge case — a backend that answers no dashboard command draws no ⋯', async () => {
    const { target } = await tree(() => undefined, R1_GROUPS.filter((g) => g !== 'dashboards'));
    expect(moreFor(target, 'Prices')).toBeUndefined();
  });
});

describe('The dashboard screen’s ⋯ menu (SM-DB-8)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-DB-8
  it('Happy path — the ⋯ beside Refresh holds the same Export items', async () => {
    const parts = [{ heading: 'Prices', sources: [], data: { kind: 'table', columns: [{ name: 'Supplier', column_type: 'text' }], rows: [['Kestrel']] } }];
    const { app, transport } = await appOn((tr) =>
      tr
        .on('dashboards.list', ok({ items: [dashboard], next_cursor: null }))
        .on('reports.list', ok({ items: [{ id: REP, run_id: RUN, title: 'Prices', as_of: AT }], next_cursor: null }))
        .on('reports.get', ok({ id: REP, run_id: RUN, title: 'Prices', as_of: AT, workspace_id: WS, parts }))
        .on('dashboards.export', ok({ saved_path: '/Users/sam/Prices.docx' })),
    );
    const target = document.createElement('div');
    document.body.appendChild(target);
    const part = mount(Dashboards, { target, props: { backend: app.local, workspaceId: WS, instance: D }, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    drawn = { target, part };
    await flush();
    await press(moreFor(target, 'Prices'));
    await press(buttonNamed(target, 'Export'));
    expect(menuItems(target)).toEqual(['Export', 'PDF…', 'PowerPoint…', 'Excel…', 'Word…', 'Add to an existing PowerPoint…']);
    await press(buttonNamed(target, 'Word…'));
    expect(transport.sentOf('dashboards.export').map((e) => e.input)).toEqual([{ dashboard_id: D, format: 'docx' }]);
    expect(await announced(app)).toBe('Prices was saved.');
  });
});
