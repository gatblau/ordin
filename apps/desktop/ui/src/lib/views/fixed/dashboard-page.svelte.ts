// One kept dashboard (SM-DB-4): its newest report — the report of the last of its `refresh_runs` that has
// one — and **Refresh** (`dashboards.refresh`). Read with `dashboards.list`, `reports.list` and
// `reports.get`, again on each `workspace_changed`, so a refresh's report shows once it is made.
//
// spec: ordin-desktop/components/ScreensModule#SM-DB-4
import type { BackendHandle } from '../../backend/backend';
import type { Dashboard, Report, ReportSummary, ServiceError } from '../../service-api/types.gen';

/** The newest report among `reports` made by one of the dashboard's runs; `null` when none is made yet. */
export function newestReportOf(dashboard: Pick<Dashboard, 'refresh_runs'>, reports: readonly ReportSummary[]): ReportSummary | null {
  for (let i = dashboard.refresh_runs.length - 1; i >= 0; i--) {
    const found = reports.find((r) => r.run_id === dashboard.refresh_runs[i]);
    if (found !== undefined) return found;
  }
  return null;
}

export class DashboardPage {
  dashboard = $state.raw<Dashboard | null>(null);
  report = $state.raw<Report | null>(null);
  error = $state.raw<ServiceError | null>(null);
  loaded = $state(false);
  refreshing = $state(false);
  readonly dashboardId: string;
  readonly #handle: Pick<BackendHandle, 'call'>;
  readonly #workspaceId: string;

  /** False when the backend does not answer `reports.*` (SM-UP-1): no report is drawn. */
  readonly #readsReports: boolean;

  constructor(handle: Pick<BackendHandle, 'call'>, workspaceId: string, dashboardId: string, readsReports = true) {
    this.#readsReports = readsReports;
    this.#handle = handle;
    this.#workspaceId = workspaceId;
    this.dashboardId = dashboardId;
  }

  async load(): Promise<void> {
    const [list, reports] = await Promise.all([
      this.#handle.call('dashboards.list', { workspace_id: this.#workspaceId }),
      this.#readsReports ? this.#handle.call('reports.list', { workspace_id: this.#workspaceId }) : Promise.resolve(null),
    ]);
    this.loaded = true;
    if (list.kind === 'error') {
      this.error = list.error;
      return;
    }
    this.dashboard = list.result.items.find((d) => d.id === this.dashboardId) ?? null;
    if (this.dashboard === null) {
      this.error = null;
      this.report = null;
      return;
    }
    if (reports === null) return;
    if (reports.kind === 'error') {
      this.error = reports.error;
      return;
    }
    const newest = newestReportOf(this.dashboard, reports.result.items);
    if (newest === null) {
      this.error = null;
      this.report = null;
      return;
    }
    if (this.report?.id === newest.id) return;
    const got = await this.#handle.call('reports.get', { report_id: newest.id });
    if (got.kind === 'error') {
      this.error = got.error;
      return;
    }
    this.error = null;
    this.report = got.result;
  }

  /** `dashboards.refresh`; true once the refresh has started. */
  async refresh(): Promise<boolean> {
    if (this.refreshing) return false;
    this.refreshing = true;
    const out = await this.#handle.call('dashboards.refresh', { workspace_id: this.#workspaceId, dashboard_id: this.dashboardId });
    this.refreshing = false;
    if (out.kind === 'error') {
      this.error = out.error;
      return false;
    }
    this.error = null;
    return true;
  }
}
