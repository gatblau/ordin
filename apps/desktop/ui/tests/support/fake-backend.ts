// A recorded / scripted backend for the tests: a `Transport` that answers commands from a table of
// replies, records every envelope it was sent, and lets a test push events, stream messages and
// desktop events. No network, no Tauri.
import {
  IPC_NAMES,
  type Commands,
  type BackendId,
  type CommandEnvelope,
  type CommandName,
  type CommandOutcome,
  type CommandReply,
  type DesktopEventName,
  type EventBatch,
  type EventEnvelope,
  type EventsOpened,
  type ServiceEvent,
  type StreamMessage,
  type Transport,
} from '../../src/lib/backend/backend';

export const LOCAL: BackendId = '0192f100-0000-7000-8000-00000000000a';
export const SERVER: BackendId = '0192f100-0000-7000-8000-0000000000b1';

const ftl = import.meta.glob('../../../../../crates/ordin-localisation/catalogues/en-GB/*.ftl', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

/** The en-GB source of every `.ftl` file the core would serve, concatenated. */
export const EN_GB_FTL = Object.values(ftl).join('\n');

export type Reply = CommandOutcome<unknown> | ((input: unknown, envelope: CommandEnvelope<unknown>) => CommandOutcome<unknown> | Promise<CommandOutcome<unknown>>);

export const ok = <T>(result: T): CommandOutcome<T> => ({ kind: 'ok', result });
export const err = (code: string, message: string, extra: { action?: 'try_again' | 'sign_in_again'; reference?: string } = {}): CommandOutcome<never> => ({
  kind: 'error',
  error: { code, message, ...extra },
});

/** A `service.hello` result for a backend whose build answers `groups` (sorted, no repeats). */
export function hello(backend: BackendId, groups: string[], lastSeq = 0, profile: 'desktop' | 'personal_server' = 'desktop'): Commands['service.hello']['result'] {
  return {
    api_version: { major: 1, minor: 0 },
    backend,
    backend_name: backend === LOCAL ? 'This computer' : 'Northwind server',
    groups: [...new Set(groups)].sort(),
    last_seq: lastSeq,
    profile,
  };
}

/** The groups of an R1 build with no build switch on (a recorded `service.hello`). */
export const R1_GROUPS = [
  'agents', 'app', 'approval', 'brand', 'catalogue', 'checker', 'dashboards', 'dictation', 'files', 'handover',
  'home', 'identity', 'knowledge', 'localisation', 'memory', 'ordin', 'projects', 'search', 'servers', 'service',
  'settings', 'setup', 'usage', 'workflows', 'workspaces',
];

/**
 * Every command and input any `FakeTransport` was sent in this test file, kept apart from `sent` (which
 * tests clear). `tests/support/record-requests.ts` writes it out for the backend's own input check
 * (plan 04, safety net A).
 */
export const RECORDED: { command: string; input: unknown; replied?: string }[] = [];

/** `app.agent_settings` with nothing chosen: Claude Code ready, Copilot not in this version. */
export const AGENT_SETTINGS_DEFAULT = {
  organisation_harness: 'claude_code',
  harness_in_use: 'claude_code',
  harnesses: [
    {
      harness: 'claude_code',
      readiness: 'ready',
      models: [
        { level: 'smaller', default_model: 'haiku' },
        { level: 'standard', default_model: 'sonnet' },
        { level: 'most_capable', default_model: 'opus' },
      ],
      effort_levels: ['low', 'medium', 'high', 'xhigh', 'max'],
      effort: [{ kind: 'talking' }, { kind: 'coordinating' }, { kind: 'routine' }, { kind: 'documents' }, { kind: 'code' }],
    },
    {
      harness: 'copilot',
      readiness: 'not_in_this_version',
      models: [{ level: 'smaller' }, { level: 'standard' }, { level: 'most_capable' }],
      effort_levels: [],
      effort: [],
    },
  ],
};

export class FakeTransport implements Transport {
  readonly sent: CommandEnvelope<unknown>[] = [];
  readonly replies = new Map<string, Reply>();
  readonly opened: { after: Record<BackendId, number> }[] = [];
  /** When true every `invoke` rejects, as when the Command Bridge is unreachable. */
  bridgeDown = false;
  /** Commands that never reply (for the timeout test). */
  readonly hang = new Set<string>();
  version = '1.0.0';
  #onBatch: ((batch: EventBatch) => void) | null = null;
  #onStream: ((m: StreamMessage) => void) | null = null;
  readonly #desktop = new Map<DesktopEventName, (payload: unknown) => void>();
  readonly #byIpc = new Map(Object.entries(IPC_NAMES).map(([command, ipc]) => [ipc, command as CommandName]));

  constructor(options: { localGroups?: string[]; local?: BackendId } = {}) {
    const local = options.local ?? LOCAL;
    this.on('service.hello', ok(hello(local, options.localGroups ?? R1_GROUPS)));
    this.on('localisation.get_settings', ok({
      active: { locale: 'en-GB', source: 'operating_system', direction: 'ltr', fallback_chain: ['en-GB'] },
      following_operating_system: true,
      offered: [{ locale: 'en-GB', native_name: 'English (UK)', direction: 'ltr' }],
    }));
    this.on('localisation.get_catalogue', (input) => {
      const locale = (input as { locale: string }).locale;
      return ok({ locale, direction: locale === 'ar-SA' ? 'rtl' : 'ltr', resources: [{ locale, ftl: EN_GB_FTL }] });
    });
    this.on('brand.get', ok({
      brand_identifier: 'ordin', version: '1', source: 'build', design_tokens: {}, font_count: 0, links: { help_url: '', privacy_url: '' },
      refusal: null, strings: { product_name: 'ordin', coordinator_name: 'Ordin', about_text: '', legal_text: '', locale: 'en-GB' },
    }));
    this.on('settings.get', ok({ section: 'display', settings: [] }));
    this.on('servers.list', ok([]));
    this.on('workspaces.list', ok([]));
    this.on('workspaces.tree', ok({}));
    this.on('home.needs_you', ok([]));
    this.on('approval.list_pending', ok({ items: [], next_cursor: null }));
    // A build with no models input: the memory card is never drawn (ScreensModule.md SM-MD-1). A memory-models test replaces it.
    this.on('app.memory_models', ok({ state: 'not_in_build', size_bytes: 0, host: '', received_bytes: 0, ask: false }));
    // Settings › Agents and models with every choice on its default (ScreensModule.md SM-AGM-1).
    this.on('app.agent_settings', ok(AGENT_SETTINGS_DEFAULT));
    // Not a first start: the window draws its three regions (ScreensModule.md SM-FS-1). A first-start test replaces it.
    this.on('setup.status', ok({ needs_guided: false, progress: { done: 0, total: 0, running: false, steps: [] }, attachments: [] }));
  }

  /** Sets (or replaces) the reply of a command. */
  on(command: CommandName, reply: Reply): this {
    this.replies.set(command, reply);
    return this;
  }

  commandsSent(): string[] {
    return this.sent.map((e) => e.command);
  }
  sentOf(command: CommandName): CommandEnvelope<unknown>[] {
    return this.sent.filter((e) => e.command === command);
  }

  async invoke(ipcName: string, envelope: CommandEnvelope<unknown>): Promise<CommandReply<unknown>> {
    if (this.bridgeDown) throw new Error('bridge unreachable');
    const command = this.#byIpc.get(ipcName);
    if (command === undefined) throw new Error(`unknown ipc name ${ipcName}`);
    this.sent.push(envelope);
    const record: { command: string; input: unknown; replied?: string } = { command, input: envelope.input };
    RECORDED.push(record);
    if (this.hang.has(command)) return new Promise(() => undefined);
    const reply = this.replies.get(command);
    if (reply === undefined) return { request_id: envelope.request_id, kind: 'error', error: { code: 'TEST_NO_REPLY', message: command } };
    const outcome = typeof reply === 'function' ? await reply(envelope.input, envelope) : reply;
    // The code the test answered with: a refusal the backend's input check gives with the same code is
    // what the test meant to draw (plan 04, safety net A).
    if (outcome.kind === 'error') record.replied = outcome.error.code;
    return { request_id: envelope.request_id, ...outcome } as CommandReply<unknown>;
  }

  async openEvents(after: Record<BackendId, number>, onBatch: (batch: EventBatch) => void): Promise<EventsOpened> {
    this.opened.push({ after });
    this.#onBatch = onBatch;
    return { local_backend: LOCAL, reset: [] };
  }
  async openStreams(onMessage: (m: StreamMessage) => void): Promise<void> {
    this.#onStream = onMessage;
  }
  async listenDesktop(name: DesktopEventName, onEvent: (payload: unknown) => void): Promise<() => void> {
    this.#desktop.set(name, onEvent);
    return () => this.#desktop.delete(name);
  }
  async appVersion(): Promise<string> {
    return this.version;
  }

  /** Delivers one batch to the window, as `service_events` would. */
  emit(...events: EventEnvelope[]): void {
    this.#onBatch?.({ events });
  }
  stream(message: StreamMessage): void {
    this.#onStream?.(message);
  }
  desktop(name: DesktopEventName, payload: unknown): void {
    this.#desktop.get(name)?.(payload);
  }
}

let counter = 0;
/** One `EventEnvelope`. */
export function event(backend: BackendId, seq: number, body: ServiceEvent, at = '2026-01-10T10:43:58.120Z'): EventEnvelope {
  counter++;
  return { api_version: { major: 1, minor: 0 }, backend, seq, at, ...body } as EventEnvelope;
}

/** Waits until queued promise work has run. */
export async function settle(times = 20): Promise<void> {
  for (let i = 0; i < times; i++) await new Promise((resolve) => setTimeout(resolve, 0));
}

/** The approval card of the spec's worked example. */
export function approvalCard(overrides: Record<string, unknown> = {}): never {
  return {
    approval_id: '0192f1a0-5b6c-7d8e-9f00-112233445566',
    backend: LOCAL,
    workspace_id: '0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d',
    asked_by: { kind: 'run', run_id: '0192f19f-0000-7000-8000-0000000000b1' },
    question: 'Publish the revised account brief?',
    verb: 'Publish',
    where_line: 'Northwind team site — Account briefs',
    affects: [{ kind: 'file', label: 'Account brief.docx', version: 'v7' }],
    changes: 'Replaces version 6 with the revised brief',
    reject_style: 'reject',
    buttons: { approve: '7Hq2mVx9LpA3dRk5sT0uWa', reject: '0bN8wE4yJfG6hZ1iQ2o3Pc', show_details: 'Mm5Ve8Ux2Tw4Sy6Rz9Q1Kd' },
    created_at: '2026-01-10T10:43:58.101Z',
    ...overrides,
  } as never;
}
