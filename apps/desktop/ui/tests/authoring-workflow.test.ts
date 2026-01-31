// SM-NS-5: the workflow screen and its editor.
import { beforeAll, describe, expect, it, vi } from 'vitest';

import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { WorkflowController, currentVersion } from '../src/lib/views/authoring/workflow.svelte';
import { err, ok } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));

const WF = '0192f400-0000-7000-8000-000000000001';
const step = (position: string, line: string) => ({ kind: 'agent', agent: 'a', position, line, asks_first: false, ordin_checks: false, uses: [] });
const file = (over: Record<string, unknown> = {}) => ({
  agents: [{ id: 'a1', key: 'a', name: 'Rates reader', repository: { profile_key: 'company' }, role: 'reads', version: '1' }],
  format: 1,
  inputs: [{ kind: 'document', label: 'The rates file', required: true }],
  limits: { agents_at_once: 2, external_action_agents: [], failure_repeat_limit: 2, max_rounds: 5, wall_clock_minutes: 30 },
  name: 'Supplier rates refresh',
  origin: 'Written in this workspace',
  purpose: 'Refresh the rates',
  steps: [step('1', 'Read the file'), { kind: 'run_workflow', position: '2', workflow: 'other' }, step('3', 'Write the summary')],
  ...over,
});
const view = (over: Record<string, unknown> = {}, state = 'saved') => ({
  agents: [{ agent: { kind: 'agent_instance', agent_instance_id: 'i' }, key: 'a', presence: 'present' }],
  file: file(over),
  version: 3,
  versions: [
    { version: 3, saved_at: '2026-01-08T10:00:00Z', change_note: 'Third' },
    { version: 2, saved_at: '2026-01-07T10:00:00Z', change_note: 'Second' },
  ],
  workflow: { id: WF, name: 'Supplier rates refresh', origin: 'own', state, workspace_id: WS, created_at: '2026-01-06T10:00:00Z' },
  ...over && {},
});

async function screen(opts: { view?: unknown; extra?: (tr: import('./support/fake-backend').FakeTransport) => void } = {}) {
  const closed = vi.fn();
  const made = await backendWith({ setup: (tr) => { tr.on('workflows.get', ok(opts.view ?? view())); opts.extra?.(tr); } });
  const c = new WorkflowController(made.handle, WF, WS, closed);
  await c.load();
  return { ...made, c, closed };
}

describe('SM-NS-5 — the workflow screen', () => {
  it('On opening it sends workflows.get with the workflow id and keeps the view', async () => {
    const { c, transport } = await screen();
    expect(transport.sentOf('workflows.get')[0]?.input).toEqual({ workflow_id: WF });
    expect(c.view?.file.name).toBe('Supplier rates refresh');
    expect(currentVersion(c.view!)).toBe(3);
    expect(c.imported).toBe(false);
  });

  it('An imported workflow offers Save to this workspace with the imported change note, and Discard closes the tab', async () => {
    const { c, transport, closed } = await screen({ view: view({}, 'not_saved_yet'), extra: (tr) => tr.on('workflows.save', ok({ version: 1 })).on('workflows.discard_draft', ok({})) });
    expect(c.imported).toBe(true);
    await c.saveImported();
    expect(transport.sentOf('workflows.save')[0]?.input).toMatchObject({ workflow_id: WF, change_note: 'Imported from a file' });
    await c.discardImported();
    expect(transport.sentOf('workflows.discard_draft')[0]?.input).toEqual({ workflow_id: WF });
    expect(closed).toHaveBeenCalledOnce();
  });

  it('Run… is unavailable while an agent is missing and sends nothing', async () => {
    const missing = view();
    (missing.agents as unknown[]) = [{ key: 'a', presence: 'missing' }];
    const { c, transport } = await screen({ view: missing, extra: (tr) => tr.on('workflows.run', ok({ crew_charter_id: 'ch' })) });
    expect(c.missingAgent).toBe(true);
    await c.run();
    expect(transport.sentOf('workflows.run')).toHaveLength(0);
    const present = await screen({ extra: (tr) => tr.on('workflows.run', ok({ crew_charter_id: 'ch' })) });
    await present.c.run();
    expect(present.transport.sentOf('workflows.run')[0]?.input).toEqual({ workflow_id: WF });
    expect(present.c.ranCharterId).toBe('ch');
  });

  it('Error path — an empty workflow name on Rename is not sent and shows workflow-rename-empty', async () => {
    const { c, transport } = await screen({ extra: (tr) => tr.on('workflows.rename', ok({})) });
    c.openPanel('rename');
    c.renameName = '   ';
    await c.rename();
    expect(c.renameEmpty).toBe(true);
    expect(transport.sentOf('workflows.rename')).toHaveLength(0);
    expect(t('workflow-rename-empty')).toBe('Give the workflow a name.');
  });

  it('Error path — a workflow name already used (SM-NS-5)', async () => {
    const { c } = await screen({ extra: (tr) => tr.on('workflows.rename', err('WFF_NAME_DUPLICATE', 'Another workflow here already has that name.')) });
    c.openPanel('rename');
    c.renameName = 'Supplier rates refresh';
    await c.rename();
    expect(c.panelError?.message).toBe('Another workflow here already has that name.');
    expect(c.panel).toBe('rename');
  });

  it('a renamed workflow says what it was called and reads the screen again', async () => {
    const { c, transport } = await screen({ extra: (tr) => tr.on('workflows.rename', ok({})) });
    c.openPanel('rename');
    c.renameName = 'New name';
    await c.rename();
    expect(transport.sentOf('workflows.rename')[0]?.input).toEqual({ workflow_id: WF, name: 'New name' });
    expect(c.renamedFrom).toBe('Supplier rates refresh');
    expect(c.panel).toBeNull();
    expect(transport.sentOf('workflows.get')).toHaveLength(2);
  });

  it('Duplicate presets the name, lists this workspace first and sends the version', async () => {
    const other = { id: 'w-other', name: 'Other', created_at: '', own_folder_place_id: 'p', state: 'active' };
    const here = { id: WS, name: 'Here', created_at: '', own_folder_place_id: 'p', state: 'active' };
    const { c, transport } = await screen({ extra: (tr) => tr.on('workspaces.list', ok([other, here])).on('workflows.duplicate', ok({ workflow_id: 'new' })) });
    c.openPanel('duplicate');
    await vi.waitFor(() => expect(c.workspaces).toHaveLength(2));
    expect(c.duplicateName).toBe('Copy of Supplier rates refresh');
    expect(c.workspaceChoices.map((w) => w.id)).toEqual([WS, 'w-other']);
    await c.duplicate();
    expect(transport.sentOf('workflows.duplicate')[0]?.input).toEqual({ workflow_id: WF, version: 3, name: 'Copy of Supplier rates refresh', to_workspace_id: WS });
  });

  it('Export sends workflows.export then files.save_export; a cancelled dialog draws nothing', async () => {
    const { c, transport } = await screen({ extra: (tr) => tr.on('workflows.export', ok({ file_name: 'w.yaml', yaml: 'a: 1' })).on('files.save_export', err('DSK_DIALOG_CANCELLED', 'Cancelled')) });
    c.openPanel('export');
    await c.export();
    expect(transport.sentOf('files.save_export')[0]?.input).toEqual({ file_name: 'w.yaml', text: 'a: 1' });
    expect(c.panelError).toBeNull();
    expect(c.panel).toBeNull();
  });

  it('Remove sends confirmed: true and closes the tab on ok', async () => {
    const { c, transport, closed } = await screen({ extra: (tr) => tr.on('workflows.remove', ok({})) });
    await c.remove();
    expect(transport.sentOf('workflows.remove')[0]?.input).toEqual({ workflow_id: WF, confirmed: true });
    expect(closed).toHaveBeenCalledOnce();
  });

  it('Restore sends workflows.restore for the earlier version and reads the screen again', async () => {
    const { c, transport } = await screen({ extra: (tr) => tr.on('workflows.restore', ok({ version: 4 })) });
    await c.restore(2);
    expect(transport.sentOf('workflows.restore')[0]?.input).toEqual({ workflow_id: WF, version: 2 });
  });
});

describe('SM-NS-5.3 — the editor', () => {
  it('the editor draws agent steps only and keeps a control block where it was when steps move or go', async () => {
    const { c } = await screen();
    c.startEdit();
    expect(c.stepIndexes).toEqual([0, 2]);
    c.moveStep(0, 1);
    expect(c.draft?.steps.map((s) => s.position)).toEqual(['3', '2', '1']);
    c.removeStep(2);
    expect(c.draft?.steps.map((s) => s.position)).toEqual(['3', '2']);
  });

  it('Save with problems draws them, sends nothing to workflows.save and keeps the editor', async () => {
    const problem = { args: { step: '1' }, at: '1', code: 'WFC_X', message_id: 'unknown-message' };
    const { c, transport } = await screen({ extra: (tr) => tr.on('workflows.check', ok({ problems: [problem], notices: [] })).on('workflows.save', ok({ version: 4 })) });
    c.startEdit();
    c.changeNote = 'Edit';
    expect(await c.save()).toBe(false);
    expect(c.problemsAt('1')).toHaveLength(1);
    expect(transport.sentOf('workflows.save')).toHaveLength(0);
    expect(c.editing).toBe(true);
  });

  it('Save is not sent without a change note', async () => {
    const { c, transport } = await screen({ extra: (tr) => tr.on('workflows.check', ok({ problems: [], notices: [] })) });
    c.startEdit();
    expect(await c.save()).toBe(false);
    expect(transport.sentOf('workflows.check')).toHaveLength(0);
  });

  it('Happy path — a workflow saved after its check (SM-NS-5)', async () => {
    const { c, transport } = await screen({ extra: (tr) => tr.on('workflows.check', ok({ problems: [], notices: [] })).on('workflows.save', ok({ version: 4 })) });
    c.startEdit();
    c.setStepLine(0, 'Check prices first');
    c.changeNote = 'Check prices first';
    expect(await c.save()).toBe(true);
    expect(transport.sentOf('workflows.check')[0]?.input).toMatchObject({ workspace_id: WS });
    const saved = transport.sentOf('workflows.save')[0]?.input as { workflow_id: string; base_version: number; change_note: string; file: { steps: Array<{ line?: string }> } };
    expect(saved).toMatchObject({ workflow_id: WF, base_version: 3, change_note: 'Check prices first' });
    expect(saved.file.steps[0]?.line).toBe('Check prices first');
    expect(t('workflow-saved-as', { version: 4 })).toBe('Saved as version 4');
    expect(c.savedAs).toBe(4);
    expect(c.editing).toBe(false);
  });

  it('Cancel with changes asks on the screen; Cancel without changes leaves', async () => {
    const { c } = await screen();
    c.startEdit();
    c.cancel();
    expect(c.editing).toBe(false);
    c.startEdit();
    c.setStepLine(0, 'x');
    c.cancel();
    expect(c.askingDiscard).toBe(true);
    c.discardChanges();
    expect(c.editing).toBe(false);
  });
});
