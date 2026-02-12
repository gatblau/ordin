// @vitest-environment happy-dom
// SM-FE-1 to SM-FE-3: forget everything, for one workspace or every workspace, one test per Gherkin
// scenario and Error Table row.
import { flushSync, mount, unmount } from 'svelte';
import { beforeAll, describe, expect, it } from 'vitest';

import { LONG_DEADLINES_MS } from '../src/lib/backend/backend';
import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { APP_KEY } from '../src/lib/shell/context';
import MemorySettings from '../src/lib/views/Settings/MemorySettings.svelte';
import Memory from '../src/lib/views/fixed/Memory.svelte';
import { ForgetEverything } from '../src/lib/views/fixed/forget-everything.svelte';
import { err, ok, settle, type FakeTransport } from './support/fake-backend';
import { appWith, loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';


beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));
const words = (s: string | null | undefined) => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();

const NAME = 'Q3 supplier review';
const COUNT = { workspaces: 1, facts: 12, everywhere: 2, to_check: 0 };
const DONE = { workspaces: 1, facts_removed: 12, everywhere_removed: 2, forgotten_at: '2026-02-12T10:00:00Z' };

function memoryScreen(tr: FakeTransport): void {
  tr.on('workspaces.list', ok([{ id: WS, name: NAME, created_at: 'x', own_folder_place_id: 'p', state: 'active' }]));
  tr.on('agents.list', ok([]));
  tr.on('memory.list', ok({ items: [], next_cursor: null }));
  tr.on('memory.things', ok({ items: [], next_cursor: null }));
  tr.on('memory.held_writes', ok([]));
  tr.on('memory.citations_to_confirm', ok([]));
}

async function flush(): Promise<void> {
  for (let i = 0; i < 6; i++) {
    await settle();
    flushSync();
  }
}

async function onScreen<P extends Record<string, unknown>>(
  component: Parameters<typeof mount>[0],
  setup: (tr: FakeTransport) => void,
  props: (app: Awaited<ReturnType<typeof appWith>>['app']) => P,
) {
  const { app, transport } = await appWith(setup);
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(component as never, { target, props: props(app) as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await flush();
  return {
    app,
    transport,
    target,
    dialog: () => document.querySelector<HTMLElement>('[role="dialog"]'),
    button: (name: string) => Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find((b) => words(b.textContent) === name),
    close: () => {
      void unmount(part);
      target.remove();
    },
  };
}

async function type(input: HTMLInputElement, value: string): Promise<void> {
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
}

describe('Forget everything (SM-FE-1 to SM-FE-3)', () => {
  it('Scenario: Forgetting one workspace after typing its name', async () => {
    const s = await onScreen(
      Memory,
      (tr) => {
        memoryScreen(tr);
        tr.on('memory.count_everything', ok(COUNT));
        tr.on('memory.forget_everything', ok(DONE));
      },
      (app) => ({ backend: app.local, workspaceId: WS, instance: null }),
    );
    try {
      const more = s.target.querySelector<HTMLButtonElement>('.heading button[aria-haspopup="menu"]');
      expect(more).not.toBeNull();
      more?.click();
      await flush();
      s.button(t('memory-forget-workspace'))?.click();
      await flush();
      expect(s.transport.sentOf('memory.count_everything')[0]?.input).toEqual({ of: { workspace: WS } });
      const dialog = s.dialog();
      expect(dialog).not.toBeNull();
      const said = words(dialog?.textContent);
      expect(said).toContain(t('forget-everything-title-workspace', { name: NAME }));
      expect(said).toContain('Ordin forgets all 12 facts it remembers.');
      expect(said).toContain('This includes 2 facts you chose to remember everywhere; they leave your other workspaces too.');
      expect(said).not.toContain('waiting for you to check');
      expect(document.activeElement?.textContent?.trim()).toBe(t('confirm-cancel'));
      const confirm = s.button(t('forget-everything-confirm'));
      expect(confirm?.disabled).toBe(true);
      await type(dialog?.querySelector('input') as HTMLInputElement, '  q3 supplier review ');
      expect(confirm?.disabled).toBe(false);
      confirm?.click();
      await flush();
      expect(s.transport.sentOf('memory.forget_everything')[0]?.input).toEqual({ of: { workspace: WS } });
      expect(s.dialog()).toBeNull();
      expect(words(s.app.shell.announcement)).toBe('Forgot 12 facts.');
      expect(s.app.local.memoryForgotten).toBe(1);
      expect(s.transport.sentOf('memory.list').length).toBeGreaterThan(0);
    } finally {
      s.close();
    }
  });

  it('Scenario: Every workspace from Settings', async () => {
    const s = await onScreen(
      MemorySettings,
      (tr) => {
        tr.on('memory.count_everything', ok({ workspaces: 2, facts: 5, everywhere: 0, to_check: 1 }));
        tr.on('memory.forget_everything', ok({ ...DONE, workspaces: 2, facts_removed: 5 }));
      },
      (app) => ({ backend: app.local }),
    );
    try {
      s.button(t('settings-memory-forget-all'))?.click();
      await flush();
      expect(s.transport.sentOf('memory.count_everything')[0]?.input).toEqual({ of: 'every_workspace' });
      const dialog = s.dialog();
      const said = words(dialog?.textContent);
      expect(said).toContain(t('forget-everything-title-all'));
      expect(said).toContain('The fact waiting for you to check goes too.');
      expect(said).toContain('Type “every workspace” to confirm');
      const confirm = s.button(t('forget-everything-confirm'));
      await type(dialog?.querySelector('input') as HTMLInputElement, 'Q3 supplier review');
      expect(confirm?.disabled).toBe(true);
      await type(dialog?.querySelector('input') as HTMLInputElement, 'Every Workspace');
      expect(confirm?.disabled).toBe(false);
      confirm?.click();
      await flush();
      expect(s.transport.sentOf('memory.forget_everything')[0]?.input).toEqual({ of: 'every_workspace' });
      expect(words(s.app.shell.announcement)).toBe('Forgot 5 facts.');
    } finally {
      s.close();
    }
  });

  it('Scenario: Edge case — nothing to forget', async () => {
    const s = await onScreen(
      Memory,
      (tr) => {
        memoryScreen(tr);
        tr.on('memory.count_everything', ok({ workspaces: 1, facts: 0, everywhere: 0, to_check: 0 }));
      },
      (app) => ({ backend: app.local, workspaceId: WS, instance: null }),
    );
    try {
      s.target.querySelector<HTMLButtonElement>('.heading button[aria-haspopup="menu"]')?.click();
      await flush();
      s.button(t('memory-forget-workspace'))?.click();
      await flush();
      const dialog = s.dialog();
      expect(words(dialog?.textContent)).toContain(t('forget-everything-nothing'));
      const labels = Array.from(dialog?.querySelectorAll('button') ?? []).map((b) => words(b.textContent));
      expect(labels).toEqual([t('card-close')]);
      expect(dialog?.querySelector('input')).toBeNull();
    } finally {
      s.close();
    }
  });

  it('Scenario: Error — the forgetting fails: the dialog stays open with the error line and the typed name', async () => {
    const s = await onScreen(
      Memory,
      (tr) => {
        memoryScreen(tr);
        tr.on('memory.count_everything', ok(COUNT));
        tr.on('memory.forget_everything', err('MEM_DELETE_UNVERIFIED', 'Ordin could not check that everything was forgotten.'));
      },
      (app) => ({ backend: app.local, workspaceId: WS, instance: null }),
    );
    try {
      s.target.querySelector<HTMLButtonElement>('.heading button[aria-haspopup="menu"]')?.click();
      await flush();
      s.button(t('memory-forget-workspace'))?.click();
      await flush();
      const input = s.dialog()?.querySelector('input') as HTMLInputElement;
      await type(input, NAME);
      s.button(t('forget-everything-confirm'))?.click();
      await flush();
      expect(s.dialog()).not.toBeNull();
      expect(words(s.dialog()?.textContent)).toContain('Ordin could not check that everything was forgotten.');
      expect((s.dialog()?.querySelector('input') as HTMLInputElement).value).toBe(NAME);
    } finally {
      s.close();
    }
  });

  it('Error row — memory.count_everything fails: no dialog, the error line under the heading', async () => {
    const s = await onScreen(
      Memory,
      (tr) => {
        memoryScreen(tr);
        tr.on('memory.count_everything', err('MEM_UNAVAILABLE', 'Memory is not available.'));
      },
      (app) => ({ backend: app.local, workspaceId: WS, instance: null }),
    );
    try {
      s.target.querySelector<HTMLButtonElement>('.heading button[aria-haspopup="menu"]')?.click();
      await flush();
      s.button(t('memory-forget-workspace'))?.click();
      await flush();
      expect(s.dialog()).toBeNull();
      expect(words(s.target.textContent)).toContain('Memory is not available.');
    } finally {
      s.close();
    }
  });

  it('SM-FE-1 — not drawn when the screen is embedded', async () => {
    const s = await onScreen(Memory, memoryScreen, (app) => ({ backend: app.local, workspaceId: WS, instance: null, embedded: true }));
    try {
      expect(s.target.querySelector('.heading button[aria-haspopup="menu"]')).toBeNull();
    } finally {
      s.close();
    }
  });

  it('SM-FE-2.4 — the typed text matches trimmed and ignoring case; nothing is sent before it does', async () => {
    const b = await backendWith({ setup: (tr) => tr.on('memory.count_everything', ok(COUNT)).on('memory.forget_everything', ok(DONE)) });
    const model = new ForgetEverything(b.handle, { workspace: WS }, NAME);
    expect(await model.open()).toBeNull();
    model.typed = 'Q3 supplier';
    expect(model.confirmed).toBe(false);
    expect(await model.forget()).toBeNull();
    expect(b.transport.sentOf('memory.forget_everything')).toHaveLength(0);
    model.typed = '  Q3 SUPPLIER REVIEW  ';
    expect(model.confirmed).toBe(true);
    expect(await model.forget()).toBe(12);
  });

  it('SM-FE-2.5 — forgetting is given 60 seconds', () => {
    expect(LONG_DEADLINES_MS.get('memory.forget_everything')).toBe(60_000);
  });
});
