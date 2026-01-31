// Icons on the screens (ScreensModule.md amendment *Icons*, SM-IC-3; cross-cutting/VisualDesign.md V8
// *Where icons are drawn*, *Actions*): parts of an agent repository, the handover, the document's screen,
// the workflow screen and the block actions V8 names. Rendering is server-side with no DOM, so a screen
// that builds its own controller gets one with its state already set (the mocks below), and a place a
// component fills from an effect's reply is checked in its source.
import { beforeAll, describe, expect, it, vi } from 'vitest';

import BlockBody from '../src/lib/blocks/BlockBody.svelte';
import emailSource from '../src/lib/blocks/kinds/Email.svelte?raw';
import slackSource from '../src/lib/blocks/kinds/SlackList.svelte?raw';
import { fileKindIcon, fileKindTint, partKindIcon } from '../src/lib/icons/kinds';
import type { AppContext } from '../src/lib/shell/context';
import DocumentPage from '../src/lib/views/authoring/DocumentPage.svelte';
import RepositoryParts from '../src/lib/views/authoring/RepositoryParts.svelte';
import WithdrawPart from '../src/lib/views/authoring/WithdrawPart.svelte';
import WorkflowScreen from '../src/lib/views/authoring/WorkflowScreen.svelte';
import { HandoverController } from '../src/lib/views/handover/handover-controller.svelte';
import HandoverView from '../src/lib/views/handover/HandoverView.svelte';
import InstalledPartPageView from '../src/lib/views/parts/InstalledPartPageView.svelte';
import LibraryPartPageView from '../src/lib/views/parts/LibraryPartPageView.svelte';
import { InstalledPartPageController, LibraryPartPageController } from '../src/lib/views/parts/part-page.svelte';
import { hello, LOCAL, ok, R1_GROUPS } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';
import { uid } from './support/ids';

// The state each mocked controller starts with, set by a test before it renders the screen.
const preset = vi.hoisted(() => ({}) as Record<string, ((c: never) => void) | undefined>);

vi.mock('../src/lib/views/authoring/document.svelte', async (original) => {
  const m = await original<typeof import('../src/lib/views/authoring/document.svelte')>();
  class Preset extends m.DocumentController {
    constructor(...args: ConstructorParameters<typeof m.DocumentController>) {
      super(...args);
      preset.document?.(this as never);
    }
  }
  return { ...m, DocumentController: Preset };
});
vi.mock('../src/lib/views/authoring/workflow.svelte', async (original) => {
  const m = await original<typeof import('../src/lib/views/authoring/workflow.svelte')>();
  class Preset extends m.WorkflowController {
    constructor(...args: ConstructorParameters<typeof m.WorkflowController>) {
      super(...args);
      preset.workflow?.(this as never);
    }
  }
  return { ...m, WorkflowController: Preset };
});
vi.mock('../src/lib/views/authoring/repository.svelte', async (original) => {
  const m = await original<typeof import('../src/lib/views/authoring/repository.svelte')>();
  class Parts extends m.RepositoryPartsController {
    constructor(...args: ConstructorParameters<typeof m.RepositoryPartsController>) {
      super(...args);
      preset.repository?.(this as never);
    }
  }
  class Withdraw extends m.WithdrawController {
    constructor(...args: ConstructorParameters<typeof m.WithdrawController>) {
      super(...args);
      preset.withdraw?.(this as never);
    }
  }
  return { ...m, RepositoryPartsController: Parts, WithdrawController: Withdraw };
});

beforeAll(loadWindowCatalogue);

const plain = (markup: string) => markup.replace(/<!--[\s\S]*?-->/g, '').replace(/[⁨⁩]/g, '');
/** Each drawn icon's name, in order. */
const icons = (markup: string): string[] => [...markup.matchAll(/<use href="#i-([a-z]+)"/g)].map((m) => m[1]!);
/** An icon of `name` in `size`, as Icon.svelte draws it. */
const svg = (name: string, size = 'default') => `<svg class="icon icon-${size}" aria-hidden="true" focusable="false"><use href="#i-${name}"></use></svg>`;
/** The icon directly before a button's words. */
const iconButton = (name: string, words: string) => new RegExp(`<button[^>]*>${escape(svg(name))}\\s*${escape(words)}`);
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

const GROUPS = ['service', 'agents', 'catalogue', 'checker', 'ordin', 'files', 'home', 'craft', 'workflows', 'workspaces'];
const partView = (kind: string, name: string, over: Record<string, unknown> = {}) => ({
  from: 'Acme',
  offered: true,
  part: { id: `p-${name}`, identifier: `urn:ordin:acme:${name}`, kind, name, agent_repository_id: 'repo-1' },
  state_label: 'current',
  summary: 'What it does.',
  ...over,
});

describe('Parts of an agent repository — a part by kind (V8 `ptIco`)', () => {
  it.each([
    ['WorkspaceProfile', 'agent'],
    ['Guideline', 'md'],
    ['Rule', 'key'],
    ['OperatingMode', 'key'],
    ['Memory', 'memory'],
    ['ContextResource', 'docs'],
    ['ToolBinding', 'code'],
    ['Journey', 'run'],
    ['Skill', 'task'],
    ['Command', 'task'],
    ['Evaluation', 'task'],
  ])('%s draws i-%s', (kind, icon) => {
    expect(partKindIcon(kind)).toBe(icon);
  });

  it("Parts of [repository]: each part row draws its kind's icon before its name; an agent's in accent text; Read it draws i-eye", async () => {
    preset.repository = (c: never) => {
      const r = c as unknown as { repositories: unknown[]; entries: unknown[]; selected: unknown; usedBy: unknown };
      r.repositories = [{ agent_count: 1, repository: { id: 'repo-1', display_name: 'Acme', person_can_publish: true, format: 'git' }, set_by_company: false }];
      r.entries = [{ from: 'Acme', part: partView('Rule', 'Payments rule') }, { from: 'Acme', part: partView('WorkspaceProfile', 'Clause checker') }, { from: 'Acme', part: partView('Journey', 'Contract path') }];
      r.selected = partView('Rule', 'Payments rule');
      r.usedBy = { part_id: 'p-Payments rule', agents: [partView('WorkspaceProfile', 'Clause checker')], parts: [partView('Guideline', 'House style')] };
    };
    const { state } = await backendWith({ groups: GROUPS });
    const out = plain(html(RepositoryParts, { backend: state, workspaceId: WS, instance: 'repo-1' }));
    preset.repository = undefined;
    expect(out).toMatch(new RegExp(`<span class="icon-tile">${escape(svg('key'))}</span> <button[^>]*>Payments rule`));
    expect(out).toMatch(new RegExp(`<span class="icon-tile" data-tint="accent-ink">${escape(svg('agent'))}</span> <button[^>]*>Clause checker`));
    expect(out).toMatch(new RegExp(`<span class="icon-tile">${escape(svg('run'))}</span> <button[^>]*>Contract path`));
    expect(out).toMatch(iconButton('eye', 'Read it'));
    // Used by: an agent and a part, each by kind.
    expect(out).toContain(`<li><span class="icon-tile" data-tint="accent-ink">${svg('agent')}</span>Clause checker</li>`);
    expect(out).toContain(`<li><span class="icon-tile">${svg('md')}</span>House style</li>`);
    // Ask Ordin about this part draws i-chat.
    expect(out).toMatch(iconButton('chat', 'Ask Ordin about this part'));
  });

  it("an installed agent's part page draws the part's kind icon before its name in the heading", async () => {
    const urn = 'urn:ordin:acme:rules:payments';
    const { handle, state } = await backendWith({
      groups: GROUPS,
      setup: (tr) =>
        tr.on(
          'agents.part_page',
          ok({ line: { name: 'Payments rule', part: { id: urn, kind: 'Rule', commit: 'c', contentHash: 'h' }, state: 'in_use', what_it_does: 'x' }, plain_kind_message_id: 'part-kind-rule', text: 't' }),
        ),
    });
    const c = new InstalledPartPageController(handle, uid('a1'), urn);
    await c.load();
    const out = plain(html(InstalledPartPageView, { backend: state, workspaceId: WS, agentInstanceId: uid('a1'), c }));
    expect(out).toMatch(new RegExp(`<h2[^>]*><span class="icon-tile">${escape(svg('key'))}</span>Payments rule</h2>`));
  });

  it("a library part's page draws its kind icon in the heading and i-agent before each agent that uses it", async () => {
    const { handle, state } = await backendWith({
      groups: GROUPS,
      setup: (tr) => tr.on('catalogue.part', ok(partView('WorkspaceProfile', 'Clause checker'))).on('catalogue.used_by', ok({ agents: [partView('WorkspaceProfile', 'Tender lead')], part_id: uid('p1'), parts: [] })),
    });
    const c = new LibraryPartPageController(handle, uid('p1'));
    await c.load();
    const out = plain(html(LibraryPartPageView, { backend: state, workspaceId: WS, c }));
    expect(out).toMatch(new RegExp(`<h2[^>]*><span class="icon-tile" data-tint="accent-ink">${escape(svg('agent'))}</span>Clause checker</h2>`));
    expect(out).toContain(`<li><span class="icon-tile" data-tint="accent-ink">${svg('agent')}</span>Tender lead</li>`);
  });

  it('Withdraw [part]: each agent that uses it draws i-agent in accent text', async () => {
    preset.withdraw = (c: never) => {
      const w = c as unknown as { part: unknown; usedBy: unknown };
      w.part = partView('Rule', 'Payments rule');
      w.usedBy = { part_id: uid('p1'), agents: [partView('WorkspaceProfile', 'Clause checker')], parts: [] };
    };
    const { state } = await backendWith({ groups: GROUPS });
    const out = plain(html(WithdrawPart, { backend: state, workspaceId: WS, instance: uid('p1') }));
    preset.withdraw = undefined;
    expect(out).toContain(`<li><span class="icon-tile" data-tint="accent-ink">${svg('agent')}</span>Clause checker</li>`);
  });
});

describe('File kinds (V8 `KIND_ICO`, by extension)', () => {
  it.each([
    [['a.doc', 'a.DOCX', 'a.odt', 'a.rtf'], 'word', 'word'],
    [['a.xls', 'a.xlsx', 'a.ods', 'a.csv'], 'sheet', 'sheet'],
    [['a.ppt', 'a.pptx', 'a.odp'], 'slides', 'slides'],
    [['a.pdf'], 'pdf', 'pdf'],
    [['a.md', 'a.markdown'], 'md', undefined],
    [['a.png', 'a.jpg', 'a.jpeg', 'a.gif', 'a.webp', 'a.svg'], 'image', 'image'],
  ])('%j draw i-%s, tinted %s', (names, icon, tint) => {
    for (const name of names) expect(fileKindIcon(name), name).toBe(icon);
    expect(fileKindTint(icon as never)).toBe(tint);
  });

  it('a name with no or an unknown extension names no kind; i-page is tinted as Confluence', () => {
    expect(fileKindIcon('README')).toBeNull();
    expect(fileKindIcon('a.zip')).toBeNull();
    expect(fileKindTint('page')).toBe('confluence');
  });
});

describe('Handover (V8 `hoScreen`)', () => {
  const screen = {
    actions: ['accept_and_start', 'return_to', 'ask_ordin', 'close'],
    arrived: null,
    handover: {
      checks_passed: [],
      events: [{ at: '2026-01-16T10:00:00Z', by: 'Ana', kind: 'sent' }],
      id: uid('h1'),
      note: { sections: [] },
      receiver: { address: 'bo@example.org', name: 'Bo', uses_ordin: true },
      sender: 'Ana',
      stage_name: 'Review',
      state: 'sent',
      title: 'Supplier contract',
    },
    marked_lines: [],
    results: [
      { label: 'Rates.xlsx', link: 'x' },
      { label: 'Contract.pdf', link: 'y' },
      { label: 'Notes', link: 'z' },
    ],
    slack: [{ destination_line: '#contracts', state: 'posted', permalink: 'https://acme.slack.com/archives/C1/p1' }],
    still_under_way: [],
  };

  async function drawn(): Promise<string> {
    const { handle, state } = await backendWith({ setup: (tr) => tr.on('handover.open', ok(screen)) });
    const c = new HandoverController(handle, uid('h1'));
    await c.load();
    return plain(html(HandoverView, { backend: state, workspaceId: WS, c }));
  }

  it("a resource handed over draws its file kind's icon, tinted: i-sheet, i-pdf, else i-word", async () => {
    const out = await drawn();
    expect(out).toContain(`<li><span class="icon-tile" data-tint="sheet">${svg('sheet')}</span>Rates.xlsx`);
    expect(out).toContain(`<li><span class="icon-tile" data-tint="pdf">${svg('pdf')}</span>Contract.pdf`);
    expect(out).toContain(`<li><span class="icon-tile" data-tint="word">${svg('word')}</span>Notes`);
  });

  it('the lifecycle stage draws i-run (Small)', async () => {
    expect(await drawn()).toMatch(new RegExp(`<p>${escape(svg('run', 'small'))} Stage: Review</p>`));
  });

  it('Return to [sender]… draws i-send, Open in Slack i-open and Ask Ordin about this i-chat', async () => {
    const out = await drawn();
    expect(out).toMatch(iconButton('send', 'Return to Ana…'));
    expect(out).toMatch(iconButton('open', 'Open in Slack'));
    expect(out).toMatch(iconButton('chat', 'Ask Ordin about this'));
    // Actions the table does not list draw none.
    expect(out).toMatch(/<button[^>]*>Accept and start<\/button>/);
    expect(out).toMatch(/<button[^>]*>Close<\/button>/);
  });
});

describe("A document's screen (V8 `md`)", () => {
  const PLACE = '0192f200-0000-7000-8000-000000000001';
  const copy = (id: string, format: string, audience: string) => ({ id, format, audience, keep_up_to_date: false });

  async function drawn(path: string, copies: unknown[] = []): Promise<string> {
    preset.document = (c: never) => {
      const d = c as unknown as { preview: unknown; copies: unknown[] };
      d.preview = { kind: 'text', markdown: true, text: '# Rates', truncated: false };
      d.copies = copies;
    };
    const { state } = await backendWith({ groups: GROUPS });
    const out = plain(html(DocumentPage, { backend: state, workspaceId: WS, instance: `${PLACE}:${path}` }));
    preset.document = undefined;
    return out;
  }

  it("the heading draws the file kind's icon in the 44 px heading size, tinted by kind", async () => {
    expect(await drawn('Rates/rates.md')).toMatch(new RegExp(`<span class="icon-tile">${escape(svg('md', 'heading'))}</span> <h2[^>]*>rates.md</h2>`));
    expect(await drawn('Rates/rates.xlsx')).toMatch(new RegExp(`<span class="icon-tile" data-tint="sheet">${escape(svg('sheet', 'heading'))}</span> <h2`));
    expect(icons(await drawn('Rates/README'))).not.toContain('page');
  });

  // Amended January 2026 (user decision): the page has no More actions, Open in its app or Ask Ordin about this;
  // they are in the document's ⋯ menu in the tree (tree-menus.test.ts). Edit (not in the Actions table) draws none.
  it('the page draws no More actions, no Open in its app and no Ask; Edit draws no icon', async () => {
    const out = await drawn('Rates/rates.md');
    expect(out).not.toContain('⋯');
    expect(out).not.toContain('<summary aria-label="More actions');
    expect(out).not.toMatch(iconButton('open', 'Open in its app'));
    expect(out).not.toMatch(iconButton('chat', 'Ask Ordin about this'));
    expect(out).toMatch(/<button[^>]*>Edit<\/button>/);
  });

  it("a copy kept of the document draws its file kind's icon (Small), else i-page; an exact copy is the original's kind", async () => {
    const out = await drawn('Rates/rates.md', [copy('c1', 'pdf', 'Board'), copy('c2', 'page', 'Team'), copy('c3', 'exact_file', 'Archive'), copy('c4', 'slides', 'Deck')]);
    expect(out).toContain(`<li><span class="icon-tile" data-tint="pdf">${svg('pdf', 'small')}</span>Board`);
    expect(out).toContain(`<li><span class="icon-tile" data-tint="confluence">${svg('page', 'small')}</span>Team`);
    expect(out).toContain(`<li><span class="icon-tile">${svg('md', 'small')}</span>Archive`);
    expect(out).toContain(`<li><span class="icon-tile" data-tint="slides">${svg('slides', 'small')}</span>Deck`);
  });
});

describe('The workflow screen (V8 `flow`)', () => {
  it('each agent draws i-agent in accent text; Run… i-play; the other actions in the ⋯ menu; Restore none', async () => {
    preset.workflow = (c: never) => {
      (c as unknown as { view: unknown }).view = {
        agents: [{ key: 'analyst', presence: 'present' }],
        file: { agents: [{ id: 'urn:ordin:a', key: 'analyst', name: 'Analyst', repository: {}, role: 'r', version: '1' }], format: 3, inputs: [], limits: {}, name: 'Month end', origin: 'Saved from the run of 13 Jan', purpose: '', steps: [] },
        version: 2,
        versions: [
          { version: 2, saved_at: '2026-01-13T10:00:00Z', change_note: 'b' },
          { version: 1, saved_at: '2026-01-12T10:00:00Z', change_note: 'a' },
        ],
        workflow: { state: 'saved' },
      };
    };
    const { state } = await backendWith({ groups: GROUPS });
    const out = plain(html(WorkflowScreen, { backend: state, workspaceId: WS, instance: 'wf1' }));
    preset.workflow = undefined;
    expect(out).toMatch(new RegExp(`<span class="icon-tile" data-tint="accent-ink">${escape(svg('agent'))}</span> Analyst`));
    expect(out).toMatch(iconButton('play', 'Run…'));
    // Actions in menus (11 §4): Change, Duplicate, Export, Rename and Remove sit in the ⋯ menu, closed at first.
    expect(out).toMatch(/aria-haspopup="menu"/);
    expect(out).not.toMatch(/>Duplicate…</);
    expect(out).toMatch(/<button[^>]*>Restore<\/button>/);
  });
});

describe('Block actions V8 names', () => {
  const WS2 = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
  const RUN = '0192a1c2-3344-7556-8677-8899aabbccdd';
  let app: AppContext;
  beforeAll(async () => {
    ({ app } = await appWith((tr) => tr.on('service.hello', ok(hello(LOCAL, [...R1_GROUPS, 'runs'])))));
  });
  const draw = (block: unknown) => plain(html(BlockBody, { backend: app.local, workspaceId: WS2, blockId: 'b1', block }, app));

  it('a Markdown document block: Open draws i-md and Ask Ordin about this i-chat', () => {
    const out = draw({ kind: 'document', item: { kind: 'place_file', place_id: 'p', relative_path: 'Notes/Brief.md' } });
    expect(out).toMatch(iconButton('md', 'Open'));
    expect(out).toMatch(iconButton('chat', 'Ask Ordin about this'));
  });

  it('a reference line: Open draws i-md for a Markdown document and nothing for another kind', () => {
    const ref = (p: string) => ({ kind: 'place_file', place_id: 'p', relative_path: p });
    const md = draw({ kind: 'message', text: 'x', attachments: [ref('Brief.md')] });
    expect(md).toMatch(/<button type="button" class="small[^"]*">\s*<svg class="icon icon-default" aria-hidden="true" focusable="false"><use href="#i-md"><\/use><\/svg>Open/);
    const docx = draw({ kind: 'message', text: 'x', attachments: [ref('Brief.docx')] });
    expect(docx).toMatch(/<button type="button" class="small[^"]*">Open<\/button>/);
  });

  it('memory: Open Knowledge draws i-know; Ask for more changes (not Ask Ordin about …) draws none', () => {
    expect(draw({ kind: 'memory', workspace_id: WS2, facts: [] })).toMatch(iconButton('know', 'Open Knowledge'));
    const ref = (p: string) => ({ kind: 'place_file', place_id: 'p', relative_path: p });
    expect(draw({ kind: 'changes', before: ref('a.md'), after: ref('b.md'), view: 'text_differences' })).toMatch(/<button[^>]*>\s*Ask for more changes\s*<\/button>/);
  });

  it('plan_progress: Stop — files written so far are kept draws i-stop', () => {
    app.local.runs.set(RUN, { state: 'running', progressLines: [], folded: 0, count: null, plan: null, startUp: [] } as never);
    expect(draw({ kind: 'plan_progress', subject: { kind: 'run', run_id: RUN } })).toMatch(iconButton('stop', 'Stop — files written so far are kept'));
  });

  // Filled from an effect's reply, so not drawn server-side: checked in the source.
  it('slack_list: Open in Slack draws i-open', () => {
    expect(slackSource).toContain(`<Icon name="open" />{t('block-open-in-slack')}`);
  });

  it('email: an attachment draws i-sheet, i-pdf or else i-word (Small), tinted, before its name', () => {
    expect(emailSource).toMatch(/kind === 'sheet' \|\| kind === 'pdf' \? kind : 'word'/);
    expect(emailSource).toContain(`<span class="icon-tile" data-tint={fileKindTint(attachmentIcon(attachment.file_name))}><Icon name={attachmentIcon(attachment.file_name)} size="small" /></span> {attachment.file_name}`);
  });
});
