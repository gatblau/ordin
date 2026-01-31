// The message-id catalogue test: every id the source uses exists in the en-GB .ftl files, and no id is
// defined twice. It scans all of src/ and all of the en-GB catalogue directory, so ids that other parts
// of the module add are covered with no change here.
import { describe, expect, it } from 'vitest';

import { FIXED_TITLES } from '../src/lib/shell/screens';

const ftlFiles = import.meta.glob('../../../../crates/ordin-localisation/catalogues/en-GB/*.ftl', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const sourceFiles = import.meta.glob(['../src/**/*.ts', '../src/**/*.svelte'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

const ID = '[a-z][a-z0-9]*(?:-[a-z0-9]+)*';

function definedIds(): Map<string, string[]> {
  const defined = new Map<string, string[]>();
  for (const [path, content] of Object.entries(ftlFiles)) {
    for (const match of content.matchAll(/^([a-z][a-zA-Z0-9_-]*)\s*=/gm)) {
      const id = match[1] as string;
      defined.set(id, [...(defined.get(id) ?? []), path.split('/').pop() as string]);
    }
  }
  return defined;
}

/** Ids named as message ids in source: `t('id'…)`, and `titleId` / `label` / `labelId` / `messageId` properties. */
function usedIds(): Array<{ id: string; path: string }> {
  const used: Array<{ id: string; path: string }> = [];
  for (const [path, content] of Object.entries(sourceFiles)) {
    if (path.endsWith('.gen.ts')) continue;
    const short = path.replace('../src/', 'src/');
    for (const m of content.matchAll(new RegExp(`\\bt\\(\\s*['"\`](${ID})['"\`]`, 'g'))) used.push({ id: m[1] as string, path: short });
    for (const m of content.matchAll(new RegExp(`\\b(?:titleId|labelId|messageId|label)\\s*[:=]\\s*['"](${ID})['"]`, 'g'))) used.push({ id: m[1] as string, path: short });
  }
  for (const id of Object.values(FIXED_TITLES)) used.push({ id, path: 'src/lib/shell/screens.ts (FIXED_TITLES)' });
  return used;
}

describe('message catalogue', () => {
  it('finds the en-GB catalogue and the module sources', () => {
    expect(Object.keys(ftlFiles).length).toBeGreaterThan(3);
    expect(usedIds().length).toBeGreaterThan(30);
  });

  it('every message id used in the source exists in the en-GB .ftl files', () => {
    const defined = definedIds();
    const missing = usedIds().filter((u) => !defined.has(u.id)).map((u) => `${u.id} (${u.path})`);
    expect([...new Set(missing)]).toEqual([]);
  });

  it('an id is defined in exactly one en-GB file', () => {
    const twice = [...definedIds()].filter(([, files]) => files.length > 1).map(([id, files]) => `${id}: ${files.join(', ')}`);
    expect(twice).toEqual([]);
  });

  it('the shell file defines the words the shell draws', () => {
    expect(Object.keys(ftlFiles).some((p) => p.endsWith('/screens.ftl'))).toBe(true);
  });
});
