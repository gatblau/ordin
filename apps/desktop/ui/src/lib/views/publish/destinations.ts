// Publish to… (SM-PUB-1, SM-PUB-2): the destinations a document can be published to, and the copy request
// the Push Service's `push.publish_copy` takes for one of them. A destination is offered only when the
// workspace is connected to it (an active link of its kind), the backend answers the commands it needs
// (SM-UP-1), and the document's kind can go there under the 2C `PUB_*` format rules. No destination → the
// menu has no Publish to… item at all.
//
// spec: ordin-desktop/components/ScreensModule#SM-PUB-1
// spec: ordin-desktop/components/ScreensModule#SM-PUB-2
import { answersAll, type HasGroups } from '../../backend/groups';
import type {
  ConfluenceLocation,
  CopyDestination,
  CopyFormat,
  PublishCopyRequest,
  SharePointLocation,
  SourceLink,
} from '../../service-api/types.gen';
import type { BackendState } from '../../store/backend-state.svelte';

export type PublishSystem = 'confluence' | 'sharepoint';

/** Each destination's link kind, the command that finds a place in it, and its name (an integration name). */
export const PUBLISH_SYSTEMS: Readonly<Record<PublishSystem, { linkKind: SourceLink['kind']; find: string; name: string }>> = {
  confluence: { linkKind: 'confluence_location', find: 'confluence.find', name: 'Confluence' },
  sharepoint: { linkKind: 'sharepoint_location', find: 'sharepoint.find', name: 'SharePoint' },
};

const ORDER: readonly PublishSystem[] = ['confluence', 'sharepoint'];

/** Lower-case extension of a file name, without the dot; empty when there is none. */
export function extensionOf(name: string): string {
  const at = name.lastIndexOf('.');
  return at <= 0 ? '' : name.slice(at + 1).toLowerCase();
}

/** A Markdown document becomes a page (Confluence) or a Word file (SharePoint); any other file goes to
 * Confluence as an attachment, exactly as it is. SharePoint takes only made copies (PUB_FORMAT_MAKER_MISMATCH). */
export function formatFor(system: PublishSystem, fileName: string): CopyFormat | null {
  const markdown = extensionOf(fileName) === 'md';
  if (system === 'confluence') return markdown ? 'page' : 'exact_file';
  return markdown ? 'word' : null;
}

/**
 * The destinations offered for a document, in a fixed order. `links` are the workspace's links;
 * `inRepository` is true for a document whose place is a code repository, whose version is a commit the
 * screens do not hold (a gap: such a document offers no destination).
 */
export function publishDestinations(backend: HasGroups, links: readonly SourceLink[], fileName: string, inRepository: boolean): PublishSystem[] {
  if (inRepository || !answersAll(backend, ['push.publish_copy', 'files.versions'])) return [];
  return ORDER.filter(
    (system) =>
      answersAll(backend, [PUBLISH_SYSTEMS[system].find]) &&
      links.some((l) => l.state === 'active' && l.kind === PUBLISH_SYSTEMS[system].linkKind) &&
      formatFor(system, fileName) !== null,
  );
}

/** True when the place is a code repository folder of the workspace. */
export function isRepositoryPlace(links: readonly SourceLink[], placeId: string): boolean {
  return links.some((l) => l.place_id === placeId && (l.kind === 'git_repository_folder' || l.kind === 'git_remote'));
}

/** Reads the workspace's links (`workspaces.links`) and answers the destinations of one document; a failed
 * read offers none, so the menu shows no Publish to… rather than one that cannot work. */
export async function loadDestinations(
  backend: Pick<BackendState, 'groups' | 'handle'>,
  workspaceId: string,
  placeId: string,
  fileName: string,
): Promise<PublishSystem[]> {
  if (!answersAll(backend, ['workspaces.links'])) return [];
  const out = await backend.handle.call('workspaces.links', { workspace_id: workspaceId });
  if (out.kind !== 'ok') return [];
  return publishDestinations(backend, out.result, fileName, isRepositoryPlace(out.result, placeId));
}

/** A place found in a destination, as the dialog lists it. */
export type FoundPlace = { kind: 'confluence'; location: ConfluenceLocation } | { kind: 'sharepoint'; location: SharePointLocation };

/** Only a Confluence page can hold a page or an attachment; only a SharePoint folder can hold a file. */
export function canHold(found: FoundPlace): boolean {
  if (found.kind === 'confluence') return found.location.page_id !== null && found.location.page_id !== undefined && found.location.page_id !== '';
  return found.location.is_folder && found.location.item_id !== null && found.location.item_id !== undefined && found.location.item_id !== '';
}

/** The words of a found place: "Procurement › Supplier pages", "Finance › Documents › Quotes". */
export function foundName(found: FoundPlace): string {
  if (found.kind === 'confluence') return `${found.location.space_name} › ${found.location.title}`;
  const path = found.location.path === '' ? [] : [found.location.path];
  return [found.location.site_name, found.location.drive_name, ...path].join(' › ');
}

function destinationOf(found: FoundPlace, format: CopyFormat): CopyDestination {
  if (found.kind === 'confluence') {
    const at = { space: found.location.space_key, parent_page_id: found.location.page_id ?? '' };
    return format === 'exact_file' ? { kind: 'confluence_attachment', ...at } : { kind: 'confluence_page', ...at };
  }
  // The Push Service resolves `graph:drives/<library>/items/<folder>` (PushService.md §E.1).
  return { kind: 'sharepoint_file', site: found.location.site_name, library: found.location.drive_id, folder: found.location.item_id ?? '' };
}

/** The `push.publish_copy` request for a document of a place at its newest File Service version. */
export function copyRequest(args: {
  workspaceId: string;
  placeId: string;
  path: string;
  name: string;
  version: number;
  found: FoundPlace;
}): PublishCopyRequest | null {
  const format = formatFor(args.found.kind, args.name);
  if (format === null || !canHold(args.found)) return null;
  return {
    workspace_id: args.workspaceId,
    original: { kind: 'place_file', place_id: args.placeId, path: args.path },
    original_version: String(args.version),
    original_name: args.name,
    page_file: null,
    destination: destinationOf(args.found, format),
    format,
    followed_branch: null,
    keep_up_to_date: false,
    file_name: args.name,
  };
}
