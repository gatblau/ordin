// The words of the engineering and fixed screens: every id exists once, in screens-fixed.ftl; the safety
// wording is marked; and the vocabulary rules of the amendments hold for the en-GB source.
import { describe, expect, it } from 'vitest';

import { answerLine } from '../src/lib/views/engineering/project-settings.svelte';
import { problemsLine } from '../src/lib/views/engineering/problems';
import { KIND_MESSAGE } from '../src/lib/views/fixed/facts';
import { teamErrorMessage } from '../src/lib/views/fixed/team.svelte';

const ftl = import.meta.glob('../../../../crates/ordin-localisation/catalogues/en-GB/*.ftl', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const mine = Object.entries(ftl).find(([p]) => p.endsWith('/screens-fixed.ftl'))?.[1] ?? '';
const others = Object.entries(ftl).filter(([p]) => !p.endsWith('/screens-fixed.ftl')).map(([, c]) => c).join('\n');
const sources = import.meta.glob(['../src/lib/views/engineering/**/*.{ts,svelte}', '../src/lib/views/fixed/**/*.{ts,svelte}'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

/** Message id → its full text (continuation lines included), for the messages of one file. */
function messages(source: string): Map<string, { body: string; safety: boolean }> {
  const out = new Map<string, { body: string; safety: boolean }>();
  let id: string | null = null;
  let safety = false;
  for (const line of source.split('\n')) {
    const m = /^([a-z][a-z0-9-]*)\s*=\s?(.*)$/.exec(line);
    if (m) {
      id = m[1] as string;
      out.set(id, { body: m[2] as string, safety });
      safety = false;
    } else if (/^\s+\S/.test(line) && id !== null) {
      const cur = out.get(id) as { body: string; safety: boolean };
      cur.body += `\n${line}`;
    } else {
      safety = line.trim() === '# safety';
      if (line.trim() === '' || line.startsWith('##')) id = null;
    }
  }
  return out;
}
const defined = messages(mine);
const idsWith = (...prefixes: string[]) => [...defined].filter(([id]) => prefixes.some((p) => id.startsWith(p)));

describe('screens-fixed.ftl', () => {
  it('exists and defines no id that another en-GB file defines', () => {
    expect(defined.size).toBeGreaterThan(150);
    const elsewhere = messages(others);
    expect([...defined.keys()].filter((id) => elsewhere.has(id))).toEqual([]);
  });

  it('defines every id the engineering and fixed sources use by name or by table', () => {
    const used = new Set<string>();
    for (const [path, content] of Object.entries(sources)) {
      for (const m of content.matchAll(/\bt\(\s*['"`]([a-z][a-z0-9-]*)['"`]/g)) used.add(m[1] as string);
      void path;
    }
    for (const id of Object.values(KIND_MESSAGE)) used.add(id);
    for (const answer of [true, false, null]) {
      const l = answerLine(answer);
      used.add(l.message);
      used.add(l.button);
    }
    for (const code of ['TEAM_NOTHING_TO_SHARE', 'TEAM_REMOTE_UNREACHABLE', 'TEAM_SIGN_IN_NEEDED', 'TEAM_READ_ONLY']) used.add(teamErrorMessage(code).id);
    for (const [u, c] of [[false, 0], [false, 1], [true, 0], [true, 1]] as const) {
      for (const id of problemsLine({ task: { new_problems: c, problems_unchecked: u }, new_problems: [] } as never).messageIds) used.add(id);
    }
    const everything = new Set([...defined.keys(), ...messages(others).keys()]);
    expect([...used].filter((id) => !everything.has(id))).toEqual([]);
    expect(used.size).toBeGreaterThan(100);
  });

  it('the four-line safety wording carries the safety comment: the code-checking body', () => {
    expect(defined.get('code-checking-ask-body')?.safety).toBe(true);
  });

  it('Edge case — no Git words in the team messages (branch, commit, push, pull, merge, rebase, conflict, clone)', () => {
    const banned = /\b(branch|commit|push|pull|merge|rebase|conflict|clone)\b/i;
    expect(idsWith('team-').filter(([, m]) => banned.test(m.body)).map(([id]) => id)).toEqual([]);
  });

  it('Edge case — no forbidden words in the knowledge messages (vault, wiki, backlink, alias, graph)', () => {
    const banned = /\b(vault|wiki|backlink|alias(es)?|graph)\b/i;
    expect(idsWith('knowledge-').filter(([, m]) => banned.test(m.body)).map(([id]) => id)).toEqual([]);
  });

  it('Edge case — no store words on the memory screens', () => {
    const banned = /\b(entity|entities|relationship|graph|node|edge|episodic|semantic|consolidated|salience|decay|embedding|score)\b/i;
    expect(idsWith('memory-').filter(([, m]) => banned.test(m.body)).map(([id]) => id)).toEqual([]);
  });

  it('no text of the November amendment names a branch, commit, push, pull, merge, repository, hash or language server', () => {
    const banned = /\b(branch|commit|push|pull|merge|repository|hash|language server)\b/i;
    const nov = idsWith('changes-problems-', 'code-checking-', 'memory-held-', 'memory-citation', 'memory-cited-', 'confluence-auto-', 'memory-to-check', 'memory-tab-to-check');
    expect(nov.length).toBeGreaterThan(30);
    expect(nov.filter(([, m]) => banned.test(m.body)).map(([id]) => id)).toEqual([]);
  });

  it('the spec\'s quoted sentences are exact', () => {
    const body = (id: string) => (defined.get(id) as { body: string }).body;
    expect(body('changes-problems-none')).toBe('No new problems');
    expect(body('changes-problems-unchecked')).toBe('Not checked for new problems: some changed files had no code checking.');
    expect(body('code-checking-no')).toBe('Keep off');
    expect(body('memory-held-kept')).toBe('Kept. Ordin will use this from now on.');
    expect(body('memory-held-discarded')).toBe("Not kept. Ordin won't use this.");
    expect(body('confluence-auto-on')).toBe('Your own changes here are published to Confluence automatically.');
  });

  it('every spec-less id is in the "Chosen here" section at the end of the file', () => {
    const marker = mine.indexOf('## Chosen here');
    expect(marker).toBeGreaterThan(0);
    const chosen = messages(mine.slice(marker));
    for (const id of ['engineering-save', 'check-fix-it', 'fixed-show-more', 'team-error-unreachable', 'slack-open-in-slack']) expect(chosen.has(id), id).toBe(true);
  });
});
