// Resolving a `ContentRef` to something drawable. A reference is resolved through a command, never opened as
// a URL (ScreensModule step 6). A file version goes through `lib/backend/wiring.ts` only (SM-SF-1).
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-1
// spec: ordin-desktop/components/ScreensModule#SM-WR-3
import { answers } from '../backend/groups';
import type { BackendHandle } from '../backend/backend';
import { isNotWired, readFileVersion } from '../backend/wiring';
import type { BackendState } from '../store/backend-state.svelte';
import type { ContentRef, Preview, ServiceError } from '../service-api/types.gen';

export type Resolved =
  | { kind: 'ok'; name: string; preview: Preview }
  /** The command is not wired yet (`file_version`) or the backend does not answer it. */
  | { kind: 'not_wired' }
  | { kind: 'error'; error: ServiceError }
  /** The reference names nothing (an artefact no row has). */
  | { kind: 'unresolvable' };

/** The last segment of a path. */
export function lastSegment(path: string): string {
  const trimmed = path.endsWith('/') ? path.slice(0, -1) : path;
  const at = trimmed.lastIndexOf('/');
  return at < 0 ? trimmed : trimmed.slice(at + 1);
}

/** A name for the reference when its data carries one without a command: a place file's path. */
export function refName(ref: ContentRef): string | null {
  return ref.kind === 'place_file' ? lastSegment(ref.relative_path) : null;
}

/** A stable key for lists of references. */
export function refKey(ref: ContentRef): string {
  switch (ref.kind) {
    case 'artefact':
      return `a:${ref.artefact_id}`;
    case 'place_file':
      return `p:${ref.place_id}:${ref.relative_path}`;
    case 'file_version':
      return `v:${ref.file_version_id}`;
  }
}

export async function resolveRef(backend: BackendState, ref: ContentRef): Promise<Resolved> {
  const handle: BackendHandle = backend.handle;
  switch (ref.kind) {
    case 'place_file': {
      if (!answers(backend, 'files.preview')) return { kind: 'not_wired' };
      const out = await handle.call('files.preview', { place_id: ref.place_id, path: ref.relative_path });
      return out.kind === 'ok' ? { kind: 'ok', name: lastSegment(ref.relative_path), preview: out.result } : { kind: 'error', error: out.error };
    }
    case 'file_version': {
      // SM-SF-1: the version's own read, never `files.preview` of the file as it is now.
      const out = await readFileVersion(handle, ref.file_version_id);
      if (isNotWired(out)) return { kind: 'not_wired' };
      return out.kind === 'ok' ? { kind: 'ok', name: lastSegment(out.result.path), preview: out.result.preview } : { kind: 'error', error: out.error };
    }
    case 'artefact': {
      // SM-WR-3: `artefacts.get`, then the file as it is now in its place; an agent folder is never read.
      if (!answers(backend, 'artefacts.get')) return { kind: 'not_wired' };
      const got = await handle.call('artefacts.get', { artefact_id: ref.artefact_id });
      if (got.kind === 'error') return { kind: 'error', error: got.error };
      const artefact = got.result.artefact;
      if (!artefact) return { kind: 'unresolvable' };
      const name = lastSegment(artefact.path);
      if (artefact.location.kind === 'agent_folder') return { kind: 'ok', name, preview: { kind: 'none' } };
      if (!answers(backend, 'files.preview')) return { kind: 'not_wired' };
      const out = await handle.call('files.preview', { place_id: artefact.location.place_id, path: artefact.path });
      return out.kind === 'ok' ? { kind: 'ok', name, preview: out.result } : { kind: 'error', error: out.error };
    }
  }
}
