// What Ordin used for an answer (ADR-111 step 7) and Remembered during this run (step 8). Nothing is
// fetched until the person chooses the link or opens the disclosure. The pages its facts and links name
// are read by id, once each (SM-CU-11).
//
// spec: ordin-desktop/components/ScreensModule#ADR-111
// spec: ordin-desktop/components/ScreensModule#SM-CU-11
import type { FactView, LinkView, RememberedItem, ServiceError } from '../../service-api/types.gen';
import { AgentNames, notPassage } from './facts';
import { MemoryController } from './memory-controller.svelte';
import { pageIdsOf, type PageTitleMap, type PageTitles } from './page-titles.svelte';
import type { Handle } from './util';

export class UsedController {
  facts = $state.raw<FactView[]>([]);
  links = $state.raw<LinkView[]>([]);
  gone = $state(0);
  /** The pages the facts and links name, each read once with `knowledge.get`. */
  readonly pages: PageTitles;
  loaded = $state(false);
  error = $state.raw<ServiceError | null>(null);
  /** The same actions as the Facts and Things tabs: Forget asks first and confirms. */
  readonly memory: MemoryController;
  readonly #handle: Handle;
  readonly workspaceId: string;
  readonly recallIds: readonly string[];

  constructor(handle: Handle, workspaceId: string, recallIds: readonly string[]) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
    this.recallIds = recallIds;
    this.memory = new MemoryController(handle, workspaceId);
    this.pages = this.memory.pages;
  }

  /** Page titles by id; `null` for a page that is gone (SM-CU-11). */
  get pageTitles(): PageTitleMap {
    return this.pages.titles;
  }

  /** An answer with no `memory_used` shows no link and never sends `memory.used`. */
  get offered(): boolean {
    return this.recallIds.length > 0;
  }

  async load(): Promise<void> {
    if (!this.offered) return;
    const out = await this.#handle.call('memory.used', { workspace_id: this.workspaceId, recall_ids: [...this.recallIds] });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    // A recall result that is a page section is never drawn in a fact list (SM-SF-18).
    // spec: ordin-desktop/components/ScreensModule#SM-SF-18
    const facts = out.result.facts.filter(notPassage);
    this.facts = facts;
    this.links = out.result.links;
    this.gone = out.result.gone;
    this.memory.facts = facts;
    this.loaded = true;
    await this.memory.loadAgents();
    await this.pages.read(pageIdsOf(facts, out.result.links));
  }

  /** Forgetting from this panel removes the line once the person confirmed. */
  async confirmForget(): Promise<void> {
    await this.memory.confirmPending();
    this.facts = this.memory.facts;
  }
}

export class RememberedController {
  items = $state.raw<RememberedItem[]>([]);
  open = $state(false);
  loaded = $state(false);
  names = $state.raw(new AgentNames());
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly runId: string;
  readonly workspaceId: string | null;

  constructor(handle: Handle, runId: string, workspaceId: string | null) {
    this.#handle = handle;
    this.runId = runId;
    this.workspaceId = workspaceId;
  }

  async toggle(): Promise<void> {
    this.open = !this.open;
    if (this.open) await this.load();
  }

  /** Opened: sent on opening and again on each `run_progress` while open and the run is running. */
  async load(): Promise<void> {
    const out = await this.#handle.call('memory.remembered_in_run', { run_id: this.runId });
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.error = null;
    this.items = out.result.items;
    this.loaded = true;
    if (this.workspaceId !== null) {
      const agents = await this.#handle.call('agents.list', { workspace_id: this.workspaceId });
      if (agents.kind === 'ok') this.names = new AgentNames(agents.result);
    }
  }

  /** `memory-remembered-by` with the agent's name, or `memory-remembered-by-ordin`. */
  byLine(item: RememberedItem): { id: string; args: Record<string, string> } {
    if (item.by.kind === 'ordin') return { id: 'memory-remembered-by-ordin', args: {} };
    const agent = this.names.nameOf(item.by.agent_instance_id);
    return agent === null ? { id: 'memory-remembered-by-ordin', args: {} } : { id: 'memory-remembered-by', args: { agent } };
  }
}
