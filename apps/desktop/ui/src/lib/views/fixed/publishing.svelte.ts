// SM-SF-7: a linked folder's automatic publishing. The line states a fact and offers no switch:
// publishing follows the folder's access and nothing else (WorkspaceService.md).
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-7
import { confluencePublishing } from '../../backend/wiring';
import { formatList } from '../../format/intl';
import { t } from '../../i18n/fluent';
import type { ConfluencePublishing, Grantee, ServiceError } from '../../service-api/types.gen';
import { AgentNames } from './facts';
import { settle, type Handle } from './util';

export const MAX_NAMES = 3;

/** The names for `confluence-auto-off`: each grantee once, up to 3 joined, the rest counted (SM-SF-7.3). */
export function grantNames(turnedOffBy: readonly Grantee[], names: AgentNames): { names: string; more: number } {
  const seen: string[] = [];
  for (const g of turnedOffBy) {
    const name = g.kind === 'agent_instance' ? (names.nameOf(g.agent_instance_id) ?? t('confluence-auto-an-agent')) : t('confluence-auto-a-crew');
    if (!seen.includes(name)) seen.push(name);
  }
  return { names: formatList(seen.slice(0, MAX_NAMES)), more: Math.max(0, seen.length - MAX_NAMES) };
}

export type PublishingLine =
  | { kind: 'none' }
  | { kind: 'on' }
  | { kind: 'off'; names: string; more: number };

export class PublishingController {
  line = $state.raw<PublishingLine>({ kind: 'none' });
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly linkId: string;

  constructor(handle: Handle, linkId: string) {
    this.#handle = handle;
    this.linkId = linkId;
  }

  /** Read when the folder opens and again on each `workspace_changed` for its workspace. */
  async load(): Promise<void> {
    const out = settle(await confluencePublishing(this.#handle, this.linkId));
    if (!out.ok) {
      // `not_wired` draws no line; a refusal draws no line and the owner's sentence in its place.
      this.line = { kind: 'none' };
      this.error = 'error' in out ? out.error : null;
      return;
    }
    this.error = null;
    this.line = await this.#lineFor(out.value);
  }

  async #lineFor(view: ConfluencePublishing): Promise<PublishingLine> {
    if (view.automatic === 'on') return { kind: 'on' };
    // SA-UI-SF-05 and SM-SF-7.3: an agent's name is the display name of the matching `agents.list` entry, no workspace filter.
    const agents = await this.#handle.call('agents.list', {});
    const names = new AgentNames(agents.kind === 'ok' ? agents.result : []);
    return { kind: 'off', ...grantNames(view.turned_off_by, names) };
  }
}
