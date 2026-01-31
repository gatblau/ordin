// Documents by where they live, under a workspace's Documents row (SM-RL-1), and removing one of those
// links (SM-RL-2). `workspaces.links` is read when the row is first opened and again on each
// `workspace_changed` for the workspace while it is open; `proposed` links are not drawn.
//
// spec: ordin-desktop/components/ScreensModule#SM-RL-1
// spec: ordin-desktop/components/ScreensModule#SM-RL-2
import type { BackendHandle } from '../../backend/backend';
import type { IconName } from '../../icons/sprite';
import type { ServiceError, SourceLink, SourceLinkKind } from '../../service-api/types.gen';

/** The order of the kinds under Documents: SharePoint, Confluence, This computer, code repositories, Jira. */
const KIND_ORDER: readonly SourceLinkKind[] = ['sharepoint_location', 'confluence_location', 'local_folder', 'git_repository_folder', 'jira_issue'];

/** Each kind's icon (V8: where documents live is the origin's icon, tinted; code `i-code`; Jira `i-task`). */
export const LINK_KIND_ICON: Readonly<Partial<Record<SourceLinkKind, { icon: IconName; tint?: string }>>> = {
  sharepoint_location: { icon: 'sp', tint: 'sharepoint' },
  confluence_location: { icon: 'cf', tint: 'confluence' },
  local_folder: { icon: 'lc', tint: 'local' },
  git_repository_folder: { icon: 'code' },
  jira_issue: { icon: 'task' },
};

/** The active links in the tree's order, grouped by kind; a kind the tree does not draw is left out. */
export function orderedLinks(links: readonly SourceLink[]): SourceLink[] {
  return KIND_ORDER.flatMap((kind) => links.filter((l) => l.kind === kind && l.state === 'active'));
}

export class DocumentLinksModel {
  links = $state.raw<SourceLink[]>([]);
  error = $state.raw<ServiceError | null>(null);
  /** The link whose removal is being confirmed, and the error of the last try (drawn in the dialog). */
  removing = $state.raw<SourceLink | null>(null);
  removeError = $state.raw<ServiceError | null>(null);
  readonly workspaceId: string;
  readonly #handle: Pick<BackendHandle, 'call'>;

  constructor(handle: Pick<BackendHandle, 'call'>, workspaceId: string) {
    this.#handle = handle;
    this.workspaceId = workspaceId;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('workspaces.links', { workspace_id: this.workspaceId });
    if (out.kind === 'ok') {
      this.links = orderedLinks(out.result);
      this.error = null;
    } else this.error = out.error;
  }

  /** **Remove from this workspace…** opens the confirmation. */
  askRemove(link: SourceLink): void {
    this.removing = link;
    this.removeError = null;
  }

  /** **Cancel** sends nothing. */
  cancelRemove(): void {
    this.removing = null;
    this.removeError = null;
  }

  /**
   * The confirmation's verb: `workspaces.remove_link { source_link_id }`. Returns the removed link's name on
   * success (the row goes on the next `workspace_changed`); an error stays in the dialog, which stays open.
   */
  async confirmRemove(): Promise<string | null> {
    const link = this.removing;
    if (link === null) return null;
    const out = await this.#handle.call('workspaces.remove_link', { source_link_id: link.id });
    if (out.kind === 'error') {
      this.removeError = out.error;
      return null;
    }
    this.removing = null;
    this.removeError = null;
    return link.display_name;
  }
}
