// SM-NS-3: Diagnostics — the journal and window headings, and Save logs for support….
import { beforeAll, describe, expect, it } from 'vitest';

import { backendWith } from './fixed-support';
import { err, type FakeTransport, ok, R1_GROUPS } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { text } from './settings-support';
import Diagnostics from '../src/lib/views/Diagnostics.svelte';
import { DiagnosticsModel, MAX_RUNS } from '../src/lib/views/diagnostics-model.svelte';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const clean = (markup: string) => markup.replace(/<!--.*?-->/g, '');
const GROUPS = [...R1_GROUPS, 'diagnostics', 'runs'];
const line = (i: number) => ({ at: `2026-01-10T10:00:${String(i).padStart(2, '0')}Z`, level: 'warn', code: `CODE_${i}`, summary: `Summary ${i}`, reference: `ref-${i}` });
const run = (i: number) => ({ id: uid(`run-${String(i).padStart(2, '0')}`), state: 'finished', queued_at: `2026-01-10T10:${String(i).padStart(2, '0')}:00Z` });

async function open(extra: (tr: FakeTransport) => void = () => undefined, runs = 3) {
  const t = await backendWith({
    groups: GROUPS,
    setup: (tr) => {
      tr.on('diagnostics.lines', ok({ items: [line(1), line(2)], next_cursor: null }));
      tr.on('runs.list', ok({ items: Array.from({ length: runs }, (_, i) => run(i)), next_cursor: null }));
      extra(tr);
    },
  });
  const model = new DiagnosticsModel(t.handle);
  await model.load();
  return { t, model };
}

describe('Diagnostics headings and lines (SM-NS-3)', () => {
  it('Happy path — the journal and window headings, and each line with its time, level, code, summary and reference', async () => {
    const { t, model } = await open();
    const out = text(clean(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model })));
    expect(out).toContain('Journal events');
    expect(out).toContain('This window');
    expect(out.indexOf('Journal events')).toBeLessThan(out.indexOf('This window'));
    for (const word of ['warn', 'CODE_1', 'Summary 1', 'ref-1', 'CODE_2']) expect(out).toContain(word);
    // SM-NS-3: no line offers Send to IT.
    expect(out).not.toContain('Send to IT');
  });

  it('Show more reads the next page with its cursor while next_cursor is present', async () => {
    const { t, model } = await open((tr) =>
      tr.on('diagnostics.lines', (input: unknown) => ((input as { cursor?: string }).cursor === undefined ? ok({ items: [line(1)], next_cursor: 'c2' }) : ok({ items: [line(2)], next_cursor: null }))),
    );
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).toContain('Show more');
    await model.more();
    expect(t.transport.sentOf('diagnostics.lines')[1]?.input).toEqual({ cursor: 'c2' });
    expect(model.lines).toHaveLength(2);
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).not.toContain('Show more');
  });
});

describe('Save logs for support… (SM-NS-3)', () => {
  it('the button is drawn on the local backend only, when the build answers runs and diagnostics', async () => {
    const { t, model } = await open();
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).toContain('Save logs for support…');
    const server = await backendWith({ groups: GROUPS, kind: 'server', setup: (tr) => tr.on('diagnostics.lines', ok({ items: [], next_cursor: null })) });
    expect(text(html(Diagnostics, { backend: server.state, workspaceId: null, instance: null, model: new DiagnosticsModel(server.handle) }))).not.toContain('Save logs for support…');
    const noRuns = await backendWith({ groups: R1_GROUPS.concat('diagnostics'), setup: (tr) => tr.on('diagnostics.lines', ok({ items: [], next_cursor: null })) });
    expect(text(html(Diagnostics, { backend: noRuns.state, workspaceId: null, instance: null, model: new DiagnosticsModel(noRuns.handle) }))).not.toContain('Save logs for support…');
  });

  it('the panel lists the runs newest first with a tick, id, state and time, over the note; Save… is disabled with no tick', async () => {
    const { t, model } = await open();
    await model.openPanel();
    expect(t.transport.sentOf('runs.list')[0]?.input).toEqual({});
    expect(model.runs.map((r) => r.id)).toEqual([uid('run-02'), uid('run-01'), uid('run-00')]);
    const markup = clean(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }));
    const out = text(markup);
    expect(out).toContain('Choose up to 20 runs to include.');
    expect(out).toContain('The file holds the chosen runs');
    expect(out).toContain(uid('run-02'));
    expect(out).toContain('finished');
    expect(markup).toMatch(/<button[^>]*disabled[^>]*>\s*Save…/);
  });

  it('Happy path — saving logs for support (SM-NS-3)', async () => {
    const { t, model } = await open((tr) => tr.on('diagnostics.support_export', ok({ saved_path: '/Users/sam/Desktop/ordin-support.zip' })));
    await model.openPanel();
    model.toggle(uid('run-00'));
    model.toggle(uid('run-02'));
    expect(await model.exportSupport()).toBe(true);
    expect(t.transport.sentOf('diagnostics.support_export')[0]?.input).toEqual({ run_ids: [uid('run-00'), uid('run-02')] });
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).toContain('Saved to /Users/sam/Desktop/ordin-support.zip');
  });

  it('nothing is sent with no tick', async () => {
    const { t, model } = await open();
    await model.openPanel();
    expect(await model.exportSupport()).toBe(false);
    expect(t.transport.sentOf('diagnostics.support_export')).toEqual([]);
  });

  it('Error path — a 21st run is refused (SM-NS-3)', async () => {
    const { t, model } = await open(() => undefined, 25);
    await model.openPanel();
    for (const r of model.runs.slice(0, MAX_RUNS)) model.toggle(r.id);
    expect(model.ticked.size).toBe(20);
    const twentyFirst = model.runs[20]!;
    model.toggle(twentyFirst.id);
    expect(model.ticked.has(twentyFirst.id)).toBe(false);
    expect(model.ticked.size).toBe(20);
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).toContain('You can include up to 20 runs.');
    // Unticking clears the line.
    model.toggle(model.runs[0]!.id);
    expect(model.limitShown).toBe(false);
  });

  it('Error path — DSK_DIALOG_CANCELLED draws nothing', async () => {
    const { t, model } = await open((tr) => tr.on('diagnostics.support_export', err('DSK_DIALOG_CANCELLED', 'You cancelled.')));
    await model.openPanel();
    model.toggle(uid('run-00'));
    expect(await model.exportSupport()).toBe(false);
    expect(model.exportError).toBeNull();
    expect(model.savedPath).toBeNull();
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).not.toContain('You cancelled.');
  });

  it('Error path — any other refusal is drawn as the owner\'s sentence', async () => {
    const { t, model } = await open((tr) => tr.on('diagnostics.support_export', err('DSK_EXPORT_FAILED', 'ordin could not write the file.')));
    await model.openPanel();
    model.toggle(uid('run-00'));
    await model.exportSupport();
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).toContain('ordin could not write the file.');
  });

  it('runs are read 50 a page with Show more while next_cursor is present', async () => {
    const { t, model } = await open((tr) =>
      tr.on('runs.list', (input: unknown) => ((input as { cursor?: string }).cursor === undefined ? ok({ items: [run(5)], next_cursor: 'r2' }) : ok({ items: [run(9)], next_cursor: null }))),
    );
    await model.openPanel();
    expect(model.runsNext).toBe('r2');
    await model.moreRuns();
    expect(t.transport.sentOf('runs.list')[1]?.input).toEqual({ cursor: 'r2' });
    expect(model.runs.map((r) => r.id)).toEqual([uid('run-09'), uid('run-05')]);
    expect(model.runsNext).toBeNull();
  });
});
