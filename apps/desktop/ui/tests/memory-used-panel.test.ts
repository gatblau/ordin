// @vitest-environment happy-dom
// What Ordin used, drawn in a DOM after the person chooses it: a fact from a page names the page, and a
// page that is gone reads `memory-page-gone` with no Open the page (ScreensModule.md SM-CU-11).
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import MemoryUsedPanel from '../src/lib/views/fixed/MemoryUsedPanel.svelte';
import { backendWith, WS } from './fixed-support';
import { err, ok, settle, type Reply } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const fact = (id: string, over: Record<string, unknown> = {}) => ({ id, statement: `Fact ${id}`, kind: 'known', true_from: '2026-01-10T00:00:00Z', written_at: '2026-01-10T00:00:00Z', came_from: { kind: 'person' }, fading: false, may_be_out_of_date: false, needs_check: false, ...over });

let drawn: { target: HTMLElement; part: ReturnType<typeof mount> } | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn.part);
  drawn?.target.remove();
  drawn = null;
});

async function opened(pageReply: Reply): Promise<string> {
  const { state } = await backendWith({
    setup: (tr) =>
      tr
        .on('agents.list', ok([]))
        .on('memory.used', ok({ facts: [fact('a', { page_id: uid('P'), page_section: 'Contract terms' })], links: [], gone: 0 }))
        .on('knowledge.get', pageReply),
  });
  const target = document.createElement('div');
  document.body.appendChild(target);
  const part = mount(MemoryUsedPanel, { target, props: { backend: state, workspaceId: WS, recallIds: [uid('R1')], onopenpage: () => undefined } });
  drawn = { target, part };
  flushSync();
  Array.from(target.querySelectorAll('button')).find((b) => b.textContent?.trim() === 'What Ordin used')?.click();
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
  return (target.textContent ?? '').replace(/[⁨⁩]/g, '');
}

describe('SM-CU-11 — What Ordin used names the page of a fact', () => {
  it('Happy path — the page is read by its id and drawn as "Supplier Borealis › Contract terms", with Open the page', async () => {
    const out = await opened(ok({ page: { id: uid('P'), title: 'Supplier Borealis' } }));
    expect(out).toContain('Supplier Borealis › Contract terms');
    expect(out).toContain('Open the page');
  });

  it('Error path — a page that was removed reads "From a page that is no longer here", with no Open the page', async () => {
    const out = await opened(err('KNOW_PAGE_NOT_FOUND', 'That page is no longer here.'));
    expect(out).toContain('From a page that is no longer here');
    expect(out).not.toContain('Open the page');
    expect(out).toContain('Fact a');
  });
});
