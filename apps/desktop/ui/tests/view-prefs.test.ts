import { describe, expect, it } from 'vitest';

import { Diagnostics } from '../src/lib/store/diagnostics.svelte';
import { KEYS, ViewPrefs, type StorageLike } from '../src/lib/store/view-prefs';

class MemoryStorage implements StorageLike {
  data = new Map<string, string>();
  getItem(key: string) {
    return this.data.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.data.set(key, value);
  }
}
const throwing: StorageLike = {
  getItem() {
    throw new Error('blocked');
  },
  setItem() {
    throw new Error('blocked');
  },
};

describe('ViewPrefs', () => {
  it('returns the defaults when nothing is stored', () => {
    const prefs = new ViewPrefs(new MemoryStorage());
    expect(prefs.railWidth()).toBe(290);
    expect(prefs.conversationWidth()).toBe(420);
    expect(prefs.treeShow('w')).toEqual({ documents: true, code: true });
    expect(prefs.tabs('b:w')).toEqual([]);
    expect(prefs.expanded(['w'])).toEqual(['w']);
  });

  it('clamps widths to their range', () => {
    const prefs = new ViewPrefs(new MemoryStorage());
    expect(prefs.setRailWidth(9999)).toBe(480);
    expect(prefs.setRailWidth(1)).toBe(200);
    expect(prefs.setConversationWidth(9999)).toBe(640);
    expect(prefs.setConversationWidth(1)).toBe(280);
    expect(prefs.railWidth()).toBe(200);
  });

  it('keeps at most 6 tabs and always one of Documents and Code', () => {
    const prefs = new ViewPrefs(new MemoryStorage());
    prefs.setTabs('b:w', ['a', 'b', 'c', 'd', 'e', 'f', 'g']);
    expect(prefs.tabs('b:w')).toEqual(['a', 'b', 'c', 'd', 'e', 'f']);
    prefs.setTreeShow('w', { documents: false, code: false });
    expect(prefs.treeShow('w')).toEqual({ documents: true, code: true });
  });

  it('Error path — localStorage throws: defaults are used and UI_STORAGE_UNAVAILABLE is recorded once', () => {
    const diagnostics = new Diagnostics();
    const prefs = new ViewPrefs(throwing, diagnostics);
    expect(prefs.railWidth()).toBe(290);
    prefs.setConversationWidth(400);
    expect(prefs.tabs('b:w')).toEqual([]);
    expect(diagnostics.codes()).toEqual(['UI_STORAGE_UNAVAILABLE']);
  });

  it('Error path — an unparsable value is replaced by its default', () => {
    const storage = new MemoryStorage();
    storage.setItem(KEYS.rail, '{not json');
    storage.setItem(KEYS.tabs, JSON.stringify({ 'b:w': 'not a list', 'b:x': ['k'] }));
    const diagnostics = new Diagnostics();
    const prefs = new ViewPrefs(storage, diagnostics);
    expect(prefs.railWidth()).toBe(290);
    expect(prefs.tabs('b:w')).toEqual([]);
    expect(prefs.tabs('b:x')).toEqual(['k']);
    expect(diagnostics.codes()).toEqual(['UI_STORAGE_UNAVAILABLE']);
  });

  it('works with no storage at all', () => {
    const prefs = new ViewPrefs(null);
    expect(prefs.railWidth()).toBe(290);
  });
});
