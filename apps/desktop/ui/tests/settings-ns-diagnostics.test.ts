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
    // Amended November 2026: a compact table of reference, time, level and summary; the code on request.
    for (const word of ['Reference', 'ref-1', 'Time', 'Level', 'What happened', 'warn', 'Summary 1', 'Summary 2']) expect(out).toContain(word);
    expect(out).not.toContain('CODE_1');
    expect(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model })).toContain('Quote it to support');
    // SM-NS-3: no line offers Send to IT.
    expect(out).not.toContain('Send to IT');
  });

  it('an info row\'s reference is drawn quieter than a warning\'s', async () => {
    const { t, model } = await open((tr) =>
      tr.on('diagnostics.lines', ok({ items: [{ ...line(1), level: 'info', reference: 'info-ref' }, line(2)], next_cursor: null })),
    );
    const markup = html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model });
    expect(markup).toMatch(/class="[^"]*\bquiet\b[^"]*"[^>]*><code>info-ref<\/code>/);
    expect(markup).not.toMatch(/class="[^"]*\bquiet\b[^"]*"[^>]*><code>ref-2<\/code>/);
  });

  it('Show more reads the next page with its cursor while next_cursor is present', async () => {
    const { t, model } = await open((tr) =>
      tr.on('diagnostics.lines', (input: unknown) => ((input as { cursor?: string }).cursor === undefined ? ok({ items: [line(1)], next_cursor: 'c2' }) : ok({ items: [line(2)], next_cursor: null }))),
    );
    expect(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model })).toContain('aria-label="Show more"');
    await model.more();
    expect(t.transport.sentOf('diagnostics.lines')[1]?.input).toEqual({ cursor: 'c2' });
    expect(model.lines).toHaveLength(2);
    expect(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model })).not.toContain('aria-label="Show more"');
  });
});

describe('Save logs for support… (SM-NS-3)', () => {
  it('the button is drawn on the local backend only, when the build answers runs and diagnostics', async () => {
    const { t, model } = await open();
    expect(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model })).toContain('aria-label="Save logs for support…"');
    const server = await backendWith({ groups: GROUPS, kind: 'server', setup: (tr) => tr.on('diagnostics.lines', ok({ items: [], next_cursor: null })) });
    expect(html(Diagnostics, { backend: server.state, workspaceId: null, instance: null, model: new DiagnosticsModel(server.handle) })).not.toContain('Save logs for support…');
    const noRuns = await backendWith({ groups: R1_GROUPS.concat('diagnostics'), setup: (tr) => tr.on('diagnostics.lines', ok({ items: [], next_cursor: null })) });
    expect(html(Diagnostics, { backend: noRuns.state, workspaceId: null, instance: null, model: new DiagnosticsModel(noRuns.handle) })).not.toContain('Save logs for support…');
  });

  it('Happy path — one action saves the newest 20 runs and the logs, with no ticks (SM-NS-3)', async () => {
    const { t, model } = await open((tr) => tr.on('diagnostics.support_export', ok({ saved_path: '/Users/sam/Desktop/ordin-support.zip' })), 25);
    const markup = clean(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }));
    expect(markup).not.toContain('type="checkbox"');
    expect(await model.saveForSupport()).toBe(true);
    expect(t.transport.sentOf('runs.list')[0]?.input).toEqual({});
    const sent = (t.transport.sentOf('diagnostics.support_export')[0]?.input as { run_ids: string[] }).run_ids;
    expect(sent).toHaveLength(MAX_RUNS);
    expect(sent[0]).toBe(uid('run-24'));
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).toContain('Saved to /Users/sam/Desktop/ordin-support.zip');
  });

  it('the save button says what the file holds in its tooltip (SM-NS-3, DSK-LOG-5)', async () => {
    const { t, model } = await open();
    const markup = html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }).replace(/&#39;|&apos;|’/g, "'");
    expect(markup).toContain("the last 3 days of ordin's logs");
    expect(markup).toContain('It holds no document or conversation text.');
  });

  it('Edge — with no runs nothing is sent and the screen says so', async () => {
    const { t, model } = await open(() => undefined, 0);
    expect(await model.saveForSupport()).toBe(false);
    expect(t.transport.sentOf('diagnostics.support_export')).toEqual([]);
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).toContain('There is nothing to save yet.');
  });

  it('Error path — DSK_DIALOG_CANCELLED draws nothing', async () => {
    const { t, model } = await open((tr) => tr.on('diagnostics.support_export', err('DSK_DIALOG_CANCELLED', 'You cancelled.')));
    expect(await model.saveForSupport()).toBe(false);
    expect(model.exportError).toBeNull();
    expect(model.savedPath).toBeNull();
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).not.toContain('You cancelled.');
  });

  it('Error path — any other refusal is drawn as the owner\'s sentence', async () => {
    const { t, model } = await open((tr) => tr.on('diagnostics.support_export', err('DSK_EXPORT_FAILED', 'ordin could not write the file.')));
    await model.saveForSupport();
    expect(text(html(Diagnostics, { backend: t.state, workspaceId: null, instance: null, model }))).toContain('ordin could not write the file.');
  });
});
