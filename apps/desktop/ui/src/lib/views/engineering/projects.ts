// The projects the screens have seen, by backend and id. The commands that return a `Project`
// (`projects.open`, `projects.set_check_commands`, `projects.get`) feed it; the editor's bar and the
// project settings read the display name and the check commands from it. A screen that needs a project
// not seen on its backend reads it with `projects.get` (SM-WR-4), so no opener has to remember it.
//
// spec: ordin-desktop/components/ScreensModule#SM-WR-4
import type { Project, ServiceError } from '../../service-api/types.gen';
import type { Handle } from '../fixed/util';

const known = new Map<string, Project>();
const key = (backend: string, id: string): string => `${backend}:${id}`;

export function rememberProject(backend: string, project: Project): void {
  known.set(key(backend, project.id), project);
}

export function projectOf(backend: string, id: string): Project | undefined {
  return known.get(key(backend, id));
}

export function forgetProjects(): void {
  known.clear();
}

export type ProjectRead = { ok: true; project: Project } | { ok: false; error: ServiceError };

/** The project as seen, or `projects.get { project_id }` when it has not been seen on that backend. */
export async function ensureProject(handle: Handle, projectId: string): Promise<ProjectRead> {
  const seen = projectOf(handle.id, projectId);
  if (seen !== undefined) return { ok: true, project: seen };
  const out = await handle.call('projects.get', { project_id: projectId });
  if (out.kind === 'error') return { ok: false, error: out.error };
  rememberProject(handle.id, out.result);
  return { ok: true, project: out.result };
}
