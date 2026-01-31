// The agent repository screens (SM-NS-7): Parts of [display name] (`repository-parts:<id>`, Parts, Words, Layout,
// Health, New part…) and Withdraw [part] (`withdraw-part:<part id>`). The part draft is `draft.svelte.ts`.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-7
import type {
  HealthReport,
  KindLine,
  LibraryEntry,
  PartHistory,
  PartKind,
  PartView,
  RepositorySummary,
  ServiceError,
  UsedBy,
  VocabularyTerm,
} from '../../service-api/types.gen';
import type { Handle } from '../fixed/util';

export type RepoTab = 'parts' | 'words' | 'layout' | 'health';
export type Show = 'in_use' | 'not_for_new' | 'withdrawn';

/** The id of a part state's words (SM-NS-7.2). */
export const STATE_MESSAGE: Readonly<Record<PartView['state_label'], string>> = {
  current: 'repo-state-current',
  not_for_new_agents: 'repo-state-not-for-new',
  withdrawn: 'repo-state-withdrawn',
  not_offered: 'repo-state-not-offered',
};

/** Which rows a **Show** choice keeps. A part read but not offered is listed with the ones in use. */
export function matchesShow(show: Show, label: PartView['state_label']): boolean {
  if (show === 'in_use') return label === 'current' || label === 'not_offered';
  if (show === 'not_for_new') return label === 'not_for_new_agents';
  return label === 'withdrawn';
}

/** The health group of a line: the message id of its heading (SM-NS-7.2). */
export const HEALTH_MESSAGE: Readonly<Record<HealthReport['lines'][number]['kind'], string>> = {
  unused_part: 'repo-health-unused',
  look_alike: 'repo-health-look-alike',
  shared_identifier: 'repo-health-shared-identifier',
  broken_borrowed_reference: 'repo-health-broken-reference',
  used_while_deprecated: 'repo-health-used-deprecated',
  used_while_withdrawn: 'repo-health-used-withdrawn',
  uncited_term: 'repo-health-uncited-term',
  journey_not_offered: 'repo-health-journey-not-offered',
};

export function healthGroups(report: HealthReport): Array<{ kind: HealthReport['lines'][number]['kind']; count: number }> {
  const counts = new Map<HealthReport['lines'][number]['kind'], number>();
  for (const line of report.lines) counts.set(line.kind, (counts.get(line.kind) ?? 0) + 1);
  return [...counts].map(([kind, count]) => ({ kind, count }));
}

export class RepositoryPartsController {
  repositories = $state.raw<RepositorySummary[]>([]);
  repoId = $state('');
  tab = $state<RepoTab>('parts');
  query = $state('');
  show = $state<Show>('in_use');
  entries = $state.raw<LibraryEntry[]>([]);
  selected = $state.raw<PartView | null>(null);
  usedBy = $state.raw<UsedBy | null>(null);
  history = $state.raw<PartHistory | null>(null);
  vocabulary = $state.raw<VocabularyTerm[]>([]);
  layout = $state.raw<KindLine[]>([]);
  health = $state.raw<HealthReport | null>(null);
  error = $state.raw<ServiceError | null>(null);
  newPartOpen = $state(false);
  newKind = $state<PartKind>('Skill');
  newName = $state('');
  readonly #handle: Handle;
  readonly #onopen: (key: string) => void;

  constructor(handle: Handle, repoId: string, onopen: (key: string) => void = () => undefined) {
    this.#handle = handle;
    this.repoId = repoId;
    this.#onopen = onopen;
  }

  /** The repositories the person can publish to; **Agent repository:** switches among them. */
  get publishable(): RepositorySummary[] {
    return this.repositories.filter((r) => r.repository.person_can_publish);
  }
  get current(): RepositorySummary | null {
    return this.publishable.find((r) => r.repository.id === this.repoId) ?? null;
  }
  get isApm(): boolean {
    return this.current?.repository.format === 'apm';
  }
  /** The entries a **Show** choice keeps. */
  get shown(): LibraryEntry[] {
    return this.entries.filter((e) => matchesShow(this.show, e.part.state_label));
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('catalogue.list_repositories', {});
    if (out.kind === 'error') return void (this.error = out.error);
    this.error = null;
    this.repositories = out.result;
    if (this.current !== null) await this.search();
  }

  async switchTo(id: string): Promise<void> {
    this.repoId = id;
    this.selected = null;
    this.tab = 'parts';
    await this.search();
  }

  async search(): Promise<void> {
    const text = this.query.trim();
    const out = await this.#handle.call('catalogue.search', {
      repositories: [this.repoId],
      kinds: [],
      include_not_for_new: this.show !== 'in_use',
      ...(text === '' ? {} : { text }),
    });
    if (out.kind === 'error') return void (this.error = out.error);
    this.error = null;
    this.entries = out.result.items;
  }

  choose(entry: LibraryEntry): void {
    this.selected = entry.part;
    this.usedBy = null;
    this.history = null;
  }

  async loadUsedBy(): Promise<void> {
    if (this.selected === null) return;
    const out = await this.#handle.call('catalogue.used_by', { id: this.selected.part.id });
    if (out.kind === 'ok') this.usedBy = out.result;
    else this.error = out.error;
  }

  async loadHistory(): Promise<void> {
    if (this.selected === null) return;
    const out = await this.#handle.call('catalogue.part_history', { id: this.selected.part.id });
    if (out.kind === 'ok') this.history = out.result;
    else this.error = out.error;
  }

  async openTab(tab: RepoTab): Promise<void> {
    this.tab = tab;
    if (tab === 'words') {
      const out = await this.#handle.call('catalogue.vocabulary', { id: this.repoId });
      if (out.kind === 'ok') this.vocabulary = out.result;
      else this.error = out.error;
    } else if (tab === 'layout') {
      const out = await this.#handle.call('catalogue.layout', { id: this.repoId });
      if (out.kind === 'ok') this.layout = out.result.kinds;
      else this.error = out.error;
    } else if (tab === 'health') {
      const out = await this.#handle.call('catalogue.health', { id: this.repoId });
      if (out.kind === 'ok') this.health = out.result;
      else this.error = out.error;
    }
  }

  /** **Read it** opens the library part's page (SM-NS-15.2); **Withdraw…** opens the withdraw screen. */
  readIt(): void {
    if (this.selected !== null) this.#onopen(`part-page:library:${this.selected.part.id}`);
  }
  withdraw(): void {
    if (this.selected !== null) this.#onopen(`withdraw-part:${this.selected.part.id}`);
  }

  /** **New part…** sends `authoring.new_part` and opens the part draft it returns. */
  async newPart(workspaceLinksCode: boolean): Promise<void> {
    const name = this.newName.trim();
    if (name === '') return;
    const out = await this.#handle.call('authoring.new_part', {
      agent_repository_id: this.repoId,
      kind: this.newKind,
      name,
      workspace_links_code: workspaceLinksCode,
    });
    if (out.kind === 'error') return void (this.error = out.error);
    this.newPartOpen = false;
    this.newName = '';
    this.#onopen(`part-draft:${out.result.draft.id}`);
  }
}

export class WithdrawController {
  part = $state.raw<PartView | null>(null);
  usedBy = $state.raw<UsedBy | null>(null);
  replacements = $state.raw<LibraryEntry[]>([]);
  reason = $state('');
  replacement = $state('');
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly #onopen: (key: string) => void;

  constructor(handle: Handle, readonly partId: string, onopen: (key: string) => void = () => undefined) {
    this.#handle = handle;
    this.#onopen = onopen;
  }

  /** The reason is required and at most 1,000 characters (SM-NS-7.4). */
  get reasonValid(): boolean {
    const n = [...this.reason.trim()].length;
    return n >= 1 && n <= 1000;
  }

  async load(): Promise<void> {
    const [part, used] = await Promise.all([
      this.#handle.call('catalogue.part', { id: this.partId }),
      this.#handle.call('catalogue.used_by', { id: this.partId }),
    ]);
    if (part.kind === 'error') return void (this.error = part.error);
    this.error = null;
    this.part = part.result;
    if (used.kind === 'ok') this.usedBy = used.result;
    else this.error = used.error;
    const found = await this.#handle.call('catalogue.search', {
      repositories: [part.result.part.agent_repository_id],
      kinds: [part.result.part.kind],
      include_not_for_new: false,
    });
    if (found.kind === 'ok') this.replacements = found.result.items.filter((e) => e.part.part.id !== this.partId);
  }

  /** **Withdraw…** sends `authoring.withdraw`; its `PartDraftView` opens `part-draft:<id>`. */
  async withdraw(): Promise<void> {
    if (!this.reasonValid) return;
    const out = await this.#handle.call('authoring.withdraw', {
      part: this.partId,
      reason: this.reason.trim(),
      ...(this.replacement === '' ? {} : { replacement: this.replacement }),
    });
    if (out.kind === 'error') return void (this.error = out.error);
    this.#onopen(`part-draft:${out.result.draft.id}`);
  }
}
