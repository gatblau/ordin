// @vitest-environment happy-dom
// A knowledge page drawn as a formatted page, and which of its links are page links (SM-KN-1). Plan 04,
// FU-089: the page "Paying suppliers" was shown as its Markdown source.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import { keptMarkdownLink, pageBlocks } from '../src/lib/views/fixed/wikilinks';
import Knowledge from '../src/lib/views/fixed/Knowledge.svelte';
import { ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const PAGE = uid('page-paying');
const OTHER = uid('page-day-rates');
const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();

const view = (body: string, linksOut: unknown[] = []) => ({
  page: { id: PAGE, title: 'Paying suppliers', body_markdown: body, last_changed_at: '2026-01-12T10:00:00Z' },
  relative_path: 'knowledge/Paying suppliers.md',
  aliases: [], tags: [], links_in: [], links_out: linksOut, links_truncated: 0, suggestions: [], unregistered_aliases: [], changes: [],
  cites_other_computer: false, read_only: false, past_stale_after: false, pending_update: false,
});

async function draw(page: ReturnType<typeof view>) {
  const b = await backendWith({ setup: (t) => t.on('knowledge.list', ok({ this_workspace: [], from_other_teams: [], from_agents: [], not_adopted: [] })).on('knowledge.get', ok(page)) });
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(Knowledge as never, { target, props: { backend: b.state, workspaceId: WS, instance: PAGE } as never });
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
  const body = target.querySelector('.page-body');
  if (body === null) throw new Error('no page body');
  const linksOut = target.querySelector('section[aria-labelledby="knowledge-links-out"] ul');
  return { ...b, target, body, linksOut };
}

describe('SM-KN-1 a knowledge page is drawn formatted', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-KN-1
  it('FU-089 — the page Ordin wrote shows a heading and a paragraph, not its source; [Rates](Rates) is words only and not listed', async () => {
    const { body, linksOut } = await draw(view("# Paying suppliers\n\nSupplier invoices are due within 30 days. Kestrel Freight's rate is in [Rates](Rates).\n"));
    expect(clean(body.querySelector('h3')?.textContent)).toBe('Paying suppliers');
    expect(clean(body.querySelector('p')?.textContent)).toBe("Supplier invoices are due within 30 days. Kestrel Freight's rate is in Rates.");
    expect(body.textContent).not.toContain('#');
    expect(body.textContent).not.toContain('](');
    expect(body.querySelectorAll('button, a, .muted-link')).toHaveLength(0);
    expect(linksOut?.children).toHaveLength(0);
  });

  it('Happy path — a Markdown link to a page file and a wiki link are links, paired with links_out in body order', async () => {
    const { body, transport } = await draw(
      view('Intro with **bold** text.\n\n- See [Day rates](Day%20rates.md#Kestrel)\n- And [[Purchasing policy]]\n\n```\n[[Not a link]]\n```\n', [
        { name: 'Day rates', section: 'Kestrel', target: { page_id: OTHER, title: 'Day rates' } },
        { name: 'Purchasing policy' },
      ]),
    );
    expect(clean(body.querySelector('strong')?.textContent)).toBe('bold');
    expect(body.querySelectorAll('li')).toHaveLength(2);
    expect(clean(body.querySelector('pre')?.textContent)).toBe('[[Not a link]]');
    const link = body.querySelector('button.link') as HTMLButtonElement;
    expect(clean(link.textContent)).toBe('Day rates');
    const missing = body.querySelector('.muted-link');
    expect(clean(missing?.textContent)).toBe('Purchasing policy');
    expect(clean(missing?.getAttribute('title'))).toBe('No page called Purchasing policy');
    transport.sent.length = 0;
    link.click();
    await settle();
    expect(transport.sentOf('knowledge.get').map((e) => e.input)).toEqual([{ page_id: OTHER }]);
  });

  it('Edge case — markup in the body stays text', async () => {
    const { body } = await draw(view('<img src=x onerror="alert(1)"> and <b>bold</b>'));
    expect(body.querySelector('img, b')).toBeNull();
    expect(body.textContent).toContain('<img src=x');
  });
});

describe('SM-KN-1 which links are page links (KnowledgeService.md §P)', () => {
  it.each([
    ['Rates', false],
    ['https://example.org/a.md', false],
    ['Day%20rates.md', true],
    ['<Day rates.md>', true],
    ['/team/Rota.md#On%20call', true],
    ['../Outside.md', false],
    ['notes.txt', false],
  ])('a Markdown link to %j is a page link: %s', (dest, kept) => {
    expect(keptMarkdownLink(dest, 'knowledge/Paying suppliers.md')).toBe(kept);
  });

  it('a link in a sub-folder may go up one level and stay under knowledge/', () => {
    expect(keptMarkdownLink('../Rates.md', 'knowledge/team/Page.md')).toBe(true);
  });

  it('wiki links with an empty target or another extension use no links_out entry', () => {
    const blocks = pageBlocks('[[#Local]] ![[Pic.png]] [[A|shown]] `[[B]]`', [{ name: 'A' }], 'knowledge/P.md');
    const para = blocks[0];
    if (para?.kind !== 'paragraph') throw new Error('expected a paragraph');
    const links = para.inlines.filter((i) => i.kind === 'page_link');
    expect(links).toEqual([{ kind: 'page_link', shown: 'shown', link: { name: 'A' } }]);
  });
});
