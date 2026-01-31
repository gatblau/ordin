// The crew plan block (ScreensModule.md SM-WR-15): the charter read, the agents' names, the limits and
// **Start**. Rendering has no DOM, so the reads and the press are driven through the controller.
import { beforeAll, describe, expect, it } from 'vitest';

import { CrewPlanController, limitLines } from '../src/lib/blocks/crew-plan.svelte';
import { formatNumber } from '../src/lib/format/intl';
import { stripIsolates, t as rawT } from '../src/lib/i18n/fluent';
import { err, ok, R1_GROUPS } from './support/fake-backend';
import { loadWindowCatalogue } from './support/ssr';
import { backendWith, WS } from './fixed-support';

beforeAll(loadWindowCatalogue);
const t = (id: string, args?: Record<string, string | number>) => stripIsolates(rawT(id, args));

const CHARTER = '0192a1c5-1111-7222-8333-444455556666';
const BLOCK = 'b1';
const KESTREL = '0192a0f9-aaaa-7bbb-8ccc-ddddeeee0001';
const BOREALIS = '0192a0f9-aaaa-7bbb-8ccc-ddddeeee0002';
const GROUPS = [...R1_GROUPS, 'crews'];

const charter = (over: Record<string, unknown> = {}) =>
  ({
    id: CHARTER,
    workspace_id: WS,
    purpose: 'Compare the three quotes',
    charter_version: 1,
    coordinator: 'ordin',
    members: [
      { member: { kind: 'agent_instance', agent_instance_id: KESTREL }, role: 'Finder' },
      { member: { kind: 'agent_instance', agent_instance_id: BOREALIS }, role: 'Checker' },
      { member: { kind: 'remote_agent', remote_agent_id: '0192a0f9-aaaa-7bbb-8ccc-ddddeeee0003' }, role: 'Reviewer' },
    ],
    allowed_handoffs: [],
    guardrails: {
      max_rounds: 6,
      wall_clock_secs: 1201,
      request_budget: 40,
      max_members_running: 2,
      members_allowed_external_actions: [],
      approval_points: ['sending email', 'publishing'],
      failure_repeat_limit: 2,
    },
    input_fields: [],
    steps: [],
    state: 'valid',
    written_at: '2026-01-10T10:00:00Z',
    ...over,
  }) as never;

const agent = (id: string, name: string) => ({ instance: { id, display_name: name } }) as never;

async function plan(_unused?: undefined, options: { charter?: unknown; groups?: string[]; kind?: 'local' | 'server' } = {}) {
  const made = await backendWith({
    groups: options.groups ?? GROUPS,
    ...(options.kind === undefined ? {} : { kind: options.kind }),
    setup: (tr) => {
      tr.on('crews.charter', ok({ charter: options.charter ?? charter() }));
      tr.on('agents.list', ok([agent(KESTREL, 'Kestrel buyer'), agent(BOREALIS, 'Borealis')]));
      tr.on('crews.start', ok({ crew_run_id: '0192a1c5-9999-7222-8333-444455556666' }));
    },
  });
  made.state.conversation.set(WS, [{ block_id: BLOCK, workspace_id: WS, block: { kind: 'crew_plan' } as never, seq: 1, at: '', waiting: true }]);
  made.transport.sent.length = 0;
  return { ...made, controller: new CrewPlanController(made.state, WS, BLOCK, CHARTER) };
}

describe('SM-WR-15 — the crew plan block', () => {
  it('Happy path — the purpose, the agents by name and the limits are read, and Start is offered', async () => {
    const { controller, transport } = await plan();
    await controller.load();
    expect(transport.sentOf('crews.charter')[0]?.input).toEqual({ crew_charter_id: CHARTER });
    expect(transport.sentOf('agents.list')[0]?.input).toEqual({ workspace_id: WS });
    expect(controller.charter?.purpose).toBe('Compare the three quotes');
    const lines = controller.charter!.members.map((m) => t('block-crew-member', { agent: controller.agentName(m) ?? t('block-crew-agent-unknown'), role: m.role }));
    expect(lines).toEqual(['Kestrel buyer: Finder', 'Borealis: Checker', 'An agent: Reviewer']);
    expect(controller.offersStart).toBe(true);
  });

  it('the limits come in the stated order, the time in minutes rounded up, and the approval points only when there are some', async () => {
    const { controller } = await plan();
    await controller.load();
    const lines = limitLines(controller.charter!, formatNumber).map((l) => t(l.id, l.args));
    expect(lines).toEqual([
      'Rounds: up to 6',
      'Time: up to 21 min',
      'Requests: up to 40',
      'Agents at the same time: up to 2',
      'Same failure allowed: 2 times',
      'Asks you first: sending email, publishing',
    ]);
    const none = charter({ guardrails: { ...(charter() as { guardrails: object }).guardrails, approval_points: [] } });
    expect(limitLines(none, formatNumber)).toHaveLength(5);
  });

  it('Edge case — a charter with no request budget (0) draws no requests line', () => {
    const base = charter() as { guardrails: object };
    const unbudgeted = charter({ guardrails: { ...base.guardrails, request_budget: 0, approval_points: [] } });
    const lines = limitLines(unbudgeted, formatNumber).map((l) => t(l.id, l.args));
    expect(lines).toEqual(['Rounds: up to 6', 'Time: up to 21 min', 'Agents at the same time: up to 2', 'Same failure allowed: 2 times']);
  });

  it('Happy path — Start sends crews.start once, marks the item answered and reads "Started"', async () => {
    const { controller, transport, state } = await plan();
    await controller.load();
    const first = controller.start();
    const second = controller.start();
    await Promise.all([first, second]);
    expect(transport.sentOf('crews.start')).toHaveLength(1);
    expect(transport.sentOf('crews.start')[0]?.input).toEqual({ crew_charter_id: CHARTER });
    expect(controller.startState).toBe('started');
    expect(controller.offersStart).toBe(false);
    expect(state.conversation.get(WS)?.[0]?.waiting).toBe(false);
    expect(t('block-crew-started')).toBe('Started');
  });

  it('Error path — Start is refused: the error is kept and Start stays', async () => {
    const { controller, transport } = await plan();
    transport.on('crews.start', err('OH_CREW_RUNNING', 'A crew is already running in this workspace.'));
    await controller.load();
    await controller.start();
    expect(controller.startError?.code).toBe('OH_CREW_RUNNING');
    expect(controller.startState).toBe('button');
    expect(controller.offersStart).toBe(true);
  });

  it('Edge case — an older backend has no crews.charter: the estimate alone, no error and no Start', async () => {
    const { controller, transport } = await plan();
    transport.on('crews.charter', err('SVC_NOT_IN_THIS_VERSION', 'newer'));
    await controller.load();
    expect(controller.unavailable).toBe(true);
    expect(controller.error).toBeNull();
    expect(controller.offersStart).toBe(false);
    expect(controller.startState).toBeNull();
  });

  it('Edge case — a backend without the crews group is not asked', async () => {
    const { controller, transport } = await plan(undefined, { groups: R1_GROUPS });
    await controller.load();
    expect(transport.sentOf('crews.charter')).toHaveLength(0);
    expect(controller.unavailable).toBe(true);
  });

  it('Error path — any other error is drawn, with the estimate kept', async () => {
    const { controller, transport } = await plan();
    transport.on('crews.charter', err('OH_CHARTER_UNKNOWN', 'That plan is no longer there.'));
    await controller.load();
    expect(controller.error?.code).toBe('OH_CHARTER_UNKNOWN');
    expect(controller.unavailable).toBe(false);
    expect(controller.charter).toBeNull();
  });

  it('Edge case — a plan that was started elsewhere reads "Started" and has no button', async () => {
    const { controller } = await plan(undefined, { charter: charter({ state: 'started' }) });
    await controller.load();
    expect(controller.startState).toBe('started');
    expect(controller.offersStart).toBe(false);
  });

  it('Edge case — a plan that is written, discarded, kept or archived cannot be started', async () => {
    for (const state of ['written', 'discarded', 'kept', 'archived']) {
      const { controller } = await plan(undefined, { charter: charter({ state }) });
      await controller.load();
      expect(controller.startState, state).toBe('not-startable');
      expect(controller.offersStart, state).toBe(false);
    }
    expect(t('block-crew-not-startable')).toBe("This plan can't be started.");
  });

  it('Edge case — Start is not offered once the item no longer waits, or while a server is not connected', async () => {
    const waiting = await plan();
    await waiting.controller.load();
    waiting.state.markAnswered(WS, BLOCK);
    expect(waiting.controller.offersStart).toBe(false);
    const server = await plan(undefined, { kind: 'server' });
    await server.controller.load();
    server.state.setConnection('disconnected' as never);
    expect(server.controller.offersStart).toBe(false);
  });

  it('Edge case — agents.list failing leaves every agent unnamed', async () => {
    const { controller, transport } = await plan();
    transport.on('agents.list', err('AGENT_LIST_FAILED', 'no'));
    await controller.load();
    expect(controller.agentName(controller.charter!.members[0]!)).toBeNull();
  });
});
