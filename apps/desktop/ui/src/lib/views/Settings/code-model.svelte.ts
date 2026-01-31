// Settings › Code (11 §4, §5.11, UX-A25, UX-A26): which IDEs "Open code in" offers, and the code checking of
// each language. A language server's own settings are not drawn (G-CU-7); a change to a row sends the row's
// `server_settings` back unchanged.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-1
import type { BackendHandle } from '../../backend/backend';
import type { Ide, LanguagePlugin, ServiceError } from '../../service-api/types.gen';

/** The server a row uses: the chosen one, else the default. */
export const serverInUse = (plugin: LanguagePlugin): string => plugin.chosen_server ?? plugin.default_server;

export class CodeModel {
  ides = $state.raw<Ide[]>([]);
  plugins = $state.raw<LanguagePlugin[]>([]);
  error = $state<ServiceError | null>(null);
  /** The line that says what the last action did. */
  done = $state('');
  readonly #handle: BackendHandle;

  constructor(handle: BackendHandle) {
    this.#handle = handle;
  }

  async load(): Promise<void> {
    const [ides, plugins] = await Promise.all([this.#handle.call('projects.ides', {}), this.#handle.call('projects.language_plugins', {})]);
    if (ides.kind === 'ok') this.ides = ides.result;
    else this.error = ides.error;
    if (plugins.kind === 'ok') this.plugins = plugins.result;
    else this.error = plugins.error;
    if (ides.kind === 'ok' && plugins.kind === 'ok') this.error = null;
  }

  /** The switch keeps the person's choice while the command is in flight and returns on a refusal. */
  async setIde(ide: Ide, enabled: boolean): Promise<boolean> {
    const stored = this.ides;
    this.ides = stored.map((i) => (i.id === ide.id ? { ...i, enabled } : i));
    const out = await this.#handle.call('projects.set_ide_enabled', { ide_id: ide.id, enabled });
    if (out.kind === 'ok') {
      this.error = null;
      return true;
    }
    this.ides = stored;
    this.error = out.error;
    return false;
  }

  /** Sends the row with the one field changed and its `chosen_server` and `server_settings` otherwise as they are. */
  async configure(plugin: LanguagePlugin, change: { enabled?: boolean; chosen_server?: string }): Promise<boolean> {
    const stored = this.plugins;
    const enabled = change.enabled ?? plugin.enabled;
    const chosen = change.chosen_server ?? plugin.chosen_server;
    this.plugins = stored.map((p) => (p.id === plugin.id ? { ...p, enabled, chosen_server: chosen ?? null } : p));
    const out = await this.#handle.call('projects.configure_language', {
      language: plugin.language,
      enabled,
      chosen_server: chosen ?? null,
      // The backend takes only an object here; a row that came back without settings sends an empty one.
      server_settings: plugin.server_settings !== null && typeof plugin.server_settings === 'object' && !Array.isArray(plugin.server_settings) ? plugin.server_settings : {},
    });
    if (out.kind === 'ok') {
      this.plugins = stored.map((p) => (p.id === plugin.id ? out.result : p));
      this.error = null;
      return true;
    }
    this.plugins = stored;
    this.error = out.error;
    return false;
  }

  /** **Restart**, then the line that says it restarted. */
  async restart(plugin: LanguagePlugin, doneLine = ''): Promise<void> {
    this.done = '';
    const out = await this.#handle.call('projects.restart_language_server', { language: plugin.language });
    this.error = out.kind === 'error' ? out.error : null;
    if (out.kind === 'ok') this.done = doneLine;
  }
}
