// Keep as dashboard… on a report (SM-DB-1, SM-DB-2). A `report` block carries no run, so the screens find
// the Journal's report of the same title and "as of" (`reports.list`), read its run (`runs.get`) and send
// `dashboards.keep` with `made_by` and `inputs` from that run: a task run's task, or Ordin's conversation.
// A report a crew made is kept from the crew's workflow, so it is refused here.
//
// spec: ordin-desktop/components/ScreensModule#SM-DB-1
// spec: ordin-desktop/components/ScreensModule#SM-DB-2
import type { BackendHandle } from '../backend/backend';
import type { Dashboard, DashboardSource, ReportSummary, RunOrigin, ServiceError } from '../service-api/types.gen';

/** 2C `Dashboard.name`: 1 to 80 characters. */
export const DASHBOARD_NAME_MAX = 80;

export type KeepOutcome =
  | { kind: 'kept'; dashboard: Dashboard }
  | { kind: 'not_found' }
  | { kind: 'from_crew' }
  | { kind: 'name_needed' }
  | { kind: 'error'; error: ServiceError };

/** The report of the Journal that a block shows: same title, same moment. */
export function matchReport(reports: readonly ReportSummary[], title: string, asOf: string): ReportSummary | null {
  const at = Date.parse(asOf);
  return reports.find((r) => r.title === title && Date.parse(r.as_of) === at) ?? null;
}

/** `made_by` of a run's origin; `null` for a crew's round. */
export function sourceOf(origin: RunOrigin): DashboardSource | null {
  switch (origin.kind) {
    case 'task':
      return { kind: 'task', task_id: origin.task_id };
    case 'conversation':
      return { kind: 'conversation' };
    default:
      return null;
  }
}

/** A default name: the report's title, cut to 80 characters. */
export function defaultName(title: string): string {
  return [...title.trim()].slice(0, DASHBOARD_NAME_MAX).join('');
}

export async function keepAsDashboard(
  handle: Pick<BackendHandle, 'call'>,
  workspaceId: string,
  report: { title: string; as_of: string },
  name: string,
): Promise<KeepOutcome> {
  const wanted = name.trim();
  if (wanted.length === 0 || [...wanted].length > DASHBOARD_NAME_MAX) return { kind: 'name_needed' };
  const reports = await handle.call('reports.list', { workspace_id: workspaceId });
  if (reports.kind === 'error') return { kind: 'error', error: reports.error };
  const found = matchReport(reports.result.items, report.title, report.as_of);
  if (found === null) return { kind: 'not_found' };
  const run = await handle.call('runs.get', { run_id: found.run_id });
  if (run.kind === 'error') return { kind: 'error', error: run.error };
  const made_by = sourceOf(run.result.run.origin);
  if (made_by === null) return { kind: 'from_crew' };
  const kept = await handle.call('dashboards.keep', { workspace_id: workspaceId, name: wanted, made_by, inputs: run.result.run.inputs_used, run_id: found.run_id });
  if (kept.kind === 'error') return { kind: 'error', error: kept.error };
  return { kind: 'kept', dashboard: kept.result };
}
