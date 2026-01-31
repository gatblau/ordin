// @vitest-environment happy-dom
// SM-PC-1 (ScreensModule.md, amended January 2026; user decision; Approved by the user January 2026): the side
// panel and the conversation each collapse to a slim strip holding the button that restores them; the state
// is remembered in the view preferences (`ordin.view.panel.collapsed`).
import { flushSync, mount, unmount } from 'svelte';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';

import App from '../src/App.svelte';
import { startApp } from '../src/lib/app/bootstrap';
import '../src/lib/shell/load-registrations';
import { KEYS, ViewPrefs, type StorageLike } from '../src/lib/store/view-prefs';
import { FakeTransport, ok, settle } from './support/fake-backend';

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const store = (): { data: Map<string, string>; storage: StorageLike } => {
  const data = new Map<string, string>();
  return { data, storage: { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) } };
};

async function window_(storage: StorageLike, width = 1400): Promise<HTMLElement> {
  Object.defineProperty(window, 'innerWidth', { value: width, configurable: true });
  const transport = new FakeTransport();
  transport.on('setup.update_offer', ok({ offer: null })).on('setup.whats_new', ok({ whats_new: null }));
  const app = await startApp(transport, { document: null, storage });
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(App, { target, props: { app } });
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
  return target;
}
const toggle = (target: HTMLElement, which: 'rail' | 'conversation'): HTMLButtonElement => target.querySelector<HTMLButtonElement>(`#pane-toggle-${which}`)!;

describe('ViewPrefs — collapsed panes', () => {
  it('Happy path — reads back what was written', () => {
    const { storage, data } = store();
    const prefs = new ViewPrefs(storage);
    expect(prefs.collapsed()).toEqual({ rail: false, conversation: false });
    prefs.setCollapsed({ rail: true, conversation: false });
    expect(JSON.parse(data.get(KEYS.collapsed)!)).toEqual({ rail: true, conversation: false });
    expect(new ViewPrefs(storage).collapsed()).toEqual({ rail: true, conversation: false });
  });

  it('Edge — a value that does not parse, or no storage, reads as both open', () => {
    const { storage, data } = store();
    data.set(KEYS.collapsed, '"nonsense"');
    expect(new ViewPrefs(storage).collapsed()).toEqual({ rail: false, conversation: false });
    expect(new ViewPrefs(null).collapsed()).toEqual({ rail: false, conversation: false });
  });
});

describe('SM-PC-1 — collapse and restore the side panes', () => {
  it('Happy path — Hide the side panel leaves a strip with Show the side panel; pressed, the panel returns', async () => {
    const { storage, data } = store();
    const target = await window_(storage);
    const button = toggle(target, 'rail');
    expect(button.getAttribute('aria-label')).toBe('Hide the side panel');
    expect(button.title).toBe('Hide the side panel');
    expect(button.getAttribute('aria-expanded')).toBe('true');
    button.click();
    flushSync();
    const rail = target.querySelector('.rail')!;
    expect(rail.classList.contains('collapsed')).toBe(true);
    expect((rail.querySelector('.pane-body') as HTMLElement).hidden).toBe(true);
    const restore = toggle(target, 'rail');
    expect(restore.getAttribute('aria-label')).toBe('Show the side panel');
    expect(restore.getAttribute('aria-expanded')).toBe('false');
    expect(JSON.parse(data.get(KEYS.collapsed)!)).toEqual({ rail: true, conversation: false });
    restore.click();
    flushSync();
    expect(rail.classList.contains('collapsed')).toBe(false);
    expect((rail.querySelector('.pane-body') as HTMLElement).hidden).toBe(false);
  });

  it('Happy path — the conversation collapses and restores the same way, its resize edge out of the tab order while collapsed', async () => {
    const { storage } = store();
    const target = await window_(storage);
    expect(toggle(target, 'conversation').getAttribute('aria-label')).toBe('Hide the conversation');
    toggle(target, 'conversation').click();
    flushSync();
    expect(target.querySelector('.conversation')!.classList.contains('collapsed')).toBe(true);
    expect(toggle(target, 'conversation').getAttribute('aria-label')).toBe('Show the conversation');
    const edges = Array.from(target.querySelectorAll<HTMLElement>('[role="separator"]'));
    expect(edges.at(-1)!.classList.contains('parked')).toBe(true);
    expect(edges.at(-1)!.tabIndex).toBe(-1);
    expect(edges.at(-1)!.getAttribute('aria-hidden')).toBe('true');
    expect(edges[0]!.classList.contains('parked')).toBe(false);
    expect(edges[0]!.tabIndex).toBe(0);
  });

  it('Edge — a window started with both collapsed opens collapsed, each strip keeping its restore button in reach', async () => {
    const { storage } = store();
    new ViewPrefs(storage).setCollapsed({ rail: true, conversation: true });
    const target = await window_(storage);
    for (const which of ['rail', 'conversation'] as const) {
      const b = toggle(target, which);
      expect(b.tagName).toBe('BUTTON');
      expect(b.tabIndex).toBe(0);
      expect(b.getAttribute('aria-expanded')).toBe('false');
      expect(b.hasAttribute('data-region-focus')).toBe(true);
    }
  });

  it('Edge — in a narrow window the conversation slides over, has no collapse button and ignores a stored collapse', async () => {
    const { storage } = store();
    new ViewPrefs(storage).setCollapsed({ rail: false, conversation: true });
    const target = await window_(storage, 800);
    expect(toggle(target, 'conversation')).toBeNull();
    expect(toggle(target, 'rail')).not.toBeNull();
  });
});

// The window's real grid (tokens.css `.shell`): five columns, rail · edge · screen area · edge · conversation.
// happy-dom computes no grid layout, so the placement is worked out from the stylesheet's own rules: a child
// with `hidden` (display: none) leaves the grid; every other child takes the column its rule names.
const TOKENS = readFileSync(join(__dirname, '../src/styles/tokens.css'), 'utf8');
const placed = (cls: string, wide: boolean): number | null => {
  const rule = new RegExp(`\\.shell${wide ? '(?::not\\(\\.narrow\\))?' : ''} > \\.${cls} \\{\\s*grid-column: (\\d+);`).exec(TOKENS);
  return rule === null ? null : Number(rule[1]);
};
const COLUMN_OF: Record<string, string> = { rail: 'rail', 'separator-rail': 'separator-rail', centre: 'centre', 'separator-conversation': 'separator-conversation', conversation: 'conversation' };
function layout(target: HTMLElement): Record<string, number> {
  const shell = target.querySelector<HTMLElement>('.shell')!;
  const columns = (/\.shell \{[^}]*grid-template-columns: ([^;]+);/.exec(TOKENS)?.[1] ?? '').trim().split(/\s+(?![^(]*\))/);
  expect(columns).toHaveLength(5);
  const out: Record<string, number> = {};
  for (const child of Array.from(shell.children) as HTMLElement[]) {
    // A child taken out of the grid is the bug this guards against.
    expect(child.hidden, `${child.className} stays in the grid`).toBe(false);
    const cls = Object.keys(COLUMN_OF).find((c) => child.classList.contains(c));
    if (cls === undefined) continue;
    const column = placed(cls, !shell.classList.contains('narrow'));
    expect(column, `${cls} has a column of its own`).not.toBeNull();
    out[cls] = column!;
  }
  return out;
}

describe('SM-PC-1 — the layout keeps its columns (amended January 2026)', () => {
  const WHOLE = { rail: 1, 'separator-rail': 2, centre: 3, 'separator-conversation': 4, conversation: 5 };

  it('Happy path — both open: the five regions are in their five columns', async () => {
    const { storage } = store();
    expect(layout(await window_(storage))).toEqual(WHOLE);
  });

  it('Error path — the side panel collapsed: the screen area and the conversation do not move', async () => {
    const { storage } = store();
    const target = await window_(storage);
    toggle(target, 'rail').click();
    flushSync();
    expect(layout(target)).toEqual(WHOLE);
    expect((target.querySelector('.shell') as HTMLElement).style.getPropertyValue('--rail-width')).toBe('var(--pane-strip)');
    expect((target.querySelector('.centre') as HTMLElement).hidden).toBe(false);
    expect((target.querySelector('.conversation .pane-body') as HTMLElement).hidden).toBe(false);
  });

  it('Error path — the conversation collapsed: its strip stays in the last column with Show the conversation', async () => {
    const { storage } = store();
    const target = await window_(storage);
    toggle(target, 'conversation').click();
    flushSync();
    expect(layout(target)).toEqual(WHOLE);
    expect((target.querySelector('.shell') as HTMLElement).style.getPropertyValue('--conversation-width')).toBe('var(--pane-strip)');
    const restore = toggle(target, 'conversation');
    expect(restore.closest('.conversation')).not.toBeNull();
    expect(restore.closest('[hidden]')).toBeNull();
    restore.click();
    flushSync();
    expect(target.querySelector('.conversation')!.classList.contains('collapsed')).toBe(false);
  });

  it('Edge — both collapsed: every region keeps its column and both restore buttons are drawn', async () => {
    const { storage } = store();
    new ViewPrefs(storage).setCollapsed({ rail: true, conversation: true });
    const target = await window_(storage);
    expect(layout(target)).toEqual(WHOLE);
    for (const which of ['rail', 'conversation'] as const) expect(toggle(target, which).closest('[hidden]')).toBeNull();
  });

  it('Edge — the parked edge is kept in the layout but not drawn', () => {
    expect(/\.separator\.parked \{[^}]*visibility: hidden;[^}]*pointer-events: none;/.test(TOKENS)).toBe(true);
    expect(/\.separator\.parked \{[^}]*display: none/.test(TOKENS)).toBe(false);
  });
});

// The agent-work layout (SM-PC-1, amended January 2026; user decision; Approved by the user January 2026): one
// icon button beside the conversation's collapse button collapses the side panel and splits the width left
// 60% conversation, 40% centre; pressed again, the layout before it comes back exactly.
describe('SM-PC-1 — the agent-work layout', () => {
  const agent = (target: HTMLElement): HTMLButtonElement => target.querySelector<HTMLButtonElement>('#agent-work-toggle')!;
  const shellOf = (target: HTMLElement): HTMLElement => target.querySelector<HTMLElement>('.shell')!;

  it('Happy path — pressed, the side panel collapses and the conversation takes 60% of the rest; pressed again, the widths and states come back', async () => {
    const { storage, data } = store();
    new ViewPrefs(storage).setConversationWidth(500);
    new ViewPrefs(storage).setRailWidth(300);
    const target = await window_(storage);
    const button = agent(target);
    expect(button.getAttribute('aria-pressed')).toBe('false');
    expect(button.getAttribute('aria-label')).toBe('Make the conversation wide for working with agents');
    expect(button.title).toBe('Make the conversation wide for working with agents');
    expect(shellOf(target).style.getPropertyValue('--conversation-width')).toBe('500px');
    button.click();
    flushSync();
    expect(agent(target).getAttribute('aria-pressed')).toBe('true');
    expect(shellOf(target).classList.contains('agent-work')).toBe(true);
    expect(target.querySelector('.rail')!.classList.contains('collapsed')).toBe(true);
    expect(shellOf(target).style.getPropertyValue('--rail-width')).toBe('var(--pane-strip)');
    expect(shellOf(target).style.getPropertyValue('--conversation-width')).toBe('calc((100% - var(--rail-width)) * 0.6)');
    expect(JSON.parse(data.get(KEYS.agentWork)!)).toMatchObject({ on: true });
    agent(target).click();
    flushSync();
    expect(agent(target).getAttribute('aria-pressed')).toBe('false');
    expect(shellOf(target).style.getPropertyValue('--conversation-width')).toBe('500px');
    expect(shellOf(target).style.getPropertyValue('--rail-width')).toBe('300px');
    expect(target.querySelector('.rail')!.classList.contains('collapsed')).toBe(false);
    expect(new ViewPrefs(storage).collapsed()).toEqual({ rail: false, conversation: false });
    expect(new ViewPrefs(storage).conversationWidth()).toBe(500);
    expect(new ViewPrefs(storage).railWidth()).toBe(300);
    expect(new ViewPrefs(storage).agentWork()).toBeNull();
  });

  it('Edge — from both panes collapsed: the conversation opens for agent work, and leaving it collapses both again', async () => {
    const { storage } = store();
    new ViewPrefs(storage).setCollapsed({ rail: true, conversation: true });
    const target = await window_(storage);
    agent(target).click();
    flushSync();
    expect(target.querySelector('.conversation')!.classList.contains('collapsed')).toBe(false);
    expect(target.querySelector('.rail')!.classList.contains('collapsed')).toBe(true);
    agent(target).click();
    flushSync();
    expect(target.querySelector('.conversation')!.classList.contains('collapsed')).toBe(true);
    expect(target.querySelector('.rail')!.classList.contains('collapsed')).toBe(true);
    expect(new ViewPrefs(storage).collapsed()).toEqual({ rail: true, conversation: true });
  });

  it('Edge — remembered: a window opened in the agent-work layout opens in it', async () => {
    const { storage } = store();
    new ViewPrefs(storage).setAgentWork({ restore: { rail: false, conversation: false } });
    const target = await window_(storage);
    expect(agent(target).getAttribute('aria-pressed')).toBe('true');
    expect(shellOf(target).style.getPropertyValue('--conversation-width')).toBe('calc((100% - var(--rail-width)) * 0.6)');
  });

  it('Error path — a stored value that does not parse reads as off', () => {
    const { storage, data } = store();
    data.set(KEYS.agentWork, '"nonsense"');
    expect(new ViewPrefs(storage).agentWork()).toBeNull();
  });
});

describe('the pane buttons are the size of the icon buttons beside them', () => {
  it('Happy path — the collapse button drops the 36 px button height', () => {
    const app = readFileSync(join(__dirname, '../src/App.svelte'), 'utf8');
    expect(/\.pane-toggle \{[^}]*block-size: 1\.75rem;[^}]*min-height: 0;/.test(app)).toBe(true);
  });
});

describe('the centre fills the width a collapsed pane leaves', () => {
  it('Happy path — the grid gives the centre the rest, and a document or block screen has no width cap', () => {
    expect(/\.shell \{[^}]*grid-template-columns: [^;]*minmax\(0, 1fr\)/.test(TOKENS)).toBe(true);
    expect(/\.screen-panel > section\.document \{\s*max-inline-size: none;/.test(TOKENS)).toBe(true);
    expect(/\.screen-panel > section\.block-screen \{\s*max-inline-size: none;/.test(TOKENS)).toBe(true);
  });
});
