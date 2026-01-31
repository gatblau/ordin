// The import screen's state (SM-NS-10): what is brought in (an agent or one part and its kind), the
// agent repository it is for (only those the person can publish to), the folder, and **Import**.
// A file (an APM package) as the source is not offered in R1 (G-CU-10).
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-10
import type { DraftRef, PartKind, RepositorySummary, ServiceError } from '../../service-api/types.gen';
import { lastSegment, type Handle } from '../fixed/util';

export class ImportController {
  repositories = $state.raw<RepositorySummary[]>([]);
  what = $state<'agent' | 'part'>('agent');
  partKind = $state<PartKind | null>(null);
  repositoryId = $state<string | null>(null);
  folder = $state<string | null>(null);
  busy = $state(false);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly #picker: Handle;
  readonly #workspaceId: string;

  /** `picker` is the local backend: `app.pick_folder` is answered only by this computer. */
  constructor(handle: Handle, picker: Handle, workspaceId: string) {
    this.#handle = handle;
    this.#picker = picker;
    this.#workspaceId = workspaceId;
  }

  /** The agent repositories the person can publish to. */
  get choices(): RepositorySummary[] {
    return this.repositories.filter((r) => r.repository.person_can_publish);
  }

  get folderName(): string | null {
    return this.folder === null ? null : lastSegment(this.folder);
  }

  get ready(): boolean {
    return this.repositoryId !== null && this.folder !== null && this.#workspaceId !== '' && (this.what === 'agent' || this.partKind !== null);
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('catalogue.list_repositories', {});
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    this.repositories = out.result;
    this.error = null;
    if (this.repositoryId === null && this.choices.length === 1) this.repositoryId = this.choices[0]?.repository.id ?? null;
  }

  async chooseFolder(): Promise<void> {
    const out = await this.#picker.call('app.pick_folder', {});
    if (out.kind === 'error') {
      this.error = out.error;
      return;
    }
    if (out.result.folder != null) this.folder = out.result.folder;
  }

  /** Sends `authoring.import`; `as_part` is absent for an agent. Returns the draft to open. */
  async run(): Promise<DraftRef | null> {
    if (!this.ready || this.busy || this.repositoryId === null || this.folder === null) return null;
    this.busy = true;
    const out = await this.#handle.call('authoring.import', {
      agent_repository_id: this.repositoryId,
      source: this.folder,
      workspace_id: this.#workspaceId,
      ...(this.what === 'part' && this.partKind !== null ? { as_part: this.partKind } : {}),
    });
    this.busy = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return null;
    }
    this.error = null;
    return out.result;
  }
}

/** The screen an import opens: `agent-draft:<id>` or `part-draft:<id>`. */
export const draftScreenKey = (ref: DraftRef): string => `${ref.kind === 'agent' ? 'agent-draft' : 'part-draft'}:${ref.id}`;
