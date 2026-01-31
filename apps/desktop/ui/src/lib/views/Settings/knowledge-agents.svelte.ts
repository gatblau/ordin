// Settings › Knowledge agents (ADR-112, ADR-113, ADR-115): the connections of a workspace, the connect
// dialog (address, then what the front door keeps, then Keep it or Remove), Details and Remove. A front
// door is never connected by Ordin or an agent: connecting and removing are buttons of this module only.
//
// spec: ordin-desktop/components/ScreensModule#SA-UI-KA-01
// spec: ordin-desktop/components/ScreensModule#SA-UI-KA-02
import type { BackendHandle, DiagnosticSink } from '../../backend/backend';
import { t } from '../../i18n/fluent';
import type {
  AccountLinkState,
  ConnectionAccount,
  KnowledgeAgentsAddResult,
  KnowledgeAgentsDetailsResult,
  KnowledgeConnectionView,
  ServiceError,
  SourceSystem,
} from '../../service-api/types.gen';

/** The product names of the systems an account can be linked to (proper nouns, as Settings › Sign-ins draws them). */
export const SYSTEM_NAMES: Partial<Record<SourceSystem, string>> = { github: 'GitHub', gitlab: 'GitLab', confluence: 'Confluence' };

/**
 * An account line to draw from `knowledge_agents.details`'s `accounts` (SM-WR-6), never from
 * `about.accounts`; `null` when the system has no name on Settings › Sign-ins (SharePoint), or for a state a
 * newer backend sends that this version does not know.
 */
// spec: ordin-desktop/components/ScreensModule#SM-WR-6
export function accountLine(account: ConnectionAccount): { messageId: string; system: string; signIn: boolean; kind: SourceSystem } | null {
  const system = SYSTEM_NAMES[account.system];
  const state: AccountLinkState | undefined = account.state;
  if (system === undefined || (state !== 'linked' && state !== 'not_signed_in' && state !== 'refused')) return null;
  const messageId =
    state === 'linked' ? 'knowledge-agents-account-linked' : state === 'not_signed_in' ? 'knowledge-agents-sign-in-to-include' : 'knowledge-agents-account-refused';
  return { messageId, system, signIn: state !== 'linked', kind: account.system };
}

export class KnowledgeAgentsModel {
  workspaceId = $state('');
  connections = $state.raw<KnowledgeConnectionView[]>([]);
  error = $state<ServiceError | null>(null);

  connectOpen = $state(false);
  address = $state('');
  addressNotHttps = $state(false);
  connectError = $state<ServiceError | null>(null);
  added = $state.raw<KnowledgeAgentsAddResult | null>(null);
  busy = $state(false);

  details = $state.raw<KnowledgeAgentsDetailsResult | null>(null);
  detailsError = $state<ServiceError | null>(null);
  confirmRemove = $state.raw<KnowledgeConnectionView | null>(null);

  readonly #handle: BackendHandle;
  readonly #announce: (text: string) => void;
  readonly #diagnostics: DiagnosticSink | undefined;

  constructor(handle: BackendHandle, workspaceId: string, announce: (text: string) => void = () => undefined, diagnostics?: DiagnosticSink) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
    this.#announce = announce;
    this.#diagnostics = diagnostics;
  }

  async load(): Promise<void> {
    if (this.workspaceId === '') return;
    const out = await this.#handle.call('knowledge_agents.list', { workspace_id: this.workspaceId });
    if (out.kind === 'ok') {
      this.connections = out.result.connections;
      this.error = null;
    } else this.error = out.error;
  }

  async chooseWorkspace(id: string): Promise<void> {
    this.workspaceId = id;
    this.details = null;
    await this.load();
  }

  // ---- connect ------------------------------------------------------------------------------------

  openConnect(): void {
    this.connectOpen = true;
    this.address = '';
    this.addressNotHttps = false;
    this.connectError = null;
    this.added = null;
  }

  closeConnect(): void {
    this.connectOpen = false;
    this.added = null;
    this.connectError = null;
  }

  /** The address must start with `https://` (checked here, `UI_KNOWLEDGE_ADDRESS_NOT_HTTPS` otherwise). */
  async connect(): Promise<boolean> {
    const url = this.address.trim();
    if (!url.startsWith('https://')) {
      this.addressNotHttps = true;
      this.#diagnostics?.record('UI_KNOWLEDGE_ADDRESS_NOT_HTTPS');
      return false;
    }
    this.addressNotHttps = false;
    this.busy = true;
    const out = await this.#handle.call('knowledge_agents.add', { workspace_id: this.workspaceId, url });
    this.busy = false;
    if (out.kind === 'ok') {
      this.added = out.result;
      this.connectError = null;
      await this.load();
      return true;
    }
    this.connectError = out.error;
    return false;
  }

  /** **Keep it**: closes the dialog and announces the connection. */
  keepIt(): void {
    const added = this.added;
    this.closeConnect();
    if (added !== null) this.#announce(t('knowledge-agents-connected', { name: added.about.agent }));
  }

  /** **Remove** on the second stage: at once, with no further confirmation. */
  async removeJustAdded(): Promise<void> {
    const added = this.added;
    this.closeConnect();
    if (added === null) return;
    const out = await this.#handle.call('knowledge_agents.remove', { connection_id: added.connection_id });
    if (out.kind === 'error') this.error = out.error;
    await this.load();
  }

  // ---- details and remove -------------------------------------------------------------------------

  async openDetails(connectionId: string): Promise<void> {
    const out = await this.#handle.call('knowledge_agents.details', { connection_id: connectionId });
    if (out.kind === 'ok') {
      this.details = out.result;
      this.detailsError = null;
    } else {
      this.detailsError = out.error;
    }
  }

  closeDetails(): void {
    this.details = null;
    this.detailsError = null;
  }

  async remove(connection: KnowledgeConnectionView): Promise<void> {
    this.confirmRemove = null;
    const out = await this.#handle.call('knowledge_agents.remove', { connection_id: connection.connection_id });
    if (out.kind === 'ok') this.#announce(t('knowledge-agents-removed', { name: connection.name }));
    else this.error = out.error;
    await this.load();
  }
}
