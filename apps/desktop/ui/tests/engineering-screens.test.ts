// Changes waiting, project settings (SA-CLOSE-04), the terminal, and the rule that Monaco and xterm load lazily.
import { beforeAll, describe, expect, it } from 'vitest';

import { ChangesController } from '../src/lib/views/engineering/changes.svelte';
import { ProjectSettingsController, MAX_CHECK_COMMANDS } from '../src/lib/views/engineering/project-settings.svelte';
import { decodeBytes, TerminalController } from '../src/lib/views/engineering/terminal-controller.svelte';
import { forgetProjects, rememberProject } from '../src/lib/views/engineering/projects';
import { editorText } from '../src/lib/views/engineering/editor-hub.svelte';
import { languageFor } from '../src/lib/views/engineering/monaco';
import Terminal from '../src/lib/views/engineering/Terminal.svelte';
import ProjectSettings from '../src/lib/views/engineering/ProjectSettings.svelte';
import { SCREEN_IDS, clearScreens, getScreen } from '../src/lib/shell/screens';
import { StreamHub } from '../src/lib/shell/context';
import { answersAll } from '../src/lib/backend/groups';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, PROJECT, project, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const task = (id: string, over: Record<string, unknown> = {}) => ({ id, request: `Request ${id}`, branch: `ordin/${id}`, new_problems: 0, state: 'ready_for_review', ...over });
const entry = (id: string) => ({ id, file: `src/${id}.rs`, hunk: '@@ -1 +1 @@', state: 'pending' });

describe('Changes waiting', () => {
  it('loads the review and the engineer\'s own changes', async () => {
    const { handle, transport } = await backendWith({
      setup: (t) => t.on('projects.review', ok([{ task: task('a'), new_problems: [], change_set: { entries: [entry(uid('e1'))] } }])).on('projects.status', ok([{ path: 'README.md', status: 'modified' }])),
    });
    const c = new ChangesController(handle, PROJECT);
    await c.load();
    expect(transport.sentOf('projects.review')[0]?.input).toEqual({ project_id: PROJECT });
    expect(c.tasks).toHaveLength(1);
    expect(c.own[0]?.path).toBe('README.md');
  });

  it('accepting an entry sends the decision and reloads; a conflict is kept for the person', async () => {
    const { handle, transport } = await backendWith({
      setup: (t) => t.on('projects.review', ok([])).on('projects.status', ok([])).on('projects.decide_entry', ok({ kind: 'applied', file_version_id: 'v1' })),
    });
    const c = new ChangesController(handle, PROJECT);
    await c.decide(uid('e1'), { kind: 'accept' });
    expect(transport.sentOf('projects.decide_entry')[0]?.input).toEqual({ decision: { kind: 'accept' }, entry: uid('e1') });
    expect(transport.commandsSent()).toContain('projects.review');
    transport.on('projects.decide_entry', ok({ kind: 'conflict', agents: 'a', yours: 'b' }));
    transport.sent.length = 0;
    await c.decide(uid('e2'), { kind: 'accept' });
    expect(c.conflicts[uid('e2')]).toEqual({ agents: 'a', yours: 'b' });
    expect(transport.commandsSent()).toEqual(['projects.decide_entry']);
  });

  it('commit then push, and no push after a failed commit', async () => {
    const { handle, transport } = await backendWith({
      setup: (t) => t.on('projects.commit', ok({})).on('projects.push', ok({})).on('projects.review', ok([])).on('projects.status', ok([])),
    });
    const c = new ChangesController(handle, PROJECT);
    expect(await c.commitAndPush('Fix totals')).toBe(true);
    expect(transport.commandsSent().slice(0, 2)).toEqual(['projects.commit', 'projects.push']);
    transport.sent.length = 0;
    transport.on('projects.commit', err('PROJ_FILES_FAILED', 'Nothing to commit.'));
    expect(await c.commitAndPush('x')).toBe(false);
    expect(transport.commandsSent()).toEqual(['projects.commit']);
    expect(c.error?.message).toBe('Nothing to commit.');
  });

  it('keeping one of several attempts sends keep_attempt', async () => {
    const { handle, transport } = await backendWith({ setup: (t) => t.on('projects.keep_attempt', ok({})).on('projects.review', ok([])).on('projects.status', ok([])) });
    expect(await new ChangesController(handle, PROJECT).keepAttempt(uid('a'))).toBe(true);
    expect(transport.sentOf('projects.keep_attempt')[0]?.input).toEqual({ task: uid('a') });
  });
});

describe('Project settings — Check commands (SA-CLOSE-04)', () => {
  const settings = async (setup: Parameters<typeof backendWith>[0] = {}) => {
    forgetProjects();
    const b = await backendWith(setup);
    rememberProject(b.handle.id, project());
    return { ...b, c: new ProjectSettingsController(b.handle, PROJECT) };
  };

  it('Happy path — the engineer sets a project\'s check commands (SA-CLOSE-04)', async () => {
    const { c, transport } = await settings({ setup: (t) => t.on('projects.set_check_commands', (input) => ok(project({ check_commands: (input as { commands: string[] }).commands }))) });
    expect(c.commands).toEqual([]);
    const empty = html(ProjectSettings, { backend: { handle: { call: async () => ({ kind: 'ok', result: null }) }, kind: 'local' }, workspaceId: WS, instance: PROJECT });
    expect(text(empty)).toContain('No checks set');
    c.add();
    c.edit(0, 'cargo test');
    expect(await c.save()).toBe(true);
    expect(transport.sentOf('projects.set_check_commands')[0]?.input).toEqual({ commands: ['cargo test'], project_id: PROJECT });
    expect(c.commands).toEqual(['cargo test']);
  });

  it('Error path — check commands refused: the error line shows the message and the 21 boxes keep their text', async () => {
    const { c } = await settings({ setup: (t) => t.on('projects.set_check_commands', err('PROJ_CHECKS_INVALID', 'At most 20 checks can be set.')) });
    for (let i = 0; i < 21; i++) {
      c.commands = [...c.commands, `check ${i}`];
    }
    expect(await c.save()).toBe(false);
    expect(c.error?.message).toBe('At most 20 checks can be set.');
    expect(c.commands).toHaveLength(21);
    expect(c.commands[20]).toBe('check 20');
  });

  it('Add stops at 20 boxes and a blank box is not sent', async () => {
    const { c, transport } = await settings({ setup: (t) => t.on('projects.set_check_commands', ok(project())) });
    for (let i = 0; i < 25; i++) c.add();
    expect(c.commands).toHaveLength(MAX_CHECK_COMMANDS);
    c.commands = ['  ', 'make check'];
    await c.save();
    expect(transport.sentOf('projects.set_check_commands')[0]?.input).toEqual({ commands: ['make check'], project_id: PROJECT });
  });

  it('PROJ_NOT_FOUND is the command\'s error line', async () => {
    const { c } = await settings({ setup: (t) => t.on('projects.set_check_commands', err('PROJ_NOT_FOUND', 'That project is not open.')) });
    await c.save();
    expect(c.error?.message).toBe('That project is not open.');
  });
});

describe('Terminal', () => {
  it('decodes {bytes_b64} and nothing else', () => {
    expect([...(decodeBytes({ bytes_b64: btoa('hi') }) ?? [])]).toEqual([104, 105]);
    expect(decodeBytes({})).toBeNull();
    expect(decodeBytes(null)).toBeNull();
    expect(decodeBytes({ bytes_b64: '***' })).toBeNull();
  });

  it('opens a terminal, draws only the stream the open command returned, sends input and resize, and closes', async () => {
    const { handle, transport } = await backendWith({
      setup: (t) => t.on('projects.terminal_open', ok(uid('term-1'))).on('projects.terminal_input', ok({})).on('projects.terminal_resize', ok({})).on('projects.terminal_close', ok({})),
    });
    const hub = new StreamHub();
    const written: string[] = [];
    const c = new TerminalController(handle, hub, (b) => written.push(new TextDecoder().decode(b)));
    expect(await c.open(PROJECT, 80, 24)).toBe(true);
    expect(transport.sentOf('projects.terminal_open')[0]?.input).toEqual({ project_id: PROJECT, cols: 80, rows: 24 });
    hub.dispatch({ stream: uid('term-1'), kind: 'terminal', payload: { bytes_b64: btoa('ls\r\n') } });
    hub.dispatch({ stream: 'other', kind: 'terminal', payload: { bytes_b64: btoa('no') } });
    hub.dispatch({ stream: uid('term-1'), kind: 'lsp', payload: { bytes_b64: btoa('no') } });
    expect(written).toEqual(['ls\r\n']);
    await c.input('é');
    expect(transport.sentOf('projects.terminal_input')[0]?.input).toEqual({ terminal: uid('term-1'), bytes: [0xc3, 0xa9] });
    await c.resize(100, 30);
    expect(transport.sentOf('projects.terminal_resize')[0]?.input).toEqual({ terminal: uid('term-1'), cols: 100, rows: 30 });
    await c.close();
    expect(transport.sentOf('projects.terminal_close')[0]?.input).toEqual({ terminal: uid('term-1') });
    hub.dispatch({ stream: uid('term-1'), kind: 'terminal', payload: { bytes_b64: btoa('late') } });
    expect(written).toEqual(['ls\r\n']);
    await c.input('x');
    expect(transport.sentOf('projects.terminal_input')).toHaveLength(1);
  });

  it('a refused open keeps the owner sentence', async () => {
    const { handle } = await backendWith({ setup: (t) => t.on('projects.terminal_open', err('PROJ_NOT_FOUND', 'Not open.')) });
    const c = new TerminalController(handle, new StreamHub(), () => undefined);
    expect(await c.open(PROJECT, 80, 24)).toBe(false);
    expect(c.error?.message).toBe('Not open.');
  });

  it('on a server\'s workspace the screen says code views are not available there', async () => {
    const { state } = await backendWith({ kind: 'server' });
    const out = html(Terminal, { backend: state, workspaceId: WS, instance: PROJECT });
    expect(text(out)).toContain("Code views aren't available for workspaces on a server yet.");
  });
});

describe('Editor helpers and the lazy loading of Monaco and xterm', () => {
  it('only a text preview becomes editor text', () => {
    expect(editorText({ kind: 'text', markdown: false, text: 'fn main() {}', truncated: true })).toEqual({ text: 'fn main() {}', truncated: true });
    expect(editorText({ kind: 'none' })).toBeNull();
  });

  it('the language comes from the extension, plaintext when unknown', () => {
    const known = [{ id: 'rust', extensions: ['.rs'] }, { id: 'typescript', extensions: ['.ts'] }];
    expect(languageFor('src/lib.rs', known)).toBe('rust');
    expect(languageFor('notes', known)).toBe('plaintext');
    expect(languageFor('a.unknown', known)).toBe('plaintext');
  });

  it('Monaco and xterm are never statically imported: initial JavaScript stays inside its budget', () => {
    const files = import.meta.glob(['../src/**/*.ts', '../src/**/*.svelte'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
    const staticValueImport = /^\s*import\s+(?!type\b)[^;]*from\s+['"](monaco-editor|@xterm\/xterm)[^'"]*['"]/m;
    const offenders = Object.entries(files).filter(([, c]) => staticValueImport.test(c)).map(([p]) => p);
    expect(offenders).toEqual([]);
    const dynamic = Object.entries(files).filter(([, c]) => /import\(\s*['"](monaco-editor|@xterm\/xterm)/.test(c)).map(([p]) => p.split('/').pop());
    expect(dynamic.sort()).toEqual(['monaco.ts', 'xterm.ts']);
  });
});

describe('SM-UP-1 — the engineering screens are offered only when every command is answered', () => {
  it('registers four screens, each gated by its commands', async () => {
    clearScreens();
    await import('../src/lib/views/engineering/register');
    const { ENGINEERING_SCREENS } = await import('../src/lib/views/engineering/register');
    for (const id of Object.values(ENGINEERING_SCREENS)) {
      const def = getScreen(id);
      expect(def?.commands?.length, id).toBeGreaterThan(0);
      expect(answersAll({ groups: new Set(['projects', 'files']) }, def?.commands ?? []), id).toBe(true);
      expect(answersAll({ groups: new Set(['files']) }, def?.commands ?? []), id).toBe(false);
    }
    expect(SCREEN_IDS.home).toBe('home');
  });
});
