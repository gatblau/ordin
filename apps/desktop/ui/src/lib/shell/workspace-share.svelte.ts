// The sharing state of one workspace row in the tree (SM-TW-13, amended January 2026, user decision;
// Approved by the user January 2026): the row's ⋯ menu offers **Share with your team…** on a workspace that
// is not a team workspace, and **Shared with your team** and **Leave the team workspace** on one that is.
// The state outlives the menu, so the dialogs it opens stay open after the menu closes.
//
// spec: ordin-desktop/components/ScreensModule#SM-TW-13
// spec: ordin-desktop/components/ScreensModule#SM-TW-12
import { answers } from '../backend/groups';
import type { BackendState } from '../store/backend-state.svelte';
import { TeamController } from '../views/fixed/team.svelte';
import { offersTeamWorkspace } from '../views/fixed/team-workspace.svelte';

/** Sharing is offered for a workspace of the local backend that answers both commands (SM-UP-1, SM-TW-12). */
export function sharingOffered(backend: Pick<BackendState, 'kind' | 'groups'>): boolean {
  return offersTeamWorkspace(backend, 'team_knowledge.share_workspace') && answers(backend, 'team_knowledge.status');
}

export class WorkspaceShare {
  readonly controller: TeamController;
  /** The share dialog of SM-TW-2 is open. */
  sharing = $state(false);
  /** The card under the row: the team folder and who it is shared with. */
  showing = $state(false);
  /** The Leave confirmation of SM-TW-10 is open. */
  leaving = $state(false);

  constructor(backend: BackendState, workspaceId: string) {
    this.controller = new TeamController(backend.handle, workspaceId);
  }

  /** The status has been read; nothing about sharing is offered before. */
  get known(): boolean {
    return this.controller.status !== null;
  }
  get shared(): boolean {
    return (this.controller.status?.team_workspace ?? null) !== null;
  }
  get folder(): string {
    const status = this.controller.status;
    return status?.header_line ?? status?.binding?.folder ?? '';
  }
}
