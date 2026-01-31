// The places a task's form offers documents from, and how a chosen document reads (SM-TF-3, SM-TF-9).
// The places at the top are the workspace's places that no link names (its own folder), then the active
// links that have a place, in the side tree's order: SharePoint, Confluence, This computer, then the linked
// code repositories. A Jira issue and a link with no place yet hold no file to choose.
//
// spec: ordin-desktop/components/ScreensModule#SM-TF-9
import { t } from '../../i18n/fluent';
import type { Place, SourceLink } from '../../service-api/types.gen';
import { orderedLinks } from '../add/links-model.svelte';

export interface DocumentRoot {
  placeId: string;
  name: string;
  /** A linked code repository (`git_repository_folder`). */
  code: boolean;
}

export function documentRoots(places: readonly Place[], links: readonly SourceLink[]): DocumentRoot[] {
  const linked = orderedLinks(links).filter((l) => l.kind !== 'jira_issue' && l.place_id != null && l.place_id !== '');
  const named = new Set(linked.map((l) => l.place_id as string));
  const own = places.filter((p) => !named.has(p.id)).map((p) => ({ placeId: p.id, name: p.friendly_name, code: false }));
  return [...own, ...linked.map((l) => ({ placeId: l.place_id as string, name: l.display_name, code: l.kind === 'git_repository_folder' }))];
}

/** The place's name, or empty when the workspace no longer holds it. */
export function placeName(roots: readonly DocumentRoot[], placeId: string): string {
  return roots.find((r) => r.placeId === placeId)?.name ?? '';
}

/** "03-nats-key-rotation.md, in docs/wip/plan of ordin", or "notes.md, in Docs" at the top of a place. */
export function documentLine(roots: readonly DocumentRoot[], placeId: string, path: string): string {
  const parts = path.split('/').filter((p) => p !== '');
  const file = parts.pop() ?? path;
  const place = placeName(roots, placeId);
  if (parts.length === 0) return t('task-form-chosen-document-top', { file, place });
  return t('task-form-chosen-document', { file, folder: parts.join('/'), place });
}
