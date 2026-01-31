// What [agent] is made of (SM-NS-11): the files ordin wrote from an installed agent as a tree, and a
// reader for the one chosen. Read only.
//
// spec: ordin-desktop/components/ScreensModule#SM-NS-11
import type { InstalledFileEntry, InstalledFileText, ServiceError } from '../../service-api/types.gen';
import type { Handle } from '../fixed/util';

export interface FileNode {
  kind: 'file';
  name: string;
  path: string;
  entry: InstalledFileEntry;
}

export interface FolderNode {
  kind: 'folder';
  name: string;
  path: string;
  children: TreeNode[];
}

export type TreeNode = FileNode | FolderNode;

const byName = (a: TreeNode, b: TreeNode): number => (a.kind === b.kind ? a.name.localeCompare(b.name) : a.kind === 'folder' ? -1 : 1);

/**
 * The files as a tree of their folders: in each folder, its folders first, then its files, each by name.
 * An empty path segment (`a//b`, a leading or trailing `/`) is skipped.
 */
export function buildTree(files: readonly InstalledFileEntry[]): TreeNode[] {
  const root: FolderNode = { kind: 'folder', name: '', path: '', children: [] };
  for (const entry of files) {
    const parts = entry.path.split('/').filter((p) => p !== '');
    const leaf = parts.pop();
    if (leaf === undefined) continue;
    let folder = root;
    for (const [i, name] of parts.entries()) {
      const path = parts.slice(0, i + 1).join('/');
      const found = folder.children.find((n): n is FolderNode => n.kind === 'folder' && n.path === path);
      const next: FolderNode = found ?? { kind: 'folder', name, path, children: [] };
      if (found === undefined) folder.children.push(next);
      folder = next;
    }
    folder.children.push({ kind: 'file', name: leaf, path: entry.path, entry });
  }
  const sort = (nodes: TreeNode[]): TreeNode[] => {
    nodes.sort(byName);
    for (const node of nodes) if (node.kind === 'folder') sort(node.children);
    return nodes;
  };
  return sort(root.children);
}

/** Every folder path of the tree, for **Open all folders**. */
export function folderPaths(nodes: readonly TreeNode[]): string[] {
  return nodes.flatMap((n) => (n.kind === 'folder' ? [n.path, ...folderPaths(n.children)] : []));
}

/** A file the reader draws as formatted text rather than as plain monospace. */
export function isMarkdown(path: string): boolean {
  return /\.(md|markdown|mdx)$/i.test(path);
}

export class InstalledFilesController {
  files = $state.raw<InstalledFileEntry[]>([]);
  error = $state.raw<ServiceError | null>(null);
  /** The path chosen in the tree; null until one is chosen. */
  selected = $state<string | null>(null);
  /** The chosen file's text, once read. */
  file = $state.raw<InstalledFileText | null>(null);
  fileError = $state.raw<ServiceError | null>(null);
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

  /** Reads one file for the reader; a reply for a file no longer chosen is dropped. */
  async open(path: string): Promise<void> {
    this.selected = path;
    this.file = null;
    this.fileError = null;
    const out = await this.#handle.call('agents.installed_file', { agent_instance_id: this.#agentInstanceId, path });
    if (this.selected !== path) return;
    if (out.kind === 'ok') this.file = out.result;
    else this.fileError = out.error;
  }
}
