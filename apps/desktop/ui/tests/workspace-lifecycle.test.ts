// @vitest-environment happy-dom
// Renaming, archiving and the Bin for workspaces (ScreensModule.md, amendment January 2026, SM-WS-2 to
// SM-WS-8): one test per Gherkin scenario, plus the confirmation's default button and the refusals.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import { getScreen } from '../src/lib/shell/screens';
import { ShellState } from '../src/lib/shell/shell-state.svelte';
import SideRail from '../src/lib/shell/SideRail.svelte';
import type { Workspace } from '../src/lib/service-api/types.gen';
import ArchivedScreen from '../src/lib/views/workspaces/ArchivedScreen.svelte';
import BinScreen from '../src/lib/views/workspaces/BinScreen.svelte';
import LifecycleFoot from '../src/lib/views/workspaces/LifecycleFoot.svelte';
import RenameWorkspace from '../src/lib/views/workspaces/RenameWorkspace.svelte';
import { ARCHIVED_SCREEN, BIN_SCREEN, lifecycleOf, WorkspaceLifecycle } from '../src/lib/views/workspaces/lifecycle.svelte';
import '../src/lib/views/workspaces/register';
import { backendWith, WS } from './fixed-support';
import { err, hello, LOCAL, ok, R1_GROUPS, settle, type FakeTransport } from './support/fake-backend';
import { uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const OLD = uid('ws-old-pilot');
const ws = (id: string, name: string, state: Workspace['state'], over: Partial<Workspace> = {}): Workspace => ({
  id,
  name,
  state,
  created_at: '2026-01-05T09:00:00.000Z',
  own_folder_place_id: uid(`own-${id}`),
  ...over,
});
const SUPPLIER = ws(WS, 'Supplier review', 'active');
const clean = (s: string): string => s.replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ');
const text = (el: Element): string => clean(el.textContent ?? '');
const button = (root: ParentNode, words: string): HTMLButtonElement => {
  const found = [...root.querySelectorAll('button')].find((b) => text(b).trim() === words);
  if (found === undefined) throw new Error(`no button "${words}" in ${text(document.body)}`);
  return found as HTMLButtonElement;
};

/** The element whose aria-label reads `label` once Fluent's isolation marks are dropped. */
const labelled = (root: ParentNode, label: string): HTMLElement | null =>
  ([...root.querySelectorAll('[aria-label]')].find((el) => clean(el.getAttribute('aria-label') ?? '') === label) as HTMLElement | undefined) ?? null;

/** `workspaces.list` answered from three lists the test can change. */
function lists(t: FakeTransport, initial: { active?: Workspace[]; archived?: Workspace[]; in_bin?: Workspace[] }) {
  const by = { active: initial.active ?? [], archived: initial.archived ?? [], in_bin: initial.in_bin ?? [] };
  t.on('workspaces.list', (input) => ok(by[(input as { filter: keyof typeof by }).filter] ?? []));
  return by;
}

async function model(setup: (t: FakeTransport) => void) {
  const b = await backendWith({ setup });
  const shell = new ShellState();
  shell.openWorkspaceTab(LOCAL, WS);
  return { ...b, shell, model: lifecycleOf(b.state, shell) };
}

/** Mounts a component with an app context holding `shell`, as the window does. */
async function show(component: unknown, props: Record<string, unknown>, shell: ShellState) {
  const target = document.createElement('div');
  document.body.appendChild(target);
  const app = { shell } as unknown as AppContext;
  drawn = mount(component as never, { target, props: props as never, context: new Map([[APP_KEY, app]]) });
  await tick();
  return target;
}
async function tick(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

describe('The workspace’s actions and the rail’s foot (SM-WS-2, SM-WS-5, SM-WS-7)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-WS-2
  it('the ⋯ menu offers Rename…, Archive and Remove workspace…; Rename… opens its field; the foot draws the Bin', async () => {
    const { app } = await appWith((tr) => {
      tr.on('workspaces.list', ok([SUPPLIER]));
      tr.on('service.hello', ok(hello(LOCAL, R1_GROUPS)));
    });
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(SideRail as never, { target, context: new Map([[APP_KEY, app]]) });
    await tick();
    labelled(target, 'More actions for Supplier review')!.click();
    flushSync();
    const items = [...target.querySelectorAll('[role="menuitem"]')].map((el) => text(el).trim());
    expect(items).toEqual(['Add to this workspace…', 'Rename…', 'Archive', 'Remove workspace…']);
    button(target, 'Rename…').click();
    await tick();
    expect((target.querySelector('.rename-workspace input') as HTMLInputElement).value).toBe('Supplier review');
    expect(target.querySelector('[role="menu"]')).toBeNull();
    expect(text(target)).toContain('Bin');
    expect(getScreen(BIN_SCREEN)?.titleId).toBe('bin-title');
    expect(getScreen(ARCHIVED_SCREEN)?.commands).toEqual(['workspaces.list', 'workspaces.restore']);
  });

  it('the foot shows Archived and the Bin with their counts, read at once', async () => {
    const b = await backendWith({ setup: (t) => lists(t, { archived: [ws(uid('a'), 'Last year', 'archived')], in_bin: [ws(OLD, 'Old pilot', 'in_bin')] }) });
    const target = await show(LifecycleFoot, { backend: b.state }, new ShellState());
    expect(labelled(target, 'Archived, 1')).not.toBeNull();
    expect(labelled(target, 'Bin, 1')).not.toBeNull();
  });
});

describe('Rename… (SM-WS-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-WS-3
  it('Happy path — renaming a workspace sends the trimmed name and reads the tree again', async () => {
    const { model: m, transport, state, shell } = await model((t) => {
      const by = lists(t, { active: [SUPPLIER] });
      t.on('workspaces.rename', (input) => {
        const name = (input as { name: string }).name;
        by.active = [{ ...SUPPLIER, name }];
        return ok({ ...SUPPLIER, name });
      });
    });
    m.startRename(WS);
    const target = await show(RenameWorkspace, { model: m, workspace: SUPPLIER }, shell);
    const field = target.querySelector('input') as HTMLInputElement;
    expect(field.value).toBe('Supplier review');
    expect(button(target, 'Rename').disabled).toBe(true);
    field.value = '  Q3 supplier review ';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    button(target, 'Rename').click();
    await tick();
    expect(transport.sentOf('workspaces.rename').map((e) => e.input)).toEqual([{ workspace_id: WS, name: 'Q3 supplier review' }]);
    expect(state.workspaces.map((w) => w.name)).toEqual(['Q3 supplier review']);
    expect(m.renaming).toBeNull();
  });

  it('Error path — the name is taken: the error line under the field, and the field keeps what was typed', async () => {
    const { model: m, shell } = await model((t) => t.on('workspaces.rename', err('WS_NAME_DUPLICATE', 'Another workspace already has that name.')));
    m.startRename(WS);
    const target = await show(RenameWorkspace, { model: m, workspace: SUPPLIER }, shell);
    const field = target.querySelector('input') as HTMLInputElement;
    field.value = 'Onboarding';
    field.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    button(target, 'Rename').click();
    await tick();
    expect(text(target)).toContain('Another workspace already has that name.');
    expect(field.value).toBe('Onboarding');
    expect(m.renaming).toBe(WS);
  });

  it('an empty or unchanged name cannot be sent', () => {
    expect(WorkspaceLifecycle.canRename('Supplier review', '   ')).toBe(false);
    expect(WorkspaceLifecycle.canRename('Supplier review', ' Supplier review ')).toBe(false);
    expect(WorkspaceLifecycle.canRename('Supplier review', 'Q3')).toBe(true);
  });
});

describe('Archive and Restore (SM-WS-4, SM-WS-5)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-WS-4
  // spec: ordin-desktop/components/ScreensModule#SM-WS-5
  it('Happy path — archiving and restoring: it leaves the tree and its tab, then comes back', async () => {
    const { model: m, transport, state, shell } = await model((t) => {
      const by = lists(t, { active: [SUPPLIER] });
      t.on('workspaces.archive', () => {
        by.active = [];
        by.archived = [{ ...SUPPLIER, state: 'archived' }];
        return ok(by.archived[0]);
      });
      t.on('workspaces.restore', () => {
        by.archived = [];
        by.active = [SUPPLIER];
        return ok(SUPPLIER);
      });
    });
    expect(await m.archive(SUPPLIER)).toBe(true);
    expect(transport.sentOf('workspaces.archive').map((e) => e.input)).toEqual([{ workspace_id: WS }]);
    expect(state.workspaces).toEqual([]);
    expect(m.archived.map((w) => w.name)).toEqual(['Supplier review']);
    expect(shell.openWorkspace).toBeNull();
    expect(shell.conversationTabs).toEqual([]);

    const target = await show(ArchivedScreen, { backend: state, workspaceId: null, instance: null }, shell);
    expect(text(target)).toContain('Supplier review');
    button(target, 'Restore').click();
    await tick();
    expect(transport.sentOf('workspaces.restore').map((e) => e.input)).toEqual([{ workspace_id: WS }]);
    expect(state.workspaces.map((w) => w.name)).toEqual(['Supplier review']);
    expect(text(target)).toContain('No workspace is archived.');
  });

  it('Error path — Archive refused is the error line under the tree', async () => {
    const { model: m, state, shell } = await model((t) => t.on('workspaces.archive', err('WS_STATE_INVALID', "That can't be done to this workspace right now.")));
    expect(await m.archive(SUPPLIER)).toBe(false);
    const target = await show(LifecycleFoot, { backend: state }, shell);
    expect(text(target)).toContain("That can't be done to this workspace right now.");
    expect(shell.openWorkspace).toBe(WS);
  });
});

describe('Remove workspace… (SM-WS-6)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-WS-6
  const impact = { unsent_changes: 2, documents: ['Supplier summary', 'rates.md'] };

  it('Happy path — the confirmation names the unsent changes; Move to Bin moves it and offers Undo', async () => {
    const { model: m, transport, state, shell } = await model((t) => {
      const by = lists(t, { active: [SUPPLIER] });
      t.on('workspaces.bin_impact', ok(impact));
      t.on('workspaces.move_to_bin', () => {
        by.active = [];
        by.in_bin = [{ ...SUPPLIER, state: 'in_bin', moved_to_bin_at: '2026-01-20T10:00:00.000Z' }];
        return ok(by.in_bin[0]);
      });
      t.on('workspaces.restore_from_bin', () => {
        by.in_bin = [];
        by.active = [SUPPLIER];
        return ok(SUPPLIER);
      });
    });
    const target = await show(LifecycleFoot, { backend: state }, shell);
    await m.askBin(SUPPLIER, 'rail');
    await tick();
    expect(transport.sentOf('workspaces.bin_impact').map((e) => e.input)).toEqual([{ workspace_id: WS }]);
    const dialog = target.querySelector('[role="dialog"]')!;
    expect(text(dialog)).toContain('Move this workspace to the Bin?');
    expect(text(dialog)).toContain('Your documents stay where they are.');
    expect(text(dialog)).toContain('It also takes 2 changes not yet sent back: Supplier summary and rates.md.');
    // The removing verb is never the default button (UX-A57).
    expect(button(dialog, 'Move to Bin').getAttribute('data-default')).toBeNull();
    expect(button(dialog, 'Cancel').getAttribute('data-default')).toBe('true');

    button(dialog, 'Move to Bin').click();
    await tick();
    expect(transport.sentOf('workspaces.move_to_bin').map((e) => e.input)).toEqual([{ workspace_id: WS }]);
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(state.workspaces).toEqual([]);
    expect(shell.openWorkspace).toBeNull();
    expect(text(target)).toContain('Moved “Supplier review” to the Bin. Your documents are where they were.');
    expect(labelled(target, 'Bin, 1')).not.toBeNull();

    button(target, 'Undo').click();
    await tick();
    expect(transport.sentOf('workspaces.restore_from_bin').map((e) => e.input)).toEqual([{ workspace_id: WS }]);
    expect(state.workspaces.map((w) => w.name)).toEqual(['Supplier review']);
  });

  it('Edge case — the confirmation cancelled sends nothing more and the workspace stays', async () => {
    const { model: m, transport, state, shell } = await model((t) => {
      lists(t, { active: [SUPPLIER] });
      t.on('workspaces.bin_impact', ok({ unsent_changes: 0, documents: [] }));
    });
    const target = await show(LifecycleFoot, { backend: state }, shell);
    await m.askBin(SUPPLIER, 'rail');
    await tick();
    expect(text(target)).not.toContain('not yet sent back:');
    button(target.querySelector('[role="dialog"]')!, 'Cancel').click();
    await tick();
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(transport.sentOf('workspaces.move_to_bin')).toHaveLength(0);
    expect(shell.openWorkspace).toBe(WS);
  });

  it('Error path — Move to Bin refused keeps the dialog open with the error line', async () => {
    const { model: m, state, shell } = await model((t) => {
      t.on('workspaces.bin_impact', ok({ unsent_changes: 0, documents: [] }));
      t.on('workspaces.move_to_bin', err('WS_STATE_INVALID', "That can't be done to this workspace right now."));
    });
    const target = await show(LifecycleFoot, { backend: state }, shell);
    await m.askBin(SUPPLIER, 'rail');
    await tick();
    button(target.querySelector('[role="dialog"]')!, 'Move to Bin').click();
    await tick();
    const dialog = target.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();
    expect(text(dialog!)).toContain("That can't be done to this workspace right now.");
    expect(shell.openWorkspace).toBe(WS);
  });

  it('Error path — bin_impact refused opens no dialog and draws the error line', async () => {
    const { model: m, transport, state, shell } = await model((t) => t.on('workspaces.bin_impact', err('WS_NOT_FOUND', 'That workspace is no longer in ordin.')));
    const target = await show(LifecycleFoot, { backend: state }, shell);
    expect(await m.askBin(SUPPLIER, 'rail')).toBe(false);
    await tick();
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(text(target)).toContain('That workspace is no longer in ordin.');
    expect(transport.sentOf('workspaces.move_to_bin')).toHaveLength(0);
  });
});

describe('The Bin (SM-WS-7, SM-WS-8)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-WS-7
  // spec: ordin-desktop/components/ScreensModule#SM-WS-8
  const binned = () => [
    ws(WS, 'Supplier review', 'in_bin', { moved_to_bin_at: '2026-01-20T10:00:00.000Z' }),
    ws(OLD, 'Old pilot', 'in_bin', { moved_to_bin_at: '2026-01-18T10:00:00.000Z' }),
  ];

  it('Happy path — the Bin restores and deletes', async () => {
    const { transport, state, shell } = await model((t) => {
      const by = lists(t, { in_bin: binned() });
      t.on('workspaces.restore_from_bin', (input) => {
        const id = (input as { workspace_id: string }).workspace_id;
        by.in_bin = by.in_bin.filter((w) => w.id !== id);
        by.active = [SUPPLIER];
        return ok(SUPPLIER);
      });
      t.on('workspaces.bin_impact', ok({ unsent_changes: 1, documents: ['notes.md'] }));
      t.on('workspaces.delete_from_bin', (input) => {
        const id = (input as { workspace_id: string }).workspace_id;
        by.in_bin = by.in_bin.filter((w) => w.id !== id);
        return ok({});
      });
    });
    const target = await show(BinScreen, { backend: state, workspaceId: null, instance: null }, shell);
    expect(text(target)).toContain('Supplier review');
    expect(text(target)).toContain('Old pilot');
    expect(text(target)).toContain('Removed ');
    expect(text(target)).toContain('Empty Bin…');

    button(target.querySelector('li')!, 'Restore').click();
    await tick();
    expect(transport.sentOf('workspaces.restore_from_bin').map((e) => e.input)).toEqual([{ workspace_id: WS }]);
    expect(state.workspaces.map((w) => w.name)).toEqual(['Supplier review']);
    expect(text(target)).not.toContain('Supplier review');

    button(target, 'Delete now…').click();
    await tick();
    expect(transport.sentOf('workspaces.bin_impact').map((e) => e.input)).toEqual([{ workspace_id: OLD }]);
    const dialog = target.querySelector('[role="dialog"]')!;
    expect(text(dialog)).toContain('Delete “Old pilot” now?');
    expect(text(dialog)).toContain("This can't be undone.");
    expect(text(dialog)).toContain('1 change not yet sent back is lost: notes.md.');
    expect(button(dialog, 'Delete').getAttribute('data-default')).toBeNull();
    button(dialog, 'Delete').click();
    await tick();
    expect(transport.sentOf('workspaces.delete_from_bin').map((e) => e.input)).toEqual([{ workspace_id: OLD }]);
    expect(text(target)).toContain('The Bin is empty.');
    expect(text(target)).not.toContain('Empty Bin…');
  });

  it('Happy path — Empty Bin names what goes and announces the count', async () => {
    const { transport, state, shell } = await model((t) => {
      const by = lists(t, { in_bin: binned() });
      t.on('workspaces.empty_bin', () => {
        by.in_bin = [];
        return ok({ deleted: 2 });
      });
    });
    const target = await show(BinScreen, { backend: state, workspaceId: null, instance: null }, shell);
    button(target, 'Empty Bin…').click();
    await tick();
    const dialog = target.querySelector('[role="dialog"]')!;
    expect(text(dialog)).toContain('Empty the Bin?');
    expect(text(dialog)).toContain('2 workspaces are deleted, with their links, conversation and history: Supplier review and Old pilot.');
    expect(button(dialog, 'Empty Bin').getAttribute('data-default')).toBeNull();
    button(dialog, 'Empty Bin').click();
    await tick();
    await new Promise((r) => setTimeout(r, 0));
    expect(transport.sentOf('workspaces.empty_bin').map((e) => e.input)).toEqual([{}]);
    expect(text(target)).toContain('The Bin is empty.');
    expect(clean(shell.announcement)).toBe('Emptied the Bin: 2 workspaces deleted.');
  });

  it('Edge case — Empty Bin cancelled sends nothing', async () => {
    const { transport, state, shell } = await model((t) => lists(t, { in_bin: binned() }));
    const target = await show(BinScreen, { backend: state, workspaceId: null, instance: null }, shell);
    button(target, 'Empty Bin…').click();
    await tick();
    button(target.querySelector('[role="dialog"]')!, 'Cancel').click();
    await tick();
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(transport.sentOf('workspaces.empty_bin')).toHaveLength(0);
  });

  it('Error path — a restore refused is the error line on its row', async () => {
    const { state, shell } = await model((t) => {
      lists(t, { in_bin: binned() });
      t.on('workspaces.restore_from_bin', err('WS_NAME_DUPLICATE', 'Another workspace already has that name.'));
    });
    const target = await show(BinScreen, { backend: state, workspaceId: null, instance: null }, shell);
    const row = target.querySelector('li')!;
    button(row, 'Restore').click();
    await tick();
    expect(text(row)).toContain('Another workspace already has that name.');
    expect(text(target)).toContain('Supplier review');
  });
});
