// The crew plan block's reads and its **Start** (ScreensModule.md SM-WR-15): the charter by `crews.charter`,
// the agents' names by `agents.list`, and `crews.start` once at a time. The component only draws this.
//
// spec: ordin-desktop/components/ScreensModule#SM-WR-15
import type { BackendState } from '../store/backend-state.svelte';
import type { CrewCharter, ServiceError } from '../service-api/types.gen';
import { AgentNames } from '../views/fixed/facts';
import { dismissBlock } from './dismiss';

/** One line of the limits list: the message id and its arguments. */
export interface LimitLine {
  id: string;
  args: Record<string, number | string>;
}

/**
 * The limits of a charter in the order SM-WR-15.4 states: rounds, minutes (the seconds divided by 60 and
 * rounded up), requests (only when there is a budget), agents at the same time, the failure repeat limit, then the approval points when
 * there are any. `format` turns a number into its text (the window's `formatNumber`).
 */
export function limitLines(charter: CrewCharter, format: (n: number) => string): LimitLine[] {
  const g = charter.guardrails;
  const lines: LimitLine[] = [
    { id: 'block-crew-limit-rounds', args: { count: format(g.max_rounds) } },
    { id: 'block-crew-limit-time', args: { minutes: format(Math.ceil(g.wall_clock_secs / 60)) } },
    // SM-WR-15.4: a request budget of 0 is no budget, so no requests line is drawn.
    ...(g.request_budget > 0 ? [{ id: 'block-crew-limit-requests', args: { count: format(g.request_budget) } }] : []),
    { id: 'block-crew-limit-running', args: { count: format(g.max_members_running) } },
    { id: 'block-crew-limit-repeat', args: { count: format(g.failure_repeat_limit) } },
  ];
  if (g.approval_points.length > 0) lines.push({ id: 'block-crew-approval-points', args: { points: g.approval_points.join(', ') } });
  return lines;
}

/** What the plan says about starting: a button, "Started", or that it can't be started (SM-WR-15.5). */
export type StartState = 'button' | 'started' | 'not-startable';

export class CrewPlanController {
  charter = $state.raw<CrewCharter | null>(null);
  names = $state.raw(new AgentNames());
  /** The backend has no `crews.charter`: the estimate is drawn alone. */
  unavailable = $state(false);
  error = $state.raw<ServiceError | null>(null);
  startError = $state.raw<ServiceError | null>(null);
  busy = $state(false);
  /** `crews.start` answered `ok` here. */
  startedHere = $state(false);

  readonly #backend: BackendState;
  readonly #workspaceId: string;
  readonly #blockId: string;
  readonly #crewCharterId: string;

  constructor(backend: BackendState, workspaceId: string, blockId: string, crewCharterId: string) {
    this.#backend = backend;
    this.#workspaceId = workspaceId;
    this.#blockId = blockId;
    this.#crewCharterId = crewCharterId;
  }

  /** Whether the plan's item still waits for the person (read as `Question` reads it). */
  get waiting(): boolean {
    return this.#backend.conversation.get(this.#workspaceId)?.find((i) => i.block_id === this.#blockId)?.waiting ?? true;
  }

  get startState(): StartState | null {
    if (this.charter === null) return null;
    if (this.startedHere || this.charter.state === 'started') return 'started';
    return this.charter.state === 'valid' ? 'button' : 'not-startable';
  }

  /** Start is drawn when the plan is valid and waiting, the backend can be reached and answers `crews` (SM-WR-15.5). */
  get offersStart(): boolean {
    const b = this.#backend;
    const reachable = !(b.kind === 'server' && b.connection !== 'connected');
    return this.startState === 'button' && this.waiting && reachable && b.groups.has('crews');
  }

  /** Start may be pressed: it is drawn and no press is awaiting its reply. */
  get canStart(): boolean {
    return this.offersStart && !this.busy;
  }

  /** SM-WR-15.1: the charter, then the agents' names. */
  async load(): Promise<void> {
    const b = this.#backend;
    if (!b.groups.has('crews')) {
      this.unavailable = true;
      return;
    }
    const out = await b.handle.call('crews.charter', { crew_charter_id: this.#crewCharterId });
    if (out.kind === 'error') {
      if (out.error.code === 'SVC_NOT_IN_THIS_VERSION') this.unavailable = true;
      else this.error = out.error;
      return;
    }
    this.error = null;
    this.charter = out.result.charter;
    if (b.groups.has('agents')) {
      const agents = await b.handle.call('agents.list', { workspace_id: this.#workspaceId });
      if (agents.kind === 'ok') this.names = new AgentNames(agents.result);
    }
  }

  /** The line of one member: the agent's name, or none when `agents.list` does not hold it. */
  agentName(member: CrewCharter['members'][number]): string | null {
    return member.member.kind === 'agent_instance' ? this.names.nameOf(member.member.agent_instance_id) : null;
  }

  /** SM-WR-15.5: `crews.start` once at a time; `ok` marks the item answered. */
  async start(): Promise<void> {
    if (!this.canStart) return;
    this.busy = true;
    const out = await this.#backend.handle.call('crews.start', { crew_charter_id: this.#crewCharterId });
    this.busy = false;
    if (out.kind === 'ok') {
      this.startError = null;
      this.startedHere = true;
      this.#backend.markAnswered(this.#workspaceId, this.#blockId);
    } else {
      this.startError = out.error;
    }
  }

  /** SM-DIS-1: **Not now** sets the plan aside (`ordin.answer_block`, dismissed); no crew starts. */
  async notNow(): Promise<void> {
    if (!this.canStart || !this.#backend.groups.has('ordin')) return;
    this.busy = true;
    this.startError = await dismissBlock(this.#backend, this.#workspaceId, this.#blockId);
    this.busy = false;
  }
}
