// SM-PG-4: the progress card says where the work is, what it is doing and for how long.
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import BlockBody from '../src/lib/blocks/BlockBody.svelte';
import { event, LOCAL, settle } from './support/fake-backend';
import { appWith, html, loadWindowCatalogue } from './support/ssr';
import { text } from './settings-support';

beforeAll(loadWindowCatalogue);
afterEach(() => vi.useRealTimers());

const WS = '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d';
const RUN = '0192a1c2-3344-7556-8677-8899aabbccdd';
const BLOCK = { kind: 'plan_progress', subject: { kind: 'run', run_id: RUN } };
const step = (position: number, mark: string) => ({ id: `s${position}`, position, line: `Step ${position}`, mark, plan_version: 1, run_id: RUN, events_seen: [], marks_set: [], came_from: 'agent' });

async function start() {
  const { app, transport } = await appWith(() => undefined);
  let seq = 0;
  const send = async (body: unknown, at: string) => {
    transport.emit(event(LOCAL, ++seq, body as never, at));
    await settle();
  };
  const draw = () => html(BlockBody, { backend: app.local, workspaceId: WS, blockId: 'b1', block: BLOCK }, app);
  return { app, send, draw };
}

describe('The progress card (SM-PG-4)', () => {
  // spec: ordin-desktop/components/ScreensModule#SM-PG-4
  it('Happy path — a plan: Now, the step bar with one segment per mark, Doing, and seconds elapsed', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-02-12T10:00:40Z'));
    const { send, draw } = await start();
    await send({ kind: 'run_state_changed', run_id: RUN, state: 'running' }, '2026-02-12T10:00:00Z');
    await send({ kind: 'plan_changed', run_id: RUN, plan_version: 1, steps: [step(1, 'done'), step(2, 'now'), step(3, 'next')] }, '2026-02-12T10:00:05Z');
    await send({ kind: 'run_progress', run_id: RUN, progress: { line: 'Reading pages 11–20 of Agent-Platform.pdf' } }, '2026-02-12T10:00:30Z');
    const out = draw();
    expect(text(out)).toContain('Now: Step 2 — step 2 of 3');
    expect(text(out)).toContain('Running for 40 s');
    expect(text(out)).toContain('Doing: Reading pages 11–20 of Agent-Platform.pdf');
    expect(out).toMatch(/class="step-bar[^"]*" aria-hidden="true"/);
    expect([...out.matchAll(/class="segment (\w+)[^"]*"/g)].map((m) => m[1])).toEqual(['done', 'now', 'next']);
    expect(text(out)).not.toMatch(/\d+%/);
  });

  it('Edge case — no plan and no count: Now, elapsed in minutes and Doing only, no bar', async () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date('2026-02-12T10:03:10Z'));
    const { send, draw } = await start();
    await send({ kind: 'run_state_changed', run_id: RUN, state: 'running' }, '2026-02-12T10:00:00Z');
    await send({ kind: 'run_progress', run_id: RUN, progress: { line: 'Read Agent-Platform.pdf' } }, '2026-02-12T10:00:20Z');
    const out = draw();
    expect(text(out)).toContain('Running for 3 min');
    expect(text(out)).toContain('Doing: Read Agent-Platform.pdf');
    expect(out).not.toContain('step-bar');
    expect(out).not.toContain('count-line');
  });

  it('table: the count line — no total, a known total, a total of zero, more done than the total', async () => {
    const cases: Array<[unknown, string]> = [
      [{ done: 7 }, 'Actions taken so far: 7'],
      [{ done: 3, total: 20 }, 'Items worked through: 3 of 20 · 15%'],
      [{ done: 2, total: 0 }, 'Actions taken so far: 2'],
      [{ done: 25, total: 20 }, 'Items worked through: 25 of 20 · 100%'],
    ];
    for (const [count, line] of cases) {
      const { send, draw } = await start();
      await send({ kind: 'run_progress', run_id: RUN, progress: { line: 'x', count } }, '2026-02-12T10:00:00Z');
      expect(text(draw()), line).toContain(line);
    }
  });

  it('Edge case — an ended run: elapsed stops at its end, and Doing is not drawn', async () => {
    const { send, draw } = await start();
    await send({ kind: 'run_state_changed', run_id: RUN, state: 'running' }, '2026-02-12T10:00:00Z');
    await send({ kind: 'run_progress', run_id: RUN, progress: { line: 'Wrote the note' } }, '2026-02-12T10:01:00Z');
    await send({ kind: 'run_state_changed', run_id: RUN, state: 'completed' }, '2026-02-12T10:05:30Z');
    const out = text(draw());
    expect(out).toContain('Running for 5 min');
    expect(out).not.toContain('Doing:');
  });

  it('Edge case — a run the window has no time for draws no elapsed time', async () => {
    const { app, draw } = await start();
    app.local.runs.set(RUN, { state: 'running', progressLines: [], folded: 0, count: null, plan: null, startUp: [] } as never);
    expect(text(draw())).not.toContain('Running for');
  });
});
