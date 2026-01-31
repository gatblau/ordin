// @vitest-environment happy-dom
// The screen tabs' menu and Home that closes (ScreensModule.md, Screen area and tabs; amended January 2026;
// user decision; Approved by the user January 2026): a right press, Shift+F10 or the menu key on a tab opens
// Close, Close others, Close tabs to the right and Close all.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import ScreenTabs from '../src/lib/shell/ScreenTabs.svelte';
import { ScreenAreaState, tabsToClose } from '../src/lib/shell/screen-area.svelte';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

const scope = 'b:w';
const TABS = [{ key: 'home' }, { key: 'a' }, { key: 'b' }, { key: 'c' }];

describe('tabsToClose', () => {
  it.each([
    ['others', 1, ['home', 'b', 'c']],
    ['right', 1, ['b', 'c']],
    ['all', 1, ['home', 'a', 'b', 'c']],
    ['right', 3, []],
    ['others', 9, []],
  ] as const)('%s of the tab at %i', (which, index, keys) => {
    expect(tabsToClose(TABS, index, which)).toEqual(keys);
  });
});

describe('ScreenAreaState.closeMany', () => {
  const area = (front: string): ScreenAreaState => {
    const a = new ScreenAreaState();
    for (const t of TABS) a.open(scope, t.key);
    a.show(scope, front);
    return a;
  };

  it('Happy path — Close others keeps the one tab and puts it in front', () => {
    const a = area('c');
    a.closeMany(scope, tabsToClose(a.tabs(scope), 1, 'others'));
    expect(a.tabs(scope).map((t) => t.key)).toEqual(['a']);
    expect(a.front(scope)).toBe('a');
  });

  it('Edge — the front tab closed with those after it moves the front to the nearest one before', () => {
    const a = area('c');
    a.closeMany(scope, ['b', 'c']);
    expect(a.front(scope)).toBe('a');
  });

  it('Edge — the front tab left open stays in front', () => {
    const a = area('home');
    a.closeMany(scope, ['b', 'c']);
    expect(a.front(scope)).toBe('home');
  });

  it('Edge — Close all, Home included, leaves the area empty', () => {
    const a = area('a');
    a.closeMany(scope, tabsToClose(a.tabs(scope), 0, 'all'));
    expect(a.tabs(scope)).toEqual([]);
    expect(a.front(scope)).toBeNull();
  });

  it('Edge — keys not open change nothing', () => {
    const a = area('a');
    a.closeMany(scope, ['nope']);
    expect(a.tabs(scope)).toHaveLength(4);
  });

  it('Home closes like any tab', () => {
    const a = area('home');
    a.close(scope, 'home');
    expect(a.tabs(scope).map((t) => t.key)).toEqual(['a', 'b', 'c']);
  });
});

describe('ScreenTabs — the tab menu', () => {
  let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
  afterEach(() => {
    if (drawn) {
      unmount(drawn.part);
      drawn.target.remove();
      drawn = null;
    }
  });
  const draw = (onclose = vi.fn(), onclosemany = vi.fn()) => {
    const target = document.createElement('div');
    document.body.appendChild(target);
    const part = mount(ScreenTabs, { target, props: { tabs: TABS, front: 'a', titleOf: (t: { key: string }) => t.key, iconOf: () => 'docs', onselect: () => undefined, onclose, onclosemany } as never });
    drawn = { target, part };
    flushSync();
    return { target, onclose, onclosemany };
  };
  const menuItems = (): HTMLButtonElement[] => Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menu"] [role="menuitem"]'));
  const item = (name: string): HTMLButtonElement => menuItems().find((b) => b.textContent?.trim() === name)!;

  it('Happy path — a right press on a tab opens its menu with the four entries; Close others closes the rest', () => {
    const { target, onclosemany } = draw();
    const tab = target.querySelectorAll('.screen-tab')[1]!;
    tab.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 40, clientY: 30 }));
    flushSync();
    expect(menuItems().map((b) => b.textContent?.trim())).toEqual(['Close', 'Close others', 'Close tabs to the right', 'Close all']);
    expect(document.querySelector('[role="menu"]')!.getAttribute('aria-label')?.replace(/[⁨⁩]/g, '')).toBe('Tab actions for a');
    item('Close others').click();
    flushSync();
    expect(onclosemany).toHaveBeenCalledWith(['home', 'b', 'c']);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it('Happy path — Close tabs to the right, Close all and Close', () => {
    const { target, onclose, onclosemany } = draw();
    const open = (i: number): void => {
      target.querySelectorAll('.screen-tab')[i]!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
      flushSync();
    };
    open(1);
    item('Close tabs to the right').click();
    expect(onclosemany).toHaveBeenLastCalledWith(['b', 'c']);
    open(0);
    item('Close all').click();
    expect(onclosemany).toHaveBeenLastCalledWith(['home', 'a', 'b', 'c']);
    open(0);
    item('Close').click();
    expect(onclose).toHaveBeenCalledWith('home');
  });

  it('Keyboard — Shift+F10 on a tab opens the menu; Escape closes it; the last tab cannot close tabs to its right', () => {
    const { target } = draw();
    const tabs = target.querySelectorAll<HTMLElement>('[role="tab"]');
    expect(tabs[3]!.getAttribute('aria-haspopup')).toBe('menu');
    tabs[3]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    flushSync();
    expect(item('Close tabs to the right').disabled).toBe(true);
    document.querySelector('[role="menu"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    flushSync();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it('Edge — every tab, Home included, has a Close button', () => {
    const { target, onclose } = draw();
    const closes = target.querySelectorAll<HTMLButtonElement>('.tab-close');
    expect(closes).toHaveLength(4);
    closes[0]!.click();
    expect(onclose).toHaveBeenCalledWith('home');
  });
});
