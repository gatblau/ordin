// A dashboard's Export items (SM-DB-5, SM-DB-6): saved as PDF, PowerPoint, Excel or Word, or added to the end
// of a PowerPoint presentation the person picks. Both commands run on this computer (`Local`), open the
// operating system's own dialog and wait for the person; a cancelled dialog shows nothing.
//
// spec: ordin-desktop/components/ScreensModule#SM-DB-5
// spec: ordin-desktop/components/ScreensModule#SM-DB-6
import type { BackendHandle } from '../../backend/backend';
import { answers, type HasGroups } from '../../backend/groups';
import type { IconName } from '../../icons/sprite';
import type { DashboardExportFormat, ServiceError } from '../../service-api/types.gen';

const DIALOG_CANCELLED = 'DSK_DIALOG_CANCELLED';

/** The Export items, in the order the menu draws them, each with its label's message id. */
export const EXPORT_FORMATS: readonly { format: DashboardExportFormat; label: string; icon: IconName }[] = [
  { format: 'pdf', label: 'dashboard-export-pdf', icon: 'pdf' },
  { format: 'pptx', label: 'dashboard-export-pptx', icon: 'slides' },
  { format: 'xlsx', label: 'dashboard-export-xlsx', icon: 'sheet' },
  { format: 'docx', label: 'dashboard-export-docx', icon: 'word' },
];

/** Export is offered for a dashboard on this computer, when the program answers both commands. */
export function canExport(backend: HasGroups & { readonly kind: 'local' | 'server' }): boolean {
  return backend.kind === 'local' && answers(backend, 'dashboards.export') && answers(backend, 'dashboards.add_to_presentation');
}

export type ExportOutcome =
  | { kind: 'saved' }
  | { kind: 'added'; asCopy: boolean }
  | { kind: 'cancelled' }
  | { kind: 'error'; error: ServiceError };

/** `dashboards.export { dashboard_id, format }`. */
export async function exportDashboard(handle: Pick<BackendHandle, 'call'>, dashboardId: string, format: DashboardExportFormat): Promise<ExportOutcome> {
  const out = await handle.call('dashboards.export', { dashboard_id: dashboardId, format });
  if (out.kind === 'ok') return { kind: 'saved' };
  return out.error.code === DIALOG_CANCELLED ? { kind: 'cancelled' } : { kind: 'error', error: out.error };
}

/** `dashboards.add_to_presentation { dashboard_id }`. */
export async function addToPresentation(handle: Pick<BackendHandle, 'call'>, dashboardId: string): Promise<ExportOutcome> {
  const out = await handle.call('dashboards.add_to_presentation', { dashboard_id: dashboardId });
  if (out.kind === 'ok') return { kind: 'added', asCopy: out.result.as_copy };
  return out.error.code === DIALOG_CANCELLED ? { kind: 'cancelled' } : { kind: 'error', error: out.error };
}

/** The line announced after an export, by its message id; `null` when nothing is announced. */
export function announcement(outcome: ExportOutcome): 'dashboard-exported' | 'dashboard-added-to-presentation' | 'dashboard-added-as-copy' | null {
  switch (outcome.kind) {
    case 'saved':
      return 'dashboard-exported';
    case 'added':
      return outcome.asCopy ? 'dashboard-added-as-copy' : 'dashboard-added-to-presentation';
    default:
      return null;
  }
}
