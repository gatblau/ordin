// The read-only lists of the fixed screens: documents of a folder, workflows, dashboards, the agents of a
// workspace, the Telegram and email links. One small loader each, over the generated commands.
//
// spec: ordin-desktop/components/ScreensModule#SM-UP-1
import { formatDate } from '../../format/intl';
import type { InstalledAgent, ServiceError } from '../../service-api/types.gen';
import type { Handle } from './util';

export interface ListRow {
  id: string;
  title: string;
  detail?: string | undefined;
}
export interface Loaded {
  rows: ListRow[];
  error: ServiceError | null;
}

const failed = (error: ServiceError): Loaded => ({ rows: [], error });

export async function loadFolder(h: Handle, placeId: string, folder = ''): Promise<Loaded> {
  const out = await h.call('files.list_folder', { place_id: placeId, folder, sort: 'name_asc' });
  if (out.kind === 'error') return failed(out.error);
  return { error: null, rows: out.result.items.map((f) => ({ id: f.path, title: f.name, detail: f.is_folder ? undefined : formatDate(f.changed_at) })) };
}

export async function loadWorkflows(h: Handle, workspaceId: string): Promise<Loaded> {
  const out = await h.call('workflows.list', { workspace_id: workspaceId });
  if (out.kind === 'error') return failed(out.error);
  return { error: null, rows: out.result.items.map((w) => ({ id: w.workflow_id, title: w.name })) };
}

export async function loadDashboards(h: Handle, workspaceId: string): Promise<Loaded> {
  const out = await h.call('dashboards.list', { workspace_id: workspaceId });
  if (out.kind === 'error') return failed(out.error);
  return { error: null, rows: out.result.items.map((d) => ({ id: d.id, title: d.name, detail: formatDate(d.kept_at) })) };
}

/** `onread` receives the agents as listed, so a screen can use their `update` offers (SM-AG-2). */
export async function loadAgents(h: Handle, workspaceId: string, onread?: (agents: InstalledAgent[]) => void): Promise<Loaded> {
  const out = await h.call('agents.list', { workspace_id: workspaceId });
  if (out.kind === 'error') return failed(out.error);
  onread?.(out.result);
  return { error: null, rows: out.result.map((a) => ({ id: a.instance.id, title: a.instance.display_name })) };
}

export async function loadTelegram(h: Handle, workspaceId: string): Promise<Loaded> {
  const out = await h.call('telegram.links', { ws: workspaceId });
  if (out.kind === 'error') return failed(out.error);
  return { error: null, rows: out.result.map((l) => ({ id: l.link.id, title: l.link.title, detail: String(l.new_messages) })) };
}
