// Which workspace links code (SM-WR-14): one that `workspaces.links` shows with an active link of kind
// `git_repository_folder` or `git_remote`. An error reply is taken as "does not link code".
//
// spec: ordin-desktop/components/ScreensModule#SM-WR-14
import type { SourceLink } from '../../service-api/types.gen';
import type { Handle } from './util';

const CODE_KINDS: ReadonlySet<SourceLink['kind']> = new Set(['git_repository_folder', 'git_remote']);

export function linksCode(links: readonly SourceLink[]): boolean {
  return links.some((l) => l.state === 'active' && CODE_KINDS.has(l.kind));
}

/** `workspaces.links { workspace_id }`, read as "links code"; `false` on any refusal. */
export async function readLinksCode(handle: Handle, workspaceId: string | null): Promise<boolean> {
  if (workspaceId === null || workspaceId === '') return false;
  const out = await handle.call('workspaces.links', { workspace_id: workspaceId });
  return out.kind === 'ok' && linksCode(out.result);
}
