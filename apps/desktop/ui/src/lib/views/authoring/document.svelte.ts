// The document page and its editor (SM-NS-4; screen `document:<place id>:<path>`): the preview, the versions,
// the published copies of a Markdown document, Open in its app, the editor, restore and Move to Bin.
// Send back…, Publish a copy… and a copy's own actions are not drawn in R1 (G-CU-3).
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-4
// spec: ordin-desktop/components/ScreensModule#FILE_CHANGED_MEANWHILE
import { formatDate } from '../../format/intl';
import { t } from '../../i18n/fluent';
import type { FileVersion, FileVersionOrigin, Preview, PublishedCopy, ServiceError } from '../../service-api/types.gen';
import { Announcer, lastSegment, type Handle } from '../fixed/util';

/** Who changed a version, in words (SM-NS-4.2). */
export function originWords(origin: FileVersionOrigin): string {
  if (origin.kind === 'person') return t('doc-origin-person');
  if (origin.kind === 'run') return t('doc-origin-agent');
  return t('doc-origin-app', { app: origin.app });
}

export type OpenState = 'idle' | 'no_editor';

export class DocumentController {
  preview = $state.raw<Preview | null>(null);
  versions = $state.raw<FileVersion[]>([]);
  copies = $state.raw<PublishedCopy[]>([]);
  error = $state.raw<ServiceError | null>(null);
  saveError = $state.raw<ServiceError | null>(null);
  openState = $state<OpenState>('idle');
  editing = $state(false);
  draft = $state('');
  savedNotSent = $state(false);
  askingDiscard = $state(false);
  saving = $state(false);
  /** The name of the linked location the document lives in, once read (`workspaces.links`); else null. */
  placeName = $state<string | null>(null);
  readonly announcer = new Announcer();
  readonly #handle: Handle;
  readonly #onclosed: () => void;
  #original = '';

  constructor(
    handle: Handle,
    readonly placeId: string,
    readonly path: string,
    readonly workspaceId: string | null,
    onclosed: () => void = () => undefined,
  ) {
    this.#handle = handle;
    this.#onclosed = onclosed;
  }

  get name(): string {
    return lastSegment(this.path);
  }

  /**
   * Where the document lives, under its name (SM-NS-4, amended January 2026, user decision; Approved by the
   * user January 2026): the location's name, then the folders of `path`, joined by › ; null when neither is known.
   */
  get location(): string | null {
    const folders = this.path.split('/').slice(0, -1).filter((s) => s !== '');
    const parts = [...(this.placeName === null ? [] : [this.placeName]), ...folders];
    return parts.length === 0 ? null : parts.join(' › ');
  }

  /** Reads the name of the location the document lives in; a refusal leaves the folders alone. */
  async loadPlaceName(): Promise<void> {
    if (this.workspaceId === null) return;
    const out = await this.#handle.call('workspaces.links', { workspace_id: this.workspaceId });
    if (out.kind !== 'ok') return;
    this.placeName = out.result.find((link) => link.place_id === this.placeId)?.display_name ?? null;
  }

  /** Versions newest first. */
  get ordered(): FileVersion[] {
    return [...this.versions].sort((a, b) => b.version - a.version);
  }
  get newest(): FileVersion | null {
    return this.ordered[0] ?? null;
  }
  get glance(): string | null {
    const v = this.newest;
    return v === null ? null : t('doc-glance', { date: formatDate(v.created_at), who: originWords(v.origin) });
  }
  get isMarkdown(): boolean {
    return this.preview?.kind === 'text' && this.preview.markdown;
  }
  /** **Edit** is offered on a Markdown text preview that is not cut (SM-NS-4.6). */
  get canEdit(): boolean {
    return this.preview?.kind === 'text' && this.preview.markdown && !this.preview.truncated;
  }
  get truncated(): boolean {
    return this.preview?.kind === 'text' && this.preview.truncated;
  }
  get dirty(): boolean {
    return this.editing && this.draft !== this.#original;
  }

  /** On opening: the preview, the versions and `home.document_opened` (SM-NS-4.1), then the copies of a Markdown document. */
  async load(): Promise<void> {
    const item = { kind: 'place_file' as const, place_id: this.placeId, relative_path: this.path };
    const [preview, versions] = await Promise.all([
      this.#handle.call('files.preview', { place_id: this.placeId, path: this.path }),
      this.#handle.call('files.versions', { place_id: this.placeId, path: this.path }),
      this.workspaceId === null ? Promise.resolve(null) : this.#handle.call('home.document_opened', { item, workspace_id: this.workspaceId }),
    ]);
    this.error = null;
    if (preview.kind === 'ok') this.preview = preview.result;
    else this.error = preview.error;
    if (versions.kind === 'ok') this.versions = versions.result;
    else this.error ??= versions.error;
    if (this.isMarkdown) await this.loadCopies();
  }

  async loadCopies(): Promise<void> {
    const out = await this.#handle.call('files.published_copies', { original: { kind: 'place_file', place_id: this.placeId, path: this.path } });
    if (out.kind === 'ok') this.copies = out.result;
    else this.error = out.error;
  }

  /** **Open in [app]**: `opened_in_app` and `opened_with_system_default` draw nothing; `no_editor` asks (SM-NS-4.2). */
  async openInApp(): Promise<void> {
    const out = await this.#handle.call('craft.open', { place_id: this.placeId, path: this.path });
    if (out.kind === 'error') return void (this.error = out.error);
    this.openState = out.result.kind === 'no_editor' ? 'no_editor' : 'idle';
  }

  async openUsualApp(): Promise<void> {
    const out = await this.#handle.call('craft.open_with_system_default', { place_id: this.placeId, path: this.path });
    if (out.kind === 'error') return void (this.error = out.error);
    this.openState = 'idle';
  }

  /** **Restore** after the confirmation (SM-NS-4.4). */
  async restore(version: FileVersion): Promise<void> {
    const out = await this.#handle.call('files.restore_version', { file_version_id: version.id });
    if (out.kind === 'error') return void (this.error = out.error);
    this.error = null;
    await this.load();
  }

  startEdit(): void {
    if (this.preview?.kind !== 'text' || !this.canEdit) return;
    this.#original = this.preview.text;
    this.draft = this.preview.text;
    this.saveError = null;
    this.savedNotSent = false;
    this.askingDiscard = false;
    this.editing = true;
  }

  /** **Save** sends `files.save_edit`; `FILE_CHANGED_MEANWHILE` keeps the editor open with the text typed (SM-NS-4.6). */
  async save(): Promise<boolean> {
    if (!this.editing || this.saving) return false;
    this.saving = true;
    const out = await this.#handle.call('files.save_edit', { place_id: this.placeId, path: this.path, text: this.draft });
    this.saving = false;
    if (out.kind === 'error') {
      this.saveError = out.error;
      return false;
    }
    this.saveError = null;
    this.editing = false;
    this.askingDiscard = false;
    this.savedNotSent = true;
    this.announcer.say(t('doc-saved-not-sent'));
    await this.load();
    return true;
  }

  /** **Cancel**: with changes it asks on the bar; without, the editor closes. */
  cancel(): void {
    if (this.dirty) this.askingDiscard = true;
    else this.editing = false;
  }
  discardChanges(): void {
    this.askingDiscard = false;
    this.editing = false;
    this.saveError = null;
  }
  keepChanging(): void {
    this.askingDiscard = false;
  }

  /** **Move to Bin** after the confirmation; the tab closes on `ok` (SM-NS-4.7). */
  async moveToBin(): Promise<boolean> {
    const out = await this.#handle.call('files.move_to_bin', { place_id: this.placeId, path: this.path });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.#onclosed();
    return true;
  }
}
