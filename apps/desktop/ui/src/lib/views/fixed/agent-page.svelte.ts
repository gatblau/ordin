// The page of one agent (SM-AG-7): what it does, its recent tasks in this workspace, a waiting update and
// its choices. Read with `agents.get`, `agents.made_of` and `runs.list` (the workspace's runs, kept when
// their agent is this one), again on each `workspace_changed`.
//
// spec: ordin-desktop/components/ScreensModule#SM-AG-7
import type { BackendHandle } from '../../backend/backend';
import type { InstalledAgent, InstalledMadeOf, Run, ServiceError } from '../../service-api/types.gen';

/** The most recent tasks the page lists. */
export const RECENT_TASKS = 5;

/** The agent's own runs among `runs`, newest first, at most `RECENT_TASKS`. */
export function tasksOf(runs: readonly Run[], agentInstanceId: string): Run[] {
  return runs
    .filter((r) => r.agent.kind === 'agent_instance' && r.agent.agent_instance_id === agentInstanceId)
    .sort((a, b) => Date.parse(b.queued_at) - Date.parse(a.queued_at))
    .slice(0, RECENT_TASKS);
}

export class AgentPage {
  agent = $state.raw<InstalledAgent | null>(null);
  madeOf = $state.raw<InstalledMadeOf | null>(null);
  tasks = $state.raw<Run[]>([]);
  error = $state.raw<ServiceError | null>(null);
  loaded = $state(false);
  readonly agentInstanceId: string;
  readonly #handle: Pick<BackendHandle, 'call'>;
  readonly #workspaceId: string;
  /** False when the backend does not answer `runs.list` (SM-UP-1): the page then lists no tasks. */
  readonly #listsRuns: boolean;

  constructor(handle: Pick<BackendHandle, 'call'>, workspaceId: string, agentInstanceId: string, listsRuns = true) {
    this.#handle = handle;
    this.#workspaceId = workspaceId;
    this.agentInstanceId = agentInstanceId;
    this.#listsRuns = listsRuns;
  }

  async load(): Promise<void> {
    const [got, made, runs] = await Promise.all([
      this.#handle.call('agents.get', { agent_instance_id: this.agentInstanceId }),
      this.#handle.call('agents.made_of', { agent_instance_id: this.agentInstanceId, detail: 'business_user' }),
      this.#listsRuns ? this.#handle.call('runs.list', { workspace_id: this.#workspaceId }) : Promise.resolve(null),
    ]);
    this.loaded = true;
    if (got.kind === 'error') {
      this.error = got.error;
      this.agent = null;
      return;
    }
    this.agent = got.result;
    this.madeOf = made.kind === 'ok' ? made.result : null;
    this.tasks = runs !== null && runs.kind === 'ok' ? tasksOf(runs.result.items, this.agentInstanceId) : [];
    this.error = made.kind === 'error' ? made.error : runs !== null && runs.kind === 'error' ? runs.error : null;
  }

  /** The agent's state after an update was accepted or declined. */
  replace(agent: InstalledAgent): void {
    this.agent = agent;
  }
}
