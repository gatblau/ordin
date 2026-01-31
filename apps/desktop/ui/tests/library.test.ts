// @vitest-environment happy-dom
// The agent library and Add it (SM-AL-1 to SM-AL-4), mounted so its searching and previewing run.
import { mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import Library from '../src/lib/views/fixed/Library.svelte';
import { backendWith, WS } from './fixed-support';
import { err, ok, settle } from './support/fake-backend';
import { uid } from './support/ids';
import { loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const DOC = 'urn:ordin:default:workspaces:document-agent';
const DOC_ID = uid('part-document-agent');
const OTHER_ID = uid('part-cspa');

function entry(id: string, identifier: string, name: string, summary: string) {
  return {
    from: 'ordin',
    part: {
      from: 'ordin',
      offered: true,
      state_label: 'current',
      summary,
      part: { id, identifier, name, kind: 'WorkspaceProfile', version: '0.1.0', owner: 'gatblau', status: 'draft', withdrawn: false, uses: [], vocabulary_terms_cited: [], agent_repository_id: uid('repo'), commit: 'ordin-0.1.0', content_digest: '0'.repeat(64), file_path: 'models/@default/workspaces/x/workspace-profile.yaml' },
    },
  };
}
const LIST = ok({
  items: [
    entry(OTHER_ID, 'urn:ordin:default:workspaces:cspa-431', 'Software Engineer (CSPA-431)', 'Runs the design, spec, plan and apply loop.'),
    entry(DOC_ID, DOC, 'Document agent', 'Summarises, compares and drafts documents.'),
  ],
  next_cursor: null,
});
const PREVIEW = (available: unknown = { kind: 'available' }) =>
  ok({ agent_name: 'Document agent', from: 'ordin', asks: [], available, uses: [{ changeable: true, chosen_by: 'agent-default', identifier: 'urn:ordin:default:overlays:style:plain-language', kind: 'StyleOverlay', label: 'Plain language' }] });

async function draw(backend: unknown, instance: string | null): Promise<HTMLElement> {
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(Library as never, { target, props: { backend, workspaceId: WS, instance } as never });
  await settle(30);
  return target;
}

/** The text the person reads: the direction marks Fluent puts round each argument removed. */
const text = (el: Element): string => (el.textContent ?? '').replace(/[\u2068\u2069]/g, '');
const button = (target: HTMLElement, text: string) => [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);

describe('The agent library (SM-AL-1, SM-AL-2)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-AL-1
  // spec: ordin-desktop/components/ScreensModule#SM-AL-2
  it('Happy path — the proposed agent comes first, marked and chosen, with Add it', async () => {
    const { state, transport } = await backendWith({ setup: (t) => t.on('catalogue.search', LIST).on('agents.preview_add', PREVIEW()) });
    const target = await draw(state, DOC);
    const rows = [...target.querySelectorAll('li')].map((li) => text(li));
    expect(rows[0]).toContain('Document agent');
    expect(rows[0]).toContain('Ordin suggests this one');
    expect(rows[0]).toContain('From: ordin');
    expect(transport.sentOf('agents.preview_add').map((e) => e.input)).toEqual([{ workspace_id: WS, agent: DOC_ID, choices: [], interface_locale: 'en-GB' }]);
    expect(target.querySelector('#library-chosen-heading')?.textContent?.replace(/[\u2068\u2069]/g, '')).toBe('Document agent');
    expect(text(target)).toContain('Uses: Plain language');
    expect(button(target, 'Add it')).toBeDefined();
    expect(text(target)).toContain('Ask Ordin about this agent');
  });

  it('Edge case — an agent already in the workspace shows so, with no Add it', async () => {
    const { state } = await backendWith({ setup: (t) => t.on('catalogue.search', LIST).on('agents.preview_add', err('PKG_ALREADY_ADDED', 'That agent is already in this workspace.')) });
    const target = await draw(state, DOC);
    expect(text(target)).toContain('Added to this workspace.');
    expect(button(target, 'Add it')).toBeUndefined();
  });

  // spec: ordin-desktop/components/ScreensModule#SM-AL-4
  it('Edge case — an agent that needs approval offers no IT action', async () => {
    const { state } = await backendWith({ setup: (t) => t.on('catalogue.search', LIST).on('agents.preview_add', PREVIEW({ kind: 'needs_it_approval', parts: ['A tool'] })) });
    const target = await draw(state, DOC);
    expect(text(target)).toContain("This agent needs your company's approval before it can be added.");
    expect(button(target, 'Add it')).toBeUndefined();
    expect(text(target)).not.toMatch(/Ask IT|Send to IT/);
  });

  it('Edge case — with no proposal nothing is chosen until a row is', async () => {
    const { state, transport } = await backendWith({ setup: (t) => t.on('catalogue.search', LIST).on('agents.preview_add', PREVIEW()) });
    const target = await draw(state, null);
    expect(text(target)).not.toContain('Ordin suggests this one');
    expect(transport.sentOf('agents.preview_add')).toEqual([]);
    (target.querySelectorAll('li button')[0] as HTMLButtonElement).click();
    await settle(20);
    expect(transport.sentOf('agents.preview_add').map((e) => (e.input as { agent: string }).agent)).toEqual([OTHER_ID]);
  });
});

describe("ordin's own agents (SM-AL-6)", () => {
  // spec: ordin-desktop/components/ScreensModule#SM-AL-6
  it('Edge case — Ordin and the agent maker in the bundled catalogue are not listed', async () => {
    const BUNDLED = '01900000-0000-7000-8000-000000000001';
    const own = (id: string, identifier: string, name: string) => {
      const e = entry(id, identifier, name, 'Ordin itself.');
      e.part.part.agent_repository_id = BUNDLED;
      return e;
    };
    const list = ok({ items: [own(uid('part-ordin'), 'urn:ordin:default:workspaces:coordinator', 'Ordin'), own(uid('part-maker'), 'urn:ordin:default:workspaces:advisor', 'Agent maker'), entry(DOC_ID, DOC, 'Document agent', 'Summarises.')], next_cursor: null });
    const { state } = await backendWith({ setup: (t) => t.on('catalogue.search', list).on('agents.preview_add', PREVIEW()) });
    const target = await draw(state, null);
    const rows = [...target.querySelectorAll('li')].map((li) => text(li));
    expect(rows).toHaveLength(1);
    expect(rows[0]).toContain('Document agent');
  });
});

describe('Add it (SM-AL-3)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-AL-3
  it('Happy path — Add it sends the preview input and says the agent is added', async () => {
    const { state, transport } = await backendWith({
      setup: (t) => t.on('catalogue.search', LIST).on('agents.preview_add', PREVIEW()).on('agents.add', ok({ kind: 'ready', agent: { instance: { display_name: 'Document agent' }, tasks: [], uses: [], workspaces: [WS] } })),
    });
    const target = await draw(state, DOC);
    button(target, 'Add it')?.click();
    await settle(20);
    expect(transport.sentOf('agents.add').map((e) => e.input)).toEqual([{ workspace_id: WS, agent: DOC_ID, choices: [], interface_locale: 'en-GB' }]);
    expect(text(target)).toContain('Document agent is added to this workspace. Tell Ordin to go ahead in the conversation.');
    expect(button(target, 'Add it')).toBeUndefined();
  });

  it('Error path — a refused Add it shows the error line and keeps Add it', async () => {
    const { state } = await backendWith({
      setup: (t) => t.on('catalogue.search', LIST).on('agents.preview_add', PREVIEW()).on('agents.add', err('PKG_INSTALL_FAILED', 'The agent could not be set up. Try again.', { action: 'try_again' })),
    });
    const target = await draw(state, DOC);
    button(target, 'Add it')?.click();
    await settle(20);
    expect(text(target)).toContain('The agent could not be set up.');
    expect(button(target, 'Add it')).toBeDefined();
  });

  it('Edge case — an outcome that refuses the agent shows its sentence and no action', async () => {
    const { state } = await backendWith({ setup: (t) => t.on('catalogue.search', LIST).on('agents.preview_add', PREVIEW()).on('agents.add', ok({ kind: 'unsafe' })) });
    const target = await draw(state, DOC);
    button(target, 'Add it')?.click();
    await settle(20);
    expect(text(target)).toContain('This agent could not be added safely.');
    expect(button(target, 'Add it')).toBeUndefined();
  });
});
