// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';

import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import { ViewPrefs, type StorageLike } from '../src/lib/store/view-prefs';
import { readReactively } from './support/reactive.svelte';

const scope = 'b:w';

describe('ScreenAreaState drawn by a template', () => {
  // The window stayed blank because reading the tabs of a scope not yet seen wrote reactive state, which
  // Svelte refuses while a screen is drawn (state_unsafe_mutation).
  it('reading the tabs and the front of a scope not yet seen writes nothing', () => {
    const area = new ScreenAreaState();
    expect(readReactively(() => area.tabs(scope))).toEqual([]);
    expect(readReactively(() => area.front(scope))).toBeNull();
  });

  it('a scope with saved tabs shows them, the first in front, without opening anything', () => {
    const data = new Map<string, string>();
    const storage: StorageLike = { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
    const first = new ScreenAreaState(new ViewPrefs(storage));
    first.open(scope, 'home');
    first.open(scope, 'settings');
    const area = new ScreenAreaState(new ViewPrefs(storage));
    expect(readReactively(() => area.tabs(scope).map((t) => t.key))).toEqual(['home', 'settings']);
    expect(readReactively(() => area.front(scope))).toBe('home');
    // Ordin opening a tab for a scope not yet seen leaves the person's place where it was.
    area.open(scope, 'extra', { inFront: false });
    expect(area.front(scope)).toBe('home');
  });
});

describe('ScreenAreaState', () => {
  it('something already open comes to the front instead of opening twice', () => {
    const area = new ScreenAreaState();
    area.open(scope, 'home');
    area.open(scope, 'settings');
    area.open(scope, 'home');
    expect(area.tabs(scope).map((t) => t.key)).toEqual(['home', 'settings']);
    expect(area.front(scope)).toBe('home');
  });

  it('Edge case — the seventh tab closes the oldest tab neither in front nor showing running work', () => {
    const area = new ScreenAreaState(undefined, (_s, key) => key === 'k1');
    for (const k of ['k0', 'k1', 'k2', 'k3', 'k4', 'k5']) area.open(scope, k);
    area.show(scope, 'k0');
    area.open(scope, 'k6', { inFront: false });
    // k0 is in front, k1 shows running work: k2 is the oldest left over.
    expect(area.tabs(scope).map((t) => t.key)).toEqual(['k0', 'k1', 'k3', 'k4', 'k5', 'k6']);
    expect(area.front(scope)).toBe('k0');
  });

  it('a screen Ordin opens is marked new and leaves the front alone; looking at it clears the mark', () => {
    const area = new ScreenAreaState();
    area.open(scope, 'home');
    area.open(scope, 'approval:1', { inFront: false });
    expect(area.front(scope)).toBe('home');
    expect(area.tabs(scope)[1]?.isNew).toBe(true);
    area.show(scope, 'approval:1');
    expect(area.tabs(scope)[1]?.isNew).toBeUndefined();
  });

  it('closing moves the front to the neighbour and never deletes anything else', () => {
    const area = new ScreenAreaState();
    for (const k of ['a', 'b', 'c']) area.open(scope, k);
    area.close(scope, 'c');
    expect(area.front(scope)).toBe('b');
    area.close(scope, 'a');
    area.close(scope, 'b');
    expect(area.tabs(scope)).toEqual([]);
    expect(area.front(scope)).toBeNull();
  });

  it('remembers only the screen keys, per workspace', () => {
    const data = new Map<string, string>();
    const storage: StorageLike = { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v) };
    const area = new ScreenAreaState(new ViewPrefs(storage));
    area.open(scope, 'home');
    area.open(scope, 'document:p1', { title: 'Quarterly plan.docx' });
    const stored = [...data.values()].join('');
    expect(stored).toContain('document:p1');
    expect(stored).not.toContain('Quarterly');
    const again = new ScreenAreaState(new ViewPrefs(storage));
    expect(again.tabs(scope).map((t) => t.key)).toEqual(['home', 'document:p1']);
  });
});
