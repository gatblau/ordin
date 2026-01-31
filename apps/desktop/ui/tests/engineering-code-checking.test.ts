// SM-SF-3: the code-checking question (bar above the editor, Turn on / Keep off) and the Code checking
// section of the project settings.
import { beforeAll, describe, expect, it } from 'vitest';

import { EditorHub } from '../src/lib/views/engineering/editor-hub.svelte';
import { answerLine, ProjectSettingsController } from '../src/lib/views/engineering/project-settings.svelte';
import { forgetProjects, rememberProject } from '../src/lib/views/engineering/projects';
import CodeCheckingBar from '../src/lib/views/engineering/CodeCheckingBar.svelte';
import { t } from '../src/lib/i18n/fluent';
import { err, ok, settle } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, buttons, PROJECT, project, text } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const session = (asks: boolean, stream = uid('s1')) => ok({ asks_to_start_servers: asks, language: 'rust', server_line: 'highlighted · language server waiting for your answer', stream });

async function hubWith(asks = true) {
  forgetProjects();
  let answered = false;
  const b = await backendWith({
    setup: (tr) => {
      tr.on('projects.editor_open', (input) => ok({ asks_to_start_servers: asks && !answered, language: 'rust', server_line: 'line', stream: uid(`stream-${(input as { path: string }).path}`) }));
      tr.on('projects.editor_close', ok({}));
      tr.on('projects.answer_language_servers', () => {
        answered = true;
        return ok({});
      });
    },
  });
  rememberProject(b.handle.id, project());
  return { ...b, hub: new EditorHub(b.handle) };
}

describe('SM-SF-3 — the bar', () => {
  it('Happy path — the person turns on code checking: the bar reads the question, neither button is the default and none has focus', async () => {
    const { hub } = await hubWith();
    await hub.open(PROJECT, 'src/lib.rs');
    expect(hub.bars.has(PROJECT)).toBe(true);
    const out = html(CodeCheckingBar, { project: hub.projectName(PROJECT), busy: false, error: null, onanswer: () => undefined });
    expect(text(out)).toContain('Turn on code checking for Billing?');
    expect(text(out)).toContain('Turn on');
    expect(text(out)).toContain('Keep off');
    expect(out).toMatch(/<h3[^>]*>/);
    for (const b of buttons(out)) {
      expect(b).not.toContain('data-default');
      expect(b).not.toContain('autofocus');
    }
    expect(out).not.toContain('role="dialog"');
  });

  it('asks_to_start_servers false draws no bar', async () => {
    const { hub } = await hubWith(false);
    await hub.open(PROJECT, 'src/lib.rs');
    expect(hub.bars.size).toBe(0);
  });

  it('both buttons are disabled while the answer is in flight', () => {
    const out = html(CodeCheckingBar, { project: 'Billing', busy: true, error: null, onanswer: () => undefined });
    expect(buttons(out).every((b) => b.includes('disabled'))).toBe(true);
  });

  it('Happy path — Turn on sends allowed true, closes and reopens each open editor of the project, and announces it', async () => {
    const { hub, transport } = await hubWith();
    await hub.open(PROJECT, 'src/lib.rs');
    await hub.open(PROJECT, 'src/main.rs');
    transport.sent.length = 0;
    expect(await hub.answer(PROJECT, true)).toBe(true);
    expect(transport.sentOf('projects.answer_language_servers')[0]?.input).toEqual({ allowed: true, project_id: PROJECT });
    expect(transport.commandsSent()).toEqual([
      'projects.answer_language_servers',
      'projects.editor_close', 'projects.editor_open',
      'projects.editor_close', 'projects.editor_open',
    ]);
    expect(transport.sentOf('projects.editor_close').map((e) => (e.input as { session: string }).session)).toEqual([uid('stream-src/lib.rs'), uid('stream-src/main.rs')]);
    expect(transport.sentOf('projects.editor_open').map((e) => (e.input as { path: string }).path)).toEqual(['src/lib.rs', 'src/main.rs']);
    expect(hub.bars.size).toBe(0);
    await settle();
    expect(hub.announcer.text).toBe(t('code-checking-on'));
  });

  it('Edge case — the person keeps code checking off: allowed false, the bar goes, no editor is reopened', async () => {
    const { hub, transport } = await hubWith();
    await hub.open(PROJECT, 'src/lib.rs');
    transport.sent.length = 0;
    await hub.answer(PROJECT, false);
    expect(transport.commandsSent()).toEqual(['projects.answer_language_servers']);
    expect(transport.sentOf('projects.answer_language_servers')[0]?.input).toEqual({ allowed: false, project_id: PROJECT });
    expect(hub.bars.size).toBe(0);
    await settle();
    expect(hub.announcer.text).toBe(t('code-checking-off'));
  });

  it('a refusal keeps the bar and carries the owner sentence', async () => {
    const { hub, transport } = await hubWith();
    transport.on('projects.answer_language_servers', err('PROJ_NOT_FOUND', 'That project is no longer open.'));
    await hub.open(PROJECT, 'src/lib.rs');
    expect(await hub.answer(PROJECT, true)).toBe(false);
    expect(hub.bars.get(PROJECT)?.error?.message).toBe('That project is no longer open.');
    expect(hub.bars.get(PROJECT)?.busy).toBe(false);
    const out = html(CodeCheckingBar, { project: 'Billing', busy: false, error: hub.bars.get(PROJECT)?.error ?? null, onanswer: () => undefined });
    expect(text(out)).toContain('That project is no longer open.');
  });

  it('a later editor_open that asks again draws the bar again (the screens keep no answer)', async () => {
    const { hub } = await hubWith();
    await hub.open(PROJECT, 'src/lib.rs');
    await hub.answer(PROJECT, false);
    expect(hub.bars.size).toBe(0);
    await hub.open(PROJECT, 'src/other.rs');
    expect(hub.bars.has(PROJECT)).toBe(false); // the stub now answers "no longer asks"
    const asking = await backendWith({ setup: (tr) => tr.on('projects.editor_open', session(true)) });
    const again = new EditorHub(asking.handle);
    await again.open(PROJECT, 'src/other.rs');
    expect(again.bars.has(PROJECT)).toBe(true);
  });

  // Scenario: Happy path — a project screen opened first reads its project (SM-WR-4)
  it('a project the screens have not seen is read with projects.get and its display name is drawn (SM-WR-4)', async () => {
    forgetProjects();
    const { handle, transport } = await backendWith({ setup: (tr) => tr.on('projects.get', ok(project())) });
    const hub = new EditorHub(handle);
    expect(hub.projectName(PROJECT)).toBe('');
    await hub.loadProject(PROJECT);
    expect(transport.sentOf('projects.get')[0]?.input).toEqual({ project_id: PROJECT });
    expect(hub.projectName(PROJECT)).toBe('Billing');
    await hub.loadProject(PROJECT);
    expect(transport.sentOf('projects.get')).toHaveLength(1);
  });

  it('PROJ_NOT_FOUND is kept for ErrorLine in place of the name, and no folder stands in (SM-WR-4)', async () => {
    forgetProjects();
    const { handle } = await backendWith({ setup: (tr) => tr.on('projects.get', err('PROJ_NOT_FOUND', 'That project is no longer open.')) });
    const hub = new EditorHub(handle);
    await hub.loadProject(PROJECT);
    expect(hub.projectErrors.get(PROJECT)?.code).toBe('PROJ_NOT_FOUND');
    expect(hub.projectName(PROJECT)).toBe('');
  });

  it('the project settings read an unseen project with projects.get (SM-WR-4)', async () => {
    forgetProjects();
    const { handle } = await backendWith({ setup: (tr) => tr.on('projects.get', ok(project({ check_commands: ['cargo test'] }))) });
    const c = new ProjectSettingsController(handle, PROJECT);
    await c.loadProject();
    expect(c.displayName).toBe('Billing');
    expect(c.commands).toEqual(['cargo test']);
  });
});

describe('SM-SF-3.3 — project settings: Code checking', () => {
  const settingsFor = async (answer: boolean | null) => {
    forgetProjects();
    const b = await backendWith({ setup: (tr) => tr.on('projects.language_servers_answer', ok(answer)).on('projects.answer_language_servers', ok({})) });
    return { ...b, c: new ProjectSettingsController(b.handle, PROJECT) };
  };

  it.each([
    { answer: true, text: 'Code checking is on for this project.', button: 'Turn off' },
    { answer: false, text: 'Code checking is off for this project.', button: 'Turn on' },
    { answer: null, text: "Code checking hasn't been turned on for this project.", button: 'Turn on' },
  ])('Happy path — project settings show the answer $answer', async ({ answer, text: line, button }) => {
    const { c, transport } = await settingsFor(answer);
    await c.loadAnswer();
    expect(transport.sentOf('projects.language_servers_answer')[0]?.input).toEqual({ project_id: PROJECT });
    const l = answerLine(c.answer as boolean | null);
    expect(t(l.message)).toBe(line);
    expect(t(l.button)).toBe(button);
  });

  it('a button sends the answer and the section re-reads it after ok', async () => {
    const { c, transport } = await settingsFor(null);
    await c.loadAnswer();
    transport.sent.length = 0;
    transport.on('projects.language_servers_answer', ok(true));
    await c.setAnswer(true);
    expect(transport.commandsSent()).toEqual(['projects.answer_language_servers', 'projects.language_servers_answer']);
    expect(c.answer).toBe(true);
  });

  it('a refusal is the owner sentence and the answer is not re-read', async () => {
    const { c, transport } = await settingsFor(null);
    transport.on('projects.answer_language_servers', err('PROJ_NOT_FOUND', 'Gone.'));
    await c.loadAnswer();
    transport.sent.length = 0;
    await c.setAnswer(true);
    expect(c.answerError?.message).toBe('Gone.');
    expect(transport.commandsSent()).toEqual(['projects.answer_language_servers']);
  });
});
