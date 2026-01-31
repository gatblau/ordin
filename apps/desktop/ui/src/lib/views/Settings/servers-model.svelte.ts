// Settings › Servers (SM-UP-5, SM-LG-7): each server with its state, **Connect to a server…**,
// **Disconnect this computer…**, **Remove this server**, **Languages…**, and a personal server's update.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-5
// spec: ordin-desktop/components/ScreensModule#SM-LG-7
import type { BackendHandle } from '../../backend/backend';
import type { BackendState } from '../../store/backend-state.svelte';
import type { ServerUpdate, ServerView, ServiceError } from '../../service-api/types.gen';

/** What is drawn under a server's row about its update. */
export type ServerUpdateLine =
  | { kind: 'none' }
  | { kind: 'available'; update: ServerUpdate; selfUpdate: boolean }
  | { kind: 'started' };

/** An error drawn under a server's row with its owner's sentence. */
export interface RowError {
  error: ServiceError;
  /** **Try again** re-opens the confirmation (`SETUP_UPDATE_REPLACE_FAILED`). */
  retry: boolean;
}

/** Errors whose own sentence is drawn with a **Try again** that re-opens the confirmation (SM-UP-5.4). */
const REOPEN_CONFIRMATION = 'SETUP_UPDATE_REPLACE_FAILED';

export class ServersModel {
  servers = $state.raw<ServerView[]>([]);
  error = $state<ServiceError | null>(null);
  /** The connect form. */
  connecting = $state(false);
  address = $state('');
  code = $state('');
  connectError = $state<ServiceError | null>(null);
  busy = $state(false);
  /** By `server_connection_id`. */
  lines = $state.raw<Record<string, ServerUpdateLine>>({});
  rowErrors = $state.raw<Record<string, RowError>>({});
  /** The server whose Disconnect confirmation is open. */
  confirmDisconnect = $state<string | null>(null);
  /** The server whose Update server confirmation is open. */
  confirmUpdate = $state<string | null>(null);
  announcement = $state('');
  readonly #local: BackendHandle;
  readonly #stateFor: (backend: string) => BackendState | undefined;

  constructor(local: BackendHandle, stateFor: (backend: string) => BackendState | undefined) {
    this.#local = local;
    this.#stateFor = stateFor;
  }

  async load(): Promise<void> {
    const out = await this.#local.call('servers.list', {});
    if (out.kind === 'ok') {
      this.servers = out.result;
      this.error = null;
      await Promise.all(out.result.map((s) => this.loadUpdate(s)));
    } else this.error = out.error;
  }

  #setLine(id: string, line: ServerUpdateLine): void {
    this.lines = { ...this.lines, [id]: line };
  }

  /** `setup.server_update` to the server's own backend, when it is connected and loaded (SM-UP-5). */
  async loadUpdate(server: ServerView): Promise<void> {
    if (server.state !== 'connected' || server.backend === undefined || server.backend === null) {
      this.#setLine(server.server_connection_id, { kind: 'none' });
      return;
    }
    const state = this.#stateFor(server.backend);
    if (state === undefined || !state.loaded) {
      this.#setLine(server.server_connection_id, { kind: 'none' });
      return;
    }
    const out = await state.handle.call('setup.server_update', {});
    if (out.kind === 'ok') {
      const update = out.result.update ?? null;
      this.#setLine(
        server.server_connection_id,
        update === null ? { kind: 'none' } : { kind: 'available', update, selfUpdate: update.self_update },
      );
    } else if (out.error.code === 'SETUP_SELF_UPDATE_UNAVAILABLE') {
      this.#setLine(server.server_connection_id, { kind: 'none' });
    } else {
      this.#setLine(server.server_connection_id, { kind: 'none' });
      this.#setError(server.server_connection_id, { error: out.error, retry: false });
    }
  }

  #setError(id: string, error: RowError | null): void {
    const next = { ...this.rowErrors };
    if (error === null) delete next[id];
    else next[id] = error;
    this.rowErrors = next;
  }

  /** After `server_state_changed` for a server: the list and that server's update are read again. */
  async onServerStateChanged(serverConnectionId: string): Promise<void> {
    const out = await this.#local.call('servers.list', {});
    if (out.kind !== 'ok') return;
    this.servers = out.result;
    const server = out.result.find((s) => s.server_connection_id === serverConnectionId);
    if (server !== undefined) await this.loadUpdate(server);
  }

  line(id: string): ServerUpdateLine {
    return this.lines[id] ?? { kind: 'none' };
  }

  // ---- connect ------------------------------------------------------------------------------------

  openConnect(): void {
    this.connecting = true;
    this.connectError = null;
  }
  cancelConnect(): void {
    this.connecting = false;
    this.address = '';
    this.code = '';
    this.connectError = null;
  }

  /**
   * Address and code, or the one pasted line.
   * SPEC-GAP-2: ScreensModule.md says "(address and code, or the one pasted line)" and lists no message id
   * for a separate pasted-line field; with the code box empty the address box's text is sent as the line.
   */
  async connect(): Promise<boolean> {
    const address = this.address.trim();
    const code = this.code.trim();
    if (address === '') return false;
    this.busy = true;
    const out = await this.#local.call(
      'servers.connect',
      code === '' ? { kind: 'pairing_line', line: address } : { kind: 'address_and_code', address, code },
    );
    this.busy = false;
    if (out.kind === 'ok') {
      this.cancelConnect();
      await this.load();
      return true;
    }
    this.connectError = out.error;
    return false;
  }

  // ---- disconnect and remove ------------------------------------------------------------------------

  async disconnect(id: string): Promise<void> {
    this.confirmDisconnect = null;
    const out = await this.#local.call('servers.disconnect', { server_connection_id: id });
    if (out.kind === 'ok') await this.load();
    else this.error = out.error;
  }

  async remove(id: string): Promise<void> {
    const out = await this.#local.call('servers.remove', { server_connection_id: id });
    if (out.kind === 'ok') await this.load();
    else this.error = out.error;
  }

  // ---- update ---------------------------------------------------------------------------------------

  /** Confirming sends `setup.update_server { version }` to that server's backend (SM-UP-5.4). */
  async updateServer(server: ServerView): Promise<void> {
    this.confirmUpdate = null;
    this.#setError(server.server_connection_id, null);
    const line = this.line(server.server_connection_id);
    if (line.kind !== 'available' || server.backend === undefined || server.backend === null) return;
    const state = this.#stateFor(server.backend);
    if (state === undefined) return;
    const out = await state.handle.call('setup.update_server', { version: line.update.version });
    if (out.kind === 'ok' && out.result.accepted) {
      this.#setLine(server.server_connection_id, { kind: 'started' });
      this.announcement = server.display_name;
      return;
    }
    if (out.kind === 'error') {
      const retry = out.error.code === REOPEN_CONFIRMATION;
      // SETUP_UPDATE_REPLACE_FAILED is drawn with Try again, which re-opens the confirmation (SM-UP-5.4).
      const failed: RowError = { error: retry ? { ...out.error, action: 'try_again' } : out.error, retry };
      if (out.error.code === 'SETUP_UPDATE_VERSION_CHANGED') await this.loadUpdate(server);
      this.#setError(server.server_connection_id, failed);
    }
  }

  /** **Try again** after `SETUP_UPDATE_REPLACE_FAILED`: the confirmation opens again. */
  reopenUpdate(server: ServerView): void {
    this.#setError(server.server_connection_id, null);
    void this.loadUpdate(server).then(() => (this.confirmUpdate = server.server_connection_id));
  }
}
