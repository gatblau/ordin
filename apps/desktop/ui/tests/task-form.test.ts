// @vitest-environment happy-dom
// A task's form (SM-TF-1 to SM-TF-8): opened from the tree, drawn from `agents.list`, checked before it is
// sent, and started with `runs.start_task`; and the form Ordin opened, drawn by the same form.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import BlockBody from '../src/lib/blocks/BlockBody.svelte';
import { APP_KEY, type AppContext } from '../src/lib/shell/context';
import AgentsTreeGroup from '../src/lib/shell/AgentsTreeGroup.svelte';
import { ScreenAreaState } from '../src/lib/shell/screen-area.svelte';
import TaskFormScreen from '../src/lib/views/tasks/TaskFormScreen.svelte';
import { err, hello, LOCAL, ok, R1_GROUPS, settle, type FakeTransport, type Reply } from './support/fake-backend';
import { uid } from './support/ids';
import { appWith, loadWindowCatalogue } from './support/ssr';

beforeAll(loadWindowCatalogue);

let drawn: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (drawn !== null) void unmount(drawn);
  drawn = null;
  document.body.innerHTML = '';
});

const WS = uid('ws');
const AGENT = uid('agent');
const DOCS = uid('place');
const REPO = uid('repository-place');
const RUN = uid('run');
const SUMMARISE = 'urn:ordin:default:commands:documents:summarise-document';
const COMPARE = 'urn:ordin:default:commands:documents:compare-documents';
const DRAFT = 'urn:ordin:default:commands:documents:draft-document';
const APPLY = 'urn:ordin:default:commands:lifecycle:apply';
const COUNT = 'urn:x:count';
const text = (el: Element | null): string => (el?.textContent ?? '').replace(/[⁨⁩]/g, '').replace(/\s+/g, ' ').trim();

const input = (name: string, label: string, kind: string, extra: Record<string, unknown> = {}) => ({ name, label, kind, required: true, choices: [], hint: null, max: null, default: null, whole: false, ...extra });
const FORMS = [
  {
    command_id: SUMMARISE,
    title: 'Summarise a document',
    description: 'Summarises one document for the reader you name.',
    startable: true,
    argument_label: null,
    inputs: [
      input('document', 'Document to summarise', 'document'),
      input('reader', 'Who it is for', 'short_text', { required: false, hint: 'For example the board.' }),
      input('length', 'Length', 'choice', { choices: ['A few lines', 'One page', 'Two to three pages'], default: 'One page' }),
      input('focus', 'Anything to bring out', 'long_text', { required: false }),
    ],
  },
  {
    command_id: COMPARE,
    title: 'Compare documents',
    description: 'Compares two documents.',
    startable: true,
    argument_label: null,
    inputs: [input('reference', 'Earlier or reference document', 'document'), input('other', 'Document to compare with it', 'document')],
  },
  {
    command_id: DRAFT,
    title: 'Draft a document',
    description: 'Drafts a new document.',
    startable: true,
    argument_label: null,
    inputs: [input('request', 'What the document should be', 'long_text'), input('sources', 'Documents to build on', 'documents', { required: false, max: 3 })],
  },
  {
    command_id: APPLY,
    title: 'Apply a plan phase',
    description: 'Applies one phase of a plan.',
    startable: true,
    argument_label: null,
    inputs: [input('phase', 'Phase number', 'number'), input('due', 'Due', 'date', { required: false })],
  },
  {
    command_id: COUNT,
    title: 'Update the plan index',
    description: 'Updates one plan.',
    startable: true,
    argument_label: null,
    inputs: [input('plan', 'Plan number', 'number', { whole: true })],
  },
  { command_id: 'urn:x:rca', title: 'Rca', description: 'Finds a cause.', startable: true, argument_label: 'error', inputs: null },
  { command_id: 'urn:x:broken', title: 'Compare quotes', description: '', startable: false, argument_label: null, inputs: null },
];
const installed = { instance: { id: AGENT, display_name: 'Document agent' }, tasks: FORMS.map((f) => f.title), task_forms: FORMS, uses: [], workspaces: [WS] };

// A linked code repository "ordin" holding docs/wip/plan/03-nats-key-rotation.md, and a Jira issue (no files).
const LINK_REPO = { id: uid('link'), workspace_id: WS, kind: 'git_repository_folder', display_name: 'ordin', origin_location: '/src/ordin', place_id: REPO, state: 'active', added_at: '2026-01-10T10:00:00.000Z' };
const LINK_JIRA = { id: uid('link'), workspace_id: WS, kind: 'jira_issue', display_name: 'KEY-1', origin_location: 'KEY-1', place_id: null, state: 'active', added_at: '2026-01-10T10:00:00.000Z' };
const folderEntry = (path: string) => ({ name: path.replace(/\/$/, '').split('/').pop(), path, is_folder: true });
const fileEntry = (path: string) => ({ name: path.split('/').pop(), path, is_folder: false });
const REPO_TREE: Record<string, unknown[]> = {
  '': [folderEntry('docs/'), fileEntry('README.md')],
  'docs/': [folderEntry('docs/wip/')],
  'docs/wip/': [folderEntry('docs/wip/plan/')],
  'docs/wip/plan/': [fileEntry('docs/wip/plan/03-nats-key-rotation.md')],
};

async function flush(): Promise<void> {
  for (let i = 0; i < 6; i++) {
    await settle();
    flushSync();
  }
}

const scope = (): string => ScreenAreaState.scope(LOCAL, WS);
const formKey = (command: string): string => `task-form:${AGENT}:${command}`;

async function backend(setup: (t: FakeTransport) => void = () => undefined, groups = [...R1_GROUPS, 'runs']): Promise<{ app: AppContext; transport: FakeTransport }> {
  const { app, transport } = await appWith((t) => {
    t.on('service.hello', ok(hello(LOCAL, groups)))
      .on('agents.list', ok([installed]))
      .on('workspaces.grants', ok([]))
      .on('workspaces.links', ok([LINK_REPO, LINK_JIRA]))
      .on('files.list_folder', (raw) => {
        const i = raw as { place_id: string; folder: string };
        if (i.place_id === REPO) return { kind: 'ok', result: { items: REPO_TREE[i.folder] ?? [], next_cursor: null } } as never;
        return { kind: 'ok', result: { items: [{ name: 'strategy.pdf', path: 'plans/strategy.pdf', is_folder: false }, { name: 'q2.pdf', path: 'q2.pdf', is_folder: false }, { name: 'notes.md', path: 'notes.md', is_folder: false }], next_cursor: null } } as never;
      });
    setup(t);
  });
  app.local.trees.set(WS, { places: [{ id: DOCS, friendly_name: 'Docs', kind: 'workspace_folder', path: '/docs', added_at: '2026-01-10T10:00:00.000Z' }] } as never);
  transport.sent.length = 0;
  return { app, transport };
}

async function drawForm(command: string, setup: (t: FakeTransport) => void = () => undefined) {
  const { app, transport } = await backend(setup);
  app.screens.open(scope(), formKey(command));
  const target = document.createElement('div');
  document.body.appendChild(target);
  drawn = mount(TaskFormScreen as never, { target, props: { backend: app.local, workspaceId: WS, instance: `${AGENT}:${command}` } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
  await flush();
  return { app, transport, target };
}

const button = (target: HTMLElement, label: string): HTMLButtonElement => {
  const found = [...target.querySelectorAll('button')].find((b) => text(b) === label || text(b).includes(label) || b.getAttribute('aria-label') === label);
  expect(found, `a button named ${label}`).toBeDefined();
  return found as HTMLButtonElement;
};
const field = (target: HTMLElement, name: string): HTMLElement => target.querySelector(`[data-field="${name}"]`) as HTMLElement;
const labels = (target: HTMLElement): string[] => [...target.querySelectorAll('.label')].map(text);
const dialog = (): HTMLElement | null => document.querySelector('[role="dialog"]');
/** Opens the field's document browser, opens `place`, then each folder of `folders`, and chooses `path`. */
async function choose(target: HTMLElement, name: string, path: string, place = DOCS, folders: string[] = []): Promise<void> {
  const opener = field(target, name);
  (opener.tagName === 'BUTTON' ? opener : (opener.querySelector('button') as HTMLElement)).click();
  await flush();
  (dialog()?.querySelector(`[data-place="${place}"]`) as HTMLElement).click();
  await flush();
  for (const f of folders) {
    const row = [...(dialog()?.querySelectorAll('button') ?? [])].find((b) => text(b) === f) as HTMLElement;
    row.click();
    await flush();
  }
  (dialog()?.querySelector(`[data-path="${CSS.escape(path)}"]`) as HTMLElement).click();
  await flush();
}
async function type(target: HTMLElement, name: string, value: string): Promise<void> {
  const el = field(target, name) as HTMLInputElement;
  el.value = value;
  el.dispatchEvent(new Event('input', { bubbles: true }));
  await flush();
}
async function press(target: HTMLElement, label: string): Promise<void> {
  button(target, label).click();
  await flush();
}

describe("A task's form (SM-TF-1 to SM-TF-8)", () => {
  // spec: ordin-desktop/components/ScreensModule#SM-AT-2
  // spec: ordin-desktop/components/ScreensModule#SM-TF-1
  it('Happy path — a task from the tree opens its form', async () => {
    const { app, transport } = await backend();
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(AgentsTreeGroup as never, { target, props: { backend: app.local, workspaceId: WS, initiallyOpen: true } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    await flush();
    await press(target, 'Document agent');
    transport.sent.length = 0;
    await press(target, 'Summarise a document');
    expect(app.screens.tabs(scope()).map((t) => t.key)).toContain(formKey(SUMMARISE));
    expect(app.screens.front(scope())).toBe(formKey(SUMMARISE));
    expect(transport.commandsSent()).toEqual([]);
    void unmount(drawn);
    document.body.innerHTML = '';
    const form = await drawForm(SUMMARISE);
    expect(labels(form.target)).toEqual(['Document to summarise (required)', 'Who it is for (optional)', 'Length (required)', 'Anything to bring out (optional)']);
    // SM-TF-3: one button opens the browser; the hint sits below the label, above its field.
    expect(text(field(form.target, 'document'))).toBe('Choose a document…');
    const reader = form.target.querySelector('[data-field="reader"]')?.closest('.field');
    const parts = [...(reader?.children ?? [])];
    expect(parts[0]?.classList.contains('label')).toBe(true);
    expect(parts[1]?.classList.contains('hint')).toBe(true);
    expect(parts[2]?.getAttribute('data-field')).toBe('reader');
    const checked = form.target.querySelector<HTMLInputElement>('input[type="radio"]:checked');
    expect(checked?.value).toBe('One page');
    expect(form.transport.commandsSent().filter((c) => c !== 'agents.list' && c !== 'workspaces.grants' && c !== 'workspaces.links')).toEqual([]);
    // The cursor is in the first field without a value; a hint is joined to its field.
    expect(document.activeElement).toBe(field(form.target, 'document'));
    expect(field(form.target, 'reader').getAttribute('aria-describedby')).toContain('hint');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TF-5
  it('Error — a required document left empty', async () => {
    const { target, transport } = await drawForm(SUMMARISE);
    await press(target, 'Summarise a document');
    expect(text(target)).toContain('Choose a document to start.');
    expect(field(target, 'document').getAttribute('aria-invalid')).toBe('true');
    expect(document.activeElement).toBe(field(target, 'document'));
    expect(transport.sentOf('runs.start_task')).toHaveLength(0);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TF-4
  // spec: ordin-desktop/components/ScreensModule#SM-TF-6
  it('Happy path — starting opens the run', async () => {
    const { app, target, transport } = await drawForm(SUMMARISE, (t) => t.on('runs.start_task', ok({ kind: 'started', run_id: RUN })));
    await choose(target, 'document', 'plans/strategy.pdf');
    expect(text(target)).toContain('strategy.pdf, in plans of Docs');
    expect(dialog()).toBeNull();
    expect(text(target)).toContain('When this task starts, Document agent can read Docs, for this task and later ones.');
    await press(target, 'Summarise a document');
    const sent = transport.sentOf('runs.start_task')[0]?.input as Record<string, unknown>;
    expect(sent).toEqual({
      workspace_id: WS,
      agent_instance_id: AGENT,
      command_id: SUMMARISE,
      values: [
        { name: 'document', kind: 'document', document: { kind: 'place_file', place_id: DOCS, relative_path: 'plans/strategy.pdf' } },
        { name: 'length', kind: 'choice', choice: 'One page' },
      ],
    });
    const keys = app.screens.tabs(scope()).map((t) => t.key);
    expect(keys).toContain(`run:${RUN}`);
    expect(keys).not.toContain(formKey(SUMMARISE));
    expect(app.shell.announcement.replace(/[⁨⁩]/g, '')).toBe('Started. Document agent is working on Summarise a document.');
  });

  // SM-TF-4: a folder the agent already holds a grant on is not named.
  it('Edge — no access line for a folder the agent may read', async () => {
    const { target } = await drawForm(SUMMARISE, (t) =>
      t.on('workspaces.grants', ok([{ id: uid('grant'), workspace_id: WS, place_id: DOCS, grantee: { kind: 'agent_instance', agent_instance_id: AGENT }, access: 'read', granted_at: '2026-01-10T10:00:00.000Z' }])),
    );
    await choose(target, 'document', 'plans/strategy.pdf');
    expect(text(target)).not.toContain('When this task starts');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TF-6
  it('Edge — the conversation asks first', async () => {
    const { app, target } = await drawForm(SUMMARISE, (t) => t.on('runs.start_task', ok({ kind: 'waiting', text: 'Waiting for the person to choose a pack.' })));
    await choose(target, 'document', 'plans/strategy.pdf');
    await press(target, 'Summarise a document');
    expect(text(target.querySelector('[aria-live="polite"]'))).toBe('Nothing has started yet: the conversation needs you first.');
    expect(app.screens.tabs(scope()).map((t) => t.key)).toContain(formKey(SUMMARISE));
  });

  it('Error — a document that left the workspace', async () => {
    const { target } = await drawForm(COMPARE, (t) => t.on('runs.start_task', ok({ kind: 'refused', problems: [{ name: 'reference', problem: 'document_gone', document: 'q2.pdf' }] })));
    await choose(target, 'reference', 'q2.pdf');
    await choose(target, 'other', 'plans/strategy.pdf');
    await press(target, 'Compare documents');
    const problem = document.getElementById(field(target, 'reference').getAttribute('aria-describedby') ?? '');
    expect(text(problem)).toBe("q2.pdf isn't in this workspace any more. Choose another document.");
    expect(text(target)).toContain('Earlier or reference document');
  });

  // SM-TF-5: a refused task that changed since the screen read it reads the form again.
  it('Error — the task changed since the form opened', async () => {
    const { target, transport } = await drawForm(COMPARE, (t) => t.on('runs.start_task', ok({ kind: 'refused', problems: [{ name: 'step', problem: 'unknown_input' }] })));
    await choose(target, 'reference', 'q2.pdf');
    await choose(target, 'other', 'q2.pdf');
    const reads = transport.sentOf('agents.list').length;
    await press(target, 'Compare documents');
    expect(text(target)).toContain('This task has changed since the form opened. Check the fields and try again.');
    expect(transport.sentOf('agents.list').length).toBeGreaterThan(reads);
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TF-3
  it('Edge — the client checks of a number, a date and a documents field', async () => {
    const { target, transport } = await drawForm(APPLY, (t) => t.on('runs.start_task', ok({ kind: 'started', run_id: RUN })));
    // The platform's date field holds only whole dates, so a number is the field the person can mistype.
    await type(target, 'phase', 'two');
    await press(target, 'Apply a plan phase');
    expect(text(target)).toContain('Enter a number, such as 2 or 3.5.');
    expect(field(target, 'due').getAttribute('type')).toBe('date');
    expect(field(target, 'phase').getAttribute('inputmode')).toBe('decimal');
    expect(transport.sentOf('runs.start_task')).toHaveLength(0);
    await type(target, 'phase', '2');
    await type(target, 'due', '2026-02-28');
    await press(target, 'Apply a plan phase');
    expect((transport.sentOf('runs.start_task')[0]?.input as { values: unknown }).values).toEqual([
      { name: 'phase', kind: 'text', text: '2' },
      { name: 'due', kind: 'text', text: '2026-02-28' },
    ]);
  });

  it('Edge — a documents field takes two or more, and Add stops at its most', async () => {
    const { target, transport } = await drawForm(DRAFT);
    await type(target, 'request', 'A brief for the board.');
    const add = (path: string) => choose(target, 'sources', path);
    expect(text(field(target, 'sources'))).toBe('Choose a document…');
    await add('q2.pdf');
    expect(text(field(target, 'sources'))).toBe('Add');
    expect(text(target)).toContain('1 of 3 documents');
    await press(target, 'Draft a document');
    expect(text(target)).toContain('This field takes two or more documents.');
    expect(transport.sentOf('runs.start_task')).toHaveLength(0);
    await add('plans/strategy.pdf');
    await add('q2.pdf');
    expect(text(target)).toContain('2 of 3 documents');
    await add('notes.md');
    expect(text(target)).toContain('3 of 3 documents');
    expect(button(target, 'Add').disabled).toBe(true);
    await press(target, 'Remove q2.pdf');
    expect(text(target)).toContain('2 of 3 documents');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TF-2
  it('Edge — a task without inputs has one field, and a task that cannot start says so', async () => {
    const { target } = await drawForm('urn:x:rca', (t) => t.on('runs.start_task', ok({ kind: 'started', run_id: RUN })));
    expect(labels(target)).toEqual(['error (optional)']);
    void unmount(drawn as never);
    drawn = null;
    document.body.innerHTML = '';
    const broken = await drawForm('urn:x:broken');
    expect(text(broken.target)).toContain("This task can't be started from here. Ask Ordin in the conversation instead.");
    expect(broken.target.querySelector('[data-field]')).toBeNull();
    await press(broken.target, 'Close');
    expect(broken.app.screens.tabs(scope()).map((t) => t.key)).not.toContain(formKey('urn:x:broken'));
  });

  it('Edge — a task no longer offered is gone', async () => {
    const { target } = await drawForm('urn:x:gone');
    expect(text(target)).toContain("This task isn't available any more.");
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TF-7
  it('Edge — Cancel and Escape close the form and send nothing', async () => {
    const { app, target, transport } = await drawForm(SUMMARISE);
    await press(target, 'Cancel');
    expect(app.screens.tabs(scope()).map((t) => t.key)).not.toContain(formKey(SUMMARISE));
    app.screens.open(scope(), formKey(SUMMARISE));
    field(target, 'reader').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await flush();
    expect(app.screens.tabs(scope()).map((t) => t.key)).not.toContain(formKey(SUMMARISE));
    expect(transport.sentOf('runs.start_task')).toHaveLength(0);
  });

  it('Error — an error reply is drawn under the buttons', async () => {
    const { target } = await drawForm(SUMMARISE, (t) => t.on('runs.start_task', err('OH_TASK_UNREADABLE', "This task can't be started from here.") as Reply));
    await choose(target, 'document', 'plans/strategy.pdf');
    await press(target, 'Summarise a document');
    expect(text(target)).toContain("can't be started");
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TF-8
  it('Edge — a form Ordin opened', async () => {
    const { app, transport } = await backend((t) => t.on('runs.start_task', ok({ kind: 'started', run_id: RUN })));
    const blockId = uid('block');
    const block = {
      kind: 'form',
      title: 'Compare documents',
      verb: 'Compare documents',
      target: { kind: 'agent_instance', agent_instance_id: AGENT },
      command: '/compare-documents',
      task: { agent_instance_id: AGENT, command_id: COMPARE },
      fields: [
        { name: 'reference', label: 'Earlier or reference document', required: true, input: 'document', value: { kind: 'place_file', place_id: DOCS, relative_path: 'q2.pdf' }, filled_by_ordin: true },
        { name: 'other', label: 'Document to compare with it', required: true, input: 'document', value: null, filled_by_ordin: false },
      ],
    };
    app.local.conversation.set(WS, [{ block_id: blockId, workspace_id: WS, block, seq: 1, at: 'x', waiting: true }] as never);
    app.screens.open(scope(), `block:${blockId}`);
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(BlockBody as never, { target, props: { backend: app.local, workspaceId: WS, blockId, block } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    await flush();
    expect(text(target)).toContain('q2.pdf, in Docs');
    expect(text(target.querySelector('.field'))).toContain('Filled in by');
    // Cancel closes the screen only: the card still waits.
    await press(target, 'Cancel');
    expect(app.screens.tabs(scope()).map((t) => t.key)).not.toContain(`block:${blockId}`);
    expect(app.local.conversation.get(WS)?.[0]?.waiting).toBe(true);
    await choose(target, 'other', 'plans/strategy.pdf');
    await press(target, 'Compare documents');
    expect((transport.sentOf('runs.start_task')[0]?.input as { block_id: string }).block_id).toBe(blockId);
    expect(app.local.conversation.get(WS)?.[0]?.waiting).toBe(false);
    await flush();
    expect(text(target)).toContain('Sent. The work has started.');
    expect(target.querySelector('button[type="submit"]')).toBeNull();
  });

  // spec: ordin-desktop/components/ScreensModule#SM-DIS-1
  it('Edge — Dismiss on a form Ordin opened sends a dismissal and starts nothing (SM-DIS-1)', async () => {
    const { app, transport } = await backend((t) => t.on('ordin.answer_block', ok({ accepted: true })));
    const blockId = uid('block');
    const block = {
      kind: 'form',
      title: 'Compare documents',
      verb: 'Compare documents',
      target: { kind: 'agent_instance', agent_instance_id: AGENT },
      command: '/compare-documents',
      task: { agent_instance_id: AGENT, command_id: COMPARE },
      fields: [],
    };
    app.local.conversation.set(WS, [{ block_id: blockId, workspace_id: WS, block, seq: 1, at: 'x', waiting: true }] as never);
    app.screens.open(scope(), `block:${blockId}`);
    const target = document.createElement('div');
    document.body.appendChild(target);
    drawn = mount(BlockBody as never, { target, props: { backend: app.local, workspaceId: WS, blockId, block } as never, context: new Map<unknown, unknown>([[APP_KEY, app]]) });
    await flush();
    await press(target, 'Dismiss');
    expect(transport.sentOf('ordin.answer_block').map((e) => e.input)).toEqual([
      { block_id: blockId, workspace_id: WS, answer: { kind: 'dismissed' } },
    ]);
    expect(transport.sentOf('runs.start_task')).toHaveLength(0);
    expect(app.local.conversation.get(WS)?.[0]?.waiting).toBe(false);
    expect(app.screens.tabs(scope()).map((t) => t.key)).not.toContain(`block:${blockId}`);
  });

  it('Edge — a form opened from the tree has no Dismiss (SM-DIS-1)', async () => {
    const { target } = await drawForm(SUMMARISE);
    expect([...target.querySelectorAll('button')].some((b) => text(b) === 'Dismiss')).toBe(false);
  });
});

describe('The document browser and the number field (SM-TF-3, SM-TF-9; amended January 2026)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-TF-9
  it('Happy path — a document deep in a code repository', async () => {
    const { target, transport } = await drawForm(SUMMARISE, (t) => t.on('runs.start_task', ok({ kind: 'started', run_id: RUN })));
    await press(target, 'Choose a document…');
    const rows = [...(dialog()?.querySelectorAll('[data-place]') ?? [])].map(text);
    // The workspace's own folder first, then the linked code repository; a Jira issue holds no file.
    expect(rows).toEqual(['Docs', 'ordin']);
    expect(dialog()?.getAttribute('aria-modal')).toBe('true');
    (dialog()?.querySelector('[data-place]') as HTMLElement).click();
    await flush();
    ([...(dialog()?.querySelectorAll('button') ?? [])].find((b) => text(b) === 'Cancel') as HTMLElement).click();
    await flush();
    expect(dialog()).toBeNull();
    await choose(target, 'document', 'docs/wip/plan/03-nats-key-rotation.md', REPO, ['docs', 'wip', 'plan']);
    expect(dialog()).toBeNull();
    expect(text(target)).toContain('03-nats-key-rotation.md, in docs/wip/plan of ordin');
    expect(document.activeElement).toBe(field(target, 'document'));
    expect(transport.sentOf('files.list_folder').map((e) => (e.input as { folder: string }).folder)).toEqual(expect.arrayContaining(['', 'docs/', 'docs/wip/', 'docs/wip/plan/']));
    await press(target, 'Summarise a document');
    const values = (transport.sentOf('runs.start_task')[0]?.input as { values: unknown[] }).values;
    expect(values[0]).toEqual({ name: 'document', kind: 'document', document: { kind: 'place_file', place_id: REPO, relative_path: 'docs/wip/plan/03-nats-key-rotation.md' } });
  });

  it('Edge — Choose another opens the browser again and replaces the document', async () => {
    const { target } = await drawForm(SUMMARISE);
    await choose(target, 'document', 'q2.pdf');
    await press(target, 'Choose another');
    expect(dialog()).not.toBeNull();
    (dialog()?.querySelector(`[data-place="${DOCS}"]`) as HTMLElement).click();
    await flush();
    (dialog()?.querySelector('[data-path="notes.md"]') as HTMLElement).click();
    await flush();
    expect(text(target)).toContain('notes.md, in Docs');
    expect(text(target)).not.toContain('q2.pdf, in Docs');
  });

  it('Happy path — searching by file name', async () => {
    const { target, transport } = await drawForm(SUMMARISE, (t) =>
      t.on('files.search', ok({ coverage: { complete: true }, hits: [
        { place_id: REPO, path: 'docs/wip/plan/03-nats-key-rotation.md', name_match: true, changed_at: '2026-01-10T10:00:00.000Z' },
        { place_id: DOCS, path: 'notes.md', name_match: false, changed_at: '2026-01-10T10:00:00.000Z', snippet: 'rotation' },
      ] })),
    );
    await press(target, 'Choose a document…');
    const box = dialog()?.querySelector('input[type="search"]') as HTMLInputElement;
    expect(document.activeElement).toBe(box);
    box.value = 'rotation';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise((done) => setTimeout(done, 300));
    await flush();
    expect(transport.sentOf('files.search')[0]?.input).toEqual({ place_ids: [DOCS, REPO], query: 'rotation', limit: 50 });
    const hits = [...(dialog()?.querySelectorAll('[data-path]') ?? [])];
    expect(hits).toHaveLength(1);
    expect(text(hits[0] ?? null)).toContain('03-nats-key-rotation.md, in docs/wip/plan of ordin');
    (hits[0] as HTMLElement).click();
    await flush();
    expect(text(target)).toContain('03-nats-key-rotation.md, in docs/wip/plan of ordin');
  });

  it('Edge — a search with no file of that name', async () => {
    const { target } = await drawForm(SUMMARISE, (t) => t.on('files.search', ok({ coverage: { complete: true }, hits: [] })));
    await press(target, 'Choose a document…');
    const box = dialog()?.querySelector('input[type="search"]') as HTMLInputElement;
    box.value = 'zzz';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise((done) => setTimeout(done, 300));
    await flush();
    expect(text(dialog())).toContain('No file has that name.');
  });

  it('Error — a folder that does not read is shown in the browser with Try again', async () => {
    const { target } = await drawForm(SUMMARISE, (t) => t.on('files.list_folder', err('FILE_PLACE_UNAVAILABLE', "That folder can't be read.") as Reply));
    await press(target, 'Choose a document…');
    (dialog()?.querySelector(`[data-place="${DOCS}"]`) as HTMLElement).click();
    await flush();
    expect(text(dialog())).toContain("That folder can't be read.");
  });

  it('Edge — Escape closes the browser only, not the form', async () => {
    const { app, target, transport } = await drawForm(SUMMARISE);
    await press(target, 'Choose a document…');
    dialog()?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await flush();
    expect(dialog()).toBeNull();
    expect(app.screens.tabs(scope()).map((t) => t.key)).toContain(formKey(SUMMARISE));
    expect(document.activeElement).toBe(field(target, 'document'));
    expect(transport.sentOf('runs.start_task')).toHaveLength(0);
  });

  it('Edge — an empty folder says so', async () => {
    const { target } = await drawForm(SUMMARISE, (t) => t.on('files.list_folder', ok({ items: [], next_cursor: null })));
    await press(target, 'Choose a document…');
    (dialog()?.querySelector(`[data-place="${DOCS}"]`) as HTMLElement).click();
    await flush();
    expect(text(dialog())).toContain('This folder is empty.');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TF-3
  it('Edge — a whole number takes only digits and steps by one', async () => {
    const { target, transport } = await drawForm(COUNT, (t) => t.on('runs.start_task', ok({ kind: 'started', run_id: RUN })));
    const box = field(target, 'plan') as HTMLInputElement;
    expect(box.getAttribute('size')).toBe('6');
    expect(box.getAttribute('inputmode')).toBe('numeric');
    await type(target, 'plan', '2.5x');
    expect(box.value).toBe('25');
    await press(target, 'One more');
    expect(box.value).toBe('26');
    await press(target, 'One less');
    await press(target, 'One less');
    expect(box.value).toBe('24');
    box.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
    await flush();
    expect(box.value).toBe('25');
    await press(target, 'Update the plan index');
    expect((transport.sentOf('runs.start_task')[0]?.input as { values: unknown }).values).toEqual([{ name: 'plan', kind: 'text', text: '25' }]);
  });

  it('Edge — an empty number steps from zero', async () => {
    const { target } = await drawForm(APPLY);
    await press(target, 'One more');
    expect((field(target, 'phase') as HTMLInputElement).value).toBe('1');
    await type(target, 'phase', '2.5');
    await press(target, 'One more');
    expect((field(target, 'phase') as HTMLInputElement).value).toBe('3.5');
  });

  // spec: ordin-desktop/components/ScreensModule#SM-TF-5
  it('Error — the core refuses a fraction for a whole number', async () => {
    const { target } = await drawForm(COUNT, (t) => t.on('runs.start_task', ok({ kind: 'refused', problems: [{ name: 'plan', problem: 'not_a_number' }] })));
    await type(target, 'plan', '3');
    await press(target, 'Update the plan index');
    expect(text(target)).toContain('Enter a whole number, such as 2.');
    expect(field(target, 'plan').getAttribute('aria-invalid')).toBe('true');
  });

  it('Error — a required whole number left empty', async () => {
    const { target, transport } = await drawForm(COUNT);
    await type(target, 'plan', 'x');
    await press(target, 'Update the plan index');
    expect(text(target)).toContain('Fill this in to start.');
    expect(transport.sentOf('runs.start_task')).toHaveLength(0);
  });
});
