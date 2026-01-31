// Part pages for Check it (SM-NS-15): an installed agent's part (`<agent instance id>:<part id>`) and a
// library part (`library:<part id>`). The page reads only what the backend answers.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-15
import type { Detail, PartPage, PartView, ServiceError } from '../../service-api/types.gen';
import type { Handle } from '../fixed/util';

export type PartPageTarget = { kind: 'installed'; agentInstanceId: string; partId: string } | { kind: 'library'; partId: string };

/** `library:<part id>` or `<agent instance id>:<part id>` (neither side of the first colon contains one). */
export function parsePartPageInstance(instance: string): PartPageTarget {
  const at = instance.indexOf(':');
  if (at < 0) return { kind: 'library', partId: instance };
  const head = instance.slice(0, at);
  const rest = instance.slice(at + 1);
  return head === 'library' ? { kind: 'library', partId: rest } : { kind: 'installed', agentInstanceId: head, partId: rest };
}

export const partPageKey = (agentInstanceId: string | 'library', partId: string): string => `part-page:${agentInstanceId}:${partId}`;

// SM-WR-14: an installed part page asks for `links_code` in a workspace that links code (an active
// `git_repository_folder` or `git_remote` link, `workspaces.links`) and for `business_user` in any other.
// spec: ordin-desktop/components/ScreensModule#SM-WR-14
export const DETAIL_FOR_PART_PAGE: Detail = 'business_user';

/** The detail a part page asks for, from whether its workspace links code. */
export const detailFor = (linksCode: boolean): Detail => (linksCode ? 'links_code' : 'business_user');

export class InstalledPartPageController {
  page = $state.raw<PartPage | null>(null);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly #agentInstanceId: string;
  readonly #partId: string;
  /** The detail of the last read; the file under Details is drawn only for `links_code`. */
  detail = $state<Detail>(DETAIL_FOR_PART_PAGE);

  constructor(handle: Handle, agentInstanceId: string, partId: string, detail: Detail = DETAIL_FOR_PART_PAGE) {
    this.#handle = handle;
    this.#agentInstanceId = agentInstanceId;
    this.#partId = partId;
    this.detail = detail;
  }

  /** `agents.part_page`; with `detail`, that detail is asked for from now on. */
  async load(detail?: Detail): Promise<void> {
    if (detail !== undefined) this.detail = detail;
    const out = await this.#handle.call('agents.part_page', { agent_instance_id: this.#agentInstanceId, part_id: this.#partId, detail: this.detail });
    if (out.kind === 'ok') {
      this.page = out.result;
      this.error = null;
    } else {
      this.error = out.error;
    }
  }
}

export class LibraryPartPageController {
  part = $state.raw<PartView | null>(null);
  usedBy = $state.raw<PartView[]>([]);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly #partId: string;

  constructor(handle: Handle, partId: string) {
    this.#handle = handle;
    this.#partId = partId;
  }

  async load(): Promise<void> {
    const part = await this.#handle.call('catalogue.part', { id: this.#partId });
    if (part.kind === 'error') {
      this.error = part.error;
      return;
    }
    this.part = part.result;
    this.error = null;
    const used = await this.#handle.call('catalogue.used_by', { id: this.#partId });
    if (used.kind === 'ok') this.usedBy = used.result.agents;
    else this.error = used.error;
  }
}
