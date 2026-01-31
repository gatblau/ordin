// Open in [IDE] (11 §5.11, UX-A25; amended January 2026 (user decision)): one action for each IDE found on
// this computer and ticked in Settings › Code › *Open code in*. It opens the IDE with
// `projects.open_in_ide`, writes nothing to the conversation, and a short notice confirms it.
//
// spec: ordin-desktop/components/ScreensModule#SM-SW-7
import type { BackendHandle, ServiceError } from '../../backend/backend';
import type { Ide } from '../../service-api/types.gen';

export class OpenInIdeModel {
  /** The IDEs found and ticked, in the order `projects.ides` gives. */
  ides = $state.raw<Ide[]>([]);
  error = $state<ServiceError | null>(null);
  /** The notice after an IDE opened. */
  notice = $state('');
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  /** `projects.ides`: only the IDEs ticked are offered; a refusal offers none. */
  async load(): Promise<void> {
    const out = await this.#handle.call('projects.ides', {});
    this.ides = out.kind === 'ok' ? out.result.filter((i) => i.enabled) : [];
  }

  /** Opens the repository (no `path`) or one of its files in the IDE. */
  async open(ide: Ide, projectId: string, path: string | null, notice: string): Promise<boolean> {
    this.notice = '';
    const out = await this.#handle.call('projects.open_in_ide', { project_id: projectId, ide_id: ide.id, path });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    this.notice = notice;
    return true;
  }
}
