// Settings › Agent repositories: the rows of `catalogue.list_repositories`, **Rename…**, **Remove…** and
// **Add an agent repository…** (look it up, then add).
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-1
import type { BackendHandle } from '../../backend/backend';
import type { RepositoryProbe, RepositorySummary, ServiceError } from '../../service-api/types.gen';

/** The address field holds at most 2,048 bytes. */
export const MAX_ADDRESS_BYTES = 2048;
export const NAME_MIN = 1;
export const NAME_MAX = 80;

export const byteLength = (text: string): number => new TextEncoder().encode(text).length;
export const validName = (text: string): boolean => text.trim().length >= NAME_MIN && [...text.trim()].length <= NAME_MAX;

export class ReposModel {
  items = $state.raw<RepositorySummary[]>([]);
  error = $state<ServiceError | null>(null);
  /** The row being renamed, and the text in the dialog's field. */
  renaming = $state.raw<RepositorySummary | null>(null);
  renameText = $state('');
  removing = $state.raw<RepositorySummary | null>(null);
  adding = $state(false);
  address = $state('');
  probe = $state.raw<RepositoryProbe | null>(null);
  name = $state('');
  /** A refusal inside the add dialog (the fields keep what was typed). */
  dialogError = $state<ServiceError | null>(null);
  /** The line that says what **Add** did. */
  done = $state('');
  readonly #handle: BackendHandle;
  readonly #now: () => number;

  constructor(handle: BackendHandle, now: () => number = Date.now) {
    this.#handle = handle;
    this.#now = now;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('catalogue.list_repositories', {});
    if (out.kind === 'ok') {
      this.items = out.result;
      this.error = null;
    } else this.error = out.error;
  }

  startRename(row: RepositorySummary): void {
    this.renaming = row;
    this.renameText = row.repository.display_name;
  }

  /** **Rename**: 1–80 characters; a refusal is drawn where the row is. */
  async rename(): Promise<boolean> {
    const row = this.renaming;
    if (row === null || !validName(this.renameText)) return false;
    const out = await this.#handle.call('catalogue.rename_repository', { id: row.repository.id, display_name: this.renameText.trim() });
    this.renaming = null;
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    await this.load();
    return true;
  }

  async remove(): Promise<boolean> {
    const row = this.removing;
    if (row === null) return false;
    const out = await this.#handle.call('catalogue.remove_repository', { id: row.repository.id });
    this.removing = null;
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    await this.load();
    return true;
  }

  openAdd(): void {
    this.adding = true;
    this.address = '';
    this.probe = null;
    this.name = '';
    this.dialogError = null;
  }

  closeAdd(): void {
    this.adding = false;
    this.probe = null;
  }

  get addressValid(): boolean {
    return this.address.trim() !== '' && byteLength(this.address.trim()) <= MAX_ADDRESS_BYTES;
  }

  /** **Look it up**: `catalogue.probe_repository { web_address }`; the name field is preset to `suggested_name`. */
  async lookUp(): Promise<boolean> {
    if (!this.addressValid) return false;
    const out = await this.#handle.call('catalogue.probe_repository', { web_address: this.address.trim() });
    if (out.kind === 'error') {
      this.dialogError = out.error;
      return false;
    }
    this.dialogError = null;
    this.probe = out.result;
    this.name = out.result.suggested_name;
    return true;
  }

  /** **Add**: a probe past its `expires_at` is looked up again first; the typed name is kept. */
  async add(addedLine?: (name: string, count: number) => string): Promise<boolean> {
    if (this.probe === null || !validName(this.name)) return false;
    if (Date.parse(this.probe.expires_at) <= this.#now()) {
      const typed = this.name;
      if (!(await this.lookUp())) return false;
      this.name = typed;
    }
    const probe = this.probe;
    if (probe === null) return false;
    const out = await this.#handle.call('catalogue.add_repository', { probe_id: probe.probe_id, display_name: this.name.trim() });
    if (out.kind === 'error') {
      this.dialogError = out.error;
      return false;
    }
    const name = this.name.trim();
    this.closeAdd();
    await this.load();
    this.done = addedLine?.(name, probe.agent_count) ?? '';
    return true;
  }
}
