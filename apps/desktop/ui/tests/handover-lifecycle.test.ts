// The lifecycle screen (SM-NS-9).
import { beforeAll, describe, expect, it } from 'vitest';

import { LifecycleController } from '../src/lib/views/handover/lifecycle-controller.svelte';
import LifecycleView from '../src/lib/views/handover/LifecycleView.svelte';
import { err, ok } from './support/fake-backend';
import { html, loadWindowCatalogue } from './support/ssr';
import { backendWith, text, WS } from './fixed-support';
import { uid } from './support/ids';

beforeAll(loadWindowCatalogue);

const lifecycle = { agent_repository_id: 'r', id: uid('lc1'), identifier: 'urn:x', name: 'Contract approval', part_id: 'p', stages: [], status: 'approved', version: '1' };
const stage = (name: string, over: Record<string, unknown> = {}) => ({ days_to_answer: 2, handover_place_id: 'pl', name, next: { kind: 'end' }, role: 'Reviewer', send_backs: [], ...over });

describe('Lifecycle screen (SM-NS-9)', () => {
  it('Edge case — a workspace with no lifecycle (SM-NS-9): handover.stage_of returns null and the screen reads "This workspace doesn\'t follow a stage of a lifecycle."', async () => {
    const { handle, state, transport } = await backendWith({ setup: (tr) => tr.on('handover.stage_of', ok(null)) });
    const c = new LifecycleController(handle, WS);
    await c.load();
    expect(transport.sentOf('handover.stage_of')[0]?.input).toEqual({ workspace: WS });
    expect(transport.sentOf('handover.lifecycle_view')).toHaveLength(0);
    expect(text(html(LifecycleView, { backend: state, c }))).toContain("This workspace doesn't follow a stage of a lifecycle.");
  });

  it('draws the stages in order with the number of handovers, where the workspace is, and each stage\'s handovers', async () => {
    const view = {
      lifecycle,
      stages: [
        { handovers: [], stage: stage('Draft', { agent: 'Drafter', hands_over: 'The first draft' }), you_are_here: false },
        { handovers: [{ direction: 'handed_over', id: 'h9', other_person: 'Bo', round: 1, state: 'seen', title: 'Supplier contract' }], stage: stage('Review'), you_are_here: true },
      ],
    };
    const { handle, state, transport } = await backendWith({ setup: (tr) => tr.on('handover.stage_of', ok({ hands_on: [], lifecycle, next: [], stage_name: 'Review' })).on('handover.lifecycle_view', ok(view)) });
    const c = new LifecycleController(handle, WS);
    await c.load();
    expect(transport.sentOf('handover.lifecycle_view')[0]?.input).toEqual({ id: uid('lc1') });
    const body = text(html(LifecycleView, { backend: state, c }));
    expect(body).toContain('Lifecycle: Contract approval');
    expect(body.indexOf('1. Draft')).toBeLessThan(body.indexOf('2. Review'));
    expect(body).toContain('You are here');
    expect(body).toContain('Agent: Drafter');
    expect(body).toContain('Hands over: The first draft');
    expect(body).toContain('Supplier contract');
    expect(body).toContain('Seen by Bo');
  });

  it('Error path — a refused read draws the owner\'s sentence and no stages', async () => {
    const { handle, state } = await backendWith({ setup: (tr) => tr.on('handover.stage_of', err('HAND_UNAVAILABLE', 'The lifecycle can\'t be read right now.')) });
    const c = new LifecycleController(handle, WS);
    await c.load();
    expect(text(html(LifecycleView, { backend: state, c }))).toContain("The lifecycle can't be read right now.");
  });
});
