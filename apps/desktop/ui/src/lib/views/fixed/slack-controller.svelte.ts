// The Slack channel screen and the tree's Slack list: one row per linked channel with its new-message
// count; a channel in `no_access` says so. Messages are other people's: text only, and a message that asks
// the agent to act carries `slack-message-instruction-note`.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import type { SlackLinkSummary, SlackMessageOut, ServiceError } from '../../service-api/types.gen';
import { Announcer, type Handle } from './util';

export class SlackController {
  links = $state.raw<SlackLinkSummary[]>([]);
  messages = $state.raw<SlackMessageOut[]>([]);
  error = $state.raw<ServiceError | null>(null);
  readonly announcer = new Announcer();
  readonly #handle: Handle;
  readonly workspaceId: string;

  constructor(handle: Handle, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
  }

  async loadLinks(): Promise<void> {
    const out = await this.#handle.call('slack.links', { ws: this.workspaceId });
    if (out.kind === 'ok') this.links = out.result;
    else this.error = out.error;
  }

  async loadMessages(linkId: string): Promise<void> {
    const out = await this.#handle.call('slack.messages', { ws: this.workspaceId, link_id: linkId });
    if (out.kind === 'ok') this.messages = out.result.items;
    else this.error = out.error;
  }

  /** **Let Ordin look at new messages** (`slack.set_ordin_reads`). */
  async setOrdinReads(linkId: string, on: boolean): Promise<void> {
    const out = await this.#handle.call('slack.set_ordin_reads', { ws: this.workspaceId, link_id: linkId, on });
    if (out.kind === 'error') return void (this.error = out.error);
    this.links = this.links.map((l) => (l.link.id === linkId ? { ...l, link: out.result } : l));
  }

  /** **Remove from this workspace**, after the §5.5 confirmation. */
  async unlink(linkId: string): Promise<boolean> {
    const out = await this.#handle.call('slack.unlink', { ws: this.workspaceId, link_id: linkId });
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.links = this.links.filter((l) => l.link.id !== linkId);
    return true;
  }
}
