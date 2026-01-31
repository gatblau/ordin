// Show the installed files (SM-NS-11): the files ordin wrote from an installed agent, read only.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-11
import type { InstalledFileEntry, ServiceError } from '../../service-api/types.gen';
import type { Handle } from '../fixed/util';

export interface FolderGroup {
  folder: string;
  files: InstalledFileEntry[];
}

/** Groups by the path up to its last `/` (`''` for a file at the top); folders and files keep the reply's order. */
export function groupByFolder(files: readonly InstalledFileEntry[]): FolderGroup[] {
  const groups = new Map<string, InstalledFileEntry[]>();
  for (const file of files) {
    const at = file.path.lastIndexOf('/');
    const folder = at < 0 ? '' : file.path.slice(0, at);
    const list = groups.get(folder);
    if (list === undefined) groups.set(folder, [file]);
    else list.push(file);
  }
  return [...groups].map(([folder, list]) => ({ folder, files: list }));
}

export class InstalledFilesController {
  files = $state.raw<InstalledFileEntry[]>([]);
  error = $state.raw<ServiceError | null>(null);
  readonly #handle: Handle;
  readonly #agentInstanceId: string;

  constructor(handle: Handle, agentInstanceId: string) {
    this.#handle = handle;
    this.#agentInstanceId = agentInstanceId;
  }

  async load(): Promise<void> {
    const out = await this.#handle.call('agents.installed_files', { agent_instance_id: this.#agentInstanceId });
    if (out.kind === 'ok') {
      this.files = out.result;
      this.error = null;
    } else {
      this.files = [];
      this.error = out.error;
    }
  }
}
