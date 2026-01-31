// @vitest-environment happy-dom
// The screens where a person writes Markdown use the what-you-see editor (UX guidelines, Writing; amended
// January 2026, user decision), and saving sends the same commands as before: the document page
// (`files.save_edit`, SM-NS-4.6), a knowledge page's Edit (`knowledge.save_edit`) and Adopt for a page
// (`team_knowledge.adopt`, SM-NS-13). No Markdown code is drawn and no plain text box is left for the body.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-4
// spec: ordin-desktop/components/ScreensModule#SM-NS-13
import type { Editor } from '@tiptap/core';
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import DocumentPage from '../src/lib/views/authoring/DocumentPage.svelte';
import AdoptDialog from '../src/lib/views/fixed/AdoptDialog.svelte';
import Knowledge from '../src/lib/views/fixed/Knowledge.svelte';
import { TeamController } from '../src/lib/views/fixed/team.svelte';
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

const clean = (s: string | null | undefined): string => (s ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();
async function flush(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    await settle();
    flushSync();
  }
}
const press = (label: string): void => {
  // A button's accessible name: an icon button's aria-label (amended January 2026), else its words.
  const button = [...document.querySelectorAll('button')].filter((b) => (b.getAttribute('aria-label') ?? clean(b.textContent)) === label).pop();
  expect(button, `a button named ${label}`).toBeDefined();
  button?.click();
  flushSync();
};
const escape = (el: Element): void => {
  el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
  flushSync();
};
const dialog = (): HTMLElement | null => document.querySelector('[role="dialog"]');
const editorIn = (root: ParentNode): Editor => (root.querySelector('.md-surface[contenteditable="true"]') as unknown as { editor: Editor }).editor;
function typeAtEnd(root: ParentNode, words: string): void {
  const e = editorIn(root);
  e.chain().focus('end').insertContent(words).run();
  flushSync();
}

describe('the document page (SM-NS-4.6)', () => {
  const PLACE = '0192f200-0000-7000-8000-000000000001';
  const PATH = 'Rates/rates.md';
  it('Edit shows the formatted text, not its codes; Done sends files.save_edit with the Markdown', async () => {
    const { state, transport } = await backendWith({
      groups: ['files', 'home'],
      setup: (tr) =>
        tr
          .on('files.preview', ok({ kind: 'text', markdown: true, text: '# Rates\n\nOld **line**.\n', truncated: false }))
          .on('files.versions', ok([]))
          .on('home.document_opened', ok({}))
          .on('files.published_copies', ok([]))
          .on('files.save_edit', ok({ id: uid('v9'), version: 9, created_at: '2026-01-10T10:00:00Z', hash: 'h', path: PATH, place_id: PLACE, origin: { kind: 'person' } })),
    });
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(DocumentPage as never, { target, props: { backend: state, workspaceId: WS, instance: `${PLACE}:${PATH}` } as never });
    await flush();
    press('Edit');
    await flush();
    expect(target.querySelector('textarea')).toBeNull();
    const surface = target.querySelector('.md-surface[contenteditable="true"]') as HTMLElement;
    expect(surface.querySelector('h1')?.textContent).toBe('Rates');
    expect(surface.querySelector('strong')?.textContent).toBe('line');
    expect(surface.textContent).not.toMatch(/[#*]/);
    expect(target.querySelector('[role="toolbar"] [data-tool="table"]')).not.toBeNull();
    typeAtEnd(target, ' And more');
    press('Done');
    await flush();
    expect(transport.sentOf('files.save_edit')[0]?.input).toEqual({ place_id: PLACE, path: PATH, text: '# Rates\n\nOld **line**. And more\n' });
  });
});

describe("a knowledge page's Edit", () => {
  const PAGE = uid('page-summary');
  it('the body is written in the editor; Done sends knowledge.save_edit with the Markdown body as before', async () => {
    const view = {
      page: { id: PAGE, title: 'Payment terms', body_markdown: 'Suppliers are paid in **30 days**. See [[Suppliers]].', last_changed_at: '2026-01-12T10:00:00Z' },
      relative_path: 'knowledge/Payment terms.md',
      aliases: [], tags: [], links_in: [], links_out: [], links_truncated: 0, suggestions: [], unregistered_aliases: [], changes: [],
      cites_other_computer: false, read_only: false, past_stale_after: false, pending_update: false,
    };
    const { state, transport } = await backendWith({
      setup: (t) =>
        t
          .on('knowledge.list', ok({ this_workspace: [{ id: PAGE, title: 'Payment terms', tags: [], last_changed_at: '2026-01-12T10:00:00Z' }], from_other_teams: [], from_agents: [], not_adopted: [] }))
          .on('knowledge.get', ok(view))
          .on('knowledge.save_edit', ok({ unregistered_aliases: [], title_refused: false })),
    });
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(Knowledge as never, { target, props: { backend: state, workspaceId: WS, instance: PAGE } as never });
    await flush();
    press('Edit');
    await flush();
    const surface = target.querySelector('.md-surface[contenteditable="true"]') as HTMLElement;
    expect(surface.getAttribute('aria-label')).toBe('Page');
    expect(surface.querySelector('strong')?.textContent).toBe('30 days');
    // The page's details are hidden until asked for (amended January 2026); Other names stays a plain list.
    expect(target.querySelectorAll('form textarea')).toHaveLength(0);
    press("Show the page's details");
    expect(target.querySelectorAll('form textarea')).toHaveLength(1);
    typeAtEnd(target, ' Ask first.');
    press('Done');
    await flush();
    expect(transport.sentOf('knowledge.save_edit')[0]?.input).toEqual({ page_id: PAGE, title: 'Payment terms', body_markdown: 'Suppliers are paid in **30 days**. See [[Suppliers]]. Ask first.\n' });
  });
});

describe('Adopt for a page (SM-NS-13)', () => {
  it('the body is written in the editor and Adopt sends the changed body', async () => {
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('team_knowledge.adopt', ok({})) });
    const item = { kind: 'page', page_id: uid('page') } as const;
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(AdoptDialog as never, { target, props: { controller: new TeamController(handle, WS), backend: handle, item, ownFolder: 'o', fromFolder: 'f', sharedBy: 'n', title: 'Payment terms', body: 'Thirty *days*.', onclose: () => undefined } as never });
    await flush();
    expect(target.querySelector('textarea')).toBeNull();
    expect(target.querySelector('.md-surface em')?.textContent).toBe('days');
    typeAtEnd(target, ' Net.');
    press('Adopt');
    await flush();
    const sent = transport.sentOf('team_knowledge.adopt')[0]?.input as { edit?: { body?: string } } | undefined;
    expect(sent?.edit?.body).toBe('Thirty *days*. Net.\n');
  });
});

// Leaving the editor (amended January 2026, user decision; Approved by the user January 2026): Done saves
// and goes back to reading; Cancel goes back without saving; Escape is Cancel; with changes, both ask
// "Discard your changes?" (Discard / Keep editing) first.
describe('leaving the editor — the document page', () => {
  const PLACE = '0192f200-0000-7000-8000-000000000001';
  const PATH = 'Rates/rates.md';
  async function open(): Promise<{ target: HTMLElement; transport: Awaited<ReturnType<typeof backendWith>>['transport'] }> {
    const { state, transport } = await backendWith({
      groups: ['files', 'home'],
      setup: (tr) =>
        tr
          .on('files.preview', ok({ kind: 'text', markdown: true, text: '# Rates\n\nOld line.\n', truncated: false }))
          .on('files.versions', ok([]))
          .on('home.document_opened', ok({}))
          .on('files.published_copies', ok([]))
          .on('files.save_edit', ok({ id: uid('v9'), version: 9, created_at: '2026-01-10T10:00:00Z', hash: 'h', path: PATH, place_id: PLACE, origin: { kind: 'person' } })),
    });
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(DocumentPage as never, { target, props: { backend: state, workspaceId: WS, instance: `${PLACE}:${PATH}` } as never });
    await flush();
    press('Edit');
    await flush();
    return { target, transport };
  }
  const editing = (target: HTMLElement): boolean => target.querySelector('.md-surface[contenteditable="true"]') !== null;

  it('Done is the primary button; it saves, then goes back to reading', async () => {
    const { target, transport } = await open();
    const done = [...target.querySelectorAll('button')].find((b) => clean(b.textContent) === 'Done') as HTMLButtonElement;
    expect(done.getAttribute('data-default')).toBe('true');
    typeAtEnd(target, ' New.');
    press('Done');
    await flush();
    expect(transport.sentOf('files.save_edit')).toHaveLength(1);
    expect(editing(target)).toBe(false);
    expect([...target.querySelectorAll('button')].some((b) => clean(b.textContent) === 'Edit')).toBe(true);
  });

  it('Cancel and Escape with no change go straight back, saving nothing', async () => {
    const { target, transport } = await open();
    press('Cancel');
    await flush();
    expect(editing(target)).toBe(false);
    expect(dialog()).toBeNull();
    press('Edit');
    await flush();
    escape(target.querySelector('.md-surface') as HTMLElement);
    await flush();
    expect(editing(target)).toBe(false);
    expect(transport.sentOf('files.save_edit')).toEqual([]);
  });

  it('Cancel with changes asks first: Keep editing stays; Escape asks too; Discard goes back without saving', async () => {
    const { target, transport } = await open();
    typeAtEnd(target, ' New.');
    press('Cancel');
    await flush();
    expect(clean(dialog()?.querySelector('h2')?.textContent)).toBe('Discard your changes?');
    expect([...(dialog()?.querySelectorAll('button') ?? [])].map((b) => clean(b.textContent))).toEqual(['Discard', 'Keep editing']);
    press('Keep editing');
    await flush();
    expect(dialog()).toBeNull();
    expect(editing(target)).toBe(true);
    escape(target.querySelector('.md-surface') as HTMLElement);
    await flush();
    expect(clean(dialog()?.querySelector('h2')?.textContent)).toBe('Discard your changes?');
    press('Discard');
    await flush();
    expect(editing(target)).toBe(false);
    expect(transport.sentOf('files.save_edit')).toEqual([]);
  });
});

describe("leaving the editor — a knowledge page's Edit, with its kind and summary", () => {
  const PAGE = uid('page-okf');
  // An Open Knowledge Format page: its header (kind, summary, tags, sources, who wrote and checked it,
  // keys ordin does not know) stays in the file; the screen holds the body and the fields it shows.
  const BODY = '## Late fees\n\nSee [[Supplier Borealis]] and [[Rates|the rates]].\n\n- Read [the policy](policy.md)\n- Or [the site](https://example.com/terms)\n';
  async function open(extra: Record<string, unknown> = {}): Promise<{ target: HTMLElement; transport: Awaited<ReturnType<typeof backendWith>>['transport'] }> {
    const view = {
      page: { id: PAGE, title: 'Payment terms', body_markdown: BODY, last_changed_at: '2026-01-12T10:00:00Z', sources: [{ origin: 'place', location: 'place:x:terms.pdf', version_read: '1' }] },
      relative_path: 'knowledge/Payment terms.md',
      aliases: [], tags: ['finance'], links_in: [], links_out: [], links_truncated: 0, suggestions: [], unregistered_aliases: [], changes: [], verified: [],
      cites_other_computer: false, read_only: false, past_stale_after: false, pending_update: false,
      type: 'How-to', description: 'How to agree payment terms with a supplier.',
      ...extra,
    };
    const { state, transport } = await backendWith({
      setup: (t) =>
        t
          .on('knowledge.list', ok({ this_workspace: [], from_other_teams: [], from_agents: [], not_adopted: [] }))
          .on('knowledge.get', ok(view))
          .on('knowledge.save_edit', ok({ unregistered_aliases: [], title_refused: false })),
    });
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(Knowledge as never, { target, props: { backend: state, workspaceId: WS, instance: PAGE } as never });
    await flush();
    press('Edit');
    await flush();
    press("Show the page's details");
    await flush();
    return { target, transport };
  }
  const field = (target: HTMLElement, label: string): HTMLInputElement | HTMLSelectElement =>
    ([...target.querySelectorAll('form label')].find((l) => clean(l.textContent).startsWith(label))?.querySelector('input, select') ?? null) as HTMLInputElement | HTMLSelectElement;

  it('the kind of page and the one-sentence summary are shown with plain labels, as the page has them', async () => {
    const { target } = await open();
    const kind = field(target, 'Kind of page') as HTMLSelectElement;
    expect([...kind.options].map((o) => o.textContent)).toEqual(['Explanation', 'How-to', 'Decision', 'Reference', 'Glossary']);
    expect(kind.value).toBe('How-to');
    expect(field(target, 'Summary in one sentence').value).toBe('How to agree payment terms with a supplier.');
  });

  it('a kind another tool wrote is offered as written and kept when not changed', async () => {
    const { target, transport } = await open({ type: 'Supplier Profile' });
    const kind = field(target, 'Kind of page') as HTMLSelectElement;
    expect(kind.value).toBe('Supplier Profile');
    press('Done');
    await flush();
    expect(transport.sentOf('knowledge.save_edit')[0]?.input).not.toHaveProperty('type');
  });

  it('OKF round trip: page → editor → Done → the body comes back with its links, and the header fields are left to the file', async () => {
    const { target, transport } = await open();
    // The body shown formatted, its links as links.
    const surface = target.querySelector('.md-surface[contenteditable="true"]') as HTMLElement;
    expect(surface.querySelector('h2')?.textContent).toBe('Late fees');
    expect(surface.textContent).toContain('[[Supplier Borealis]]');
    press('Done');
    await flush();
    // Nothing changed: the same body, and no kind, summary, tags or other names sent, so the file's
    // header (and every key in it) is written back as it was (ordin-knowledge okf tests).
    expect(transport.sentOf('knowledge.save_edit')[0]?.input).toEqual({ page_id: PAGE, title: 'Payment terms', body_markdown: BODY });
    expect(target.querySelector('form')).toBeNull();
  });

  it('a changed body keeps its [[Page]] links and links; a changed kind and summary are sent; Done goes back', async () => {
    const { target, transport } = await open();
    typeAtEnd(target, 'Ask first.');
    const kind = field(target, 'Kind of page') as HTMLSelectElement;
    kind.value = 'Decision';
    kind.dispatchEvent(new Event('change', { bubbles: true }));
    const summary = field(target, 'Summary in one sentence') as HTMLInputElement;
    summary.value = 'Why suppliers are paid in thirty days.';
    summary.dispatchEvent(new Event('input', { bubbles: true }));
    flushSync();
    press('Done');
    await flush();
    const sent = transport.sentOf('knowledge.save_edit')[0]?.input as Record<string, string>;
    expect(sent.type).toBe('Decision');
    expect(sent.description).toBe('Why suppliers are paid in thirty days.');
    // Written again by the editor: the heading, the [[Page]] links (with their shown words), the list and
    // both links come back exactly as they were, with the new paragraph after them.
    expect(sent.body_markdown).toBe(`${BODY}\nAsk first.\n`);
    expect(sent).not.toHaveProperty('tags');
    expect(target.querySelector('form')).toBeNull();
  });

  it('Cancel with no change goes back; Escape with a change asks, and Discard goes back without saving', async () => {
    const { target, transport } = await open();
    press('Cancel');
    await flush();
    expect(target.querySelector('form')).toBeNull();
    expect(dialog()).toBeNull();
    press('Edit');
    await flush();
    typeAtEnd(target, ' More.');
    escape(target.querySelector('.md-surface') as HTMLElement);
    await flush();
    expect(clean(dialog()?.querySelector('h2')?.textContent)).toBe('Discard your changes?');
    press('Keep editing');
    await flush();
    expect(target.querySelector('form')).not.toBeNull();
    press('Cancel');
    await flush();
    press('Discard');
    await flush();
    expect(target.querySelector('form')).toBeNull();
    expect(transport.sentOf('knowledge.save_edit')).toEqual([]);
  });
});

describe('leaving the editor — Adopt for a page', () => {
  async function open(): Promise<{ target: HTMLElement; closed: number[] }> {
    const { handle } = await backendWith({ setup: (tr) => tr.on('team_knowledge.adopt', ok({})) });
    const closed: number[] = [];
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(AdoptDialog as never, { target, props: { controller: new TeamController(handle, WS), backend: handle, item: { kind: 'page', page_id: uid('page') }, ownFolder: 'o', fromFolder: 'f', sharedBy: 'n', title: 'Payment terms', body: 'Thirty *days*.', onclose: () => closed.push(1) } as never });
    await flush();
    return { target, closed };
  }

  it('Cancel and Escape with no change close it at once', async () => {
    const { target, closed } = await open();
    press('Cancel');
    escape(target.querySelector('.md-surface') as HTMLElement);
    expect(closed).toHaveLength(2);
    expect(dialog()).toBeNull();
  });

  it('with a change, Escape asks first; Keep editing stays; Discard closes', async () => {
    const { target, closed } = await open();
    typeAtEnd(target, ' Net.');
    escape(target.querySelector('.md-surface') as HTMLElement);
    await flush();
    expect(clean(dialog()?.querySelector('h2')?.textContent)).toBe('Discard your changes?');
    press('Keep editing');
    await flush();
    expect(closed).toEqual([]);
    press('Cancel');
    await flush();
    press('Discard');
    expect(closed).toHaveLength(1);
  });
});
