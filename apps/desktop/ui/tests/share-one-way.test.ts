// SM-TW-13 and SM-TK-2 (ScreensModule.md, amended January 2026, user decision): sharing has one way in,
// the workspace's ⋯ menu (amended again January 2026), and knowledge-only sharing is no longer offered on its own anywhere.
import { describe, expect, it } from 'vitest';

const sources = import.meta.glob(['../src/**/*.ts', '../src/**/*.svelte'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const ftl = import.meta.glob('../../../../crates/ordin-localisation/catalogues/*/*.ftl', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const handWritten = Object.entries(sources).filter(([p]) => !p.includes('/service-api/'));
const source = (path: string): string => Object.entries(sources).find(([p]) => p.endsWith(`/src/lib/${path}`))?.[1] ?? '';

describe('one way into sharing', () => {
  it('Edge — no knowledge-only entry anywhere: neither its words nor its turn-on command', () => {
    for (const id of ['team-share-with-team', 'team-share-invite', 'team-share-dialog-title', 'team-share-dialog-all', 'team-share-dialog-verb', 'team-ws-share']) {
      const pattern = new RegExp(`['"\`]${id}['"\`]`);
      expect(handWritten.filter(([, s]) => pattern.test(s)).map(([p]) => p), id).toEqual([]);
      expect(Object.entries(ftl).filter(([, s]) => new RegExp(`^${id} =`, 'm').test(s)).map(([p]) => p), id).toEqual([]);
    }
    expect(handWritten.filter(([, s]) => s.includes("'team_knowledge.turn_on'")).map(([p]) => p)).toEqual([]);
  });

  it('Memory and Knowledge draw the same team panel, with no invitation of their own', () => {
    for (const screen of ['views/fixed/Memory.svelte', 'views/fixed/Knowledge.svelte']) {
      expect(source(screen), screen).toMatch(/<TeamPanel \{backend\} workspaceId=/);
      expect(source(screen), screen).not.toMatch(/<TeamPanel[^>]*\binvite\b/);
    }
  });

  // SM-TW-13 (amended January 2026, user decision): sharing is in the workspace's ⋯ menu; the row has no share control.
  it("sharing lives in the workspace's ⋯ menu, and the row's mark is not a control", () => {
    const tree = source('shell/WorkspaceTree.svelte');
    expect(tree).toContain('<WorkspaceShareMark');
    expect(tree).toContain("pickShare(share, 'sharing')");
    expect(source('shell/WorkspaceShareMark.svelte')).not.toMatch(/share-link|shared-mark/);
  });
});
