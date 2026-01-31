import { describe, expect, it } from 'vitest';

import { nextRegion, trapIndex } from '../src/lib/shell/focus';

describe('F6 region cycling', () => {
  it.each([
    [null, false, 'tree'],
    [null, true, 'conversation'],
    ['tree', false, 'screen'],
    ['screen', false, 'conversation'],
    ['conversation', false, 'tree'],
    ['tree', true, 'conversation'],
    ['screen', true, 'tree'],
  ] as const)('from %s, backwards %s -> %s', (from, back, to) => {
    expect(nextRegion(from, back)).toBe(to);
  });

  it('skips a region that is not shown (narrow window)', () => {
    expect(nextRegion('screen', false, ['tree', 'screen'])).toBe('tree');
    expect(nextRegion('tree', true, ['tree', 'screen'])).toBe('screen');
    expect(nextRegion(null, false, [])).toBeNull();
  });
});

describe('Tab trapped in a dialog', () => {
  it('wraps forwards and backwards', () => {
    expect(trapIndex(2, 3, false)).toBe(0);
    expect(trapIndex(0, 3, true)).toBe(2);
    expect(trapIndex(0, 3, false)).toBe(1);
    expect(trapIndex(-1, 3, false)).toBe(0);
    expect(trapIndex(-1, 3, true)).toBe(2);
    expect(trapIndex(0, 0, false)).toBe(-1);
  });
});
