// The commands of the exact version an approval covers (SM-WR-1, closing G-SF-1) and of held writes
// and sources to confirm (SM-WR-2, closing G-SF-2), and of a linked folder's automatic publishing
// (SM-SF-10). Each function sends its command; a backend whose catalogue lacks the command (a server
// on an older version answers `SVC_COMMAND_UNKNOWN`) resolves to `{ kind: 'not_wired' }`, which the
// views draw as before the commands existed.
//
// spec: ordin-desktop/components/ScreensModule#SM-WR-1
// spec: ordin-desktop/components/ScreensModule#SM-WR-2
// spec: ordin-desktop/components/ScreensModule#SM-SF-10
import type { ConfluencePublishing, FilesReadVersionResult, HeldWrite } from '../service-api/types.gen';
import type { BackendHandle, CommandOutcome } from './backend';

export type { HeldLink, HeldThing, HeldWrite } from '../service-api/types.gen';
/** The generated result of `files.read_version`. */
export type VersionContent = FilesReadVersionResult;

export type NotWired = { kind: 'not_wired' };
const NOT_WIRED: NotWired = { kind: 'not_wired' };

/** The reply of a backend whose catalogue has no such command (ServiceApi.md SVC-RL-3 step 5). */
const COMMAND_UNKNOWN = 'SVC_COMMAND_UNKNOWN';

export type Wired<T> = Promise<CommandOutcome<T> | NotWired>;

export function isNotWired(value: CommandOutcome<unknown> | NotWired): value is NotWired {
  return value.kind === 'not_wired';
}

function wired<T>(out: CommandOutcome<T>): CommandOutcome<T> | NotWired {
  return out.kind === 'error' && out.error.code === COMMAND_UNKNOWN ? NOT_WIRED : out;
}

/** SM-WR-1: `files.read_version { file_version_id }`. */
export async function readFileVersion(b: BackendHandle, file_version_id: string): Wired<VersionContent> {
  return wired(await b.call('files.read_version', { file_version_id }));
}
/** SM-WR-2: `memory.held_writes { workspace_id }`. */
export async function heldWrites(b: BackendHandle, workspace_id: string): Wired<HeldWrite[]> {
  return wired(await b.call('memory.held_writes', { workspace_id }));
}
/** SM-WR-2: `memory.keep_held { workspace_id, id, content_hash }`. */
export async function keepHeld(b: BackendHandle, workspace_id: string, id: string, content_hash: string): Wired<unknown> {
  return wired(await b.call('memory.keep_held', { workspace_id, id, content_hash }));
}
/** SM-WR-2: `memory.discard_held { workspace_id, id }`. */
export async function discardHeld(b: BackendHandle, workspace_id: string, id: string): Wired<unknown> {
  return wired(await b.call('memory.discard_held', { workspace_id, id }));
}
/** SM-WR-2: `memory.citations_to_confirm { workspace_id }`. */
export async function citationsToConfirm(b: BackendHandle, workspace_id: string): Wired<string[]> {
  return wired(await b.call('memory.citations_to_confirm', { workspace_id }));
}
/** SM-WR-2: `memory.confirm_citation { workspace_id, id }`. */
export async function confirmCitation(b: BackendHandle, workspace_id: string, id: string): Wired<unknown> {
  return wired(await b.call('memory.confirm_citation', { workspace_id, id }));
}

/** SM-SF-10: `workspaces.confluence_publishing { confluence_link_id }`, its outcome as returned. */
export async function confluencePublishing(b: BackendHandle, confluence_link_id: string): Wired<ConfluencePublishing> {
  return b.call('workspaces.confluence_publishing', { confluence_link_id });
}
