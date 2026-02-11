// The backend handle and the registry of handles. Nothing here imports @tauri-apps/api (SA-UI-01);
// the transport is injected, and `tauri.ts` is the production one.
//
// spec: ordin-desktop/components/ScreensModule#SA-UI-01
import { API_VERSION, IPC_NAMES as BASE_IPC_NAMES, type Commands as BaseCommands } from '../service-api/commands.gen';
import { MINUTES_IPC_NAMES, type MinutesCommands } from '../service-api/minutes.gen';
import type { ServiceError, ServiceEvent, ServiceApiVersion } from '../service-api/types.gen';

// The rows of every build and the `minutes` rows a build with that switch adds (ServiceApi.md SVC-MIN-3).
// The screens are written against both; a backend without the switch does not name the `minutes` group in
// `service.hello`, so nothing that sends a `minutes.*` command is drawn for it (SM-UP-1).
//
// spec: ordin-desktop/components/ServiceApi#SVC-MIN-3
export type Commands = BaseCommands & MinutesCommands;
export const IPC_NAMES: Record<keyof Commands, string> = { ...BASE_IPC_NAMES, ...MINUTES_IPC_NAMES };

export type BackendId = string;
export type CommandName = keyof Commands;
export type InputOf<C extends CommandName> = Commands[C]['input'];
export type ResultOf<C extends CommandName> = Commands[C]['result'];

export type { ServiceError, ServiceEvent };

export type CommandOutcome<O> = { kind: 'ok'; result: O } | { kind: 'error'; error: ServiceError };

export interface CommandEnvelope<I> {
  api_version: ServiceApiVersion;
  request_id: string;
  command: CommandName;
  backend: BackendId;
  input: I;
}

/** `CommandReply<O>` = `{ request_id } & CommandOutcome<O>` (the reply always resolves). */
export type CommandReply<O> = { request_id: string } & CommandOutcome<O>;

export type EventEnvelope = { api_version: ServiceApiVersion; backend: BackendId; seq: number; at: string } & ServiceEvent;

export interface EventBatch {
  events: EventEnvelope[];
}
export interface EventsOpened {
  local_backend: BackendId;
  reset: BackendId[];
}
export interface StreamMessage {
  stream: string;
  kind: 'dictation' | 'terminal' | 'lsp';
  payload: unknown;
}
export type DesktopEventName = 'quit-requested' | 'quitting' | 'open-target' | 'appearance';

/** What the module needs from the Command Bridge. `tauri.ts` implements it; tests fake it. */
export interface Transport {
  /** Sends one envelope to `plugin:ordin|<ipcName>`. Rejects when the Bridge is unreachable. */
  invoke(ipcName: string, envelope: CommandEnvelope<unknown>): Promise<CommandReply<unknown>>;
  /** Opens `service_events`. `after` is the last seq drawn for each backend. */
  openEvents(after: Record<BackendId, number>, onBatch: (batch: EventBatch) => void): Promise<EventsOpened>;
  /** Opens `service_streams`, once, at start. */
  openStreams(onMessage: (message: StreamMessage) => void): Promise<void>;
  /** `listen('ordin://<name>')`; resolves to the function that stops listening. */
  listenDesktop(name: DesktopEventName, onEvent: (payload: unknown) => void): Promise<() => void>;
  /** The application version, read through the API's application-version call (SA-UI-UP-04). */
  appVersion(): Promise<string>;
}

export interface DiagnosticSink {
  record(code: string, detail?: string, backend?: BackendId): void;
}

export interface BackendHandle {
  readonly id: BackendId;
  readonly kind: 'local' | 'server';
  call<C extends CommandName>(command: C, input: InputOf<C>): Promise<CommandOutcome<ResultOf<C>>>;
}

export interface HandleOptions {
  id: BackendId;
  kind: 'local' | 'server';
  transport: Transport;
  diagnostics?: DiagnosticSink;
  /** Milliseconds after which a command that waits for no model is given up (default 10,000). */
  timeoutMs?: number;
  /** Called when a reply is `SVC_NOT_IN_THIS_VERSION`: the owner of the store reloads the backend (SM-UP-1). */
  onNotInThisVersion?: () => void;
  /** Creates a request id; replaceable for deterministic tests. */
  newRequestId?: () => string;
}

/**
 * A version-7 UUID in the 36-character lowercase hyphenated form: the only request id the core
 * accepts (`CommandRequestId::parse` refuses any other version). 48 bits of Unix milliseconds,
 * then random bits, with the version and variant bits set.
 */
export function newRequestId(now: number = Date.now()): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  for (let i = 0; i < 6; i += 1) {
    bytes[i] = Math.floor(now / 2 ** (8 * (5 - i))) & 0xff;
  }
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x70;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export const COMMAND_TIMEOUT_MS = 10_000;
/** The `ordin.*` conversation commands wait for the model, so they are never timed out here (chosen here). */
const WAITS_FOR_MODEL_GROUP = 'ordin';
// spec: ordin-desktop/components/ScreensModule#SM-FS-6
/**
 * Commands that wait for the person (a sign-in in the browser, a guided run waiting on one, or the operating
 * system's own dialog, ServiceApi.md SVC-R1-8): never timed out here. The Service API's catalogue test checks
 * this list against its `WaitsForPerson` rows, so the two cannot drift apart.
 */
export const WAITS_FOR_PERSON: ReadonlySet<string> = new Set([
  'setup.run_guided',
  'setup.sign_in_harness',
  'identity.sign_in_wait',
  'app.pick_folder',
  'files.save_export',
  'identity.import_client_certificate',
  // spec: ordin-desktop/components/ServiceApi#SVC-R1-10
  'files.save_copy',
  'diagnostics.support_export',
  // spec: ordin-desktop/components/ServiceApi#SVC-DBX-1
  'dashboards.export',
  'dashboards.add_to_presentation',
  // spec: ordin-desktop/components/ServiceApi#SVC-KEX-1
  'knowledge.export',
  // spec: ordin-desktop/components/ServiceApi#SVC-MIN-1
  // Answered only by a build with the minutes switch.
  'minutes.import',
  'minutes.export',
]);

function failure(code: string): CommandOutcome<never> {
  // The message is empty on purpose: `ErrorLine` draws the sentence for these two codes from the catalogue.
  return { kind: 'error', error: { code, message: '' } };
}

export function createHandle(options: HandleOptions): BackendHandle {
  const timeoutMs = options.timeoutMs ?? COMMAND_TIMEOUT_MS;
  const newId = options.newRequestId ?? (() => newRequestId());
  return {
    id: options.id,
    kind: options.kind,
    async call<C extends CommandName>(command: C, input: InputOf<C>): Promise<CommandOutcome<ResultOf<C>>> {
      const envelope: CommandEnvelope<InputOf<C>> = {
        api_version: API_VERSION,
        request_id: newId(),
        command,
        backend: options.id,
        input,
      };
      const group = command.slice(0, command.indexOf('.'));
      let timer: ReturnType<typeof setTimeout> | undefined;
      const sent = options.transport.invoke(IPC_NAMES[command], envelope);
      const guarded =
        group === WAITS_FOR_MODEL_GROUP || WAITS_FOR_PERSON.has(command)
          ? sent
          : Promise.race([
              sent,
              new Promise<'timeout'>((resolve) => {
                timer = setTimeout(() => resolve('timeout'), timeoutMs);
              }),
            ]);
      try {
        const reply = await guarded;
        if (reply === 'timeout') {
          options.diagnostics?.record('UI_COMMAND_TIMEOUT', command, options.id);
          return failure('UI_COMMAND_TIMEOUT');
        }
        if (reply.kind === 'error' && reply.error.code === 'SVC_NOT_IN_THIS_VERSION') {
          options.onNotInThisVersion?.();
        }
        return reply.kind === 'ok'
          ? { kind: 'ok', result: reply.result as ResultOf<C> }
          : { kind: 'error', error: reply.error };
      } catch {
        options.diagnostics?.record('UI_IPC_FAILED', command, options.id);
        return failure('UI_IPC_FAILED');
      } finally {
        if (timer !== undefined) clearTimeout(timer);
      }
    },
  };
}

export class Backends {
  readonly local: BackendHandle;
  #servers = new Map<BackendId, BackendHandle>();

  constructor(local: BackendHandle) {
    this.local = local;
  }

  servers(): BackendHandle[] {
    return [...this.#servers.values()];
  }

  /** Replaces the server handles with those of the paired servers the local core reports. */
  setServers(handles: BackendHandle[]): void {
    this.#servers = new Map(handles.map((h) => [h.id, h]));
  }

  get(id: BackendId): BackendHandle | undefined {
    return id === this.local.id ? this.local : this.#servers.get(id);
  }

  all(): BackendHandle[] {
    return [this.local, ...this.servers()];
  }
}
