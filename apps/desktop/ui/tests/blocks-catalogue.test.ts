// The words of the blocks module: screens-blocks.ftl parses, defines the words the spec fixes, marks the
// safety wording, and holds every id the blocks use that the generic catalogue test cannot see (ids built
// at run time).
import { FluentBundle, FluentResource } from '@fluent/bundle';
import { describe, expect, it } from 'vitest';

const files = import.meta.glob('../../../../crates/ordin-localisation/catalogues/en-GB/*.ftl', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const mine = Object.entries(files).find(([path]) => path.endsWith('/screens-blocks.ftl'))?.[1] ?? '';
const all = Object.values(files).join('\n');
const defined = (id: string, source = all) => new RegExp(`^${id}\\s*=`, 'm').test(source);

describe('screens-blocks.ftl', () => {
  it('exists and parses with no error', () => {
    expect(mine.length).toBeGreaterThan(500);
    const bundle = new FluentBundle('en-GB');
    expect(bundle.addResource(new FluentResource(mine))).toEqual([]);
  });

  it('defines the approval card\'s fixed lines with the spec\'s wording (brand strings for ordin)', () => {
    const bundle = new FluentBundle('en-GB', { useIsolating: false });
    bundle.addResource(new FluentResource(all));
    const say = (id: string, args: Record<string, string> = {}) => {
      const m = bundle.getMessage(id);
      return m?.value ? bundle.formatPattern(m.value, { product: 'ordin', coordinator: 'Ordin', ...args }) : `MISSING ${id}`;
    };
    expect(say('approval-from-ordin')).toBe('From ordin — built from the action, not written by an agent');
    expect(say('approval-where')).toBe('Where:');
    expect(say('approval-affects')).toBe('Affects:');
    expect(say('approval-changes')).toBe('Changes:');
    expect(say('approval-reject')).toBe('Reject');
    expect(say('approval-not-now')).toBe('Not now');
    expect(say('approval-last-line')).toBe('Nothing is sent or changed outside ordin until you choose.');
    expect(say('approval-show-whole')).toBe('Show the whole message');
    expect(say('approval-verb-read-start', { verb: 'Publish' })).toBe("Publish — you've read the start");
    expect(say('approval-decided-on', { place: "Asha's laptop", time: '10:44' })).toBe("Decided on Asha's laptop, 10:44");
    expect(say('approval-decided-in', { chat: '#sales', time: '10:44' })).toBe('Decided in #sales, 10:44');
    expect(say('approval-outside-company')).toBe('Outside the company');
    expect(say('card-decided', { verb: 'Publish' })).toBe('Approved: Publish.');
    expect(say('block-cannot-show')).toBe("This part can't be shown.");
    expect(say('report-sources')).toBe('Where these numbers come from');
    expect(say('report-show-as-table')).toBe('Show as table');
    expect(say('approval-bound-version-heading', { name: 'Quote — Kestrel.md' })).toBe('Quote — Kestrel.md — the version this approval covers');
    expect(say('approval-bound-version-note')).toBe('Changes made to this file after the card was raised are not included.');
    expect(say('approval-bound-version-cut')).toBe('Only the start of this file is shown.');
    expect(say('approval-bound-version-unavailable')).toBe("ordin can't show the exact version this approval covers. Reject the card if you need to see it first.");
    expect(say('ready-kind-changed')).toBe('Changed since it was published');
  });

  it('the four approval-bound-version ids and card-decided carry the safety comment', () => {
    for (const id of ['approval-bound-version-heading', 'approval-bound-version-note', 'approval-bound-version-cut', 'approval-bound-version-unavailable', 'card-decided']) {
      expect(mine, id).toMatch(new RegExp(`^# safety\\n${id}\\s*=`, 'm'));
    }
  });

  it('no text of the SM-SF-1 words says branch, commit, push, pull, merge, repository, hash or language server', () => {
    for (const id of ['approval-bound-version-heading', 'approval-bound-version-note', 'approval-bound-version-cut', 'approval-bound-version-unavailable', 'ready-kind-changed']) {
      const line = new RegExp(`^${id}\\s*=(.*)$`, 'm').exec(mine)?.[1] ?? '';
      expect(line.toLowerCase(), id).not.toMatch(/branch|commit|push|pull|merge|repository|hash|language server/);
    }
  });

  it('holds the ids the blocks build at run time: estimate bands, progress words, titles', () => {
    for (const band of ['light', 'medium', 'heavy']) expect(defined(`block-estimate-${band}`, mine)).toBe(true);
    for (const id of ['progress-now', 'progress-step-done', 'progress-step-now', 'progress-step-next', 'progress-step-skipped', 'progress-show-what-happened', 'progress-stop-note', 'progress-count-of', 'progress-count-so-far', 'progress-no-activity']) {
      expect(defined(id, mine), id).toBe(true);
    }
    for (const id of ['block-title-preview', 'block-title-document', 'block-title-changes', 'block-title-key-figures', 'block-title-offer', 'block-title-crew-plan']) expect(defined(id, mine), id).toBe(true);
  });

  it('the words it relies on from other files are defined somewhere', () => {
    for (const id of ['card-open', 'card-waiting', 'screen-run', 'tree-documents', 'tree-agents', 'tree-email', 'tree-knowledge', 'tree-memory', 'tree-slack', 'tree-telegram', 'server-waiting-for', 'memory-kind-happened', 'memory-kind-known', 'memory-kind-summary', 'memory-cited-by-agent', 'memory-cited-by-an-agent', 'slack-message-instruction-note']) {
      expect(defined(id), id).toBe(true);
    }
  });
});
