// @vitest-environment happy-dom
// The agent's files in two panes (ScreensModule.md SM-IF-1 to SM-IF-3): the reader beside the tree at every
// width, and Hide the files / Show the files. Mounted in a DOM so the buttons can be pressed.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { InstalledFilesController } from '../src/lib/views/parts/installed-files.svelte';
import InstalledFilesView from '../src/lib/views/parts/InstalledFilesView.svelte';
import { backendWith } from './fixed-support';
import { ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const files = [
  { path: 'rules/release.md', sha256: 'a'.repeat(64), size: 40 },
  { path: 'SKILL.md', sha256: 'b'.repeat(64), size: 30 },
];

let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn.part);
  drawn?.target.remove();
  drawn = null;
});

async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}

const byTest = (id: string): HTMLElement | null => document.querySelector(`[data-testid="${id}"]`);

async function draw(): Promise<void> {
  const { handle, state } = await backendWith({
    setup: (tr) => {
      tr.on('agents.installed_files', ok(files));
      tr.on('agents.installed_file', ok({ path: 'rules/release.md', text: '# Release\n\nShip on Tuesdays.', cut: false }));
    },
  });
  const c = new InstalledFilesController(handle, uid('agent-1'));
  await c.load();
  await c.open('rules/release.md');
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = { target, part: mount(InstalledFilesView, { target, props: { backend: state, c, agent: 'Release planner' } }) };
  await flush();
}

describe('the agent\'s files in two panes (SM-IF-1 to SM-IF-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-IF-1
  it('the chosen file is read in its own pane beside the tree', async () => {
    await draw();
    const tree = byTest('installed-files-tree');
    const reader = byTest('installed-files-reader');
    expect(tree).not.toBeNull();
    expect(reader?.textContent).toContain('Ship on Tuesdays.');
    // Siblings in one two-column grid: the reader is never inside or under the tree.
    expect(tree?.parentElement).toBe(reader?.parentElement);
    expect(tree?.contains(reader as Node)).toBe(false);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-IF-2
  it('Hide the files gives the reader the whole width; Show the files brings the tree back', async () => {
    await draw();
    const hide = byTest('installed-files-hide-tree');
    expect(hide?.getAttribute('aria-label')).toBe('Hide the files');
    expect(hide?.getAttribute('title')).toBe('Hide the files');
    hide?.click();
    await flush();
    expect(byTest('installed-files-tree')).toBeNull();
    expect(byTest('installed-files-reader')?.textContent).toContain('Ship on Tuesdays.');
    expect(byTest('installed-files-reader')?.parentElement?.classList.contains('tree-hidden')).toBe(true);
    const show = byTest('installed-files-show-tree');
    expect(show?.getAttribute('aria-label')).toBe('Show the files');
    show?.click();
    await flush();
    expect(byTest('installed-files-tree')).not.toBeNull();
    expect(byTest('installed-files-show-tree')).toBeNull();
  });
});
